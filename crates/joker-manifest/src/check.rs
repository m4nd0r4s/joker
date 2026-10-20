//! Semantic checks on the decoded manifest: steps 4–9 of ref/01 §8.1 for the
//! M0 subset (references, the path registry, bindings and units, envelopes,
//! writers and cycle groups).

use std::collections::BTreeMap;
use std::path::Path;

use joker_units::{Duration, ErrorKind, Literal, QuantityKind, Unit};

use crate::context::{AppInterface, Context, Direction, PortKind, SignalInfo, SignalType};
use crate::diag::{Diagnostic, Diagnostics};
use crate::grammar;
use crate::layers::FileSystem;
use crate::model::{App, Binding, Envelope, Kernel, Manifest, Mode, SafeValue, Spanned};
use crate::source::{Sources, Span};
use crate::value::{Node, Value};

/// Keys a manifest may set on a signal that comes from a profile (ref/01
/// §5.8.1, ref/03 §2.2.4).
const PROFILE_TUNING: [&str; 11] = [
    "alias",
    "max_age",
    "limits",
    "stuck_detect",
    "deadband",
    "debounce",
    "scale",
    "offset",
    "unit",
    "poll",
    "record",
];

/// Keys only valid on signals declared by drivers without a profile.
const DECLARATION_ONLY: [&str; 2] = ["type", "direction"];

/// Prefixes owned by the runtime (ref/01 §3.8).
const RESERVED_PREFIXES: [&str; 6] = ["calc.", "alarm.", "app.", "board.", "joker.", "bridge."];

/// Implicit cycle groups (ref/01 §5.11).
const IMPLICIT_GROUPS: [&str; 2] = ["default", "calc"];

/// The manifest limits of ref/01 §11.3.
const MAX_DEVICES: usize = 1024;
const MAX_APPS: usize = 50;
const MAX_OUTPUTS: usize = 16_384;

/// One registered path.
#[derive(Debug, Clone)]
pub struct Registered {
    /// The signal.
    pub info: SignalInfo,
    /// The canonical path, for aliases.
    pub canonical: String,
    /// The device that owns it.
    pub device: Option<String>,
    /// Where it is declared, when the manifest declares it.
    pub span: Option<Span>,
}

/// Every path of the site: canonical paths, aliases and `calc` results.
pub type PathRegistry = BTreeMap<String, Registered>;

struct Checker<'a> {
    manifest: &'a Manifest,
    context: &'a Context,
    fs: &'a dyn FileSystem,
    sources: &'a Sources,
    root: &'a Path,
    diags: &'a mut Diagnostics,
    paths: PathRegistry,
}

/// Runs the semantic checks and returns the path registry.
pub fn check(
    manifest: &Manifest,
    context: &Context,
    fs: &dyn FileSystem,
    sources: &Sources,
    root: &Path,
    diags: &mut Diagnostics,
) -> PathRegistry {
    let mut checker = Checker {
        manifest,
        context,
        fs,
        sources,
        root,
        diags,
        paths: PathRegistry::new(),
    };
    checker.limits();
    checker.project();
    checker.target();
    checker.devices();
    checker.pending();
    checker.outputs();
    checker.cycles();
    checker.apps();
    checker.paths
}

/// A short description of what a unit measures, for messages.
pub fn describe(unit: &Unit) -> String {
    let what = match unit.kind() {
        Some(QuantityKind::ActivePower) => "a power",
        Some(QuantityKind::ReactivePower) => "a reactive power",
        Some(QuantityKind::ApparentPower) => "an apparent power",
        Some(QuantityKind::Energy) => "an energy",
        Some(QuantityKind::ReactiveEnergy) => "a reactive energy",
        Some(QuantityKind::Temperature) => "a temperature",
        Some(QuantityKind::TemperatureDifference) => "a temperature difference",
        None if unit.symbol() == "%" => "a fraction",
        None if unit.is_dimensionless() => "a number",
        None if unit.is_time() => "a duration",
        None => return format!("a quantity in {}", unit.symbol()),
    };
    format!("{what} ({})", unit.symbol())
}

impl Checker<'_> {
    fn push(&mut self, diagnostic: Diagnostic) {
        self.diags.push(diagnostic);
    }

    fn limits(&mut self) {
        let m = self.manifest;
        for (count, max, what) in [
            (m.devices.len(), MAX_DEVICES, "devices"),
            (m.apps.len(), MAX_APPS, "apps"),
            (m.outputs.len(), MAX_OUTPUTS, "outputs"),
        ] {
            if count > max {
                self.push(Diagnostic::new(
                    "J0149",
                    format!("manifest limit exceeded: {count} {what}, at most {max}"),
                ));
            }
        }
    }

    fn project(&mut self) {
        if let Some(joker) = &self.manifest.project.joker
            && joker.value > self.context.runtime
        {
            let runtime = self.context.runtime;
            self.push(
                Diagnostic::new("J0147", "the manifest needs a newer runtime")
                    .at(joker.span)
                    .label(format!(
                        "this runtime is {}.{}",
                        runtime.major, runtime.minor
                    )),
            );
        }
    }

    fn target(&mut self) {
        let prod = self.manifest.project.mode == Mode::Prod;
        let Some(target) = &self.manifest.target else {
            if prod {
                self.push(Diagnostic::new(
                    "J0142",
                    "missing required key `[target] board` in `mode = \"prod\"`",
                ));
            }
            return;
        };
        let Some(board) = &target.board else {
            if prod {
                self.push(
                    Diagnostic::new(
                        "J0142",
                        "missing required key `[target] board` in `mode = \"prod\"`",
                    )
                    .at(target.span),
                );
            }
            return;
        };
        let Some(info) = self.context.boards.get(&board.value.name) else {
            let mut d = Diagnostic::new("J0172", format!("board `{}` not found", board.value.name))
                .at(board.span);
            if let Some(s) = grammar::suggest(
                &board.value.name,
                self.context.boards.keys().map(String::as_str),
            ) {
                d = d.help(format!("did you mean `{s}`?"));
            }
            self.push(d);
            return;
        };
        if let Some(major) = board.value.major
            && !info.majors.contains(&major)
        {
            self.push(
                Diagnostic::new(
                    "J0172",
                    format!("board `{}@{major}` not found", board.value.name),
                )
                .at(board.span),
            );
        }
        if let Some(arch) = &target.arch
            && arch.value != info.arch
        {
            self.push(
                Diagnostic::new("J0145", "`arch` does not match the board")
                    .at(arch.span)
                    .label(format!("`{}` is `{}`", board.value.name, info.arch)),
            );
        }
    }

    fn devices(&mut self) {
        let manifest = self.manifest;
        let mut addresses: BTreeMap<(Option<String>, String), (String, Span)> = BTreeMap::new();
        for (name, device) in &manifest.devices {
            let Some(driver_name) = &device.driver else {
                continue;
            };
            let bare = driver_name
                .value
                .strip_prefix("w:")
                .or_else(|| driver_name.value.strip_prefix("x:"))
                .unwrap_or(&driver_name.value);
            let Some(driver) = self.context.drivers.get(bare) else {
                let mut d =
                    Diagnostic::new("J0170", format!("unknown driver `{}`", driver_name.value))
                        .at(driver_name.span);
                if let Some(s) =
                    grammar::suggest(bare, self.context.drivers.keys().map(String::as_str))
                {
                    d = d.help(format!("did you mean `{s}`?"));
                }
                self.push(d);
                continue;
            };
            match &device.bus {
                None if driver.bus_required => self.push(
                    Diagnostic::new(
                        "J0142",
                        format!("missing required key `bus` for driver `{bare}`"),
                    )
                    .at(device.key),
                ),
                None => {}
                Some(bus) => match manifest.buses.get(&bus.value) {
                    None => self.push(
                        Diagnostic::new("J0171", format!("bus `{}` does not exist", bus.value))
                            .at(bus.span),
                    ),
                    Some(declared) if !driver.buses.contains(&declared.kind.value) => self.push(
                        Diagnostic::new("J0171", "bus of an incompatible kind")
                            .at(bus.span)
                            .label(format!(
                                "`{bare}` does not use a `{}` bus",
                                declared.kind.value.as_str()
                            )),
                    ),
                    Some(_) => {}
                },
            }
            let profile = match &device.profile {
                Some(reference) => match self.context.profiles.get(&reference.value) {
                    Some(p) => Some(p),
                    None => {
                        self.push(
                            Diagnostic::new(
                                "J0172",
                                format!("profile `{}` not found", reference.value),
                            )
                            .at(reference.span),
                        );
                        None
                    }
                },
                None => {
                    if driver.profile_required {
                        self.push(
                            Diagnostic::new(
                                "J0142",
                                format!("missing required key `profile` for driver `{bare}`"),
                            )
                            .at(device.key),
                        );
                    }
                    None
                }
            };
            for (key, node) in &device.config {
                if driver.config.contains(key) {
                    continue;
                }
                let elsewhere = self
                    .context
                    .drivers
                    .values()
                    .any(|d| d.config.contains(key));
                let d = if elsewhere {
                    Diagnostic::new("J0173", format!("driver `{bare}` does not accept `{key}`"))
                        .at(node.span)
                } else {
                    let mut d =
                        Diagnostic::new("J0140", format!("unknown key `{key}` in a device"))
                            .at(node.span);
                    let known = driver.config.iter().map(String::as_str).chain([
                        "driver", "bus", "profile", "poll", "timeout", "retries", "align",
                        "discover", "fallback", "signals", "actions",
                    ]);
                    if let Some(s) = grammar::suggest(key, known) {
                        d = d.help(format!("did you mean `{s}`?"));
                    }
                    d
                };
                self.push(d);
            }
            if let Some(align) = &device.align
                && !manifest.devices.contains_key(&align.value)
            {
                self.push(
                    Diagnostic::new(
                        "J0157",
                        format!("`align` names unknown device `{}`", align.value),
                    )
                    .at(align.span),
                );
            }
            if let Some(discover) = &device.discover
                && discover.value
                && !driver.discover
            {
                self.push(
                    Diagnostic::new("J0177", format!("driver `{bare}` has no discovery"))
                        .at(discover.span),
                );
            }
            if let (Some(configure), Some(p)) = (&device.fallback_configure, profile)
                && configure.value
                && !p.fallback
            {
                self.push(
                    Diagnostic::new(
                        "J0275",
                        "`fallback.configure = true` with a profile without `[fallback]`",
                    )
                    .at(configure.span),
                );
            }
            let address: Vec<String> = ["address", "unit_id", "node_id", "line"]
                .iter()
                .filter_map(|k| device.config.get(*k).map(|n| format!("{k}={}", render(n))))
                .collect();
            if !address.is_empty() {
                let key = (
                    device.bus.as_ref().map(|b| b.value.clone()),
                    address.join(","),
                );
                if let Some((other, _)) = addresses.get(&key) {
                    self.push(
                        Diagnostic::new(
                            "J0178",
                            format!("devices `{other}` and `{name}` share an address"),
                        )
                        .at(device.key),
                    );
                } else {
                    addresses.insert(key, (name.clone(), device.key));
                }
            }
            self.device_signals(name, device, driver, profile);
        }
    }

    fn device_signals(
        &mut self,
        name: &str,
        device: &crate::model::Device,
        driver: &crate::context::DriverInfo,
        profile: Option<&crate::context::ProfileInfo>,
    ) {
        if let Some(profile) = profile {
            for (signal, info) in &profile.signals {
                self.register(&format!("{name}.{signal}"), info.clone(), name, None);
            }
        }
        for (signal, tuning) in &device.signals {
            let path = format!("{name}.{signal}");
            let mut info = match profile {
                Some(p) => match p.signals.get(signal) {
                    Some(info) => Some(info.clone()),
                    None => {
                        let mut d = Diagnostic::new("J0101", format!("unknown signal `{path}`"))
                            .at(tuning.span);
                        if let Some(s) =
                            grammar::suggest(signal, p.signals.keys().map(String::as_str))
                        {
                            d = d.help(format!("did you mean `{name}.{s}`?"));
                        }
                        self.push(d);
                        None
                    }
                },
                None => None,
            };
            let declared = profile.is_none();
            let mut declared_info: Option<SignalInfo> = None;
            let mut direction = Direction::Input;
            let mut declared_type: Option<SignalType> = None;
            let mut unit: Option<Spanned<Unit>> = None;
            let mut aliases: Vec<Spanned<String>> = Vec::new();
            for (key, node) in &tuning.value {
                let key = key.as_str();
                if key.starts_with("x_") {
                    continue;
                }
                if matches!(key, "kind" | "block") {
                    self.push(
                        Diagnostic::new(
                            "J0290",
                            format!("`{key}` is reserved for a later version"),
                        )
                        .at(node.span),
                    );
                    continue;
                }
                let address_key = driver.address.get(key);
                if !declared && (DECLARATION_ONLY.contains(&key) || address_key.is_some()) {
                    self.push(
                        Diagnostic::new(
                            "J0176",
                            format!("`{key}` cannot be set on a profile signal"),
                        )
                        .at(node.span)
                        .label("the profile defines the type, direction and address"),
                    );
                    continue;
                }
                if !PROFILE_TUNING.contains(&key)
                    && !DECLARATION_ONLY.contains(&key)
                    && address_key.is_none()
                {
                    let mut d =
                        Diagnostic::new("J0140", format!("unknown key `{key}` in a signal"))
                            .at(node.span);
                    let known = PROFILE_TUNING
                        .iter()
                        .chain(DECLARATION_ONLY.iter())
                        .copied()
                        .chain(driver.address.keys().map(String::as_str));
                    if let Some(s) = grammar::suggest(key, known) {
                        d = d.help(format!("did you mean `{s}`?"));
                    }
                    self.push(d);
                    continue;
                }
                match key {
                    "direction" => match node.as_str() {
                        Some("input") => direction = Direction::Input,
                        Some("output") => direction = Direction::Output,
                        Some("param") => direction = Direction::Param,
                        _ => self.push(
                            Diagnostic::new("J0144", "value outside the allowed set")
                                .at(node.span)
                                .label("expected one of `input`, `output`, `param`"),
                        ),
                    },
                    "type" => match node.as_str().and_then(signal_type) {
                        Some(ty) => declared_type = Some(ty),
                        None => self.push(
                            Diagnostic::new("J0144", "unknown signal type")
                                .at(node.span)
                                .label("expected a type of ref/00 §6"),
                        ),
                    },
                    "unit" => match node.as_str().map(Unit::parse) {
                        Some(Ok(u)) => unit = Some(Spanned::new(u, node.span)),
                        Some(Err(e)) => {
                            self.push(Diagnostic::new(e.code(), e.kind.message()).at(node.span))
                        }
                        None => self.push(
                            Diagnostic::new("J0143", "wrong value type")
                                .at(node.span)
                                .label("expected a unit string"),
                        ),
                    },
                    "alias" => match &node.value {
                        Value::String(s) => aliases.push(Spanned::new(s.clone(), node.span)),
                        Value::Array(items) => aliases.extend(items.iter().filter_map(|i| {
                            i.as_str().map(|s| Spanned::new(s.to_owned(), i.span))
                        })),
                        _ => self.push(
                            Diagnostic::new("J0143", "wrong value type")
                                .at(node.span)
                                .label("expected a path or a list of paths"),
                        ),
                    },
                    "max_age" | "stuck_detect" | "debounce" => self.duration_value(node),
                    "poll" if node.as_str() != Some("on_demand") => self.duration_value(node),
                    _ => {}
                }
                if let Some(Some(implied)) = address_key {
                    declared_info = Some(implied.clone());
                }
            }
            if declared {
                let mut base =
                    declared_info.unwrap_or(SignalInfo::new(SignalType::Bool, Direction::Input));
                if let Some(ty) = declared_type {
                    base.ty = ty;
                }
                if tuning.value.contains_key("direction") {
                    base.direction = direction;
                }
                if let Some(u) = &unit {
                    base.unit = Some(u.value);
                }
                if base.ty.is_numeric() && base.unit.is_none() {
                    self.push(
                        Diagnostic::new("J0142", format!("numeric signal `{path}` without `unit`"))
                            .at(tuning.span),
                    );
                }
                info = Some(base);
            } else if let (Some(info), Some(u)) = (info.as_mut(), &unit) {
                match info.unit {
                    Some(profile_unit) if profile_unit.dimension() != u.value.dimension() => {
                        self.push(
                            Diagnostic::new("J0107", "incompatible unit (dimension)")
                                .at(u.span)
                                .label(format!("the profile declares {}", describe(&profile_unit))),
                        );
                    }
                    _ => info.unit = Some(u.value),
                }
            }
            if let Some(info) = info {
                self.register(&path, info.clone(), name, Some(tuning.span));
                for alias in aliases {
                    if RESERVED_PREFIXES.iter().any(|p| alias.value.starts_with(p)) {
                        self.push(
                            Diagnostic::new(
                                "J0141",
                                format!("alias `{}` uses a reserved prefix", alias.value),
                            )
                            .at(alias.span),
                        );
                    } else if !grammar::is_path(&alias.value) {
                        self.push(Diagnostic::new("J0153", "invalid path").at(alias.span));
                    } else if let Some(existing) = self.paths.get(&alias.value) {
                        let canonical = existing.canonical.clone();
                        self.push(
                            Diagnostic::new(
                                "J0174",
                                format!("alias `{}` collides with `{canonical}`", alias.value),
                            )
                            .at(alias.span),
                        );
                    } else {
                        self.paths.insert(
                            alias.value.clone(),
                            Registered {
                                info: info.clone(),
                                canonical: path.clone(),
                                device: Some(name.to_owned()),
                                span: Some(alias.span),
                            },
                        );
                    }
                }
            }
        }
    }

    fn register(&mut self, path: &str, info: SignalInfo, device: &str, span: Option<Span>) {
        self.paths.insert(
            path.to_owned(),
            Registered {
                info,
                canonical: path.to_owned(),
                device: Some(device.to_owned()),
                span,
            },
        );
    }

    fn duration_value(&mut self, node: &Node) {
        match &node.value {
            Value::String(s) => {
                if Duration::parse(s).is_err() {
                    self.push(Diagnostic::new("J0152", "invalid duration").at(node.span));
                }
            }
            _ => self.push(
                Diagnostic::new(
                    "J0151",
                    "number without a unit where a duration is required",
                )
                .at(node.span),
            ),
        }
    }

    fn pending(&mut self) {
        if let Some(calc) = self.manifest.pending.get("calc").and_then(Node::as_table) {
            for (name, entry) in calc {
                self.paths.insert(
                    format!("calc.{name}"),
                    Registered {
                        info: SignalInfo::new(SignalType::Unknown, Direction::Input),
                        canonical: format!("calc.{name}"),
                        device: None,
                        span: Some(entry.key),
                    },
                );
            }
        }
    }

    fn unknown_path(&self, path: &str, span: Span) -> Diagnostic {
        let mut d = Diagnostic::new("J0101", format!("unknown signal `{path}`")).at(span);
        if let Some(s) = grammar::suggest(path, self.paths.keys().map(String::as_str)) {
            d = d.help(format!("did you mean `{s}`?"));
        }
        d
    }

    fn group_exists(&self, group: &str) -> bool {
        self.manifest.cycles.contains_key(group) || IMPLICIT_GROUPS.contains(&group)
    }

    fn outputs(&mut self) {
        let manifest = self.manifest;
        let mut owner: BTreeMap<String, String> = BTreeMap::new();
        for (key, envelope) in &manifest.outputs {
            let targets: Vec<String> = if grammar::is_wildcard(key) {
                let matched: Vec<String> = self
                    .paths
                    .iter()
                    .filter(|(p, r)| {
                        r.info.direction == Direction::Output
                            && **p == r.canonical
                            && grammar::matches(key, p)
                    })
                    .map(|(p, _)| p.clone())
                    .collect();
                if matched.is_empty() {
                    self.push(
                        Diagnostic::new("J0155", format!("selector `{key}` matches no output"))
                            .at(envelope.key),
                    );
                }
                matched
            } else {
                match self.paths.get(key) {
                    None => {
                        let d = self.unknown_path(key, envelope.key);
                        self.push(d);
                        continue;
                    }
                    Some(r) if r.info.direction != Direction::Output => {
                        self.push(
                            Diagnostic::new(
                                "J0234",
                                format!("`{key}` is not an output of this node"),
                            )
                            .at(envelope.key),
                        );
                        continue;
                    }
                    Some(r) => vec![r.canonical.clone()],
                }
            };
            for target in targets {
                if let Some(first) = owner.get(&target) {
                    self.push(
                        Diagnostic::new("J0215", format!("output `{target}` has two envelopes"))
                            .at(envelope.key)
                            .label(format!("also declared by `{first}`")),
                    );
                    continue;
                }
                owner.insert(target.clone(), key.clone());
                if let Some(registered) = self.paths.get(&target).cloned() {
                    self.envelope(&target, &registered, envelope);
                }
            }
        }
        let missing: Vec<(String, Option<Span>, Option<String>)> = self
            .paths
            .iter()
            .filter(|(p, r)| {
                r.info.direction == Direction::Output
                    && **p == r.canonical
                    && !owner.contains_key(*p)
            })
            .map(|(p, r)| (p.clone(), r.span, r.device.clone()))
            .collect();
        for (path, span, device) in missing {
            let span =
                span.or_else(|| device.and_then(|d| manifest.devices.get(&d)).map(|d| d.key));
            let mut d = Diagnostic::new("J0201", format!("output `{path}` has no envelope")).help(
                format!("declare `[outputs.\"{path}\"]` with its `safe` value"),
            );
            if let Some(span) = span {
                d = d.at(span);
            }
            self.push(d);
        }
    }

    fn envelope(&mut self, path: &str, registered: &Registered, envelope: &Envelope) {
        let info = &registered.info;
        let discrete = matches!(info.ty, SignalType::Bool | SignalType::Enum { .. });
        let safe_value = match &envelope.safe {
            None => {
                self.push(
                    Diagnostic::new("J0201", format!("output `{path}` has no `safe`"))
                        .at(envelope.key)
                        .help("every output declares its safe state (ADR 0006)"),
                );
                None
            }
            Some(safe) => self.safe(path, info, registered, safe),
        };
        let mut bounds: Option<(f64, f64)> = None;
        if let Some(range) = &envelope.range {
            if discrete {
                self.push(
                    Diagnostic::new("J0233", "`range` does not apply to this output")
                        .at(range.span)
                        .label(format!("`{path}` is not numeric")),
                );
            } else {
                let values: Vec<Option<f64>> = range
                    .value
                    .iter()
                    .map(|bound| self.constant(path, info, bound, false))
                    .collect();
                if let [Some(lo), Some(hi)] = values[..] {
                    if lo >= hi {
                        self.push(
                            Diagnostic::new("J0230", "`range` minimum is not below the maximum")
                                .at(range.span),
                        );
                    } else {
                        bounds = Some((lo, hi));
                    }
                }
            }
        }
        if let (Some(value), Some((lo, hi)), Some(safe)) = (safe_value, bounds, &envelope.safe)
            && !(lo..=hi).contains(&value)
        {
            self.push(
                Diagnostic::new("J0203", "`safe` outside the `range`")
                    .at(safe.span)
                    .label("the safe value must lie inside the constant range"),
            );
        }
        for (rate, key) in [(&envelope.rate, "rate"), (&envelope.safe_ramp, "safe_ramp")] {
            let Some(rate) = rate else {
                continue;
            };
            if discrete {
                self.push(
                    Diagnostic::new("J0233", format!("`{key}` does not apply to this output"))
                        .at(rate.span)
                        .label(format!("`{path}` is not numeric")),
                );
                continue;
            }
            self.rate(path, info, rate, key);
        }
        if let Some(deadband) = &envelope.deadband {
            if discrete {
                self.push(
                    Diagnostic::new("J0233", "`deadband` does not apply to this output")
                        .at(deadband.span)
                        .label(format!("`{path}` is not numeric")),
                );
            } else {
                self.constant(path, info, deadband, true);
            }
        }
        if let Some(pulse) = &envelope.pulse
            && info.ty != SignalType::Bool
        {
            self.push(
                Diagnostic::new("J0233", "`pulse` only applies to boolean outputs").at(pulse.span),
            );
        }
        if let Some(group) = &envelope.group
            && !self.group_exists(&group.value)
        {
            self.push(
                Diagnostic::new("J0237", format!("group `{}` does not exist", group.value))
                    .at(group.span),
            );
        }
        for required in &envelope.requires_good {
            if grammar::is_wildcard(&required.value) {
                if !self
                    .paths
                    .keys()
                    .any(|p| grammar::matches(&required.value, p))
                {
                    self.push(
                        Diagnostic::new(
                            "J0155",
                            format!("selector `{}` matches nothing", required.value),
                        )
                        .at(required.span),
                    );
                }
            } else if !self.paths.contains_key(&required.value) {
                let d = self.unknown_path(&required.value, required.span);
                self.push(d);
            }
        }
        if let Some(verify) = envelope.verify.as_ref().and_then(Node::as_table)
            && let Some(readback) = verify.get("readback")
            && let Some(text) = readback.node.as_str()
        {
            match self.paths.get(text) {
                Some(r) if r.info.direction == Direction::Input => {}
                _ => self.push(
                    Diagnostic::new("J0235", "invalid `verify`")
                        .at(readback.node.span)
                        .label("`readback` must be an input"),
                ),
            }
        }
    }

    fn safe(
        &mut self,
        path: &str,
        info: &SignalInfo,
        registered: &Registered,
        safe: &Spanned<SafeValue>,
    ) -> Option<f64> {
        let mismatch = |this: &mut Self, expected: &str| {
            this.push(
                Diagnostic::new("J0231", "`safe` of the wrong type")
                    .at(safe.span)
                    .label(format!("`{path}` expects {expected}")),
            );
        };
        match (&info.ty, &safe.value) {
            (_, SafeValue::Relinquish) => {
                let supports = registered
                    .device
                    .as_ref()
                    .and_then(|d| self.manifest.devices.get(d))
                    .and_then(|d| d.driver.as_ref())
                    .and_then(|d| self.context.drivers.get(&d.value))
                    .is_some_and(|d| d.relinquish);
                if !supports {
                    self.push(
                        Diagnostic::new(
                            "J0207",
                            "`safe = \"relinquish\"` on a driver without support",
                        )
                        .at(safe.span),
                    );
                }
                None
            }
            (SignalType::Unknown, _) => None,
            (SignalType::Bool, SafeValue::Bool(_)) => None,
            (SignalType::Bool, _) => {
                mismatch(self, "a boolean");
                None
            }
            (SignalType::Integer, SafeValue::Integer(i)) => Some(*i as f64),
            (SignalType::Integer | SignalType::Float, SafeValue::Text(text))
                if looks_like_expression(text) =>
            {
                self.push(
                    Diagnostic::new("J0232", "`safe` must be a constant")
                        .at(safe.span)
                        .label("an expression is not a safe state"),
                );
                None
            }
            (SignalType::Float | SignalType::Integer, SafeValue::Text(text)) => {
                self.constant(path, info, &Spanned::new(text.clone(), safe.span), false)
            }
            (SignalType::Float, SafeValue::Float(_) | SafeValue::Integer(_)) => {
                let value = match safe.value {
                    SafeValue::Float(f) => f,
                    SafeValue::Integer(i) => i as f64,
                    _ => 0.0,
                };
                match info.unit {
                    Some(unit) if !unit.is_dimensionless() => {
                        self.push(
                            Diagnostic::new("J0151", "number without a unit in a quantity")
                                .at(safe.span)
                                .label(format!("`{path}` is in {}", unit.symbol()))
                                .help(format!(
                                    "write `safe = \"{} {}\"`",
                                    trim_float(value),
                                    unit.symbol()
                                )),
                        );
                        None
                    }
                    _ => Some(value),
                }
            }
            (SignalType::Integer | SignalType::Float, _) => {
                mismatch(self, "a number");
                None
            }
            (SignalType::Enum { name, variants }, SafeValue::Text(text)) => {
                let variant = text.strip_prefix(&format!("{name}.")).unwrap_or(text);
                if !variants.iter().any(|v| v == variant) {
                    self.push(
                        Diagnostic::new(
                            "J0303",
                            format!("enum `{name}` has no variant `{variant}`"),
                        )
                        .at(safe.span),
                    );
                }
                None
            }
            (SignalType::Enum { .. }, _) => {
                mismatch(self, "an enum variant");
                None
            }
            (SignalType::Text, SafeValue::Text(text)) => {
                if text.len() > 64 {
                    mismatch(self, "a string of at most 64 bytes");
                }
                None
            }
            (SignalType::Bytes, SafeValue::Text(text)) if text.starts_with("base64:") => None,
            (SignalType::Text, _) => {
                mismatch(self, "a string");
                None
            }
            (SignalType::Bytes, _) => {
                mismatch(self, "\"base64:…\" bytes");
                None
            }
        }
    }

    fn constant(
        &mut self,
        path: &str,
        info: &SignalInfo,
        text: &Spanned<String>,
        difference: bool,
    ) -> Option<f64> {
        let literal = match Literal::parse(&text.value) {
            Ok(literal) => literal,
            Err(_) if looks_like_expression(&text.value) => return None,
            Err(e) => {
                self.push(
                    Diagnostic::new(e.code(), e.kind.message())
                        .at(text.span.slice(1..text.value.len() + 1)),
                );
                return None;
            }
        };
        let expected = info
            .unit
            .map(|u| if difference { u.difference() } else { u });
        match (literal, expected) {
            (Literal::Number(n), None) => Some(n),
            (Literal::Number(n), Some(unit)) if unit.is_dimensionless() => Some(n),
            (Literal::Number(n), Some(unit)) => {
                self.push(
                    Diagnostic::new("J0151", "number without a unit in a quantity")
                        .at(text.span)
                        .label(format!("`{path}` is in {}", unit.symbol()))
                        .help(format!("write \"{} {}\"", trim_float(n), unit.symbol())),
                );
                None
            }
            (Literal::Quantity(q), None) => Some(q.value),
            (Literal::Quantity(q), Some(unit)) => match q.value_in(&unit) {
                Ok(v) => Some(v),
                Err(e) => {
                    let mut d = Diagnostic::new(e.code(), e.kind.message())
                        .at(text.span)
                        .label(format!("`{path}` is {}", describe(&unit)));
                    if difference && q.unit.is_affine() {
                        d = d.help(format!("a difference is written in {}", unit.symbol()));
                    }
                    self.push(d);
                    None
                }
            },
        }
    }

    fn rate(&mut self, path: &str, info: &SignalInfo, rate: &Spanned<String>, key: &str) {
        let Some(unit) = info.unit else {
            return;
        };
        let expected = Unit::parse("s")
            .ok()
            .and_then(|s| unit.difference().checked_div(&s).ok());
        let quantity = match Literal::parse(&rate.value) {
            Ok(Literal::Quantity(q)) => q,
            Ok(Literal::Number(_)) => {
                self.push(
                    Diagnostic::new("J0151", "number without a unit in a rate")
                        .at(rate.span)
                        .help(format!("write \"<value> {}/s\"", unit.symbol())),
                );
                return;
            }
            Err(e) => {
                self.push(Diagnostic::new(e.code(), e.kind.message()).at(rate.span));
                return;
            }
        };
        let coherent = expected.is_some_and(|e| quantity.unit.dimension() == e.dimension());
        if !coherent {
            self.push(
                Diagnostic::new("J0202", format!("`{key}` unit incoherent with the output"))
                    .at(rate.span)
                    .label(format!(
                        "`{path}` is in {}: expected {}/s or {}/min",
                        unit.symbol(),
                        unit.symbol(),
                        unit.symbol()
                    )),
            );
        } else if quantity.value <= 0.0 {
            self.push(Diagnostic::new("J0202", format!("`{key}` must be positive")).at(rate.span));
        }
    }

    fn cycles(&mut self) {
        let kernel = self
            .manifest
            .target
            .as_ref()
            .map_or(Kernel::Standard, |t| t.kernel);
        let ten_ms = Duration::from_millis(10).as_nanos();
        for (name, cycle) in &self.manifest.cycles {
            if let Some(period) = &cycle.period {
                let nanos = period.value.as_nanos();
                if !(Duration::from_millis(1).as_nanos()..=Duration::from_secs(3600).as_nanos())
                    .contains(&nanos)
                {
                    self.push(
                        Diagnostic::new("J0280", "`period` outside the limits of the target")
                            .at(period.span)
                            .label("on Linux, 1ms to 1h"),
                    );
                } else if kernel != Kernel::PreemptRt {
                    if nanos < ten_ms {
                        self.push(
                            Diagnostic::new(
                                "J0351",
                                format!(
                                    "group `{name}` below 10ms without `kernel = \"preempt_rt\"`"
                                ),
                            )
                            .at(period.span),
                        );
                    } else {
                        self.push(
                            Diagnostic::new(
                                "J0352",
                                format!(
                                    "group `{name}` without PREEMPT_RT: jitter is not guaranteed"
                                ),
                            )
                            .at(period.span)
                            .help(
                                "declare `[target] kernel = \"preempt_rt\"` on a node that has it",
                            ),
                        );
                    }
                }
            }
            if let Some(resources) = &cycle.resources
                && resources.value != "rt"
            {
                self.push(
                    Diagnostic::new("J0282", "a cycle group only takes the `rt` class")
                        .at(resources.span),
                );
            }
            for entry in &cycle.order {
                let (app, task) = match entry.value.split_once('.') {
                    Some((app, task)) => (app, Some(task)),
                    None => (entry.value.as_str(), None),
                };
                let runs_here = self.manifest.apps.get(app).is_some_and(|a| {
                    a.cycles
                        .iter()
                        .any(|(t, g)| g.value == *name && task.is_none_or(|task| task == t))
                });
                if !runs_here {
                    self.push(
                        Diagnostic::new(
                            "J0281",
                            format!("`{}` does not run in group `{name}`", entry.value),
                        )
                        .at(entry.span),
                    );
                }
            }
        }
    }

    fn apps(&mut self) {
        let manifest = self.manifest;
        for (name, app) in &manifest.apps {
            if let Some(path) = &app.path {
                let declaring = self.sources.path(path.span.file);
                let base = self.root.join(declaring.parent().unwrap_or(Path::new("")));
                if !self.fs.exists(&base.join(&path.value)) {
                    self.push(
                        Diagnostic::new("J0160", format!("app folder `{}` not found", path.value))
                            .at(path.span),
                    );
                }
            }
            for (task, group) in &app.cycles {
                if !self.group_exists(&group.value) {
                    self.push(
                        Diagnostic::new("J0182", format!("group `{}` does not exist", group.value))
                            .at(group.span),
                    );
                }
                if let Some(interface) = self.context.apps.get(name)
                    && !interface.tasks.contains_key(task)
                {
                    let mut d =
                        Diagnostic::new("J0182", format!("app `{name}` has no task `{task}`"))
                            .at(group.span);
                    if let Some(s) =
                        grammar::suggest(task, interface.tasks.keys().map(String::as_str))
                    {
                        d = d.help(format!("did you mean `{s}`?"));
                    }
                    self.push(d);
                }
            }
            if let Some(interface) = self.context.apps.get(name) {
                self.bindings(name, app, interface);
                self.params(app, interface);
            }
        }
    }

    fn bindings(&mut self, name: &str, app: &App, interface: &AppInterface) {
        for (port_name, binding) in &app.bind {
            let Some(port) = interface.ports.get(port_name) else {
                let mut d =
                    Diagnostic::new("J0140", format!("app `{name}` has no port `{port_name}`"))
                        .at(binding.span);
                if let Some(s) =
                    grammar::suggest(port_name, interface.ports.keys().map(String::as_str))
                {
                    d = d.help(format!("did you mean `{s}`?"));
                }
                self.push(d);
                continue;
            };
            let many = matches!(port.kind, PortKind::InputMany | PortKind::OutputMany);
            match &binding.value {
                Binding::Target(target) if !many => self.bind_scalar(port_name, port, target, None),
                Binding::Signal {
                    signal, conversion, ..
                } if !many => {
                    self.bind_scalar(port_name, port, signal, conversion.as_ref());
                }
                Binding::List(items) if many => {
                    for item in items {
                        if !self.paths.contains_key(&item.value) {
                            let d = self.unknown_path(&item.value, item.span);
                            self.push(d);
                        }
                    }
                }
                Binding::Target(target) if many => {
                    if !self
                        .paths
                        .keys()
                        .any(|p| grammar::matches(&target.value, p))
                    {
                        self.push(
                            Diagnostic::new(
                                "J0155",
                                format!("selector `{}` matches nothing", target.value),
                            )
                            .at(target.span),
                        );
                    }
                }
                Binding::Each(_) => {}
                _ => self.push(
                    Diagnostic::new("J0181", "invalid binding form for the port")
                        .at(binding.span)
                        .label(format!(
                            "port `{port_name}` takes {}",
                            if many {
                                "a selector or a list"
                            } else {
                                "one path"
                            }
                        )),
                ),
            }
        }
        for (port_name, port) in &interface.ports {
            if !port.optional && !app.bind.contains_key(port_name) {
                self.push(
                    Diagnostic::new("J0110", format!("required port `{port_name}` is not bound"))
                        .at(app.key),
                );
            }
        }
    }

    fn bind_scalar(
        &mut self,
        port_name: &str,
        port: &crate::context::Port,
        target: &Spanned<String>,
        conversion: Option<&Spanned<String>>,
    ) {
        let Some(registered) = self.paths.get(&target.value).cloned() else {
            let d = self.unknown_path(&target.value, target.span);
            self.push(d);
            return;
        };
        let info = &registered.info;
        if port.kind == PortKind::Output && info.direction != Direction::Output {
            self.push(
                Diagnostic::new("J0181", "an output port needs an output signal")
                    .at(target.span)
                    .label(format!("`{}` is not an output", target.value)),
            );
            return;
        }
        if info.ty == SignalType::Unknown || port.ty == SignalType::Unknown {
            return;
        }
        if port.ty.is_numeric() != info.ty.is_numeric()
            || (!port.ty.is_numeric() && port.ty != info.ty)
        {
            self.push(
                Diagnostic::new("J0181", "signal type does not match the port")
                    .at(target.span)
                    .label(format!(
                        "port `{port_name}` and `{}` have different types",
                        target.value
                    )),
            );
            return;
        }
        let (Some(port_unit), Some(signal_unit)) = (port.unit, info.unit) else {
            return;
        };
        if let Some(conversion) = conversion {
            let ok = match conversion.value.as_str() {
                "fraction" => signal_unit.symbol() == "%" && port_unit.is_dimensionless(),
                "percent" => signal_unit.is_dimensionless() && port_unit.symbol() == "%",
                _ => false,
            };
            if !ok {
                self.push(
                    Diagnostic::new(
                        "J0183",
                        format!(
                            "`as = \"{}\"` between units that are not % and 1",
                            conversion.value
                        ),
                    )
                    .at(conversion.span),
                );
            }
            return;
        }
        if let Err(e) = signal_unit.check_compatible(&port_unit) {
            let title = match e.kind {
                ErrorKind::IncompatibleKind => "incompatible quantity kind in binding",
                _ => "incompatible unit in binding",
            };
            let same: Vec<String> = self
                .paths
                .iter()
                .filter(|(p, r)| {
                    **p == r.canonical
                        && r.info
                            .unit
                            .is_some_and(|u| u.check_compatible(&port_unit).is_ok())
                })
                .map(|(p, _)| p.clone())
                .collect();
            let mut d = Diagnostic::new(e.code(), title)
                .at(target.span)
                .label(format!(
                    "`{}` is {}; port `{port_name}` expects {}",
                    target.value,
                    describe(&signal_unit),
                    describe(&port_unit)
                ));
            if !same.is_empty() {
                d = d.help(format!(
                    "signals with unit {} on this site: {}",
                    port_unit.symbol(),
                    same.join(", ")
                ));
            }
            self.push(d);
        }
    }

    fn params(&mut self, app: &App, interface: &AppInterface) {
        for (name, value) in &app.params {
            let Some(param) = interface.params.get(name) else {
                let mut d =
                    Diagnostic::new("J0121", format!("unknown parameter `{name}`")).at(value.span);
                if let Some(s) = grammar::suggest(name, interface.params.keys().map(String::as_str))
                {
                    d = d.help(format!("did you mean `{s}`?"));
                }
                self.push(d);
                continue;
            };
            let node = &value.value;
            let number = match (&param.ty, &node.value) {
                (SignalType::Bool, Value::Bool(_)) | (SignalType::Text, Value::String(_)) => None,
                (SignalType::Integer | SignalType::Float, Value::Integer(i)) => {
                    if param.unit.is_some_and(|u| !u.is_dimensionless()) {
                        self.push(
                            Diagnostic::new("J0151", "number without a unit in a quantity")
                                .at(node.span)
                                .label(format!(
                                    "parameter `{name}` is {}",
                                    param.unit.map_or_else(String::new, |u| describe(&u))
                                )),
                        );
                        None
                    } else {
                        Some(*i as f64)
                    }
                }
                (SignalType::Integer | SignalType::Float, Value::Float(f))
                    if param.unit.is_none_or(|u| u.is_dimensionless()) =>
                {
                    Some(*f)
                }
                (SignalType::Integer | SignalType::Float, Value::String(text)) => {
                    let info = SignalInfo {
                        ty: param.ty.clone(),
                        unit: param.unit,
                        direction: Direction::Param,
                    };
                    self.constant(
                        &format!("parameter `{name}`"),
                        &info,
                        &Spanned::new(text.clone(), node.span),
                        false,
                    )
                }
                (SignalType::Enum { .. } | SignalType::Unknown, _) => None,
                _ => {
                    self.push(
                        Diagnostic::new("J0143", "wrong value type")
                            .at(node.span)
                            .label(format!("found {}", node.value.type_name())),
                    );
                    None
                }
            };
            if let (Some(v), Some((lo, hi))) = (number, param.range)
                && !(lo..=hi).contains(&v)
            {
                self.push(
                    Diagnostic::new("J0120", format!("parameter `{name}` outside its range"))
                        .at(node.span)
                        .label(format!(
                            "declared range {}–{}",
                            trim_float(lo),
                            trim_float(hi)
                        )),
                );
            }
        }
        for (name, param) in &interface.params {
            if !param.has_default && !app.params.contains_key(name) {
                self.push(
                    Diagnostic::new(
                        "J0142",
                        format!("parameter `{name}` has no default and no value"),
                    )
                    .at(app.key),
                );
            }
        }
    }
}

fn looks_like_expression(text: &str) -> bool {
    text.contains(['(', ')', '&', '|', '!', '<', '>', '=', '+', '*', '/'])
        || text.split_whitespace().count() > 2
        || (text.contains('.')
            && text
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c == '-'))
            && Literal::parse(text).is_err()
}

fn signal_type(text: &str) -> Option<SignalType> {
    Some(match text {
        "bool" => SignalType::Bool,
        "i64" | "u64" => SignalType::Integer,
        "f64" | "f32" => SignalType::Float,
        "string" => SignalType::Text,
        "bytes" => SignalType::Bytes,
        t if t.starts_with("bits<") && t.ends_with('>') => SignalType::Integer,
        _ => return None,
    })
}

fn render(node: &Node) -> String {
    match &node.value {
        Value::String(s) => s.clone(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.type_name().to_owned(),
    }
}

fn trim_float(value: f64) -> String {
    let text = format!("{value}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}
