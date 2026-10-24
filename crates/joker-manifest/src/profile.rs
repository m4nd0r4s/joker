//! Loads a `.profile.toml` file (ref/03 §2.2) into a [`ProfileInfo`], the
//! same type `Context` already uses for the manifest checks.
//!
//! The file is read strictly (ref/03 §2.1.1, ADR 0064): an unknown key is
//! `J0140`, a key of the 1.0 surface that this runtime does not implement
//! yet is `J0807`, and a profile with any error is not loaded. Implemented:
//! `[profile]` (identification and `quirks`, which the driver's `plan`
//! validates), `[defaults]` (merged into every point, the point wins; `poll`
//! and `timeout` are the device defaults), `[signals]` (raw type and the
//! signal type of ref/03 §2.2.7, `signal_type`, `direction`, `unit`,
//! numeric `scale` and `offset`, `invert`, `enum`, `deadband`, `poll`, the
//! address keys of the family, and the descriptive keys `label`,
//! `description`, `readback`, `safe_hint`, `range_hint`, `monotonic`),
//! `[enums]` (variant → integer), `[fallback]` (`timeout`, `value`,
//! `applies_to`, `configurable`), `[actions]` (`steps`, `timeout`, and
//! `args`, `result`, `long`, `native` and `description` kept as
//! [`crate::context::ActionDecl`], ADR 0066) and the class actions
//! `[implements]` maps to them ([`ProfileInfo::implemented_actions`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use joker_units::{Duration, Literal, Unit};
use toml::de::DeTable;

use crate::context::{Acquisition, Direction, PointPoll, ProfileInfo, SignalInfo, SignalType};
use crate::diag::{Diagnostic, Diagnostics};
use crate::source::{FileId, Sources, Span};
use crate::value::{self, Node, Table, Value};

/// The result of loading one profile file.
#[derive(Debug)]
pub struct ProfileReport {
    /// The file, for rendering diagnostics.
    pub sources: Sources,
    /// What went wrong, if anything.
    pub diagnostics: Diagnostics,
    /// The profile, when the file has no error.
    pub profile: Option<ProfileInfo>,
}

impl ProfileReport {
    /// The findings, rendered as `jk check`/`jk profile test` would.
    pub fn render(&self) -> String {
        self.diagnostics
            .sorted(&self.sources)
            .iter()
            .map(|d| d.render(&self.sources, crate::Mode::Prod))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Parses `text` (the file at `path`, for diagnostics) into a
/// [`ProfileInfo`].
pub fn parse(path: &Path, text: &str) -> ProfileReport {
    let mut sources = Sources::default();
    let file = sources.add(path.to_path_buf(), text.to_owned());
    let mut diagnostics = Diagnostics::default();
    let table = match DeTable::parse(text) {
        Ok(parsed) => match value::from_toml(file, parsed.get_ref()) {
            Ok(table) => table,
            Err(value::InvalidNumber(span)) => {
                diagnostics.push(Diagnostic::new("J0146", "number out of range").at(span));
                return ProfileReport {
                    sources,
                    diagnostics,
                    profile: None,
                };
            }
        },
        Err(error) => {
            let range = error.span().unwrap_or(0..0);
            diagnostics.push(
                Diagnostic::new("J0146", "file is not valid TOML")
                    .at(Span::new(file, range.start, range.end))
                    .label(error.message().trim().to_owned()),
            );
            return ProfileReport {
                sources,
                diagnostics,
                profile: None,
            };
        }
    };
    let profile = load(&table, file, &mut diagnostics);
    let profile = profile.filter(|_| diagnostics.is_empty());
    ProfileReport {
        sources,
        diagnostics,
        profile,
    }
}

/// The top-level sections this runtime reads.
const SECTIONS: [&str; 9] = [
    "schema",
    "joker",
    "profile",
    "defaults",
    "signals",
    "enums",
    "fallback",
    "actions",
    "implements",
];

/// Sections of ref/03 §2.2.1 this runtime does not implement yet.
const RESERVED_SECTIONS: [&str; 6] = [
    "coherent",
    "config",
    "connect",
    "relinquish",
    "events",
    "discovery",
];

/// Keys of `[profile]` this runtime reads (ref/03 §2.2.2).
const PROFILE_KEYS: [&str; 10] = [
    "name",
    "version",
    "driver",
    "vendor",
    "model",
    "description",
    "docs",
    "license",
    "authors",
    "quirks",
];

/// Keys of `[profile]` this runtime does not implement yet.
const RESERVED_PROFILE_KEYS: [&str; 2] = ["firmware", "identity"];

/// Generic point keys (ref/03 §2.2.4): the acquisition stage consumes
/// these, a driver's address schema never sees them. Everything else in a
/// signal's table is address-family-specific (ref/03 §2.2.5), kept in
/// [`SignalInfo::raw`] as given.
pub const GENERIC_SIGNAL_KEYS: [&str; 25] = [
    "type",
    "signal_type",
    "direction",
    "unit",
    "quantity",
    "scale",
    "offset",
    "scale_factor",
    "enum",
    "bits",
    "invert",
    "encoding",
    "invalid",
    "poll",
    "max_age",
    "limits",
    "stuck_detect",
    "deadband",
    "monotonic",
    "readback",
    "safe_hint",
    "range_hint",
    "label",
    "description",
    "kind",
];

/// Generic point keys this runtime does not implement yet: the value they
/// would change is never produced without them, so a profile that
/// declares one is refused rather than read without it (ADR 0064).
const RESERVED_SIGNAL_KEYS: [&str; 8] = [
    "quantity",
    "scale_factor",
    "bits",
    "encoding",
    "invalid",
    "max_age",
    "limits",
    "stuck_detect",
];

/// Generic point keys `[defaults]` admits (ref/03 §2.2.3, the "yes" of
/// the default column of §2.2.4).
const DEFAULTABLE_SIGNAL_KEYS: [&str; 10] = [
    "type",
    "signal_type",
    "direction",
    "unit",
    "scale",
    "offset",
    "encoding",
    "invalid",
    "max_age",
    "stuck_detect",
];

/// Keys of `[fallback]` this runtime reads (ref/03 §2.2.11).
const FALLBACK_KEYS: [&str; 4] = ["timeout", "value", "applies_to", "configurable"];

/// Keys of `[fallback]` this runtime does not implement yet.
const RESERVED_FALLBACK_KEYS: [&str; 6] = [
    "timeout_register",
    "value_register",
    "values",
    "behavior",
    "enable_register",
    "enable_value",
];

/// Keys of `[actions.<a>]` this runtime reads (ref/03 §2.2.12).
const ACTION_KEYS: [&str; 7] = [
    "steps",
    "args",
    "result",
    "timeout",
    "description",
    "long",
    "native",
];

/// Keys of `[actions.<a>]` this runtime does not implement yet.
const RESERVED_ACTION_KEYS: [&str; 0] = [];

/// Checks the keys of `table` (named `what` in messages): `x_` keys are
/// ignored, `_` keys are reserved (`J0141`), keys in `reserved` are not
/// implemented yet (`J0807`), and keys in neither list are unknown
/// (`J0140`).
fn check_keys(
    table: &Table,
    known: &[&str],
    reserved: &[&str],
    what: &str,
    diagnostics: &mut Diagnostics,
) {
    for (key, entry) in table {
        if key.starts_with("x_") || known.contains(&key.as_str()) {
            continue;
        }
        let diagnostic = if key.starts_with('_') {
            Diagnostic::new("J0141", format!("{what}: `{key}` is a reserved key"))
        } else if reserved.contains(&key.as_str()) {
            Diagnostic::new(
                "J0807",
                format!("{what}: `{key}` is not supported by this runtime yet"),
            )
            .help("remove it, or wait for the runtime version that implements it (ADR 0064)")
        } else {
            Diagnostic::new("J0140", format!("{what}: unknown key `{key}`"))
        };
        diagnostics.push(diagnostic.at(entry.key));
    }
}

fn load(table: &Table, file: FileId, diagnostics: &mut Diagnostics) -> Option<ProfileInfo> {
    let Some(profile_table) = table.get("profile").and_then(|e| e.node.as_table()) else {
        diagnostics.push(
            Diagnostic::new("J0142", "missing required section `[profile]`")
                .at(Span::new(file, 0, 0)),
        );
        return None;
    };
    check_keys(table, &SECTIONS, &RESERVED_SECTIONS, "profile", diagnostics);
    check_keys(
        profile_table,
        &PROFILE_KEYS,
        &RESERVED_PROFILE_KEYS,
        "`[profile]`",
        diagnostics,
    );
    let enums = enums_of(table, diagnostics);
    let defaults = defaults_of(table, diagnostics);
    let signals = signals_of(table, &enums, &defaults, diagnostics)?;
    let fallback = fallback_of(table, &signals, diagnostics);
    let action_steps = action_steps_of(table, diagnostics);
    let actions = actions_of(table, &action_steps, diagnostics);
    let action_decls = action_decls_of(table, &action_steps, diagnostics);
    let implemented_actions = implemented_actions_of(table, &actions);
    let quirks_table = profile_table.get("quirks").and_then(|e| e.node.as_table());
    let latency = quirks_table
        .and_then(|t| t.get("latency"))
        .and_then(|e| e.node.as_str())
        .and_then(|s| Duration::parse(s).ok());
    let quirks = quirks_table
        .map(|t| t.iter().map(|(k, e)| (k.clone(), e.node.clone())).collect())
        .unwrap_or_default();
    Some(ProfileInfo {
        signals,
        fallback: fallback.present,
        configurable: fallback.configurable,
        fallback_timeout: fallback.timeout,
        latency,
        quirks,
        poll: defaults.poll,
        timeout: defaults.timeout,
        actions,
        action_steps,
        action_decls,
        implemented_actions,
        enums: enum_values_of(table),
        fallback_outputs: fallback.outputs,
        fallback_value: fallback.value,
    })
}

fn enum_values_of(table: &Table) -> BTreeMap<String, BTreeMap<String, i64>> {
    let Some(enums) = table.get("enums").and_then(|e| e.node.as_table()) else {
        return BTreeMap::new();
    };
    enums
        .iter()
        .filter_map(|(name, entry)| {
            let variants = entry.node.as_table()?;
            Some((
                name.clone(),
                variants
                    .iter()
                    .filter_map(|(v, e)| match e.node.value {
                        Value::Integer(i) => Some((v.clone(), i)),
                        _ => None,
                    })
                    .collect(),
            ))
        })
        .collect()
}

/// One `[enums]` entry: each variant, in name order, with its integer.
type Enum = Vec<(String, i64)>;

fn enums_of(table: &Table, diagnostics: &mut Diagnostics) -> BTreeMap<String, Enum> {
    let Some(enums) = table.get("enums").and_then(|e| e.node.as_table()) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (name, entry) in enums {
        let Some(variants) = entry.node.as_table() else {
            diagnostics.push(
                Diagnostic::new("J0814", format!("enum `{name}` is not a table"))
                    .at(entry.node.span),
            );
            continue;
        };
        let mut values = Vec::with_capacity(variants.len());
        let mut seen = BTreeSet::new();
        for (variant, value) in variants {
            let Value::Integer(raw) = value.node.value else {
                diagnostics.push(
                    Diagnostic::new(
                        "J0814",
                        format!("enum `{name}`: variant `{variant}` is not an integer"),
                    )
                    .at(value.node.span),
                );
                continue;
            };
            if !seen.insert(raw) {
                diagnostics.push(
                    Diagnostic::new(
                        "J0814",
                        format!("enum `{name}`: the value {raw} is repeated"),
                    )
                    .at(value.node.span),
                );
                continue;
            }
            values.push((variant.clone(), raw));
        }
        out.insert(name.clone(), values);
    }
    out
}

/// `[defaults]`: the device defaults and the point keys merged into every
/// point.
#[derive(Default)]
struct Defaults {
    poll: Option<Duration>,
    timeout: Option<Duration>,
    point: Table,
}

fn defaults_of(table: &Table, diagnostics: &mut Diagnostics) -> Defaults {
    let Some(defaults) = table.get("defaults").and_then(|e| e.node.as_table()) else {
        return Defaults::default();
    };
    let mut out = Defaults::default();
    for (key, entry) in defaults {
        match key.as_str() {
            "poll" | "timeout" => {
                let parsed = entry.node.as_str().and_then(|s| Duration::parse(s).ok());
                if parsed.is_none() {
                    diagnostics.push(
                        Diagnostic::new(
                            if key == "poll" { "J0817" } else { "J0806" },
                            format!("`[defaults] {key}` is not a duration"),
                        )
                        .at(entry.node.span),
                    );
                }
                if key == "poll" {
                    out.poll = parsed;
                } else {
                    out.timeout = parsed;
                }
            }
            k if k.starts_with("x_") => {}
            k if GENERIC_SIGNAL_KEYS.contains(&k) && !DEFAULTABLE_SIGNAL_KEYS.contains(&k) => {
                diagnostics.push(
                    Diagnostic::new("J0140", format!("`[defaults]` does not admit `{k}`"))
                        .at(entry.key),
                );
            }
            _ => {
                out.point.insert(key.clone(), entry.clone());
            }
        }
    }
    out
}

/// The signal type a raw `type` gives (ref/03 §2.2.7), before
/// `signal_type`: an integer raw type with `scale`, `offset` or a unit
/// other than `1` is a float.
fn derived_type(type_name: &str, scaled: bool) -> Option<SignalType> {
    match type_name {
        "bool" => Some(SignalType::Bool),
        "u16" | "i16" | "u32" | "i32" | "u64" | "i64" if scaled => Some(SignalType::Float),
        "u16" | "i16" | "u32" | "i32" | "u64" | "i64" | "bits16" | "bits32" | "bits64" => {
            Some(SignalType::Integer)
        }
        "f32" | "f64" => Some(SignalType::Float),
        "string" => Some(SignalType::Text),
        _ => None,
    }
}

fn is_integer_raw(type_name: &str) -> bool {
    matches!(
        type_name,
        "u16" | "i16" | "u32" | "i32" | "u64" | "i64" | "bits16" | "bits32" | "bits64"
    )
}

/// A number of a point key: an integer or a float.
fn number(node: &Node) -> Option<f64> {
    match node.value {
        Value::Integer(i) => Some(i as f64),
        Value::Float(f) => Some(f),
        _ => None,
    }
}

/// The diagnostics of one point, with its path and span.
struct PointKeys<'a> {
    path: &'a str,
    span: Span,
    diagnostics: &'a mut Diagnostics,
    ok: bool,
}

impl PointKeys<'_> {
    fn error(&mut self, code: &'static str, message: String, span: Span) {
        self.diagnostics
            .push(Diagnostic::new(code, format!("signal `{}`: {message}", self.path)).at(span));
        self.ok = false;
    }
}

fn signals_of(
    table: &Table,
    enums: &BTreeMap<String, Enum>,
    defaults: &Defaults,
    diagnostics: &mut Diagnostics,
) -> Option<BTreeMap<String, SignalInfo>> {
    let Some(signals) = table.get("signals").and_then(|e| e.node.as_table()) else {
        return Some(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    let mut ok = true;
    for (path, entry) in signals {
        let Some(own) = entry.node.as_table() else {
            diagnostics.push(
                Diagnostic::new("J0143", format!("signal `{path}` is not a table"))
                    .at(entry.node.span),
            );
            ok = false;
            continue;
        };
        let mut point = defaults.point.clone();
        point.extend(own.iter().map(|(k, e)| (k.clone(), e.clone())));
        let mut keys = PointKeys {
            path,
            span: entry.node.span,
            diagnostics,
            ok: true,
        };
        let info = signal_of(&point, enums, &mut keys);
        ok &= keys.ok;
        if let Some(info) = info.filter(|_| keys.ok) {
            out.insert(path.clone(), info);
        }
    }
    ok.then_some(out)
}

#[allow(
    clippy::too_many_lines,
    reason = "one pass over the generic keys of ref/03 §2.2.4, in the order of the table"
)]
fn signal_of(
    point: &Table,
    enums: &BTreeMap<String, Enum>,
    keys: &mut PointKeys<'_>,
) -> Option<SignalInfo> {
    for (key, entry) in point {
        if RESERVED_SIGNAL_KEYS.contains(&key.as_str()) {
            keys.error(
                "J0807",
                format!("`{key}` is not supported by this runtime yet (ADR 0064)"),
                entry.key,
            );
        }
    }
    let span = keys.span;
    let Some(type_name) = point.get("type").and_then(|e| e.node.as_str()) else {
        keys.error("J0142", "without `type`".to_owned(), span);
        return None;
    };
    let unit = match point.get("unit") {
        Some(entry) => match entry.node.as_str().map(Unit::parse) {
            Some(Ok(u)) => Some(u),
            _ => {
                keys.error(
                    "J0815",
                    format!("invalid unit `{}`", entry.node.as_str().unwrap_or("?")),
                    entry.node.span,
                );
                None
            }
        },
        None => None,
    };
    let scale = match point.get("scale") {
        Some(entry) if entry.node.as_table().is_some() => {
            keys.error(
                "J0807",
                "the two-point `scale` is not supported by this runtime yet (ADR 0064)".to_owned(),
                entry.node.span,
            );
            None
        }
        Some(entry) => match number(&entry.node) {
            Some(s) if s != 0.0 && s.is_finite() => Some(s),
            _ => {
                keys.error(
                    "J0815",
                    "`scale` must be a number other than 0".to_owned(),
                    entry.node.span,
                );
                None
            }
        },
        None => None,
    };
    let offset = match point.get("offset") {
        Some(entry) => match number(&entry.node) {
            Some(o) if o.is_finite() => Some(o),
            _ if entry.node.as_str().is_some() => {
                keys.error(
                    "J0807",
                    "an `offset` quantity is not supported by this runtime yet; write it as a number in the unit of the signal (ADR 0064)".to_owned(),
                    entry.node.span,
                );
                None
            }
            _ => {
                keys.error(
                    "J0815",
                    "`offset` must be a number".to_owned(),
                    entry.node.span,
                );
                None
            }
        },
        None => None,
    };
    let unit_is_one = unit.as_ref().is_none_or(|u| u.symbol() == "1");
    let scaled = scale.is_some() || offset.is_some() || !unit_is_one;
    let enum_name = point.get("enum").and_then(|e| e.node.as_str());
    let (mut ty, enum_values) = match enum_name {
        Some(name) => {
            let Some(variants) = enums.get(name) else {
                keys.error("J0814", format!("unknown enum `{name}`"), span);
                return None;
            };
            if !is_integer_raw(type_name) {
                keys.error(
                    "J0815",
                    format!("an enum needs a raw integer type, not `{type_name}`"),
                    span,
                );
                return None;
            }
            (
                SignalType::Enum {
                    name: name.to_owned(),
                    variants: variants.iter().map(|(v, _)| v.clone()).collect(),
                },
                variants.iter().map(|(_, raw)| *raw).collect(),
            )
        }
        None => match derived_type(type_name, scaled) {
            Some(ty) => (ty, Vec::new()),
            None => {
                keys.error("J0809", format!("unknown raw type `{type_name}`"), span);
                return None;
            }
        },
    };
    let numeric = matches!(ty, SignalType::Integer | SignalType::Float);
    if !numeric && (scale.is_some() || offset.is_some()) {
        keys.error(
            "J0815",
            format!("`scale` and `offset` do not apply to a `{type_name}` signal"),
            span,
        );
    }
    if !numeric && !unit_is_one {
        keys.error(
            "J0815",
            format!("a unit does not apply to a `{type_name}` signal"),
            span,
        );
    }
    if let Some(entry) = point.get("signal_type") {
        match (entry.node.as_str(), &ty) {
            (Some("f64"), SignalType::Integer | SignalType::Float) => ty = SignalType::Float,
            (Some("i64" | "u64"), SignalType::Integer) | (Some("bool"), SignalType::Bool) => {}
            (Some("i64" | "u64"), SignalType::Float)
                if is_integer_raw(type_name) && scale.is_none() && offset.is_none() =>
            {
                ty = SignalType::Integer;
            }
            (Some("f32"), _) => keys.error(
                "J0807",
                "`signal_type = \"f32\"` is not supported by this runtime yet (ADR 0064)"
                    .to_owned(),
                entry.node.span,
            ),
            (other, _) => keys.error(
                "J0815",
                format!(
                    "`signal_type = {}` does not fit a `{type_name}` point",
                    other.map_or_else(|| "?".to_owned(), |s| format!("\"{s}\""))
                ),
                entry.node.span,
            ),
        }
    }
    let invert = match point.get("invert") {
        Some(entry) => match entry.node.value {
            Value::Bool(b) if ty == SignalType::Bool => b,
            Value::Bool(_) => {
                keys.error(
                    "J0815",
                    "`invert` only applies to a `bool` signal".to_owned(),
                    entry.node.span,
                );
                false
            }
            _ => {
                keys.error(
                    "J0143",
                    "`invert` is not a boolean".to_owned(),
                    entry.node.span,
                );
                false
            }
        },
        None => false,
    };
    let direction = match point
        .get("direction")
        .map(|e| (e.node.as_str(), e.node.span))
    {
        None | Some((Some("input"), _)) => Direction::Input,
        Some((Some("output"), _)) => Direction::Output,
        Some((Some("param"), _)) => Direction::Param,
        Some((_, span)) => {
            keys.error(
                "J0144",
                "`direction` is `input`, `output` or `param`".to_owned(),
                span,
            );
            Direction::Input
        }
    };
    let poll = match point.get("poll") {
        Some(entry) => match entry.node.as_str() {
            Some("on_demand") if direction == Direction::Param => Some(PointPoll::OnDemand),
            Some("on_demand") => {
                keys.error(
                    "J0817",
                    "`poll = \"on_demand\"` needs `direction = \"param\"`".to_owned(),
                    entry.node.span,
                );
                None
            }
            Some(text) => match Duration::parse(text) {
                Ok(d) if d.as_nanos() > 0 => Some(PointPoll::Period(d)),
                _ => {
                    keys.error(
                        "J0817",
                        format!("`poll` is not a duration: `{text}`"),
                        entry.node.span,
                    );
                    None
                }
            },
            None => {
                keys.error(
                    "J0817",
                    "`poll` is not a duration".to_owned(),
                    entry.node.span,
                );
                None
            }
        },
        None => None,
    };
    if let Some(entry) = point.get("kind")
        && entry.node.as_str() != Some("scalar")
    {
        keys.error(
            "J0807",
            "only `kind = \"scalar\"` is supported in 1.0".to_owned(),
            entry.node.span,
        );
    }
    let deadband = match point.get("deadband") {
        Some(entry) if !numeric => {
            keys.error(
                "J0815",
                format!("`deadband` does not apply to a `{type_name}` signal"),
                entry.node.span,
            );
            None
        }
        Some(entry) => match deadband_in(&entry.node, unit.as_ref()) {
            Ok(d) => Some(d),
            Err(message) => {
                keys.error("J0815", message, entry.node.span);
                None
            }
        },
        None => None,
    };
    let converted = scale.is_some()
        || offset.is_some()
        || invert
        || enum_name.is_some()
        || (is_integer_raw(type_name) && ty == SignalType::Float);
    if direction == Direction::Output && converted {
        keys.error(
            "J0807",
            "an output whose value is converted to raw (`scale`, `offset`, `invert`, `enum`, or a float on an integer raw type) is not supported by this runtime yet (ADR 0064)".to_owned(),
            span,
        );
    }
    let mut info = SignalInfo::new(ty, direction);
    info.type_name = Some(type_name.to_owned());
    info.raw = point
        .iter()
        .filter(|(k, _)| !GENERIC_SIGNAL_KEYS.contains(&k.as_str()) && !k.starts_with("x_"))
        .map(|(k, e)| (k.clone(), e.node.clone()))
        .collect();
    info.acquisition = Acquisition {
        scale,
        offset,
        invert,
        deadband,
        enum_values,
        poll,
        ..Acquisition::NONE
    };
    if let Some(u) = unit {
        info = info.with_unit(u);
    }
    Some(info)
}

/// A `deadband` in the unit of the signal: a quantity converted to it, or
/// a plain number for a dimensionless signal.
fn deadband_in(node: &Node, unit: Option<&Unit>) -> Result<f64, String> {
    let dimensionless = unit.is_none_or(Unit::is_dimensionless);
    let value = match (&node.value, node.as_str().map(Literal::parse)) {
        (Value::Integer(_) | Value::Float(_), _) if dimensionless => number(node),
        (_, Some(Ok(Literal::Number(n)))) if dimensionless => Some(n),
        (_, Some(Ok(Literal::Quantity(q)))) => unit.and_then(|u| q.value_in(&u.difference()).ok()),
        _ => None,
    };
    match value {
        Some(v) if v >= 0.0 && v.is_finite() => Ok(v),
        _ => Err("`deadband` must be a non-negative quantity in the unit of the signal".to_owned()),
    }
}

/// `[fallback]`, as far as this runtime reads it.
struct Fallback {
    present: bool,
    configurable: bool,
    timeout: Option<Duration>,
    value: Option<Value>,
    outputs: Option<BTreeSet<String>>,
}

impl Fallback {
    const NONE: Self = Self {
        present: false,
        configurable: true,
        timeout: None,
        value: None,
        outputs: None,
    };
}

fn fallback_of(
    table: &Table,
    signals: &BTreeMap<String, SignalInfo>,
    diagnostics: &mut Diagnostics,
) -> Fallback {
    let Some(entry) = table.get("fallback") else {
        return Fallback::NONE;
    };
    let Some(fallback) = entry.node.as_table() else {
        diagnostics
            .push(Diagnostic::new("J0143", "`[fallback]` is not a table").at(entry.node.span));
        return Fallback::NONE;
    };
    check_keys(
        fallback,
        &FALLBACK_KEYS,
        &RESERVED_FALLBACK_KEYS,
        "`[fallback]`",
        diagnostics,
    );
    let configurable = !matches!(
        fallback.get("configurable").map(|e| &e.node.value),
        Some(Value::Bool(false))
    );
    let timeout = match fallback.get("timeout").and_then(|e| e.node.as_str()) {
        Some(s) => match Duration::parse(s) {
            Ok(d) => Some(d),
            Err(_) => {
                diagnostics.push(
                    Diagnostic::new(
                        "J0820",
                        format!("`[fallback] timeout` is not a duration: `{s}`"),
                    )
                    .at(entry.node.span),
                );
                None
            }
        },
        None => None,
    };
    let outputs = fallback
        .get("applies_to")
        .map(|e| applies_to(&e.node, signals, diagnostics));
    Fallback {
        present: true,
        configurable,
        timeout,
        value: fallback.get("value").map(|e| e.node.value.clone()),
        outputs,
    }
}

/// `[fallback] applies_to`: every entry is an output of the profile
/// (`J0820`).
fn applies_to(
    node: &Node,
    signals: &BTreeMap<String, SignalInfo>,
    diagnostics: &mut Diagnostics,
) -> BTreeSet<String> {
    let mut outputs = BTreeSet::new();
    let Value::Array(items) = &node.value else {
        diagnostics.push(
            Diagnostic::new("J0820", "`[fallback] applies_to` is not a list of outputs")
                .at(node.span),
        );
        return outputs;
    };
    for item in items {
        let name = item.as_str().unwrap_or("");
        match signals.get(name) {
            Some(s) if s.direction == Direction::Output => {
                outputs.insert(name.to_owned());
            }
            _ => diagnostics.push(
                Diagnostic::new(
                    "J0820",
                    format!("`[fallback] applies_to`: `{name}` is not an output of the profile"),
                )
                .at(item.span),
            ),
        }
    }
    outputs
}

/// ref/03 §2.2.12: an action's default `timeout` is the sum of its `wait`
/// and `until` budgets plus one second per step.
fn steps_default_timeout(steps: &[crate::context::ActionStep]) -> Duration {
    use crate::context::ActionStep;
    let mut nanos: u64 = 0;
    for step in steps {
        nanos += Duration::from_secs(1).as_nanos();
        nanos += match step {
            ActionStep::Wait(d) => d.as_nanos(),
            ActionStep::Until { timeout, .. } => timeout.as_nanos(),
            ActionStep::Write { .. } | ActionStep::Read { .. } | ActionStep::FailIf { .. } => 0,
        };
    }
    Duration::from_nanos(nanos)
}

/// Keys a step's kind reserves for itself: the rest of the step's table is
/// its address (driver-specific, compiled into CBOR per family in
/// `jokerd::bridge`, same treatment as a signal's own address keys in
/// [`signals_of`]).
const STEP_KEYS: [&str; 6] = ["type", "value", "into", "equals", "timeout", "message"];

fn step_addr(step: &Table) -> BTreeMap<String, Node> {
    step.iter()
        .filter(|(k, _)| !STEP_KEYS.contains(&k.as_str()))
        .map(|(k, e)| (k.clone(), e.node.clone()))
        .collect()
}

fn action_value_of(node: &Node) -> crate::context::ActionValue {
    use crate::context::ActionValue;
    match node.as_str() {
        Some(s) if s.starts_with('$') => ActionValue::Arg(s[1..].to_owned()),
        _ => ActionValue::Literal(node.value.clone()),
    }
}

fn step_type(t: &Table) -> Option<String> {
    t.get("type")
        .and_then(|e| e.node.as_str())
        .map(str::to_owned)
}

/// Compiles one step of an action, or says why it cannot be run
/// (`J0821`): a step that is skipped would change what the action does.
fn step_of(step: &Node) -> Result<crate::context::ActionStep, String> {
    use crate::context::ActionStep;
    let Some(step_table) = step.as_table() else {
        return Err("a step is a table".to_owned());
    };
    let mut kinds = step_table
        .keys()
        .filter(|k| !k.starts_with("x_"))
        .map(String::as_str);
    let (Some(kind), None) = (kinds.next(), kinds.next()) else {
        return Err(
            "a step has exactly one of `write`, `read`, `wait`, `until` or `fail_if`".to_owned(),
        );
    };
    let body = &step_table[kind].node;
    let table = || {
        body.as_table()
            .ok_or_else(|| format!("`{kind}` is a table"))
    };
    let required = |t: &Table, key: &str| {
        t.get(key)
            .map(|e| e.node.clone())
            .ok_or_else(|| format!("`{kind}` without `{key}`"))
    };
    Ok(match kind {
        "write" => {
            let t = table()?;
            ActionStep::Write {
                addr: step_addr(t),
                type_name: step_type(t),
                value: action_value_of(&required(t, "value")?),
            }
        }
        "read" => {
            let t = table()?;
            let into = required(t, "into")?;
            ActionStep::Read {
                addr: step_addr(t),
                type_name: step_type(t),
                into: into
                    .as_str()
                    .ok_or_else(|| "`read`: `into` is a field name".to_owned())?
                    .to_owned(),
            }
        }
        "wait" => ActionStep::Wait(
            body.as_str()
                .and_then(|s| Duration::parse(s).ok())
                .ok_or_else(|| "`wait` is a duration".to_owned())?,
        ),
        "until" => {
            let t = table()?;
            let timeout = required(t, "timeout")?;
            ActionStep::Until {
                addr: step_addr(t),
                type_name: step_type(t),
                equals: required(t, "equals")?.value,
                timeout: timeout
                    .as_str()
                    .and_then(|s| Duration::parse(s).ok())
                    .ok_or_else(|| "`until`: `timeout` is not a duration".to_owned())?,
            }
        }
        "fail_if" => {
            let t = table()?;
            ActionStep::FailIf {
                addr: step_addr(t),
                type_name: step_type(t),
                equals: required(t, "equals")?.value,
                message: required(t, "message")?
                    .as_str()
                    .ok_or_else(|| "`fail_if`: `message` is a string".to_owned())?
                    .to_owned(),
            }
        }
        other => return Err(format!("unknown step `{other}`")),
    })
}

fn action_steps_of(
    table: &Table,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<String, Vec<crate::context::ActionStep>> {
    let Some(actions) = table.get("actions").and_then(|e| e.node.as_table()) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (name, entry) in actions {
        let Some(action) = entry.node.as_table() else {
            diagnostics.push(
                Diagnostic::new("J0143", format!("action `{name}` is not a table"))
                    .at(entry.node.span),
            );
            continue;
        };
        check_keys(
            action,
            &ACTION_KEYS,
            &RESERVED_ACTION_KEYS,
            &format!("action `{name}`"),
            diagnostics,
        );
        let Some(steps_entry) = action.get("steps") else {
            continue;
        };
        let Value::Array(steps) = &steps_entry.node.value else {
            diagnostics.push(
                Diagnostic::new("J0821", format!("action `{name}`: `steps` is a list"))
                    .at(steps_entry.node.span),
            );
            continue;
        };
        let mut compiled = Vec::with_capacity(steps.len());
        for step in steps {
            match step_of(step) {
                Ok(s) => compiled.push(s),
                Err(message) => diagnostics.push(
                    Diagnostic::new("J0821", format!("action `{name}`: {message}")).at(step.span),
                ),
            }
        }
        out.insert(name.clone(), compiled);
    }
    out
}

/// A `name → type` table of an action (`args`, `result`), with the types
/// as written.
fn typed_names(
    action: &Table,
    key: &str,
    name: &str,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<String, String> {
    let Some(entry) = action.get(key) else {
        return BTreeMap::new();
    };
    let Some(fields) = entry.node.as_table() else {
        diagnostics.push(
            Diagnostic::new("J0821", format!("action `{name}`: `{key}` is a table"))
                .at(entry.node.span),
        );
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (field, ty) in fields {
        match ty.node.as_str() {
            Some(t) => {
                out.insert(field.clone(), t.to_owned());
            }
            None => diagnostics.push(
                Diagnostic::new(
                    "J0821",
                    format!("action `{name}`: the type of `{key}.{field}` is a string"),
                )
                .at(ty.node.span),
            ),
        }
    }
    out
}

/// Each action's default timeout; its `args` and `result` tables are
/// `name → type` strings (`J0821`).
fn actions_of(
    table: &Table,
    action_steps: &BTreeMap<String, Vec<crate::context::ActionStep>>,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<String, Option<Duration>> {
    let Some(actions) = table.get("actions").and_then(|e| e.node.as_table()) else {
        return BTreeMap::new();
    };
    let mut timeouts = BTreeMap::new();
    for (name, entry) in actions {
        let Some(action) = entry.node.as_table() else {
            continue;
        };
        typed_names(action, "args", name, diagnostics);
        typed_names(action, "result", name, diagnostics);
        if let Some(timeout) = action.get("timeout") {
            match timeout.node.as_str().map(Duration::parse) {
                Some(Ok(d)) => {
                    timeouts.insert(name.clone(), Some(d));
                }
                _ => diagnostics.push(
                    Diagnostic::new("J0821", format!("action `{name}`: invalid `timeout`"))
                        .at(entry.node.span),
                ),
            }
            continue;
        }
        match action_steps.get(name) {
            Some(steps) if !steps.is_empty() => {
                timeouts.insert(name.clone(), Some(steps_default_timeout(steps)));
            }
            _ => {
                timeouts.insert(name.clone(), None);
            }
        }
    }
    timeouts
}

fn string_table(action: &Table, key: &str) -> BTreeMap<String, String> {
    action
        .get(key)
        .and_then(|e| e.node.as_table())
        .map(|t| {
            t.iter()
                .filter_map(|(k, e)| e.node.as_str().map(|v| (k.clone(), v.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Every `"$name"` a program's `write` steps use.
fn step_args(steps: &[crate::context::ActionStep]) -> Vec<&str> {
    use crate::context::{ActionStep, ActionValue};
    steps
        .iter()
        .filter_map(|s| match s {
            ActionStep::Write {
                value: ActionValue::Arg(name),
                ..
            } => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

/// `[actions.<a>]`'s `args`, `result`, `long`, `native` and
/// `description` (ref/03 §2.2.12). A `"$name"` in a step that `args`
/// does not declare is `J0821`.
fn action_decls_of(
    table: &Table,
    action_steps: &BTreeMap<String, Vec<crate::context::ActionStep>>,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<String, crate::context::ActionDecl> {
    let Some(actions) = table.get("actions").and_then(|e| e.node.as_table()) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (name, entry) in actions {
        let Some(action) = entry.node.as_table() else {
            continue;
        };
        let decl = crate::context::ActionDecl {
            args: string_table(action, "args"),
            result: string_table(action, "result"),
            long: matches!(
                action.get("long").map(|e| &e.node.value),
                Some(Value::Bool(true))
            ),
            native: action
                .get("native")
                .and_then(|e| e.node.as_table())
                .map(|t| t.iter().map(|(k, e)| (k.clone(), e.node.clone())).collect()),
            description: action
                .get("description")
                .and_then(|e| e.node.as_str())
                .map(str::to_owned),
        };
        for arg in step_args(action_steps.get(name).map_or(&[][..], Vec::as_slice)) {
            if !decl.args.contains_key(arg) {
                diagnostics.push(
                    Diagnostic::new(
                        "J0821",
                        format!("action `{name}`: `${arg}` is not declared in `args`"),
                    )
                    .at(entry.node.span),
                );
            }
        }
        out.insert(name.clone(), decl);
    }
    out
}

/// The class `action` members `[implements."<class>"].map` binds to one
/// of this profile's actions (`m = "actions.<a>"`, ref/03 §2.2.14).
fn implemented_actions_of(
    table: &Table,
    actions: &BTreeMap<String, Option<Duration>>,
) -> BTreeMap<String, String> {
    let Some(implements) = table.get("implements").and_then(|e| e.node.as_table()) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for entry in implements.values() {
        let Some(map) = entry
            .node
            .as_table()
            .and_then(|t| t.get("map"))
            .and_then(|e| e.node.as_table())
        else {
            continue;
        };
        for (member, target) in map {
            if let Some(action) = target
                .node
                .as_str()
                .and_then(|t| t.strip_prefix("actions."))
                .filter(|a| actions.contains_key(*a))
            {
                out.insert(member.clone(), action.to_owned());
            }
        }
    }
    out
}
