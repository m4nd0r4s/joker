//! A generation plan: the effective manifest and its path registry turned
//! into the tables the core runs on (docs/01 §3.1, §15).
//!
//! The plan owns every string. The cycle thread builds the borrowed core
//! structures (registry, output configurations, writer table) from it once
//! per generation, so nothing on the tick path allocates.

use std::collections::BTreeMap;
use std::fmt;

use joker_core::output::{ClockRequirement, OnInterlock, OnRange, SafeValue, Writers};
use joker_core::registry::{Alias, Direction as CoreDirection, GroupMeta, OwnedRegistry, UnitRef};
use joker_core::{
    GroupId, Level, OutputConfig, SignalId, SignalMeta, ValueType, WriterConfig, WriterId,
    WriterKind,
};
use joker_manifest::check::PathRegistry;
use joker_manifest::context::{Context, Direction, SignalType};
use joker_manifest::model::{Binding, Envelope, Manifest, SafeValue as ManifestSafe};
use joker_units::{Duration, Literal, Quantity, Unit};

/// The implicit group of outputs no cycle task writes (docs/16 A11).
pub const DEFAULT_GROUP: &str = "default";
/// The period of the implicit `default` group.
pub const DEFAULT_PERIOD_NS: u64 = 100_000_000;
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
    fn new(code: Option<&'static str>, message: impl Into<String>) -> Self {
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
    /// same `joker_manifest::check` report, which must have no errors.
    pub fn new(
        manifest: &Manifest,
        paths: &PathRegistry,
        context: &Context,
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
                },
            );
        }

        let canonical =
            |path: &str| -> Option<String> { paths.get(path).map(|r| r.canonical.clone()) };

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
                },
            );
        }

        for (path, reg) in paths {
            if reg.canonical != *path {
                aliases.insert(path.clone(), reg.canonical.clone());
                continue;
            }
            let direction = match reg.info.direction {
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
            let variants = match &reg.info.ty {
                SignalType::Enum { variants, .. } => variants.clone(),
                _ => Vec::new(),
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
                max_age_ns: max_age(manifest, context, reg.device.as_deref()),
                device: reg.device.clone(),
                variants,
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
            outputs.push(output_plan(
                &canon,
                signal,
                envelope,
                owner[&canon].clone(),
                allowed,
            )?);
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

        let (devices, device_errors) = crate::bridge::device_plans(manifest, context);
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
        };
        let (ids, hash) = {
            let tables = plan.tables()?;
            let registry = tables.registry.as_registry();
            let mut ids = BTreeMap::new();
            for s in &plan.signals {
                if let Some(id) = registry.resolve(&s.path) {
                    ids.insert(s.path.clone(), id);
                }
            }
            for (alias, target) in &plan.aliases {
                if let Some(id) = registry.resolve(target) {
                    ids.insert(alias.clone(), id);
                }
            }
            (ids, registry.hash())
        };
        plan.ids = ids;
        plan.registry_hash = hash;
        for g in &mut plan.groups {
            let ids = &plan.ids;
            g.ports.sort_by_key(|p| ids.get(p).copied());
            g.ports.dedup();
        }
        Ok(plan)
    }

    /// The `SignalId` of a path (canonical or alias).
    pub fn id(&self, path: &str) -> Option<SignalId> {
        self.ids.get(path).copied()
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

fn max_age(manifest: &Manifest, context: &Context, device: Option<&str>) -> Option<u64> {
    let device = manifest.devices.get(device?)?;
    let poll = device.poll.map(|p| p.value).or_else(|| {
        device
            .profile
            .as_ref()
            .and_then(|p| context.profiles.get(&p.value))
            .and_then(|p| p.poll)
    })?;
    Some(poll.as_nanos().saturating_mul(3))
}

fn unit_of(signal: &SignalPlan) -> Result<Unit, PlanError> {
    Unit::parse(&signal.unit)
        .map_err(|e| PlanError::new(Some(e.code()), format!("unit of `{}`", signal.path)))
}

fn quantity_in(text: &str, unit: &Unit, what: &str) -> Result<f64, PlanError> {
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
            format!("{what} `{text}` is an expression; M0 supports constants only"),
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
        Some(r) if r.value.len() == 2 => Some((
            quantity_in(&r.value[0].value, &unit, "range")?,
            quantity_in(&r.value[1].value, &unit, "range")?,
        )),
        Some(_) => return Err(PlanError::new(Some("J0230"), "range needs two bounds")),
        None => None,
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
    if envelope.interlock.is_some() || !envelope.requires_good.is_empty() {
        return Err(PlanError::new(
            None,
            format!("output `{path}`: `interlock` and `requires_good` are not in the M0 core yet"),
        ));
    }
    let enum_values = (0..signal.variants.len() as u64).collect();
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
    })
}

fn duration_ns(d: Duration) -> u64 {
    d.as_nanos()
}
