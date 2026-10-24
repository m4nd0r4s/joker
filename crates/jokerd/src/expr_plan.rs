//! Binds `joker_expr` (parse/check/compile/eval, already built and
//! `no_std`) to a jokerd [`Plan`]'s own `SignalId` space, instead of
//! `joker-manifest::expr::Registry`'s separate, independent
//! enumeration (AUR-416).
//!
//! `joker-manifest::expr::Registry` assigns its own `u32` ids by
//! enumerating the full path registry (including aliases and `param`
//! signals) — fine for `jk check`'s own type-checking, where no id
//! needs to survive past that one pass, but wrong to reuse for a
//! `Program` that must later read `Shared.slots`: that buffer is
//! indexed by the real, canonical `SignalId` `Plan::new` resolves
//! through `joker_core::registry::Registry` (`plan.tables()?.registry`,
//! committed to by `registry_hash`), which skips aliases/params and is
//! built independently. [`PlanEnv`] closes that gap by sourcing ids
//! from `Plan` itself, so a `Program` compiled against it is
//! guaranteed to read the signal `Shared.slots` actually has it at.
//!
//! Scalar signal types resolve (`Bool`/`I64`/`U64`/`F64`/`F32`/`Bits`),
//! and so do enums (ADR 0060): an enum signal and an `Enum.variant`
//! constant resolve by the enum's declared name and the variant's
//! position in its declared list, exactly as `joker-manifest` resolves
//! them, with `AlarmState` always known. `String`/`Bytes`/`Record` never
//! appear in an expression (ref/01 §3.7: the language is scalar-only).
//!
//! [`compile_expr`] is the one path every manifest expression of a
//! generation takes (AUR-417): [`bind`] runs it for the outputs'
//! `interlock`, `range` limits and `reset_when`, the `calc`, the alarms
//! and the actions' `permit`, each with the period of the group that
//! evaluates it, and assigns the evaluation group of each `calc` and
//! alarm (ADR 0068).

use std::collections::{BTreeMap, BTreeSet};

use joker_core::alarm::{AlarmConfig, DEFAULT_MAX_SHELVE_NS, Severity};
use joker_core::class::DerivedDef;
use joker_core::output::Bound;
use joker_core::{Registry, SignalId, ValueType};
use joker_expr::{
    CheckOptions, CompileOptions, Diagnostic, EnumId, Env, EvalError, Inputs, Output, Program,
    Quality, Reading, SignalInfo, SignalType, check, compile, eval,
};
use joker_manifest::expr::{ALARM_STATE, ALARM_STATES};
use joker_manifest::model::Manifest;
use joker_manifest::value::{Node, Table, Value as Toml};
use joker_units::{Conversion, Duration, Literal, Unit};

use crate::plan::{DEFAULT_PERIOD_NS, Plan, PlanError, quantity_in};
use crate::shared::Shared;

/// The implicit group of `calc` and alarms nothing consumes (ADR 0068).
pub const CALC_GROUP: &str = "calc";
/// The period of the implicit `calc` group (ref/01 §3.7).
pub const CALC_PERIOD_NS: u64 = 100_000_000;
/// The period used only to learn what a `calc` or an alarm reads before
/// its group is known: long enough that no window exceeds a program's
/// capacity (ADR 0034).
const PROBE_PERIOD: Duration = Duration::from_secs(86_400);

fn signal_type(ty: ValueType) -> Option<SignalType> {
    match ty {
        ValueType::Bool => Some(SignalType::Bool),
        ValueType::I64 => Some(SignalType::I64),
        ValueType::U64 => Some(SignalType::U64),
        ValueType::F64 => Some(SignalType::F64),
        ValueType::F32 => Some(SignalType::F32),
        ValueType::Bits => Some(SignalType::Bits),
        ValueType::Enum | ValueType::String | ValueType::Bytes | ValueType::Record => None,
    }
}

/// `joker_expr::Env` over a jokerd [`Plan`], so a compiled `Program`'s
/// `Input.signal` is exactly the `SignalId` `Shared.slots` is indexed
/// by (see the module doc).
#[derive(Debug)]
pub struct PlanEnv<'a> {
    plan: &'a Plan,
    enums: BTreeMap<&'a str, (u32, Vec<&'a str>)>,
}

impl<'a> PlanEnv<'a> {
    /// An `Env` over `plan`'s own signals and enums. `AlarmState` is
    /// enum 0; the others follow in name order, so the ids are the same
    /// for the same plan.
    pub fn new(plan: &'a Plan) -> Self {
        let mut enums: BTreeMap<&'a str, (u32, Vec<&'a str>)> = BTreeMap::new();
        enums.insert(ALARM_STATE, (0, ALARM_STATES.to_vec()));
        let mut named: Vec<(&'a str, &'a [String])> = plan
            .signals
            .iter()
            .filter_map(|s| Some((s.enum_name.as_deref()?, s.variants.as_slice())))
            .collect();
        named.sort_by_key(|(name, _)| *name);
        for (name, variants) in named {
            let next = u32::try_from(enums.len()).unwrap_or(u32::MAX);
            enums
                .entry(name)
                .or_insert_with(|| (next, variants.iter().map(String::as_str).collect()));
        }
        Self { plan, enums }
    }
}

impl Env for PlanEnv<'_> {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        let id = self.plan.id(path)?;
        let canonical = self.plan.path(id)?;
        let s = self.plan.signals.iter().find(|s| s.path == canonical)?;
        let ty = match &s.enum_name {
            Some(name) if s.ty == ValueType::Enum => {
                SignalType::Enum(EnumId(self.enums.get(name.as_str())?.0))
            }
            _ => signal_type(s.ty)?,
        };
        let unit = (s.unit != "1")
            .then(|| joker_units::Unit::parse(&s.unit).ok())
            .flatten();
        Some(SignalInfo {
            id: u32::try_from(id.index()).ok()?,
            ty,
            unit,
        })
    }

    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)> {
        let (id, variants) = self.enums.get(enum_name)?;
        let index = variants.iter().position(|v| *v == variant)?;
        Some((EnumId(*id), u64::try_from(index).ok()?))
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo)) {
        let mut paths: Vec<&str> = self
            .plan
            .signals
            .iter()
            .map(|s| s.path.as_str())
            .filter(|p| crate::wire::matches(selector, p))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        for path in paths {
            if let Some(info) = self.signal(path) {
                found(info);
            }
        }
    }
}

/// What an expression must produce (ref/01 §3.7).
#[derive(Clone, Copy, Debug)]
pub enum Expect {
    /// A `bool`: permissives, alarm conditions, `reset_when`, `permit`.
    Bool,
    /// A number convertible to the unit: a `range` limit (E6).
    Unit(Unit),
    /// Any scalar: a `calc`.
    Any,
}

/// A manifest expression compiled against a plan's own signals.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprPlan {
    /// The expression as written.
    pub source: String,
    /// The compiled program; its inputs are the plan's `SignalId`s.
    pub program: Program,
    /// The conversion from the program's result into the unit the key
    /// requires (identity unless [`Expect::Unit`]).
    pub conversion: Conversion,
}

impl ExprPlan {
    /// The signals the program reads.
    pub fn inputs(&self) -> impl Iterator<Item = SignalId> + '_ {
        self.program.inputs().iter().map(|i| SignalId(i.signal))
    }
}

/// One limit of an output's `range` (ref/01 §5.10).
#[derive(Debug, Clone, PartialEq)]
pub enum LimitPlan {
    /// A constant, in the output's unit.
    Const(f64),
    /// An expression (docs/16 E6).
    Expr(Box<ExprPlan>),
}

impl LimitPlan {
    /// The core form of the limit, borrowing the program.
    pub fn bound(&self) -> Bound<'_> {
        match self {
            Self::Const(c) => Bound::Const(*c),
            Self::Expr(e) if e.conversion.is_identity() => Bound::Expr(&e.program),
            Self::Expr(e) => Bound::Converted(&e.program, e.conversion),
        }
    }

    fn expr(&self) -> Option<&ExprPlan> {
        match self {
            Self::Const(_) => None,
            Self::Expr(e) => Some(e),
        }
    }
}

/// The expressions of an output's envelope (docs/16 E3–E7).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutputExprs {
    /// `interlock`.
    pub interlock: Option<ExprPlan>,
    /// `range`, when at least one limit is an expression.
    pub range: Option<(LimitPlan, LimitPlan)>,
    /// `reset_when`.
    pub reset_when: Option<ExprPlan>,
    /// `requires_good`, with selectors expanded, in id order.
    pub requires_good: Vec<SignalId>,
}

impl OutputExprs {
    fn programs(&self) -> impl Iterator<Item = &ExprPlan> {
        let (low, high) = match &self.range {
            Some((low, high)) => (low.expr(), high.expr()),
            None => (None, None),
        };
        self.interlock
            .iter()
            .chain(low)
            .chain(high)
            .chain(self.reset_when.iter())
    }

    /// Every signal the envelope reads: the expressions' inputs and the
    /// `requires_good` signals.
    pub fn reads(&self) -> Vec<SignalId> {
        let mut out: Vec<SignalId> = self.programs().flat_map(ExprPlan::inputs).collect();
        out.extend(&self.requires_good);
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// A `calc` (ref/01 §5.14).
#[derive(Debug, Clone, PartialEq)]
pub struct CalcPlan {
    /// `calc.<name>`.
    pub path: String,
    /// The group that evaluates it (ADR 0068).
    pub group: String,
    /// The expression.
    pub expr: ExprPlan,
}

/// An alarm of the manifest (ref/01 §5.15).
#[derive(Debug, Clone, PartialEq)]
pub struct AlarmPlan {
    /// The alarm's name.
    pub name: String,
    /// The group that evaluates it (ADR 0068).
    pub group: String,
    /// `when`.
    pub when: ExprPlan,
    /// `suppress_when`.
    pub suppress_when: Option<ExprPlan>,
    /// `delay_on`, ns.
    pub delay_on_ns: u64,
    /// `delay_off`, ns.
    pub delay_off_ns: u64,
    /// `severity`.
    pub severity: Severity,
    /// `max_shelve`, ns.
    pub max_shelve_ns: u64,
    /// `actions.trip`, with selectors expanded, in id order.
    pub trip: Vec<SignalId>,
}

impl AlarmPlan {
    /// `alarm.<name>.state`.
    pub fn state_path(&self) -> String {
        format!("alarm.{}.state", self.name)
    }

    /// `alarm.<name>.active`.
    pub fn active_path(&self) -> String {
        format!("alarm.{}.active", self.name)
    }
}

/// Parses, type-checks and compiles `source` against `plan`'s own
/// signals (AUR-416), with windows sized for `period` (ADR 0034): the
/// one path every manifest expression of a generation goes through.
#[allow(
    clippy::result_large_err,
    reason = "joker_expr::Diagnostic is already allowed large within its own crate (crates/joker-expr/src/lib.rs); a diagnostic is a cold path, not something to box just for this"
)]
pub fn compile_expr(
    source: &str,
    plan: &Plan,
    period: Duration,
    expect: Expect,
) -> Result<ExprPlan, Diagnostic> {
    let env = PlanEnv::new(plan);
    let checked = check(source, &env, CheckOptions::default())?;
    let conversion = match expect {
        Expect::Bool => {
            checked.expect_bool()?;
            Conversion::IDENTITY
        }
        Expect::Unit(unit) => checked.expect_unit(&unit)?,
        Expect::Any => Conversion::IDENTITY,
    };
    let program = compile(&checked, CompileOptions { period })?;
    Ok(ExprPlan {
        source: source.to_owned(),
        program,
        conversion,
    })
}

/// [`compile_expr`] for a boolean expression, returning its program.
#[allow(
    clippy::result_large_err,
    reason = "joker_expr::Diagnostic is already allowed large within its own crate (crates/joker-expr/src/lib.rs); a diagnostic is a cold path, not something to box just for this"
)]
pub fn compile_bool(source: &str, plan: &Plan, period: Duration) -> Result<Program, Diagnostic> {
    compile_expr(source, plan, period, Expect::Bool).map(|e| e.program)
}

/// Reads `Shared.slots` by the `SignalId` [`PlanEnv`] assigned.
struct SlotInputs<'a> {
    shared: &'a Shared,
    now: u64,
}

impl Inputs for SlotInputs<'_> {
    fn read(&self, signal: u32) -> Reading {
        let bad = Reading {
            raw: 0,
            quality: Quality::new(joker_expr::QualityCode::Bad, joker_expr::Reason::NONE),
            acquired: self.now,
        };
        let Some(slot) = self.shared.slots.get(signal as usize) else {
            return bad;
        };
        match slot.read(8) {
            Ok(sample) => Reading {
                raw: sample.transport_bits(),
                quality: Quality::from_bits(sample.quality().to_bits()),
                acquired: sample.ts_acq(),
            },
            Err(_) => bad,
        }
    }
}

/// Evaluates `program` (compiled by [`compile_bool`]) against
/// `shared.slots` at `now` (monotonic ns), continuing from `state`: the
/// program's arena for windows, `held`, `rose`, `fell` and `hyst` (ADR
/// 0034). The caller keeps `state` between evaluations of the same
/// program (ADR 0060); an empty or wrongly sized `state` is reset to the
/// program's initial state first.
pub fn eval_bool(
    program: &Program,
    shared: &Shared,
    now: u64,
    state: &mut Vec<u64>,
) -> Result<Output, EvalError> {
    if state.len() != program.state_words() {
        state.clear();
        state.resize(program.state_words(), 0);
    }
    let inputs = SlotInputs { shared, now };
    eval(program, &inputs, now, state)
}

fn plan_error(what: &str, source: &str, e: &Diagnostic) -> PlanError {
    PlanError::new(Some(e.code()), format!("{what}: {} in `{source}`", e.kind))
}

fn period_ns(plan: &Plan, group: &str) -> Option<u64> {
    plan.groups
        .iter()
        .find(|g| g.name == group)
        .map(|g| g.period_ns)
}

fn group_period(plan: &Plan, group: &str) -> Duration {
    Duration::from_nanos(period_ns(plan, group).unwrap_or(DEFAULT_PERIOD_NS))
}

/// The signals a path or a selector names, in id order.
fn expand(plan: &Plan, target: &str) -> Vec<SignalId> {
    let mut ids: Vec<SignalId> = if joker_manifest::grammar::is_wildcard(target) {
        plan.signals
            .iter()
            .filter(|s| joker_manifest::grammar::matches(target, &s.path))
            .filter_map(|s| plan.id(&s.path))
            .collect()
    } else {
        plan.id(target).into_iter().collect()
    };
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The outputs a path or a selector names, in id order.
fn expand_outputs(plan: &Plan, target: &str) -> Vec<SignalId> {
    let mut ids: Vec<SignalId> = plan
        .outputs
        .iter()
        .filter(|o| {
            if joker_manifest::grammar::is_wildcard(target) {
                joker_manifest::grammar::matches(target, &o.path)
            } else {
                plan.id(target).is_some() && plan.id(target) == plan.id(&o.path)
            }
        })
        .filter_map(|o| plan.id(&o.path))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn output_exprs(plan: &Plan, manifest: &Manifest) -> Result<Vec<(usize, OutputExprs)>, PlanError> {
    let mut out = Vec::new();
    for (key, envelope) in &manifest.outputs {
        let path = plan.aliases.get(key).map_or(key.as_str(), String::as_str);
        let Some(index) = plan.outputs.iter().position(|o| o.path == path) else {
            continue;
        };
        let output = &plan.outputs[index];
        let period = group_period(plan, &output.group);
        let what = |k: &str| format!("outputs.\"{path}\".{k}");
        let boolean = |k: &str, source: &str| {
            compile_expr(source, plan, period, Expect::Bool)
                .map_err(|e| plan_error(&what(k), source, &e))
        };
        let interlock = envelope
            .interlock
            .as_ref()
            .map(|s| boolean("interlock", &s.value))
            .transpose()?;
        let reset_when = envelope
            .reset_when
            .as_ref()
            .map(|s| boolean("reset_when", &s.value))
            .transpose()?;
        let range = match &envelope.range {
            Some(r)
                if r.value.len() == 2
                    && r.value.iter().any(|b| Literal::parse(&b.value).is_err()) =>
            {
                let unit = Unit::parse(&output.unit).map_err(|e| {
                    PlanError::new(Some(e.code()), format!("unit of `{}`", output.path))
                })?;
                let limit = |text: &str| -> Result<LimitPlan, PlanError> {
                    if Literal::parse(text).is_ok() {
                        return Ok(LimitPlan::Const(quantity_in(text, &unit, "range")?));
                    }
                    compile_expr(text, plan, period, Expect::Unit(unit))
                        .map(|e| LimitPlan::Expr(Box::new(e)))
                        .map_err(|e| plan_error(&what("range"), text, &e))
                };
                Some((limit(&r.value[0].value)?, limit(&r.value[1].value)?))
            }
            _ => None,
        };
        let mut requires_good = Vec::new();
        for target in &envelope.requires_good {
            let found = expand(plan, &target.value);
            if found.is_empty() {
                return Err(PlanError::new(
                    Some("J0101"),
                    format!(
                        "{}: unknown signal `{}`",
                        what("requires_good"),
                        target.value
                    ),
                ));
            }
            requires_good.extend(found);
        }
        requires_good.sort_unstable();
        requires_good.dedup();
        out.push((
            index,
            OutputExprs {
                interlock,
                range,
                reset_when,
                requires_good,
            },
        ));
    }
    Ok(out)
}

/// A `calc` or an alarm as the manifest declares it.
#[derive(Debug, Clone)]
struct Declaration {
    name: String,
    group: Option<String>,
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    Calc {
        source: String,
    },
    Alarm {
        when: String,
        suppress_when: Option<String>,
        delay_on_ns: u64,
        delay_off_ns: u64,
        severity: Severity,
        max_shelve_ns: u64,
        trip: Vec<String>,
    },
}

impl Declaration {
    fn key(&self) -> String {
        match self.kind {
            Kind::Calc { .. } => format!("calc.{}", self.name),
            Kind::Alarm { .. } => format!("alarms.{}", self.name),
        }
    }

    fn paths(&self) -> Vec<String> {
        match self.kind {
            Kind::Calc { .. } => vec![format!("calc.{}", self.name)],
            Kind::Alarm { .. } => vec![
                format!("alarm.{}.state", self.name),
                format!("alarm.{}.active", self.name),
            ],
        }
    }

    fn sources(&self) -> Vec<(String, &str, Expect)> {
        match &self.kind {
            Kind::Calc { source } => vec![(self.key(), source.as_str(), Expect::Any)],
            Kind::Alarm {
                when,
                suppress_when,
                ..
            } => {
                let mut out = vec![(format!("{}.when", self.key()), when.as_str(), Expect::Bool)];
                if let Some(s) = suppress_when {
                    out.push((
                        format!("{}.suppress_when", self.key()),
                        s.as_str(),
                        Expect::Bool,
                    ));
                }
                out
            }
        }
    }
}

fn text<'t>(table: &'t Table, key: &str) -> Option<&'t str> {
    table.get(key).and_then(|e| e.node.as_str())
}

fn duration_of(table: &Table, key: &str, what: &str) -> Result<Option<u64>, PlanError> {
    text(table, key)
        .map(|t| {
            Duration::parse(t)
                .map(Duration::as_nanos)
                .map_err(|e| PlanError::new(Some(e.code()), format!("{what}.{key} `{t}`")))
        })
        .transpose()
}

fn severity(text: &str) -> Option<Severity> {
    Some(match text {
        "low" => Severity::Low,
        "medium" => Severity::Medium,
        "high" => Severity::High,
        "critical" => Severity::Critical,
        _ => return None,
    })
}

fn strings(node: &Node) -> Vec<String> {
    match &node.value {
        Toml::String(s) => vec![s.clone()],
        Toml::Array(items) => items
            .iter()
            .filter_map(|n| n.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn declarations(manifest: &Manifest) -> Result<Vec<Declaration>, PlanError> {
    let mut out = Vec::new();
    if let Some(calc) = manifest.pending.get("calc").and_then(Node::as_table) {
        for (name, entry) in calc {
            let (source, group) = match &entry.node.value {
                Toml::Table(t) => (text(t, "expr"), text(t, "group")),
                _ => (entry.node.as_str(), None),
            };
            let source = source.ok_or_else(|| {
                PlanError::new(Some("J0158"), format!("calc.{name} has no expression"))
            })?;
            out.push(Declaration {
                name: name.clone(),
                group: group.map(str::to_owned),
                kind: Kind::Calc {
                    source: source.to_owned(),
                },
            });
        }
    }
    if let Some(alarms) = manifest.pending.get("alarms").and_then(Node::as_table) {
        for (name, entry) in alarms {
            let what = format!("alarms.{name}");
            let Some(t) = entry.node.as_table() else {
                continue;
            };
            let when = text(t, "when")
                .ok_or_else(|| PlanError::new(Some("J0158"), format!("{what} has no `when`")))?;
            let severity = text(t, "severity").and_then(severity).ok_or_else(|| {
                PlanError::new(Some("J0144"), format!("{what} has no valid `severity`"))
            })?;
            let trip = t
                .get("actions")
                .and_then(|a| a.node.as_table())
                .and_then(|a| a.get("trip"))
                .map(|e| strings(&e.node))
                .unwrap_or_default();
            out.push(Declaration {
                name: name.clone(),
                group: text(t, "group").map(str::to_owned),
                kind: Kind::Alarm {
                    when: when.to_owned(),
                    suppress_when: text(t, "suppress_when").map(str::to_owned),
                    delay_on_ns: duration_of(t, "delay_on", &what)?.unwrap_or(0),
                    delay_off_ns: duration_of(t, "delay_off", &what)?.unwrap_or(0),
                    severity,
                    max_shelve_ns: duration_of(t, "max_shelve", &what)?
                        .unwrap_or(DEFAULT_MAX_SHELVE_NS),
                    trip,
                },
            });
        }
    }
    Ok(out)
}

/// What each declaration reads, compiled with [`PROBE_PERIOD`] only to
/// learn its inputs.
fn probe_reads(plan: &Plan, declared: &[Declaration]) -> Result<Vec<Vec<SignalId>>, PlanError> {
    declared
        .iter()
        .map(|d| {
            let mut reads = Vec::new();
            for (what, source, expect) in d.sources() {
                let e = compile_expr(source, plan, PROBE_PERIOD, expect)
                    .map_err(|e| plan_error(&what, source, &e))?;
                reads.extend(e.inputs());
            }
            reads.sort_unstable();
            reads.dedup();
            Ok(reads)
        })
        .collect()
}

/// The group that evaluates each declaration (ADR 0068 rule 1).
fn evaluation_groups(
    plan: &Plan,
    declared: &[Declaration],
    reads: &[Vec<SignalId>],
) -> Result<Vec<String>, PlanError> {
    let produces: Vec<Vec<SignalId>> = declared
        .iter()
        .map(|d| d.paths().iter().filter_map(|p| plan.id(p)).collect())
        .collect();
    let mut fixed: Vec<Vec<String>> = Vec::with_capacity(declared.len());
    let mut readers: Vec<Vec<usize>> = Vec::with_capacity(declared.len());
    for (i, ids) in produces.iter().enumerate() {
        let hit = |list: &[SignalId]| list.iter().any(|id| ids.contains(id));
        let mut groups: Vec<String> = plan
            .outputs
            .iter()
            .filter(|o| hit(&o.exprs.reads()))
            .map(|o| o.group.clone())
            .collect();
        groups.extend(
            plan.groups
                .iter()
                .filter(|g| {
                    g.tasks.iter().any(|t| {
                        t.bindings
                            .values()
                            .any(|p| plan.id(p).is_some_and(|id| ids.contains(&id)))
                    })
                })
                .map(|g| g.name.clone()),
        );
        fixed.push(groups);
        readers.push(
            reads
                .iter()
                .enumerate()
                .filter(|(j, r)| *j != i && hit(r))
                .map(|(j, _)| j)
                .collect(),
        );
    }
    let rank = |name: &str| (period_ns(plan, name).unwrap_or(u64::MAX), name.to_owned());
    let mut assigned: Vec<Option<String>> = Vec::with_capacity(declared.len());
    for d in declared {
        if let Some(g) = &d.group
            && period_ns(plan, g).is_none()
        {
            return Err(PlanError::new(
                Some("J0237"),
                format!("{}: group `{g}` does not exist", d.key()),
            ));
        }
        assigned.push(d.group.clone());
    }
    loop {
        let mut changed = false;
        for i in 0..declared.len() {
            if declared[i].group.is_some() {
                continue;
            }
            let mut best = assigned[i].clone();
            let candidates: Vec<String> = fixed[i]
                .iter()
                .cloned()
                .chain(readers[i].iter().filter_map(|&j| assigned[j].clone()))
                .collect();
            for g in candidates {
                if best.as_deref().is_none_or(|b| rank(&g) < rank(b)) {
                    best = Some(g);
                }
            }
            if best != assigned[i] {
                assigned[i] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(assigned
        .into_iter()
        .map(|g| g.unwrap_or_else(|| CALC_GROUP.to_owned()))
        .collect())
}

/// The `calc` in evaluation order: each after every `calc` it reads
/// (ADR 0068 rule 2), otherwise in name order.
fn calc_order(calcs: Vec<CalcPlan>, plan: &Plan) -> Result<Vec<CalcPlan>, PlanError> {
    let ids: Vec<Option<SignalId>> = calcs.iter().map(|c| plan.id(&c.path)).collect();
    let mut done = vec![false; calcs.len()];
    let mut order = Vec::with_capacity(calcs.len());
    while order.len() < calcs.len() {
        let next = (0..calcs.len()).find(|&i| {
            !done[i]
                && calcs[i].expr.inputs().all(|input| {
                    ids.iter()
                        .enumerate()
                        .all(|(j, id)| j == i || done[j] || *id != Some(input))
                })
        });
        let Some(i) = next else {
            return Err(PlanError::new(
                Some("J0190"),
                "the `calc` depend on each other in a cycle",
            ));
        };
        done[i] = true;
        order.push(i);
    }
    let mut slots: Vec<Option<CalcPlan>> = calcs.into_iter().map(Some).collect();
    Ok(order.into_iter().filter_map(|i| slots[i].take()).collect())
}

/// An action's `permit` windows are sized with the period of the fastest
/// group that owns an output of its device, or 100 ms (ADR 0068 rule 4).
pub fn permit_period(plan: &Plan, device: &str) -> Duration {
    let fastest = plan
        .outputs
        .iter()
        .filter(|o| plan.signal(&o.path).and_then(|s| s.device.as_deref()) == Some(device))
        .filter_map(|o| period_ns(plan, &o.group))
        .min();
    Duration::from_nanos(fastest.unwrap_or(DEFAULT_PERIOD_NS))
}

/// Compiles every `plan.permit_sources` (ADR 0060 rule 1, ADR 0068 rule 4).
fn compile_permits(plan: &Plan) -> Result<BTreeMap<(String, String), Program>, PlanError> {
    let mut compiled = BTreeMap::new();
    for ((target, action), source) in &plan.permit_sources {
        let instance = plan
            .instance_actions
            .iter()
            .find(|i| &i.instance == target && &i.action == action);
        let (section, device) = match instance {
            Some(i) => ("instances", i.device.as_str()),
            None => ("devices", target.as_str()),
        };
        let program = compile_bool(source, plan, permit_period(plan, device)).map_err(|e| {
            plan_error(
                &format!("{section}.{target}.actions.{action}.permit"),
                source,
                &e,
            )
        })?;
        compiled.insert((target.clone(), action.clone()), program);
    }
    Ok(compiled)
}

/// Every signal an expression of a group reads becomes one of its ports,
/// except a derived signal the group itself produces (ADR 0068 rule 3).
fn latch_reads(plan: &mut Plan) {
    let mut reads: BTreeMap<String, BTreeSet<SignalId>> = BTreeMap::new();
    let mut produced: BTreeMap<SignalId, String> = BTreeMap::new();
    for o in &plan.outputs {
        reads
            .entry(o.group.clone())
            .or_default()
            .extend(o.exprs.reads());
    }
    for c in &plan.calcs {
        reads
            .entry(c.group.clone())
            .or_default()
            .extend(c.expr.inputs());
        if let Some(id) = plan.id(&c.path) {
            produced.insert(id, c.group.clone());
        }
    }
    for a in &plan.alarms {
        let entry = reads.entry(a.group.clone()).or_default();
        entry.extend(a.when.inputs());
        entry.extend(a.suppress_when.iter().flat_map(ExprPlan::inputs));
        for path in [a.state_path(), a.active_path()] {
            if let Some(id) = plan.id(&path) {
                produced.insert(id, a.group.clone());
            }
        }
    }
    for (group, ids) in reads {
        let paths: Vec<String> = ids
            .into_iter()
            .filter(|id| produced.get(id) != Some(&group))
            .filter_map(|id| plan.path(id).map(str::to_owned))
            .collect();
        if let Some(g) = plan.groups.iter_mut().find(|g| g.name == group) {
            for path in paths {
                if !g.ports.contains(&path) {
                    g.ports.push(path);
                }
            }
        }
    }
}

/// Compiles every expression of a planned generation against its own
/// signals (AUR-417): the outputs' `interlock`, `range` limits,
/// `reset_when` and `requires_good`, the `calc`, the alarms and the
/// actions' `permit`; assigns the evaluation group of each `calc` and
/// alarm and latches what the expressions read (ADR 0068). `plan` must
/// already resolve its ids; the caller re-indexes it afterwards, since
/// derived signals gain their group here.
pub(crate) fn bind(plan: &mut Plan, manifest: &Manifest) -> Result<(), PlanError> {
    for (index, exprs) in output_exprs(plan, manifest)? {
        plan.outputs[index].exprs = exprs;
    }
    let declared = declarations(manifest)?;
    let reads = probe_reads(plan, &declared)?;
    let groups = evaluation_groups(plan, &declared, &reads)?;
    if groups.iter().any(|g| g == CALC_GROUP) && period_ns(plan, CALC_GROUP).is_none() {
        let at = plan
            .groups
            .iter()
            .position(|g| g.name.as_str() > CALC_GROUP)
            .unwrap_or(plan.groups.len());
        plan.groups.insert(
            at,
            crate::plan::GroupPlan {
                name: CALC_GROUP.to_owned(),
                period_ns: CALC_PERIOD_NS,
                tasks: Vec::new(),
                ports: Vec::new(),
            },
        );
    }
    for (d, group) in declared.iter().zip(&groups) {
        for path in d.paths() {
            if let Some(s) = plan.signals.iter_mut().find(|s| s.path == path) {
                s.group = Some(group.clone());
            }
        }
    }
    let mut calcs = Vec::new();
    let mut alarms = Vec::new();
    for (d, group) in declared.iter().zip(&groups) {
        let period = group_period(plan, group);
        let compiled = |what: &str, source: &str, expect| {
            compile_expr(source, plan, period, expect).map_err(|e| plan_error(what, source, &e))
        };
        match &d.kind {
            Kind::Calc { source } => calcs.push(CalcPlan {
                path: format!("calc.{}", d.name),
                group: group.clone(),
                expr: compiled(&d.key(), source, Expect::Any)?,
            }),
            Kind::Alarm {
                when,
                suppress_when,
                delay_on_ns,
                delay_off_ns,
                severity,
                max_shelve_ns,
                trip,
            } => {
                let mut tripped = Vec::new();
                for target in trip {
                    let found = expand_outputs(plan, target);
                    if found.is_empty() {
                        return Err(PlanError::new(
                            Some("J0234"),
                            format!(
                                "{}.actions.trip: `{target}` is not an output of this node",
                                d.key()
                            ),
                        ));
                    }
                    tripped.extend(found);
                }
                tripped.sort_unstable();
                tripped.dedup();
                alarms.push(AlarmPlan {
                    name: d.name.clone(),
                    group: group.clone(),
                    when: compiled(&format!("{}.when", d.key()), when, Expect::Bool)?,
                    suppress_when: suppress_when
                        .as_deref()
                        .map(|s| compiled(&format!("{}.suppress_when", d.key()), s, Expect::Bool))
                        .transpose()?,
                    delay_on_ns: *delay_on_ns,
                    delay_off_ns: *delay_off_ns,
                    severity: *severity,
                    max_shelve_ns: *max_shelve_ns,
                    trip: tripped,
                });
            }
        }
    }
    plan.calcs = calc_order(calcs, plan)?;
    plan.alarms = alarms;
    plan.permits = compile_permits(plan)?;
    latch_reads(plan);
    Ok(())
}

/// The `calc` of `plan` as the core evaluates them, in evaluation order.
pub fn derived_defs<'p>(
    plan: &'p Plan,
    registry: &Registry<'_>,
) -> Result<Vec<DerivedDef<'p>>, String> {
    plan.calcs
        .iter()
        .map(|c| {
            Ok(DerivedDef {
                signal: plan
                    .id(&c.path)
                    .ok_or_else(|| format!("`{}` is not registered", c.path))?,
                group: registry
                    .resolve_group(&c.group)
                    .ok_or_else(|| format!("group `{}` is not registered", c.group))?,
                expr: &c.expr.program,
            })
        })
        .collect()
}

/// The alarms of `plan` as the core evaluates them; an alarm's number is
/// its position in `plan.alarms`.
pub fn alarm_configs<'p>(
    plan: &'p Plan,
    registry: &Registry<'_>,
) -> Result<Vec<AlarmConfig<'p>>, String> {
    plan.alarms
        .iter()
        .enumerate()
        .map(|(i, a)| {
            Ok(AlarmConfig {
                id: u32::try_from(i).map_err(|_| "too many alarms".to_owned())?,
                group: registry
                    .resolve_group(&a.group)
                    .ok_or_else(|| format!("group `{}` is not registered", a.group))?,
                when: &a.when.program,
                suppress_when: a.suppress_when.as_ref().map(|e| &e.program),
                delay_on_ns: a.delay_on_ns,
                delay_off_ns: a.delay_off_ns,
                severity: a.severity,
                max_shelve_ns: a.max_shelve_ns,
                trip: &a.trip,
                state_signal: plan.id(&a.state_path()),
                active_signal: plan.id(&a.active_path()),
            })
        })
        .collect()
}
