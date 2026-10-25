//! Class instances on the node (docs/14 §5, docs/17 §4, ADR 0084): the
//! composite instances of `[instances]` and the implicit instances of the
//! devices whose profile has `[implements]`, with their methods compiled
//! into the core's method definitions and their queries turned into
//! derived signals `<instance>.<query>`.
//!
//! A member is reached through its path `<instance>.<member>`, the alias
//! the manifest checks registered for it; a method or query that reads or
//! writes a member without one (bound through an adapter, a constant, or
//! not bound) is refused with `J0807`.

use std::collections::{BTreeMap, BTreeSet};

use joker_core::ValueType;
use joker_core::class::{ARG_SIGNAL_BASE, InstanceDef, MethodDef, Stmt};
use joker_core::registry::Direction as CoreDirection;
use joker_expr::{CheckOptions, CompileOptions, EnumId, Env, Program, SignalInfo};
use joker_jkc::contract::{ContractFile, Statement, Ty};
use joker_manifest::context::{Context, class_ref};
use joker_manifest::model::Manifest;
use joker_manifest::value::{Node, Value};
use joker_units::Duration;

use crate::expr_plan::PlanEnv;
use crate::plan::{Plan, PlanError, SignalPlan};

/// A parameter of a method, as the core reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamPlan {
    /// The name its expressions use.
    pub name: String,
    /// The value type.
    pub ty: ValueType,
    /// The declared unit (`"1"` without one).
    pub unit: String,
}

/// One statement of a method, compiled.
#[derive(Debug, Clone, PartialEq)]
pub enum StmtPlan {
    /// `out = expr`.
    Assign {
        /// The canonical path of the output.
        target: String,
        /// The expression.
        program: Box<Program>,
    },
    /// `member.method(args)` on the instance bound to a fixed member.
    Call {
        /// The instance, by position in [`Plan::instances`].
        instance: usize,
        /// The method, by position in that instance's methods.
        method: usize,
        /// The arguments.
        args: Vec<Program>,
    },
}

/// A method of an instance.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodPlan {
    /// The name.
    pub name: String,
    /// The class that declares it.
    pub class: String,
    /// The parameters.
    pub params: Vec<ParamPlan>,
    /// The statements.
    pub stmts: Vec<StmtPlan>,
    /// The canonical output paths the call writes, its nested calls
    /// included: what a manual caller holds (ADR 0084).
    pub writes: Vec<String>,
}

/// An instance on the node.
#[derive(Debug, Clone, PartialEq)]
pub struct InstancePlan {
    /// Its name (the device's, for an implicit instance).
    pub name: String,
    /// The classes it fulfils, by reference.
    pub classes: Vec<String>,
    /// Whether a profile made it (`implicit`) rather than `[instances]`.
    pub implicit: bool,
    /// The capabilities it has.
    pub capabilities: Vec<String>,
    /// The group that owns its `out` members, where its method records
    /// expand; `None` without `out` members.
    pub group: Option<String>,
    /// Its methods; a method's number is its position (ADR 0084).
    pub methods: Vec<MethodPlan>,
}

impl InstancePlan {
    /// A method by name.
    pub fn method(&self, name: &str) -> Option<(usize, &MethodPlan)> {
        self.methods
            .iter()
            .enumerate()
            .find(|(_, m)| m.name == name)
    }
}

/// A class query of an instance, declared as a derived signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlan {
    /// `<instance>.<query>`.
    pub path: String,
    /// The expression over the instance's paths.
    pub source: String,
}

fn unsupported(message: impl Into<String>) -> PlanError {
    PlanError::new(Some("J0807"), message)
}

fn value_type(ty: &Ty) -> Option<ValueType> {
    Some(match ty {
        Ty::Bool => ValueType::Bool,
        Ty::I64 => ValueType::I64,
        Ty::U64 => ValueType::U64,
        Ty::F64 => ValueType::F64,
        Ty::F32 => ValueType::F32,
        Ty::Bits(_) => ValueType::Bits,
        Ty::Enum(_) => ValueType::Enum,
        _ => return None,
    })
}

/// One instance before its methods are compiled.
struct Found {
    name: String,
    implicit: bool,
    classes: Vec<String>,
    capabilities: BTreeSet<String>,
    nested: BTreeMap<String, String>,
}

fn strings(node: &Node) -> Vec<String> {
    match &node.value {
        Value::Array(items) => items
            .iter()
            .filter_map(|n| n.as_str().map(str::to_owned))
            .collect(),
        Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn found(manifest: &Manifest, context: &Context) -> Vec<Found> {
    let mut out = Vec::new();
    if let Some(instances) = manifest.pending.get("instances").and_then(Node::as_table) {
        for (name, entry) in instances {
            let Some(t) = entry.node.as_table() else {
                continue;
            };
            if t.contains_key("implemented_by") || t.contains_key("sim") {
                continue;
            }
            let Some(class) = t.get("class").and_then(|e| e.node.as_str()) else {
                continue;
            };
            let map = t.get("map").and_then(|e| e.node.as_table());
            let nested = map
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, e)| e.node.as_str().map(|v| (k.clone(), v.to_owned())))
                        .collect()
                })
                .unwrap_or_default();
            out.push(Found {
                name: name.clone(),
                implicit: false,
                classes: vec![class_ref(class)],
                capabilities: t
                    .get("capabilities")
                    .map(|e| strings(&e.node).into_iter().collect())
                    .unwrap_or_default(),
                nested,
            });
        }
    }
    for (name, device) in &manifest.devices {
        let Some(profile) = device
            .profile
            .as_ref()
            .and_then(|p| context.profiles.get(&p.value))
        else {
            continue;
        };
        if profile.implements.is_empty() {
            continue;
        }
        out.push(Found {
            name: name.clone(),
            implicit: true,
            classes: profile.implements.keys().cloned().collect(),
            capabilities: profile
                .implements
                .values()
                .flat_map(|i| i.capabilities.iter().cloned())
                .collect(),
            nested: BTreeMap::new(),
        });
    }
    out
}

fn localize(source: &str, instance: &str, names: &BTreeSet<String>) -> Result<String, PlanError> {
    let ast = joker_expr::parse(source)
        .map_err(|e| PlanError::new(Some(e.code()), format!("`{source}`: {}", e.kind)))?;
    let mut out = String::new();
    let mut last = 0;
    for r in joker_jkc::expr::reads(&ast, source) {
        let head = r.path.split('.').next().unwrap_or("");
        if names.contains(head) {
            out.push_str(source.get(last..r.range.start).unwrap_or(""));
            out.push_str(instance);
            out.push('.');
            out.push_str(&r.path);
            last = r.range.end;
        }
    }
    out.push_str(source.get(last..).unwrap_or(""));
    Ok(out)
}

fn member_names(file: &ContractFile) -> BTreeSet<String> {
    let c = &file.contract;
    c.members
        .iter()
        .map(|m| m.name.clone())
        .chain(c.queries.iter().map(|q| q.name.clone()))
        .chain(c.class_members.iter().map(|m| m.name.clone()))
        .collect()
}

/// Finds the instances of the generation, adds the derived signal of
/// every class query and returns the queries for `expr_plan` to compile
/// with the `calc` (ADR 0068). Methods are compiled by [`compile`] once
/// the groups are known.
pub(crate) fn declare(
    plan: &mut Plan,
    manifest: &Manifest,
    context: &Context,
    contracts: &BTreeMap<String, ContractFile>,
) -> Result<Vec<QueryPlan>, PlanError> {
    let mut queries = Vec::new();
    let mut instances = Vec::new();
    for f in found(manifest, context) {
        let mut files = Vec::new();
        for class in &f.classes {
            let file = contracts.get(class).ok_or_else(|| {
                PlanError::new(
                    Some("J0193"),
                    format!("instance `{}`: no contract for `{class}`", f.name),
                )
            })?;
            files.push(file);
        }
        for file in &files {
            let names = member_names(file);
            for q in &file.contract.queries {
                if q.capability
                    .as_ref()
                    .is_some_and(|c| !f.capabilities.contains(c))
                {
                    continue;
                }
                let path = format!("{}.{}", f.name, q.name);
                if plan.id(&path).is_some() || plan.signals.iter().any(|s| s.path == path) {
                    return Err(PlanError::new(
                        Some("J0604"),
                        format!("query path `{path}` collides with a signal"),
                    ));
                }
                let ty = value_type(&q.ty)
                    .ok_or_else(|| unsupported(format!("query `{path}`: type not supported")))?;
                plan.signals.push(SignalPlan {
                    path: path.clone(),
                    ty,
                    unit: q.unit.clone().unwrap_or_else(|| "1".into()),
                    direction: CoreDirection::Derived,
                    group: None,
                    max_age_ns: None,
                    device: None,
                    variants: Vec::new(),
                    enum_name: None,
                });
                queries.push(QueryPlan {
                    path,
                    source: localize(&q.expr, &f.name, &names)?,
                });
            }
        }
        instances.push(InstancePlan {
            name: f.name.clone(),
            classes: f.classes.clone(),
            implicit: f.implicit,
            capabilities: f.capabilities.iter().cloned().collect(),
            group: None,
            methods: Vec::new(),
        });
    }
    plan.signals.sort_by(|a, b| a.path.cmp(&b.path));
    plan.instances = instances;
    Ok(queries)
}

struct MethodEnv<'a> {
    base: PlanEnv<'a>,
    params: &'a [ParamPlan],
}

impl Env for MethodEnv<'_> {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        match self.params.iter().position(|p| p.name == path) {
            Some(i) => {
                let p = self.params.get(i)?;
                let ty = match p.ty {
                    ValueType::Bool => joker_expr::SignalType::Bool,
                    ValueType::I64 => joker_expr::SignalType::I64,
                    ValueType::U64 => joker_expr::SignalType::U64,
                    ValueType::F32 => joker_expr::SignalType::F32,
                    ValueType::Bits => joker_expr::SignalType::Bits,
                    _ => joker_expr::SignalType::F64,
                };
                Some(SignalInfo {
                    id: ARG_SIGNAL_BASE + u32::try_from(i).ok()?,
                    ty,
                    unit: (p.unit != "1")
                        .then(|| joker_units::Unit::parse(&p.unit).ok())
                        .flatten(),
                })
            }
            None => self.base.signal(path),
        }
    }

    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)> {
        self.base.enum_variant(enum_name, variant)
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo)) {
        self.base.select(selector, found);
    }
}

fn compile_method_expr(
    plan: &Plan,
    source: &str,
    params: &[ParamPlan],
    period: Duration,
    target: Option<&SignalPlan>,
) -> Result<Program, String> {
    let env = MethodEnv {
        base: PlanEnv::new(plan),
        params,
    };
    let checked = joker_expr::check(
        source,
        &env,
        CheckOptions {
            in_class_method: true,
        },
    )
    .map_err(|e| e.kind.to_string())?;
    if let Some(t) = target
        && t.ty != ValueType::Bool
        && t.ty != ValueType::Enum
    {
        let unit = joker_units::Unit::parse(&t.unit).unwrap_or(joker_units::Unit::ONE);
        let conversion = checked.expect_unit(&unit).map_err(|e| e.kind.to_string())?;
        if !conversion.is_identity() {
            return Err(format!(
                "the output is in `{}`, not in the unit of the class",
                t.unit
            ));
        }
    }
    joker_expr::compile(&checked, CompileOptions { period }).map_err(|e| e.kind.to_string())
}

/// Compiles the methods of every instance found by [`declare`], against
/// the generation's own signals, and gives each instance the group that
/// owns its `out` members.
pub(crate) fn compile(
    plan: &mut Plan,
    manifest: &Manifest,
    context: &Context,
    contracts: &BTreeMap<String, ContractFile>,
) -> Result<(), PlanError> {
    let founds: BTreeMap<String, Found> = found(manifest, context)
        .into_iter()
        .map(|f| (f.name.clone(), f))
        .collect();
    let index: BTreeMap<String, usize> = plan
        .instances
        .iter()
        .enumerate()
        .map(|(i, inst)| (inst.name.clone(), i))
        .collect();
    let mut tables: Vec<Vec<(String, &ContractFile, &joker_jkc::contract::MethodDef)>> = Vec::new();
    for inst in &plan.instances {
        let mut seen = BTreeSet::new();
        let mut list = Vec::new();
        for class in &inst.classes {
            let Some(file) = contracts.get(class) else {
                continue;
            };
            for m in &file.contract.methods {
                if m.capability
                    .as_ref()
                    .is_some_and(|c| !inst.capabilities.contains(c))
                {
                    continue;
                }
                if seen.insert(m.name.clone()) {
                    list.push((class.clone(), file, m));
                }
            }
        }
        tables.push(list);
    }
    let mut compiled = Vec::with_capacity(plan.instances.len());
    for (i, inst) in plan.instances.iter().enumerate() {
        let f = founds.get(&inst.name);
        let mut methods = Vec::new();
        let mut groups: BTreeSet<String> = BTreeSet::new();
        for (class, file, m) in &tables[i] {
            let names = member_names(file);
            let params: Vec<ParamPlan> = m
                .params
                .iter()
                .map(|p| {
                    Ok(ParamPlan {
                        name: p.name.clone(),
                        ty: value_type(&p.ty).ok_or_else(|| {
                            unsupported(format!(
                                "method `{}.{}`: argument `{}` of a type not supported",
                                inst.name, m.name, p.name
                            ))
                        })?,
                        unit: p.unit.clone().unwrap_or_else(|| "1".into()),
                    })
                })
                .collect::<Result<_, PlanError>>()?;
            let mut stmts = Vec::new();
            let mut writes = Vec::new();
            for s in &m.body {
                match s {
                    Statement::Assign { target, expr } => {
                        let path = format!("{}.{target}", inst.name);
                        let output = plan.output(&path).ok_or_else(|| {
                            unsupported(format!(
                                "method `{}.{}` writes `{path}`, which is not bound to an output of this node",
                                inst.name, m.name
                            ))
                        })?;
                        let group = output.group.clone();
                        let canonical = output.path.clone();
                        let period = plan
                            .groups
                            .iter()
                            .find(|g| g.name == group)
                            .map_or(Duration::from_millis(100), |g| {
                                Duration::from_nanos(g.period_ns)
                            });
                        let source = localize(expr, &inst.name, &names)?;
                        let program = compile_method_expr(
                            plan,
                            &source,
                            &params,
                            period,
                            plan.signal(&canonical),
                        )
                        .map_err(|e| {
                            unsupported(format!(
                                "method `{}.{}`: `{source}`: {e} (a member bound through an adapter or a constant, or a unit other than the class's, is not supported by this runtime yet)",
                                inst.name, m.name
                            ))
                        })?;
                        groups.insert(group);
                        writes.push(canonical.clone());
                        stmts.push(StmtPlan::Assign {
                            target: canonical,
                            program: Box::new(program),
                        });
                    }
                    Statement::Call {
                        member,
                        method,
                        args,
                    } => {
                        let bound = f.and_then(|f| f.nested.get(member)).ok_or_else(|| {
                            unsupported(format!(
                                "method `{}.{}` calls `{member}.{method}`, whose instance is not bound",
                                inst.name, m.name
                            ))
                        })?;
                        let target = *index.get(bound).ok_or_else(|| {
                            unsupported(format!("`{bound}` is not an instance of this node"))
                        })?;
                        let called = tables[target]
                            .iter()
                            .position(|(_, _, n)| &n.name == method)
                            .ok_or_else(|| {
                                PlanError::new(
                                    Some("J0193"),
                                    format!("instance `{bound}` has no method `{method}`"),
                                )
                            })?;
                        let mut programs = Vec::new();
                        for a in args {
                            let source = localize(a, &inst.name, &names)?;
                            programs.push(
                                compile_method_expr(
                                    plan,
                                    &source,
                                    &params,
                                    Duration::from_millis(100),
                                    None,
                                )
                                .map_err(|e| {
                                    unsupported(format!(
                                        "method `{}.{}`: `{source}`: {e}",
                                        inst.name, m.name
                                    ))
                                })?,
                            );
                        }
                        stmts.push(StmtPlan::Call {
                            instance: target,
                            method: called,
                            args: programs,
                        });
                    }
                }
            }
            methods.push(MethodPlan {
                name: m.name.clone(),
                class: class.clone(),
                params,
                stmts,
                writes,
            });
        }
        if groups.len() > 1 {
            return Err(unsupported(format!(
                "instance `{}`: its `out` members are in groups {}; methods across groups are not supported by this runtime yet",
                inst.name,
                groups.iter().cloned().collect::<Vec<_>>().join(", ")
            )));
        }
        compiled.push((methods, groups.into_iter().next()));
    }
    for (inst, (methods, group)) in plan.instances.iter_mut().zip(compiled) {
        inst.methods = methods;
        inst.group = group;
    }
    let snapshot = plan.instances.clone();
    for inst in &mut plan.instances {
        for m in &mut inst.methods {
            let mut extra = Vec::new();
            for s in &m.stmts {
                if let StmtPlan::Call {
                    instance, method, ..
                } = s
                {
                    nested_writes(&snapshot, *instance, *method, 1, &mut extra);
                }
            }
            m.writes.extend(extra);
            m.writes.sort();
            m.writes.dedup();
        }
    }
    Ok(())
}

fn nested_writes(
    instances: &[InstancePlan],
    instance: usize,
    method: usize,
    depth: usize,
    out: &mut Vec<String>,
) {
    if depth > joker_core::class::MAX_CALL_DEPTH {
        return;
    }
    let Some(m) = instances.get(instance).and_then(|i| i.methods.get(method)) else {
        return;
    };
    for s in &m.stmts {
        match s {
            StmtPlan::Assign { target, .. } => out.push(target.clone()),
            StmtPlan::Call {
                instance, method, ..
            } => nested_writes(instances, *instance, *method, depth + 1, out),
        }
    }
}

/// The argument programs of every call statement, by instance, method
/// and statement: the first layer of the core's method definitions.
pub fn call_args(plan: &Plan) -> Vec<Vec<Vec<Vec<&Program>>>> {
    plan.instances
        .iter()
        .map(|i| {
            i.methods
                .iter()
                .map(|m| {
                    m.stmts
                        .iter()
                        .map(|s| match s {
                            StmtPlan::Call { args, .. } => args.iter().collect(),
                            StmtPlan::Assign { .. } => Vec::new(),
                        })
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// The statements of every method, over `args` from [`call_args`].
pub fn statements<'a>(
    plan: &'a Plan,
    args: &'a [Vec<Vec<Vec<&'a Program>>>],
) -> Result<Vec<Vec<Vec<Stmt<'a>>>>, String> {
    plan.instances
        .iter()
        .zip(args)
        .map(|(inst, inst_args)| {
            inst.methods
                .iter()
                .zip(inst_args)
                .map(|(m, m_args)| {
                    m.stmts
                        .iter()
                        .zip(m_args)
                        .map(|(s, a)| match s {
                            StmtPlan::Assign { target, program } => Ok(Stmt::Assign {
                                target: plan
                                    .id(target)
                                    .ok_or_else(|| format!("`{target}` is not registered"))?,
                                expr: program,
                            }),
                            StmtPlan::Call {
                                instance, method, ..
                            } => Ok(Stmt::Call {
                                instance: u32::try_from(*instance)
                                    .map_err(|_| "too many instances".to_owned())?,
                                method: u16::try_from(*method)
                                    .map_err(|_| "too many methods".to_owned())?,
                                args: a,
                            }),
                        })
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// The methods of every instance, over `stmts` from [`statements`].
pub fn methods<'a>(plan: &Plan, stmts: &'a [Vec<Vec<Stmt<'a>>>]) -> Vec<Vec<MethodDef<'a>>> {
    plan.instances
        .iter()
        .zip(stmts)
        .map(|(inst, inst_stmts)| {
            inst.methods
                .iter()
                .zip(inst_stmts)
                .map(|(m, s)| MethodDef {
                    params: u8::try_from(m.params.len()).unwrap_or(u8::MAX),
                    stmts: s,
                })
                .collect()
        })
        .collect()
}

/// The core's instance definitions, over `methods` from [`methods`].
pub fn instances<'a>(methods: &'a [Vec<MethodDef<'a>>]) -> Vec<InstanceDef<'a>> {
    methods.iter().map(|m| InstanceDef { methods: m }).collect()
}
