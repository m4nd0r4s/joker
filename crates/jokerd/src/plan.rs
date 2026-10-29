//! A generation plan: the effective manifest and its path registry turned
//! into the tables the core runs on (docs/01 §3.1, §15).
//!
//! The plan owns every string. The cycle thread builds the borrowed core
//! structures (registry, output configurations, writer table) from it once
//! per generation, so nothing on the tick path allocates.

use std::collections::BTreeMap;
use std::fmt;

use joker_core::output::{Budget, ClockRequirement, OnInterlock, OnRange, SafeValue, Writers};
use joker_core::registry::{Alias, Direction as CoreDirection, GroupMeta, OwnedRegistry, UnitRef};
use joker_core::{
    GroupId, Level, OutputConfig, SignalId, SignalMeta, ValueType, WriterConfig, WriterId,
    WriterKind,
};
use joker_manifest::check::PathRegistry;
use joker_manifest::context::{Context, Direction, SignalType};
use joker_manifest::model::{Binding, Envelope, Manifest, SafeValue as ManifestSafe};
use joker_manifest::value::{Node, Value};
use joker_units::{Conversion, Duration, Literal, Quantity, Unit};

use crate::expr_plan::{AlarmPlan, CalcPlan, OutputExprs};

/// The implicit group of outputs no cycle task writes (docs/16 A11).
pub const DEFAULT_GROUP: &str = "default";
/// The period of the implicit `default` group.
pub const DEFAULT_PERIOD_NS: u64 = 100_000_000;
/// `alarm_after` of a group that does not declare it (ref/01 §5.11).
pub const DEFAULT_ALARM_AFTER: u32 = 10;
/// The manual levels a person can hold (ref/01 §3.11).
pub const MANUAL_LEVELS: [u8; 3] = [3, 4, 5];

/// Why a checked manifest cannot be planned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError {
    /// The registry code, when one applies.
    pub code: Option<&'static str>,
    /// What went wrong.
    pub message: String,
}

impl PlanError {
    pub(crate) fn new(code: Option<&'static str>, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(f, "error[{code}]: {}", self.message),
            None => write!(f, "error: {}", self.message),
        }
    }
}

impl std::error::Error for PlanError {}

/// A cycle group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPlan {
    /// Group name.
    pub name: String,
    /// Period in ns.
    pub period_ns: u64,
    /// Cycle tasks in tick order (ref/01 §5.11).
    pub tasks: Vec<TaskPlan>,
    /// Signals the group latches, in path order; ids are resolved later.
    pub ports: Vec<String>,
    /// `alarm_after`: consecutive overruns that raise
    /// `joker.group.<g>.overrun` (ref/01 §5.11).
    pub alarm_after: u32,
}

/// One cycle task of an app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlan {
    /// The app.
    pub app: String,
    /// The task.
    pub task: String,
    /// The app's writer.
    pub writer: WriterId,
    /// Port name to signal path.
    pub bindings: BTreeMap<String, String>,
}

/// One signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalPlan {
    /// Canonical path.
    pub path: String,
    /// Type.
    pub ty: ValueType,
    /// Unit symbol.
    pub unit: String,
    /// Direction.
    pub direction: CoreDirection,
    /// Owner group of an output.
    pub group: Option<String>,
    /// `max_age` in ns.
    pub max_age_ns: Option<u64>,
    /// The device that owns it.
    pub device: Option<String>,
    /// Enum variants, in value order.
    pub variants: Vec<String>,
    /// The enum's name (`PcsState`, `AlarmState`), as expressions spell
    /// its constants (`PcsState.fault`, ref/01 §3.7); `None` for any
    /// other type.
    pub enum_name: Option<String>,
}

/// One output and its envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputPlan {
    /// Canonical path.
    pub path: String,
    /// Owner group.
    pub group: String,
    /// Type.
    pub ty: ValueType,
    /// Unit symbol.
    pub unit: String,
    /// Safe state.
    pub safe: SafeValue,
    /// Ramp to safe, per second.
    pub safe_ramp: Option<f64>,
    /// Constant range.
    pub range: Option<(f64, f64)>,
    /// Range action.
    pub on_range: OnRange,
    /// Rate per second.
    pub rate: Option<f64>,
    /// Interlock action.
    pub on_interlock: OnInterlock,
    /// `max_lease`.
    pub max_lease_ns: u64,
    /// `max_manual`.
    pub max_manual_ns: u64,
    /// Writers allowed besides manual ones.
    pub writers: Vec<WriterId>,
    /// Valid enum values.
    pub enum_values: Vec<u64>,
    /// `permit_uncertain`.
    pub permit_uncertain: bool,
    /// `grace`, ns.
    pub grace_ns: u64,
    /// `deadband`, in the output's unit (step 6).
    pub deadband: Option<f64>,
    /// `min_interval`, ns (step 6).
    pub min_interval_ns: u64,
    /// `max_actuations` (step 6).
    pub max_actuations: Option<Budget>,
    /// `verify`, resolved against the readback signal (docs/01 §7,
    /// ADR 0072).
    pub verify: Option<VerifyPlan>,
    /// The output's own `keepalive`, ns: the period its device fallback
    /// is renewed at, in place of the device's `fallback.keepalive`
    /// (docs/01 §8, ADR 0072).
    pub keepalive_ns: Option<u64>,
    /// `pulse`, ns: after a value other than `safe` is written, the
    /// output stage writes `safe` once it has elapsed (ref/06 §2.9,
    /// ADR 0072).
    pub pulse_ns: Option<u64>,
    /// The compiled expressions and `requires_good` of the envelope,
    /// filled by `expr_plan::bind` once the registry exists.
    pub exprs: OutputExprs,
}

/// The readback verification of an output (`verify`, docs/01 §7,
/// ADR 0072).
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyPlan {
    /// The canonical path of the readback input.
    pub readback: String,
    /// The readback's unit to the output's unit.
    pub conversion: Conversion,
    /// The largest difference that confirms, in the output's unit; `None`
    /// is exact equality.
    pub tolerance: Option<f64>,
    /// How long the readback may disagree before the output is
    /// `uncertain(not_confirmed)`, ns.
    pub within_ns: u64,
}

/// One writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterPlan {
    /// Text form (`app:light`, `user:manual:4`, ref/00 §5).
    pub name: String,
    /// The core configuration.
    pub config: WriterConfig,
}

/// A generation plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Hash of the effective manifest (ref/01 §8.3).
    pub manifest_hash: String,
    /// Mode of the manifest.
    pub dev: bool,
    /// Cycle groups, in name order.
    pub groups: Vec<GroupPlan>,
    /// Signals, in path order.
    pub signals: Vec<SignalPlan>,
    /// Alias path to canonical path.
    pub aliases: BTreeMap<String, String>,
    /// Outputs, in path order.
    pub outputs: Vec<OutputPlan>,
    /// Writers, by `WriterId`.
    pub writers: Vec<WriterPlan>,
    /// Every path, canonical or alias, to its `SignalId`.
    pub ids: BTreeMap<String, SignalId>,
    /// Hash of the registry (ADR 0031).
    pub registry_hash: [u8; 32],
    /// Every device, resolved into what a driver's `Planner`/`Driver`
    /// needs (`bridge::device_plan`). Devices whose driver this build does
    /// not know how to run for real stay here regardless: `Shared::new`
    /// and the device dispatcher decide what to do with each one.
    pub devices: Vec<crate::bridge::DevicePlan>,
    /// The `[gpio]` of the `[target]` board (ref/03 §2.3.2), when the
    /// board file is among the generation's and declares one: it
    /// resolves a gpio device's `chip = "auto"` and its named lines.
    pub board_gpio: Option<joker_manifest::board::Gpio>,
    /// Every device action's declared `permit` source, by `(device,
    /// action)` name (ref/01 §5.8.2). An action with no entry here
    /// declared none and is always permitted; one with an entry is gated
    /// by its compiled program in [`Plan::permits`], and denied when
    /// that program is missing (ADR 0060).
    pub permit_sources: BTreeMap<(String, String), String>,
    /// Every declared `permit`, compiled against this plan's own signals
    /// ([`PlanEnv`]) by [`Plan::new`] once the registry is finished. A
    /// permit that does not compile fails [`Plan::new`] with the
    /// expression's own code (ADR 0060), so after a successful plan every
    /// key of [`Plan::permit_sources`] has a program here.
    ///
    /// [`PlanEnv`]: crate::expr_plan::PlanEnv
    pub permits: BTreeMap<(String, String), joker_expr::Program>,
    /// Every `calc`, compiled, in evaluation order (ADR 0068).
    pub calcs: Vec<CalcPlan>,
    /// Every alarm of the manifest, compiled, in name order.
    pub alarms: Vec<AlarmPlan>,
    /// The actions of the composite instances (ADR 0066 §18); their
    /// permits are in [`Plan::permit_sources`] under `(instance,
    /// action)`.
    pub instance_actions: Vec<crate::actions::InstanceAction>,
    /// `[security.roles]` (ref/09 §2.2).
    pub roles: joker_api::server::SocketRoles,
    /// `[telemetry]`: what the historian records and its store policy
    /// (ref/01 §5.17, ADR 0082).
    pub telemetry: crate::historian::TelemetryPlan,
    /// The class instances, composite and implicit; an instance's number
    /// in its method records is its position here (ADR 0084).
    pub instances: Vec<crate::classes::InstancePlan>,
}

/// The borrowed core tables of a plan.
#[derive(Debug)]
pub struct Tables<'p> {
    /// The registry.
    pub registry: OwnedRegistry<'p>,
    /// Output configurations, in `SignalId` order.
    pub outputs: Vec<OutputConfig<'p>>,
    /// Writer table, by `WriterId`.
    pub writers: Vec<WriterConfig>,
}

fn value_type(ty: &SignalType) -> ValueType {
    match ty {
        SignalType::Bool => ValueType::Bool,
        SignalType::Integer => ValueType::I64,
        SignalType::Float | SignalType::Unknown => ValueType::F64,
        SignalType::Enum { .. } => ValueType::Enum,
        SignalType::Text => ValueType::String,
        SignalType::Bytes => ValueType::Bytes,
    }
}

fn core_level(level: &joker_manifest::model::Level) -> Result<Level, PlanError> {
    Level::new(level.priority)
        .ok_or_else(|| PlanError::new(None, format!("level {} out of range", level.priority)))
}

fn clock_requirement(text: &str) -> ClockRequirement {
    match text {
        "synced" => ClockRequirement::Synced,
        "holdover" => ClockRequirement::Holdover,
        _ => ClockRequirement::Any,
    }
}

fn binding_paths(binding: &Binding) -> Vec<String> {
    match binding {
        Binding::Target(t) => vec![t.value.clone()],
        Binding::List(items) => items.iter().map(|t| t.value.clone()).collect(),
        Binding::Signal { signal, .. } => vec![signal.value.clone()],
        Binding::Each(_) => Vec::new(),
    }
}

impl Plan {
    /// Plans a checked manifest. `paths` and `manifest_hash` come from the
    /// same `joker_manifest::check` report, which must have no errors;
    /// `contracts` are the class contracts the context's classes came
    /// from, by reference.
    pub fn new(
        manifest: &Manifest,
        paths: &PathRegistry,
        context: &Context,
        contracts: &BTreeMap<String, joker_jkc::contract::ContractFile>,
        manifest_hash: &str,
    ) -> Result<Plan, PlanError> {
        let mut groups: BTreeMap<String, GroupPlan> = BTreeMap::new();
        for (name, cycle) in &manifest.cycles {
            let period = cycle
                .period
                .map(|p| p.value.as_nanos())
                .ok_or_else(|| PlanError::new(None, format!("group `{name}` has no period")))?;
            groups.insert(
                name.clone(),
                GroupPlan {
                    name: name.clone(),
                    period_ns: period,
                    tasks: Vec::new(),
                    ports: Vec::new(),
                    alarm_after: u32::try_from(cycle.alarm_after.max(1)).unwrap_or(u32::MAX),
                },
            );
        }

        let canonical =
            |path: &str| -> Option<String> { paths.get(path).map(|r| r.canonical.clone()) };
        check_adjustments(manifest, context, paths)?;

        let mut writers = Vec::new();
        let mut app_writer: BTreeMap<String, WriterId> = BTreeMap::new();
        let mut app_group: BTreeMap<String, String> = BTreeMap::new();
        for (name, app) in &manifest.apps {
            let level = match &app.writer {
                Some(w) => core_level(&w.level)?,
                None => Level::new(12).ok_or_else(|| PlanError::new(None, "bad default level"))?,
            };
            let kind = if app.cycles.is_empty() {
                WriterKind::Async
            } else {
                WriterKind::Cycle
            };
            let id = WriterId(writers.len() as u32);
            writers.push(WriterPlan {
                name: format!("app:{name}"),
                config: WriterConfig {
                    kind,
                    level,
                    lease_ns: app
                        .writer
                        .as_ref()
                        .and_then(|w| w.lease)
                        .map(|l| l.value.as_nanos()),
                    requires_clock: clock_requirement(&app.requires_clock),
                },
            });
            app_writer.insert(name.clone(), id);
            if let Some(group) = app.cycles.values().next() {
                app_group.insert(name.clone(), group.value.clone());
            }
        }

        let mut output_writers: BTreeMap<String, Vec<(Level, WriterId, Option<String>)>> =
            BTreeMap::new();
        for (name, app) in &manifest.apps {
            let id = app_writer[name];
            let level = writers[id.index()].config.level;
            for binding in app.bind.values() {
                for path in binding_paths(&binding.value) {
                    let Some(reg) = paths.get(&path) else {
                        continue;
                    };
                    if reg.info.direction == Direction::Output {
                        output_writers
                            .entry(reg.canonical.clone())
                            .or_default()
                            .push((level, id, app_group.get(name).cloned()));
                    }
                }
            }
        }

        let mut signals = Vec::new();
        let mut aliases = BTreeMap::new();
        let mut owner: BTreeMap<String, String> = BTreeMap::new();
        for (path, envelope) in &manifest.outputs {
            let canon = canonical(path).ok_or_else(|| {
                PlanError::new(Some("J0101"), format!("output `{path}` is not a signal"))
            })?;
            let group = match &envelope.group {
                Some(g) => g.value.clone(),
                None => output_writers
                    .get(&canon)
                    .and_then(|list| {
                        list.iter()
                            .filter(|(_, _, g)| g.is_some())
                            .min_by_key(|(level, _, g)| (*level, g.clone()))
                            .and_then(|(_, _, g)| g.clone())
                    })
                    .unwrap_or_else(|| DEFAULT_GROUP.to_owned()),
            };
            owner.insert(canon, group);
        }
        if owner.values().any(|g| g == DEFAULT_GROUP) && !groups.contains_key(DEFAULT_GROUP) {
            groups.insert(
                DEFAULT_GROUP.to_owned(),
                GroupPlan {
                    name: DEFAULT_GROUP.to_owned(),
                    period_ns: DEFAULT_PERIOD_NS,
                    tasks: Vec::new(),
                    ports: Vec::new(),
                    alarm_after: DEFAULT_ALARM_AFTER,
                },
            );
        }

        for (path, reg) in paths {
            if reg.canonical != *path {
                aliases.insert(path.clone(), reg.canonical.clone());
                continue;
            }
            let derived =
                reg.device.is_none() && (path.starts_with("calc.") || path.starts_with("alarm."));
            let direction = match reg.info.direction {
                Direction::Input if derived => CoreDirection::Derived,
                Direction::Input => CoreDirection::Input,
                Direction::Output => CoreDirection::Output,
                Direction::Param => continue,
            };
            let group = match direction {
                CoreDirection::Output => Some(owner.get(path).cloned().ok_or_else(|| {
                    PlanError::new(Some("J0201"), format!("output `{path}` has no envelope"))
                })?),
                _ => None,
            };
            let (variants, enum_name) = match &reg.info.ty {
                SignalType::Enum { name, variants } => (variants.clone(), Some(name.clone())),
                _ => (Vec::new(), None),
            };
            signals.push(SignalPlan {
                path: path.clone(),
                ty: value_type(&reg.info.ty),
                unit: reg
                    .info
                    .unit
                    .as_ref()
                    .map_or_else(|| "1".to_owned(), |u| u.symbol().to_owned()),
                direction,
                group,
                max_age_ns: adjusted_max_age(manifest, paths, path, reg.device.as_deref())
                    .or_else(|| max_age(manifest, context, reg.device.as_deref())),
                device: reg.device.clone(),
                variants,
                enum_name,
            });
        }

        let mut outputs = Vec::new();
        for (path, envelope) in &manifest.outputs {
            let Some(canon) = canonical(path) else {
                continue;
            };
            let Some(signal) = signals.iter().find(|s| s.path == canon) else {
                continue;
            };
            let allowed: Vec<WriterId> = output_writers
                .get(&canon)
                .map(|l| l.iter().map(|(_, id, _)| *id).collect())
                .unwrap_or_default();
            let mut output = output_plan(&canon, signal, envelope, owner[&canon].clone(), allowed)?;
            output.verify = envelope
                .verify
                .as_ref()
                .map(|v| verify_plan(&canon, signal, v, &signals, paths, manifest, context))
                .transpose()?;
            outputs.push(output);
        }
        outputs.sort_by(|a, b| a.path.cmp(&b.path));

        let max_manual = outputs
            .iter()
            .map(|o| o.max_manual_ns)
            .min()
            .unwrap_or(joker_core::output::DEFAULT_MAX_MANUAL_NS);
        for level in MANUAL_LEVELS {
            writers.push(WriterPlan {
                name: format!("user:manual:{level}"),
                config: WriterConfig {
                    kind: WriterKind::Manual,
                    level: Level::new(level).ok_or_else(|| PlanError::new(None, "bad level"))?,
                    lease_ns: Some(max_manual),
                    requires_clock: ClockRequirement::Any,
                },
            });
        }

        for (name, app) in &manifest.apps {
            let bindings: BTreeMap<String, String> = app
                .bind
                .iter()
                .filter_map(|(port, b)| {
                    binding_paths(&b.value)
                        .into_iter()
                        .next()
                        .and_then(|p| canonical(&p))
                        .map(|p| (port.clone(), p))
                })
                .collect();
            for (task, group) in &app.cycles {
                let g = groups.get_mut(&group.value).ok_or_else(|| {
                    PlanError::new(
                        Some("J0280"),
                        format!("group `{}` is not declared", group.value),
                    )
                })?;
                g.tasks.push(TaskPlan {
                    app: name.clone(),
                    task: task.clone(),
                    writer: app_writer[name],
                    bindings: bindings.clone(),
                });
                for path in bindings.values() {
                    if !g.ports.contains(path) {
                        g.ports.push(path.clone());
                    }
                }
            }
        }
        for (name, cycle) in &manifest.cycles {
            let Some(g) = groups.get_mut(name) else {
                continue;
            };
            let order: Vec<String> = cycle.order.iter().map(|o| o.value.clone()).collect();
            let rank = |t: &TaskPlan| -> (usize, String) {
                let full = format!("{}.{}", t.app, t.task);
                let pos = order
                    .iter()
                    .position(|o| *o == full || *o == t.app)
                    .unwrap_or(usize::MAX);
                (pos, full)
            };
            g.tasks.sort_by_key(rank);
        }
        for o in &outputs {
            if let Some(g) = groups.get_mut(&o.group)
                && !g.ports.contains(&o.path)
            {
                g.ports.push(o.path.clone());
            }
        }

        let (devices, device_errors) = crate::bridge::device_plans(manifest, context, paths);
        for e in &device_errors {
            eprintln!("warning: {e}");
        }
        let mut plan = Plan {
            manifest_hash: manifest_hash.to_owned(),
            dev: manifest.project.mode == joker_manifest::Mode::Dev,
            groups: groups.into_values().collect(),
            signals,
            aliases,
            outputs,
            writers,
            ids: BTreeMap::new(),
            registry_hash: [0; 32],
            devices,
            board_gpio: None,
            permit_sources: BTreeMap::new(),
            permits: BTreeMap::new(),
            calcs: Vec::new(),
            alarms: Vec::new(),
            instance_actions: Vec::new(),
            roles: crate::actions::socket_roles(manifest),
            telemetry: crate::historian::TelemetryPlan::default(),
            instances: Vec::new(),
        };
        plan.index()?;
        let queries = crate::classes::declare(&mut plan, manifest, context, contracts)?;
        plan.index()?;
        plan.permit_sources = crate::bridge::permit_sources(manifest, &plan.devices);
        plan.instance_actions = crate::actions::instance_actions(manifest, &plan.devices);
        for a in &plan.instance_actions {
            if let Some(source) = &a.permit {
                plan.permit_sources
                    .insert((a.instance.clone(), a.action.clone()), source.clone());
            }
        }
        crate::expr_plan::bind(&mut plan, manifest, &queries)?;
        representable_outputs(&mut plan)?;
        plan.index()?;
        plan.telemetry = crate::historian::telemetry_plan(manifest, &plan)?;
        crate::classes::compile(&mut plan, manifest, context, contracts)?;
        Ok(plan)
    }

    /// Resolves every path to its `SignalId`, hashes the registry and puts
    /// each group's ports in id order. Ids depend only on the paths, so a
    /// second call after derived signals gain their group keeps them and
    /// only changes the hash.
    fn index(&mut self) -> Result<(), PlanError> {
        let (ids, hash) = {
            let tables = self.tables()?;
            let registry = tables.registry.as_registry();
            let mut ids = BTreeMap::new();
            for s in &self.signals {
                if let Some(id) = registry.resolve(&s.path) {
                    ids.insert(s.path.clone(), id);
                }
            }
            for (alias, target) in &self.aliases {
                if let Some(id) = registry.resolve(target) {
                    ids.insert(alias.clone(), id);
                }
            }
            (ids, registry.hash())
        };
        self.ids = ids;
        self.registry_hash = hash;
        for g in &mut self.groups {
            let ids = &self.ids;
            g.ports.sort_by_key(|p| ids.get(p).copied());
            g.ports.dedup();
        }
        Ok(())
    }

    /// The `SignalId` of a path (canonical or alias).
    pub fn id(&self, path: &str) -> Option<SignalId> {
        self.ids.get(path).copied()
    }

    /// The apps that run cycle tasks in this plan, in name order: the index
    /// of `Shared::app_stopped`.
    pub fn apps(&self) -> Vec<String> {
        let names: std::collections::BTreeSet<&str> = self
            .groups
            .iter()
            .flat_map(|g| g.tasks.iter().map(|t| t.app.as_str()))
            .collect();
        names.into_iter().map(str::to_owned).collect()
    }

    /// The canonical path of a signal.
    pub fn path(&self, id: SignalId) -> Option<&str> {
        self.signals
            .iter()
            .find(|s| self.ids.get(&s.path) == Some(&id))
            .map(|s| s.path.as_str())
    }

    /// The signal plan of a path.
    pub fn signal(&self, path: &str) -> Option<&SignalPlan> {
        let canonical = self.aliases.get(path).map_or(path, String::as_str);
        self.signals.iter().find(|s| s.path == canonical)
    }

    /// The output plan of a path.
    pub fn output(&self, path: &str) -> Option<&OutputPlan> {
        let canonical = self.aliases.get(path).map_or(path, String::as_str);
        self.outputs.iter().find(|o| o.path == canonical)
    }

    /// A writer by its text form.
    pub fn writer(&self, name: &str) -> Option<WriterId> {
        self.writers
            .iter()
            .position(|w| w.name == name)
            .map(|i| WriterId(i as u32))
    }

    /// Builds the borrowed core tables.
    pub fn tables(&self) -> Result<Tables<'_>, PlanError> {
        let groups: Vec<GroupMeta<'_>> = self
            .groups
            .iter()
            .map(|g| GroupMeta {
                name: &g.name,
                period_ns: g.period_ns,
            })
            .collect();
        let signals: Vec<SignalMeta<'_>> = self
            .signals
            .iter()
            .map(|s| SignalMeta {
                path: &s.path,
                ty: s.ty,
                unit: UnitRef(&s.unit),
                direction: s.direction,
                group: s.group.as_deref(),
                group_id: None,
                max_age_ns: s.max_age_ns,
            })
            .collect();
        let aliases: Vec<Alias<'_>> = self.aliases.iter().map(|(a, t)| Alias::new(a, t)).collect();
        let registry = OwnedRegistry::build(signals, aliases, groups)
            .map_err(|e| PlanError::new(None, format!("registry: {e:?}")))?;
        let mut outputs = Vec::with_capacity(self.outputs.len());
        {
            let reg = registry.as_registry();
            for o in &self.outputs {
                let signal = reg
                    .resolve(&o.path)
                    .ok_or_else(|| PlanError::new(None, format!("unknown output `{}`", o.path)))?;
                let group = reg
                    .resolve_group(&o.group)
                    .ok_or_else(|| PlanError::new(None, format!("unknown group `{}`", o.group)))?;
                let mut config = OutputConfig::new(signal, o.ty, group, o.safe);
                config.enum_values = &o.enum_values;
                config.safe_ramp = o.safe_ramp;
                config.range = o.range;
                config.on_range = o.on_range;
                config.rate = o.rate;
                config.on_interlock = o.on_interlock;
                config.max_lease_ns = o.max_lease_ns;
                config.max_manual_ns = o.max_manual_ns;
                config.writers = Writers::Only(&o.writers);
                config.interlock = o.exprs.interlock.as_ref().map(|e| &e.program);
                config.permit_uncertain = o.permit_uncertain;
                config.requires_good = &o.exprs.requires_good;
                config.grace_ns = o.grace_ns;
                config.range_exprs = o
                    .exprs
                    .range
                    .as_ref()
                    .map(|(low, high)| (low.bound(), high.bound()));
                config.reset_when = o.exprs.reset_when.as_ref().map(|e| &e.program);
                config.deadband = o.deadband;
                config.min_interval_ns = o.min_interval_ns;
                config.max_actuations = o.max_actuations;
                outputs.push(config);
            }
        }
        outputs.sort_by_key(|o| o.signal);
        Ok(Tables {
            registry,
            outputs,
            writers: self.writers.iter().map(|w| w.config).collect(),
        })
    }

    /// The group plan of a group id of this plan's registry (groups are in
    /// name order, like the registry's).
    pub fn group(&self, id: GroupId) -> Option<&GroupPlan> {
        self.groups.get(id.index())
    }
}

fn device_poll(manifest: &Manifest, context: &Context, device: Option<&str>) -> Option<Duration> {
    let device = manifest.devices.get(device?)?;
    device.poll.map(|p| p.value).or_else(|| {
        device
            .profile
            .as_ref()
            .and_then(|p| context.profiles.get(&p.value))
            .and_then(|p| p.poll)
    })
}

fn max_age(manifest: &Manifest, context: &Context, device: Option<&str>) -> Option<u64> {
    let poll = device_poll(manifest, context, device)?;
    Some(poll.as_nanos().saturating_mul(3))
}

/// The signal name of `path` within `device`.
fn signal_of<'p>(path: &'p str, device: &str) -> Option<&'p str> {
    path.strip_prefix(device)?.strip_prefix('.')
}

fn adjusted_max_age(
    manifest: &Manifest,
    paths: &PathRegistry,
    path: &str,
    device: Option<&str>,
) -> Option<u64> {
    let device = device?;
    let signal = signal_of(path, device)?;
    let tuning = joker_manifest::adjust::tuning(manifest, paths, device, signal);
    let node = tuning.get("max_age")?;
    node.as_str()
        .and_then(|t| Duration::parse(t).ok())
        .map(Duration::as_nanos)
}

/// Every manifest adjustment of a device signal, `[signals."<path>"]`
/// included, composed with the profile (ADR 0075): one that does not fit
/// stops the plan with its code.
fn check_adjustments(
    manifest: &Manifest,
    context: &Context,
    paths: &PathRegistry,
) -> Result<(), PlanError> {
    for (path, reg) in paths {
        if reg.canonical != *path {
            continue;
        }
        let Some(device) = reg.device.as_deref() else {
            continue;
        };
        let Some(signal) = signal_of(path, device) else {
            continue;
        };
        let tuning = joker_manifest::adjust::tuning(manifest, paths, device, signal);
        if tuning.is_empty() {
            continue;
        }
        let profile = manifest
            .devices
            .get(device)
            .and_then(|d| d.profile.as_ref())
            .and_then(|p| context.profiles.get(&p.value))
            .and_then(|p| p.signals.get(signal));
        let base = match profile {
            Some(info) => info.clone(),
            None => {
                let mut declared = reg.info.clone();
                declared.acquisition = joker_manifest::context::Acquisition::NONE;
                declared.unit = tuning
                    .get("unit")
                    .and_then(Node::as_str)
                    .and_then(|u| Unit::parse(u).ok());
                declared
            }
        };
        if let Err(errors) = joker_manifest::adjust::adjust(&base, profile.is_some(), &tuning)
            && let Some(first) = errors.first()
        {
            return Err(PlanError::new(
                Some(first.code),
                format!("adjustment of `{path}`: {}", first.title),
            ));
        }
    }
    Ok(())
}

/// The output stage writes a value only if its raw type can carry it
/// (ref/06 §2.7, ADR 0075). An output without a `range` gets the range of
/// its raw type, with `on_range = "reject"`, so the envelope rejects what
/// does not fit (`Rejected(Range)`); a `safe`, a constant `range` or an
/// `enum` the raw type cannot carry, or an expression `range` on such an
/// output, is `J0858`; a form the output stage cannot write is `J0807`.
fn representable_outputs(plan: &mut Plan) -> Result<(), PlanError> {
    let not_representable = |path: &str, what: &str| {
        PlanError::new(
            Some("J0858"),
            format!("{what} of `{path}` is not representable in its raw type"),
        )
    };
    let mut points = Vec::new();
    for device in &plan.devices {
        for point in &device.points {
            if point.direction == joker_hal::Direction::Output {
                points.push(point.clone());
            }
        }
    }
    for point in points {
        let conversion = crate::bridge::output_conversion(&point)
            .map_err(|why| PlanError::new(Some("J0807"), why))?;
        let Some(conversion) = conversion else {
            continue;
        };
        let codec = conversion.codec();
        let Some(output) = plan.outputs.iter_mut().find(|o| o.path == point.path) else {
            continue;
        };
        if !codec.enum_fits() {
            return Err(not_representable(&output.path, "an `enum` value"));
        }
        let Some((lo, hi)) = codec.bounds() else {
            continue;
        };
        let fits = |v: f64| lo <= v && v <= hi;
        if let SafeValue::Value(bits) = output.safe
            && output.ty.bits_to_f64(bits).is_some_and(|v| !fits(v))
        {
            return Err(not_representable(&output.path, "the `safe`"));
        }
        if output.exprs.range.is_some() {
            return Err(not_representable(&output.path, "the expression `range`"));
        }
        match output.range {
            Some((a, b)) if !(fits(a) && fits(b)) => {
                return Err(not_representable(&output.path, "the `range`"));
            }
            Some(_) => {}
            None => {
                output.range = Some((lo, hi));
                output.on_range = OnRange::Reject;
            }
        }
    }
    Ok(())
}

/// The default poll of a device without one (ref/01 §5.8).
const DEFAULT_POLL_NS: u64 = 1_000_000_000;

fn verify_plan(
    path: &str,
    output: &SignalPlan,
    verify: &Node,
    signals: &[SignalPlan],
    paths: &PathRegistry,
    manifest: &Manifest,
    context: &Context,
) -> Result<VerifyPlan, PlanError> {
    let invalid =
        |why: String| PlanError::new(Some("J0235"), format!("`verify` of `{path}`: {why}"));
    let table = verify
        .as_table()
        .ok_or_else(|| invalid("not a table".to_owned()))?;
    let readback = table
        .get("readback")
        .and_then(|e| e.node.as_str())
        .ok_or_else(|| invalid("no `readback`".to_owned()))?;
    let canonical = paths
        .get(readback)
        .map(|r| r.canonical.clone())
        .ok_or_else(|| invalid(format!("`{readback}` is not a signal")))?;
    let input = signals
        .iter()
        .find(|s| s.path == canonical && s.direction == CoreDirection::Input)
        .ok_or_else(|| invalid(format!("`{readback}` is not an input")))?;
    let unit = unit_of(output)?;
    let conversion = if input.ty == ValueType::Enum || output.ty == ValueType::Enum {
        if input.variants != output.variants {
            return Err(invalid(format!(
                "`{readback}` does not have the variants of the output"
            )));
        }
        Conversion::IDENTITY
    } else if input.ty == ValueType::Bool || output.ty == ValueType::Bool {
        if input.ty != output.ty {
            return Err(invalid(format!("`{readback}` is not a boolean")));
        }
        Conversion::IDENTITY
    } else {
        let numeric = |ty: ValueType| ty.is_numeric();
        if !numeric(input.ty) || !numeric(output.ty) {
            return Err(invalid(format!("`{readback}` cannot be compared")));
        }
        unit_of(input)?.conversion_to(&unit).map_err(|_| {
            invalid(format!(
                "`{readback}` is not in the dimension of the output"
            ))
        })?
    };
    let tolerance = match table.get("tolerance").map(|e| &e.node.value) {
        None => None,
        Some(Value::String(text)) => {
            Some(quantity_in(text, &unit.difference(), "verify.tolerance")?)
        }
        Some(Value::Integer(i)) if unit.is_dimensionless() => Some(*i as f64),
        Some(Value::Float(f)) if unit.is_dimensionless() => Some(*f),
        Some(_) => return Err(invalid("`tolerance` is not a quantity".to_owned())),
    };
    if tolerance.is_none() && matches!(output.ty, ValueType::F64 | ValueType::F32) {
        return Err(invalid("a float output needs a `tolerance`".to_owned()));
    }
    if tolerance.is_some_and(|t| !t.is_finite() || t < 0.0) {
        return Err(invalid("`tolerance` must not be negative".to_owned()));
    }
    let poll_ns = device_poll(manifest, context, input.device.as_deref())
        .map_or(DEFAULT_POLL_NS, Duration::as_nanos);
    let within_ns = match table.get("within").and_then(|e| e.node.as_str()) {
        Some(text) => Duration::parse(text)
            .map_err(|e| PlanError::new(Some(e.code()), format!("`verify.within` of `{path}`")))?
            .as_nanos(),
        None => poll_ns.saturating_mul(2),
    };
    if within_ns < poll_ns {
        return Err(invalid(format!(
            "`within` is shorter than one poll of `{readback}` ({} ms)",
            poll_ns / 1_000_000
        )));
    }
    Ok(VerifyPlan {
        readback: canonical,
        conversion,
        tolerance,
        within_ns,
    })
}

fn unit_of(signal: &SignalPlan) -> Result<Unit, PlanError> {
    Unit::parse(&signal.unit)
        .map_err(|e| PlanError::new(Some(e.code()), format!("unit of `{}`", signal.path)))
}

pub(crate) fn quantity_in(text: &str, unit: &Unit, what: &str) -> Result<f64, PlanError> {
    match Literal::parse(text) {
        Ok(Literal::Quantity(q)) => q
            .value_in(unit)
            .map_err(|e| PlanError::new(Some(e.code()), format!("{what} `{text}`: {e}"))),
        Ok(Literal::Number(n)) if unit.is_dimensionless() => Ok(n),
        Ok(Literal::Number(_)) => Err(PlanError::new(
            Some("J0151"),
            format!("{what} `{text}` needs a unit"),
        )),
        Err(_) => Err(PlanError::new(
            None,
            format!("{what} `{text}` is not a quantity"),
        )),
    }
}

fn rate_in(text: &str, unit: &Unit, what: &str) -> Result<f64, PlanError> {
    let q = Quantity::parse(text)
        .map_err(|e| PlanError::new(Some(e.code()), format!("{what} `{text}`: {e}")))?;
    let per_second = unit
        .difference()
        .checked_div(&Unit::SECOND)
        .map_err(|e| PlanError::new(Some(e.code()), format!("{what} `{text}`")))?;
    let value = q.value_in(&per_second).map_err(|_| {
        PlanError::new(
            Some("J0202"),
            format!("{what} `{text}` does not fit the output"),
        )
    })?;
    if value <= 0.0 {
        return Err(PlanError::new(
            Some("J0202"),
            format!("{what} `{text}` must be positive"),
        ));
    }
    Ok(value)
}

fn safe_bits(
    safe: &ManifestSafe,
    signal: &SignalPlan,
    unit: &Unit,
) -> Result<SafeValue, PlanError> {
    let wrong = || {
        PlanError::new(
            Some("J0231"),
            format!("`safe` of `{}` has the wrong type", signal.path),
        )
    };
    Ok(match (safe, signal.ty) {
        (ManifestSafe::Relinquish, _) => SafeValue::Relinquish,
        (ManifestSafe::Bool(b), ValueType::Bool) => SafeValue::Value(u64::from(*b)),
        (ManifestSafe::Integer(i), ValueType::I64 | ValueType::U64 | ValueType::Bits) => {
            SafeValue::Value(*i as u64)
        }
        (ManifestSafe::Integer(i), ValueType::F64 | ValueType::F32) if unit.is_dimensionless() => {
            SafeValue::Value((*i as f64).to_bits())
        }
        (ManifestSafe::Float(f), ValueType::F64 | ValueType::F32) if unit.is_dimensionless() => {
            SafeValue::Value(f.to_bits())
        }
        (ManifestSafe::Text(t), ValueType::F64 | ValueType::F32) => {
            SafeValue::Value(quantity_in(t, unit, "safe")?.to_bits())
        }
        (ManifestSafe::Text(t), ValueType::Enum) => {
            let variant = t.rsplit('.').next().unwrap_or(t);
            let index = signal
                .variants
                .iter()
                .position(|v| v == variant)
                .ok_or_else(|| PlanError::new(Some("J0303"), format!("no variant `{t}`")))?;
            SafeValue::Value(index as u64)
        }
        _ => return Err(wrong()),
    })
}

fn output_plan(
    path: &str,
    signal: &SignalPlan,
    envelope: &Envelope,
    group: String,
    writers: Vec<WriterId>,
) -> Result<OutputPlan, PlanError> {
    let unit = unit_of(signal)?;
    let safe = envelope
        .safe
        .as_ref()
        .ok_or_else(|| PlanError::new(Some("J0201"), format!("output `{path}` has no `safe`")))?;
    let safe = safe_bits(&safe.value, signal, &unit)?;
    let range = match &envelope.range {
        Some(r) if r.value.len() != 2 => {
            return Err(PlanError::new(Some("J0230"), "range needs two bounds"));
        }
        Some(r) if r.value.iter().all(|b| Literal::parse(&b.value).is_ok()) => Some((
            quantity_in(&r.value[0].value, &unit, "range")?,
            quantity_in(&r.value[1].value, &unit, "range")?,
        )),
        _ => None,
    };
    let rate = envelope
        .rate
        .as_ref()
        .map(|r| rate_in(&r.value, &unit, "rate"))
        .transpose()?;
    let safe_ramp = envelope
        .safe_ramp
        .as_ref()
        .map(|r| rate_in(&r.value, &unit, "safe_ramp"))
        .transpose()?;
    let on_range = match envelope.on_range.as_str() {
        "reject" => OnRange::Reject,
        "trip" => OnRange::Trip,
        _ => OnRange::Clamp,
    };
    let on_interlock = match envelope.on_interlock.as_str() {
        "hold" => OnInterlock::Hold,
        _ => OnInterlock::Safe,
    };
    let enum_values = (0..signal.variants.len() as u64).collect();
    let deadband = envelope
        .deadband
        .as_ref()
        .map(|d| quantity_in(&d.value, &unit.difference(), "deadband"))
        .transpose()?;
    let max_actuations = envelope.max_actuations.as_ref().map(|b| Budget {
        count: b.value.count,
        window_ns: b.value.window.as_nanos(),
    });
    Ok(OutputPlan {
        path: path.to_owned(),
        group,
        ty: signal.ty,
        unit: signal.unit.clone(),
        safe,
        safe_ramp,
        range,
        on_range,
        rate,
        on_interlock,
        max_lease_ns: duration_ns(envelope.max_lease),
        max_manual_ns: duration_ns(envelope.max_manual),
        writers,
        enum_values,
        permit_uncertain: envelope.permit_uncertain,
        grace_ns: duration_ns(envelope.grace),
        deadband,
        min_interval_ns: duration_ns(envelope.min_interval),
        max_actuations,
        verify: None,
        keepalive_ns: envelope.keepalive.map(|k| k.value.as_nanos()),
        pulse_ns: envelope.pulse.map(|p| p.value.as_nanos()),
        exprs: OutputExprs::default(),
    })
}

fn duration_ns(d: Duration) -> u64 {
    d.as_nanos()
}
