//! Reads a `.conformance.toml` file of kind `core` into the case model,
//! with the checks and codes of ref/03 §2.1 and §2.6.

use std::path::Path;

use joker_core::ValueType;
use joker_core::output::{ClockRequirement, Level, OnRange, WriterKind};
use joker_core::time::ClockQuality;
use toml::{Table, Value};

use super::literal::{self, LiteralError, QualityLit};
use super::model::{
    Action, ActionKind, Case, Diagnostic, Expect, ExpectEntry, Expected, Feature, OutputDef,
    Predicates, SafeDef, SignalDef, Subject, Suite, When, WriterDef,
};

/// The suffix of a conformance suite file (ref/00, ADR 0026).
pub const SUFFIX: &str = ".conformance.toml";

/// The version of the schema this runner reads (`joker = "M.m"`).
const RUNTIME: (u32, u32) = (1, 0);

const ROOT_KEYS: [&str; 11] = [
    "schema",
    "joker",
    "suite",
    "signals",
    "outputs",
    "writers",
    "atomic",
    "alarms",
    "calc",
    "instances",
    "case",
];

const SUITE_KEYS: [&str; 10] = [
    "kind",
    "id",
    "class",
    "spec",
    "period",
    "hosts",
    "requires",
    "classes",
    "tiers",
    "description",
];

const SIGNAL_KEYS: [&str; 6] = ["path", "type", "unit", "quantity", "enum", "max_age"];

const OUTPUT_SUPPORTED: [&str; 15] = [
    "path",
    "type",
    "unit",
    "quantity",
    "enum",
    "safe",
    "safe_ramp",
    "range",
    "on_range",
    "rate",
    "max_lease",
    "max_manual",
    "on_interlock",
    "permit_uncertain",
    "grace",
];

const OUTPUT_MISSING: [(&str, Feature); 12] = [
    ("interlock", feature("interlock", "01 §7 step 2")),
    (
        "requires_good",
        feature("requires_good", "01 §7 step 3, 16 E4"),
    ),
    ("deadband", feature("deadband", "01 §7 step 6, 16 E8")),
    (
        "min_interval",
        feature("min_interval", "01 §7 step 6, 16 E8"),
    ),
    ("max_actuations", feature("max_actuations", "01 §7 step 6")),
    ("reset_when", feature("reset_when", "16 E7")),
    ("verify", feature("verify", "ref/01 §5.10")),
    ("safer", feature("safer", "ref/01 §5.10")),
    ("keepalive", feature("keepalive", "01 §8")),
    ("pulse", feature("pulse", "ref/01 §5.10")),
    ("group", feature("cycle groups", "16 A11")),
    ("remote_levels", feature("remote levels", "16 A12")),
];

const OUTPUT_IGNORED: [&str; 2] = ["accept_missing_fallback", "widen"];

const OUTPUT_RESERVED: [&str; 4] = ["schedule_depth", "timed", "on_late", "late_margin"];

const WRITER_KEYS: [&str; 6] = ["id", "level", "lease", "kind", "ttl", "requires_clock"];

const ATOMIC_KEYS: [&str; 5] = ["id", "outputs", "order", "commit", "on_rate"];

const CALC_KEYS: [&str; 2] = ["path", "expr"];

const INSTANCE_KEYS: [&str; 4] = ["name", "class", "map", "capabilities"];

const CASE_KEYS: [&str; 6] = ["id", "name", "spec", "duration", "do", "expect"];

const ACTION_KINDS: [&str; 7] = [
    "input",
    "propose",
    "method",
    "release",
    "reset",
    "clock",
    "generation",
];

const PREDICATES: [&str; 9] = [
    "equals",
    "min",
    "max",
    "tolerance",
    "quality",
    "outcome",
    "level",
    "state",
    "active",
];

const fn feature(name: &'static str, spec: &'static str) -> Feature {
    Feature { name, spec }
}

const CALC: Feature = feature("expressions (calc)", "02 §10, 16 §4");
const ALARMS: Feature = feature("alarms", "02 §11, ref/01 §5.15");
const ATOMIC: Feature = feature("atomic groups", "01 §7 step 7, 02 §7.3");
const ALARM_TRIP: Feature = feature("alarm trip actions", "ref/01 §5.15");
const RANGE_EXPR: Feature = feature("range expressions", "16 E6");
const METHODS: Feature = feature("methods", "17 §4.1, ref/03 §2.6.2");
const SELECTORS: Feature = feature("selectors in cases", "ref/03 §2.1.4");
const SIGNAL_ENUM: Feature = feature("enum signals", "02 §2");

/// Collects diagnostics while reading one file.
#[derive(Debug, Default)]
struct Sink {
    diagnostics: Vec<Diagnostic>,
}

impl Sink {
    fn push(&mut self, code: &'static str, location: &str, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            code,
            location: location.to_string(),
            message: message.into(),
        });
    }

    fn literal(&mut self, location: &str, err: LiteralError) {
        self.push(err.code, location, err.message);
    }
}

fn check_keys(sink: &mut Sink, table: &Table, allowed: &[&str], location: &str) {
    for key in table.keys() {
        if key.starts_with("x_") || allowed.contains(&key.as_str()) {
            continue;
        }
        if key.starts_with('_') {
            sink.push("J0141", location, format!("key `{key}` is reserved"));
        } else {
            sink.push("J0140", location, format!("unknown key `{key}`"));
        }
    }
}

fn get_str<'t>(sink: &mut Sink, table: &'t Table, key: &str, location: &str) -> Option<&'t str> {
    match table.get(key) {
        None => None,
        Some(Value::String(s)) => Some(s),
        Some(_) => {
            sink.push("J0143", location, format!("`{key}` must be a string"));
            None
        }
    }
}

fn require_str<'t>(
    sink: &mut Sink,
    table: &'t Table,
    key: &str,
    location: &str,
) -> Option<&'t str> {
    if !table.contains_key(key) {
        sink.push("J0142", location, format!("missing required key `{key}`"));
        return None;
    }
    get_str(sink, table, key, location)
}

fn str_list(sink: &mut Sink, table: &Table, key: &str, location: &str) -> Vec<String> {
    match table.get(key) {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => {
                    sink.push("J0143", location, format!("`{key}` must list strings"));
                    None
                }
            })
            .collect(),
        Some(_) => {
            sink.push("J0143", location, format!("`{key}` must be a list"));
            Vec::new()
        }
    }
}

fn tables<'t>(sink: &mut Sink, root: &'t Table, key: &str) -> Vec<&'t Table> {
    match root.get(key) {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::Table(t) => Some(t),
                _ => {
                    sink.push("J0143", key, format!("`[[{key}]]` entries must be tables"));
                    None
                }
            })
            .collect(),
        Some(_) => {
            sink.push("J0143", key, format!("`{key}` must be an array of tables"));
            Vec::new()
        }
    }
}

fn duration_key(sink: &mut Sink, table: &Table, key: &str, location: &str) -> Option<u64> {
    let text = get_str(sink, table, key, location)?;
    match literal::duration_ns(text) {
        Ok(ns) => Some(ns),
        Err(e) => {
            sink.literal(&format!("{location}.{key}"), e);
            None
        }
    }
}

fn value_type(name: &str) -> Option<ValueType> {
    Some(match name {
        "bool" => ValueType::Bool,
        "i64" => ValueType::I64,
        "u64" => ValueType::U64,
        "f64" => ValueType::F64,
        "f32" => ValueType::F32,
        "enum" => ValueType::Enum,
        "bits" => ValueType::Bits,
        "string" => ValueType::String,
        "bytes" => ValueType::Bytes,
        _ => return None,
    })
}

/// Encodes a literal value for a signal of type `ty` in `unit`.
fn expected_value(value: &Value, ty: ValueType, unit: &str) -> Result<Expected, LiteralError> {
    let wrong = |what: &str| LiteralError {
        code: "J0143",
        message: format!("{what} does not fit a `{}` signal", type_name(ty)),
    };
    match (value, ty) {
        (Value::Boolean(b), ValueType::Bool) => Ok(Expected::Bool(*b)),
        (Value::Boolean(_), _) => Err(wrong("a boolean")),
        (Value::String(s), _) if s == "relinquish" => Ok(Expected::Relinquish),
        (_, ValueType::Bool) => Err(wrong("a number")),
        (Value::Integer(i), ValueType::I64 | ValueType::U64) => {
            Ok(Expected::Integer(i128::from(*i)))
        }
        (Value::Integer(i), _) => Ok(Expected::Number(*i as f64)),
        (Value::Float(f), _) => Ok(Expected::Number(*f)),
        (Value::String(s), _) => literal::quantity_in(s, unit).map(Expected::Number),
        _ => Err(wrong("this value")),
    }
}

fn type_name(ty: ValueType) -> &'static str {
    match ty {
        ValueType::Bool => "bool",
        ValueType::I64 => "i64",
        ValueType::U64 => "u64",
        ValueType::F64 => "f64",
        ValueType::F32 => "f32",
        ValueType::Enum => "enum",
        ValueType::Bits => "bits",
        ValueType::String => "string",
        ValueType::Bytes => "bytes",
        ValueType::Record => "record",
    }
}

/// The bits of a value in the slot of a signal of type `ty` (ref/00 §4.1).
/// A value that does not fit the type keeps its natural encoding, so that a
/// proposal can reach the core and be judged there (16 E10).
fn encode(expected: Expected, ty: ValueType) -> u64 {
    match expected {
        Expected::Bool(b) => u64::from(b),
        Expected::Relinquish => 0,
        Expected::Integer(i) => match ty {
            ValueType::F64 | ValueType::F32 => (i as f64).to_bits(),
            _ => i as u64,
        },
        Expected::Number(x) => match ty {
            ValueType::I64 if x.fract() == 0.0 => x as i64 as u64,
            ValueType::U64 if x.fract() == 0.0 && x >= 0.0 => x as u64,
            ValueType::F32 => f64::from(x as f32).to_bits(),
            _ => x.to_bits(),
        },
    }
}

fn number_in(value: &Value, unit: &str, difference: bool) -> Result<f64, LiteralError> {
    match value {
        Value::Integer(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        Value::String(s) if difference => literal::difference_in(s, unit),
        Value::String(s) => literal::quantity_in(s, unit),
        _ => Err(LiteralError {
            code: "J0143",
            message: "expected a number or a quantity".to_string(),
        }),
    }
}

/// Reads one suite file. `Err` holds the diagnostics that keep the whole
/// suite from running; case-level problems stay in each [`Case`].
pub fn load(path: &Path, text: &str) -> Result<Suite, Vec<Diagnostic>> {
    let mut sink = Sink::default();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !name.ends_with(SUFFIX) {
        sink.push(
            "J0803",
            "file",
            format!("a conformance suite file name ends in `{SUFFIX}`"),
        );
        return Err(sink.diagnostics);
    }
    check_schema_first(&mut sink, text);
    let root = match text.parse::<Table>() {
        Ok(root) => root,
        Err(e) => {
            sink.push("J0146", "file", format!("not valid TOML: {}", e.message()));
            return Err(sink.diagnostics);
        }
    };
    match root.get("schema") {
        Some(Value::Integer(1)) => {}
        Some(_) => sink.push("J0801", "schema", "unsupported schema version (v1 reads 1)"),
        None => sink.push("J0800", "schema", "missing `schema`"),
    }
    if let Some(version) = get_str(&mut sink, &root, "joker", "joker") {
        match parse_version(version) {
            Some(v) if v > RUNTIME => sink.push(
                "J0802",
                "joker",
                format!("the file needs runtime {version}; this runner reads 1.0"),
            ),
            Some(_) => {}
            None => sink.push("J0806", "joker", format!("malformed version {version:?}")),
        }
    }
    check_keys(&mut sink, &root, &ROOT_KEYS, "root");
    let suite = match root.get("suite") {
        Some(Value::Table(t)) => t,
        Some(_) => {
            sink.push("J0143", "suite", "`[suite]` must be a table");
            return Err(sink.diagnostics);
        }
        None => {
            sink.push(
                "J0803",
                "suite",
                "missing `[suite]`: the content is not a suite",
            );
            return Err(sink.diagnostics);
        }
    };
    let Some(header) = read_header(&mut sink, path, suite) else {
        return Err(sink.diagnostics);
    };
    let mut fixture = Fixture::read(&mut sink, &root);
    let cases = tables(&mut sink, &root, "case");
    if cases.is_empty() {
        sink.push("J0142", "case", "a suite has one or more `[[case]]`");
    }
    if cases.len() > 1000 {
        sink.push(
            "J0149",
            "case",
            "more than 1,000 cases in a suite (ref/03 §2.1.5)",
        );
    }
    if !sink.diagnostics.is_empty() {
        return Err(sink.diagnostics);
    }
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for (i, table) in cases.into_iter().enumerate() {
        let case = read_case(&mut fixture, header.period_ns, i, table);
        if !case.id.is_empty() && seen.contains(&case.id) {
            sink.push(
                "J0805",
                &format!("case[{i}]"),
                format!("case id `{}` is not unique in the suite", case.id),
            );
        }
        seen.push(case.id.clone());
        out.push(case);
    }
    if !sink.diagnostics.is_empty() {
        return Err(sink.diagnostics);
    }
    Ok(Suite {
        id: header.id,
        spec: header.spec,
        period_ns: header.period_ns,
        signals: fixture.signals,
        outputs: fixture.outputs,
        writers: fixture.writers,
        cases: out,
    })
}

fn check_schema_first(sink: &mut Sink, text: &str) {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'));
    let ok = first.is_some_and(|l| {
        l.strip_prefix("schema")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    });
    if !ok {
        sink.push(
            "J0800",
            "schema",
            "`schema` must be the first key/value pair of the file",
        );
    }
}

fn parse_version(text: &str) -> Option<(u32, u32)> {
    let (major, minor) = text.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

#[derive(Debug)]
struct Header {
    id: String,
    spec: Vec<String>,
    period_ns: u64,
}

fn read_header(sink: &mut Sink, path: &Path, suite: &Table) -> Option<Header> {
    check_keys(sink, suite, &SUITE_KEYS, "suite");
    let kind = require_str(sink, suite, "kind", "suite");
    let expected = expected_kind(path);
    match kind {
        Some("core") if expected.is_none_or(|k| k == "core") => {}
        Some(k @ ("core" | "class" | "driver")) => {
            let place = expected.unwrap_or("a core location");
            sink.push(
                "J0845",
                "suite.kind",
                format!("kind `{k}` does not match the location of the file ({place}); this runner reads `core` suites"),
            );
        }
        Some(k) => sink.push("J0144", "suite.kind", format!("unknown kind `{k}`")),
        None => {}
    }
    for key in ["class", "requires", "tiers"] {
        if suite.contains_key(key) {
            sink.push(
                "J0145",
                "suite",
                format!("`{key}` does not apply to a core suite"),
            );
        }
    }
    let id = require_str(sink, suite, "id", "suite").map(str::to_string);
    if let Some(id) = &id
        && !literal::is_valid_id(id)
    {
        sink.push("J0805", "suite.id", format!("invalid suite id {id:?}"));
    }
    let spec = str_list(sink, suite, "spec", "suite");
    for host in str_list(sink, suite, "hosts", "suite") {
        if host != "linux" && host != "mcu" {
            sink.push("J0144", "suite.hosts", format!("unknown host `{host}`"));
        }
    }
    get_str(sink, suite, "description", "suite");
    if !suite.contains_key("period") {
        sink.push(
            "J0142",
            "suite",
            "missing required key `period` in a core suite",
        );
    }
    let period_ns = duration_key(sink, suite, "period", "suite");
    if period_ns == Some(0) {
        sink.push("J0806", "suite.period", "the period must be positive");
    }
    Some(Header {
        id: id?,
        spec,
        period_ns: period_ns.filter(|p| *p > 0)?,
    })
}

fn expected_kind(path: &Path) -> Option<&'static str> {
    let parent = path.parent()?;
    let area = parent.file_name()?.to_str()?;
    let top = parent.parent()?.file_name()?.to_str()?;
    match (top, area) {
        ("conformance", "drivers") => Some("driver"),
        ("conformance", _) => Some("core"),
        _ => None,
    }
}

#[derive(Debug, Default)]
struct Fixture {
    signals: Vec<SignalDef>,
    outputs: Vec<OutputDef>,
    writers: Vec<WriterDef>,
    calcs: Vec<String>,
    alarms: Vec<String>,
    instances: Vec<String>,
}

impl Fixture {
    fn read(sink: &mut Sink, root: &Table) -> Self {
        let mut f = Self::default();
        for (i, t) in tables(sink, root, "signals").into_iter().enumerate() {
            if let Some(s) = read_signal(sink, t, &format!("signals[{i}]")) {
                f.signals.push(s);
            }
        }
        for (i, t) in tables(sink, root, "outputs").into_iter().enumerate() {
            if let Some(o) = read_output(sink, t, &format!("outputs[{i}]")) {
                f.outputs.push(o);
            }
        }
        for (i, t) in tables(sink, root, "writers").into_iter().enumerate() {
            let location = format!("writers[{i}]");
            if let Some(w) = read_writer(sink, t, &location) {
                if f.writers.iter().any(|x| x.id == w.id) {
                    sink.push(
                        "J0805",
                        &location,
                        format!("writer id `{}` is repeated", w.id),
                    );
                }
                f.writers.push(w);
            }
        }
        for (i, t) in tables(sink, root, "calc").into_iter().enumerate() {
            let location = format!("calc[{i}]");
            check_keys(sink, t, &CALC_KEYS, &location);
            require_str(sink, t, "expr", &location);
            if let Some(path) = require_str(sink, t, "path", &location) {
                f.calcs.push(path.to_string());
            }
        }
        for (i, t) in tables(sink, root, "alarms").into_iter().enumerate() {
            let location = format!("alarms[{i}]");
            if let Some(id) = require_str(sink, t, "id", &location) {
                f.alarms.push(id.to_string());
            }
            let mut targets = Vec::new();
            if let Some(actions) = t.get("actions") {
                strings_in(actions, &mut targets);
            }
            for o in &mut f.outputs {
                if targets.contains(&o.path) {
                    add_feature(&mut o.missing, ALARM_TRIP);
                }
            }
        }
        for (i, t) in tables(sink, root, "atomic").into_iter().enumerate() {
            let location = format!("atomic[{i}]");
            check_keys(sink, t, &ATOMIC_KEYS, &location);
            require_str(sink, t, "id", &location);
            for path in str_list(sink, t, "outputs", &location) {
                match f.outputs.iter_mut().find(|o| o.path == path) {
                    Some(o) => add_feature(&mut o.missing, ATOMIC),
                    None => sink.push("J0844", &location, format!("unknown output `{path}`")),
                }
            }
        }
        for (i, t) in tables(sink, root, "instances").into_iter().enumerate() {
            let location = format!("instances[{i}]");
            check_keys(sink, t, &INSTANCE_KEYS, &location);
            if let Some(name) = require_str(sink, t, "name", &location) {
                f.instances.push(name.to_string());
            }
        }
        let mut paths: Vec<&str> = f
            .signals
            .iter()
            .map(|s| s.path.as_str())
            .chain(f.outputs.iter().map(|o| o.path.as_str()))
            .chain(f.calcs.iter().map(String::as_str))
            .collect();
        paths.sort_unstable();
        for pair in paths.windows(2) {
            if pair[0] == pair[1] {
                sink.push(
                    "J0805",
                    "fixture",
                    format!("path `{}` is declared twice", pair[0]),
                );
            }
        }
        f
    }
}

fn strings_in(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|v| strings_in(v, out)),
        Value::Table(t) => t.values().for_each(|v| strings_in(v, out)),
        _ => {}
    }
}

fn add_feature(list: &mut Vec<Feature>, f: Feature) {
    if !list.contains(&f) {
        list.push(f);
        list.sort();
    }
}

fn read_type(sink: &mut Sink, t: &Table, location: &str) -> Option<ValueType> {
    let name = require_str(sink, t, "type", location)?;
    let ty = value_type(name);
    if ty.is_none() {
        sink.push("J0144", location, format!("unknown signal type `{name}`"));
    }
    ty
}

fn read_signal(sink: &mut Sink, t: &Table, location: &str) -> Option<SignalDef> {
    check_keys(sink, t, &SIGNAL_KEYS, location);
    let path = require_str(sink, t, "path", location)?.to_string();
    let ty = read_type(sink, t, location)?;
    let unit = get_str(sink, t, "unit", location)
        .unwrap_or("1")
        .to_string();
    let max_age_ns = duration_key(sink, t, "max_age", location);
    let mut missing = Vec::new();
    if t.contains_key("enum") || !scalar(ty) {
        add_feature(&mut missing, SIGNAL_ENUM);
    }
    Some(SignalDef {
        path,
        ty,
        unit,
        max_age_ns,
        missing,
    })
}

fn scalar(ty: ValueType) -> bool {
    matches!(
        ty,
        ValueType::Bool | ValueType::I64 | ValueType::U64 | ValueType::F64 | ValueType::F32
    )
}

fn read_output(sink: &mut Sink, t: &Table, location: &str) -> Option<OutputDef> {
    let known: Vec<&str> = OUTPUT_SUPPORTED
        .iter()
        .copied()
        .chain(OUTPUT_MISSING.iter().map(|(k, _)| *k))
        .chain(OUTPUT_IGNORED)
        .chain(OUTPUT_RESERVED)
        .collect();
    check_keys(sink, t, &known, location);
    for key in OUTPUT_RESERVED {
        if t.contains_key(key) {
            sink.push("J0807", location, format!("`{key}` is reserved for 1.1"));
        }
    }
    let path = require_str(sink, t, "path", location)?.to_string();
    let ty = read_type(sink, t, location)?;
    let unit = get_str(sink, t, "unit", location)
        .unwrap_or("1")
        .to_string();
    let mut missing = Vec::new();
    for (key, f) in OUTPUT_MISSING {
        if t.contains_key(key) {
            add_feature(&mut missing, f);
        }
    }
    if t.contains_key("enum") || !scalar(ty) {
        add_feature(&mut missing, SIGNAL_ENUM);
    }
    let safe = match t.get("safe") {
        None => {
            sink.push("J0201", location, "every output declares `safe` (ADR 0006)");
            None
        }
        Some(v) => match expected_value(v, ty, &unit) {
            Ok(Expected::Relinquish) => Some(SafeDef::Relinquish),
            Ok(e) => Some(SafeDef::Value(encode(e, ty))),
            Err(e) => {
                sink.literal(&format!("{location}.safe"), e);
                None
            }
        },
    };
    let rate_key = |sink: &mut Sink, key: &str| {
        let text = get_str(sink, t, key, location)?;
        literal::rate_in(text, &unit)
            .map_err(|e| sink.literal(&format!("{location}.{key}"), e))
            .ok()
    };
    let rate = rate_key(sink, "rate");
    let safe_ramp = rate_key(sink, "safe_ramp");
    let range = match t.get("range") {
        None => None,
        Some(Value::Array(items)) if items.len() == 2 => {
            let limits: Vec<_> = items.iter().map(|v| number_in(v, &unit, false)).collect();
            match (&limits[0], &limits[1]) {
                (Ok(low), Ok(high)) => Some((*low, *high)),
                _ if items.iter().any(is_expression) => {
                    add_feature(&mut missing, RANGE_EXPR);
                    None
                }
                (Err(e), _) | (_, Err(e)) => {
                    sink.literal(&format!("{location}.range"), e.clone());
                    None
                }
            }
        }
        Some(_) => {
            sink.push("J0230", location, "`range` has two limits");
            None
        }
    };
    let on_range = match get_str(sink, t, "on_range", location) {
        None | Some("clamp") => OnRange::Clamp,
        Some("reject") => OnRange::Reject,
        Some("trip") => OnRange::Trip,
        Some(other) => {
            sink.push("J0144", location, format!("unknown on_range `{other}`"));
            OnRange::Clamp
        }
    };
    if let Some(v) = get_str(sink, t, "on_interlock", location)
        && v != "safe"
        && v != "hold"
    {
        sink.push("J0144", location, format!("unknown on_interlock `{v}`"));
    }
    let max_lease_ns = duration_key(sink, t, "max_lease", location);
    let max_manual_ns = duration_key(sink, t, "max_manual", location);
    duration_key(sink, t, "grace", location);
    Some(OutputDef {
        path,
        ty,
        unit,
        safe: safe?,
        safe_ramp,
        range,
        on_range,
        rate,
        max_lease_ns,
        max_manual_ns,
        missing,
    })
}

fn is_expression(value: &Value) -> bool {
    match value {
        Value::String(s) => s
            .trim_start_matches(['-', '+', ' '])
            .starts_with(|c: char| c.is_ascii_alphabetic()),
        _ => false,
    }
}

fn read_writer(sink: &mut Sink, t: &Table, location: &str) -> Option<WriterDef> {
    check_keys(sink, t, &WRITER_KEYS, location);
    let id = require_str(sink, t, "id", location)?.to_string();
    let level_text = require_str(sink, t, "level", location)?;
    let level = Level::parse(level_text).filter(|_| level_text.contains(':'));
    if level.is_none() {
        sink.push(
            "J0806",
            &format!("{location}.level"),
            format!("malformed level {level_text:?} (`band:n`)"),
        );
    }
    let kind = match require_str(sink, t, "kind", location)? {
        "cycle" => Some(WriterKind::Cycle),
        "async" => Some(WriterKind::Async),
        "manual" => Some(WriterKind::Manual),
        "bridge" => Some(WriterKind::Bridge),
        "impl" => Some(WriterKind::Impl),
        "proposal" => {
            sink.push(
                "J0807",
                location,
                "writer kind `proposal` is reserved for 1.1",
            );
            None
        }
        other => {
            sink.push("J0144", location, format!("unknown writer kind `{other}`"));
            None
        }
    };
    let lease = duration_key(sink, t, "lease", location);
    let ttl = duration_key(sink, t, "ttl", location);
    let lease_ns = if kind == Some(WriterKind::Manual) {
        if ttl.is_none() {
            sink.push("J0142", location, "a manual writer requires `ttl` (16 A10)");
        }
        ttl
    } else {
        if ttl.is_some() {
            sink.push("J0145", location, "`ttl` only applies to manual writers");
        }
        lease
    };
    let requires_clock = match get_str(sink, t, "requires_clock", location) {
        None | Some("any") => ClockRequirement::Any,
        Some("holdover") => ClockRequirement::Holdover,
        Some("synced") => ClockRequirement::Synced,
        Some(other) => {
            sink.push(
                "J0144",
                location,
                format!("unknown requires_clock `{other}`"),
            );
            ClockRequirement::Any
        }
    };
    Some(WriterDef {
        id,
        level: level?,
        kind: kind?,
        lease_ns,
        requires_clock,
    })
}

fn read_case(fixture: &mut Fixture, period_ns: u64, index: usize, t: &Table) -> Case {
    let mut sink = Sink::default();
    let location = format!("case[{index}]");
    let id = require_str(&mut sink, t, "id", &location)
        .unwrap_or_default()
        .to_string();
    let location = if id.is_empty() {
        location
    } else {
        format!("case[{index}] {id}")
    };
    if !id.is_empty() && !literal::is_valid_id(&id) {
        sink.push("J0805", &location, format!("invalid case id {id:?}"));
    }
    if t.contains_key("sim_only") {
        sink.push(
            "J0140",
            &location,
            "`sim_only` does not exist in core suites",
        );
    }
    for key in ["requires", "level"] {
        if t.contains_key(key) {
            sink.push(
                "J0145",
                &location,
                format!("`{key}` does not apply to a core case"),
            );
        }
    }
    let allowed: Vec<&str> = CASE_KEYS
        .iter()
        .copied()
        .chain(["sim_only", "requires", "level"])
        .collect();
    check_keys(&mut sink, t, &allowed, &location);
    let name = get_str(&mut sink, t, "name", &location).map(str::to_string);
    let spec = str_list(&mut sink, t, "spec", &location);
    let duration = duration_key(&mut sink, t, "duration", &location);
    let mut ctx = CaseCtx {
        sink,
        fixture,
        period_ns,
        missing: Vec::new(),
        last_tick: 0,
    };
    let mut actions = Vec::new();
    match t.get("do") {
        None => {}
        Some(Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                let at = format!("{location}.do[{i}]");
                match item {
                    Value::Table(a) => {
                        if let Some(action) = ctx.action(a, &at) {
                            actions.push(action);
                        }
                    }
                    _ => ctx.sink.push("J0143", &at, "an action is an inline table"),
                }
            }
        }
        Some(_) => ctx
            .sink
            .push("J0143", &location, "`do` is a list of actions"),
    }
    let mut expects = Vec::new();
    match t.get("expect") {
        None => ctx
            .sink
            .push("J0142", &location, "missing required key `expect`"),
        Some(Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                let at = format!("{location}.expect[{i}]");
                if let Some(expect) = ctx.expect(item, &at) {
                    expects.push(ExpectEntry {
                        expect,
                        source: item.to_string(),
                    });
                }
            }
        }
        Some(_) => ctx.sink.push("J0143", &location, "`expect` is a list"),
    }
    let mut ticks = ctx.last_tick + 1;
    if let Some(d) = duration {
        ticks = ticks.max(d.div_ceil(period_ns));
    }
    Case {
        id,
        name,
        spec,
        ticks,
        actions,
        expects,
        missing: ctx.missing,
        errors: ctx.sink.diagnostics,
    }
}

#[derive(Debug)]
struct CaseCtx<'f> {
    sink: Sink,
    fixture: &'f mut Fixture,
    period_ns: u64,
    missing: Vec<Feature>,
    last_tick: u64,
}

impl CaseCtx<'_> {
    fn tick(&mut self, text: &str, location: &str) -> Option<u64> {
        match literal::instant_ns(text) {
            Ok(ns) => self.tick_ns(ns, text, location),
            Err(e) => {
                self.sink.literal(location, e);
                None
            }
        }
    }

    fn tick_ns(&mut self, ns: u64, text: &str, location: &str) -> Option<u64> {
        if !ns.is_multiple_of(self.period_ns) {
            self.sink.push(
                "J0843",
                location,
                format!(
                    "instant {text:?} is not a multiple of the period {}",
                    literal::format_ns(self.period_ns)
                ),
            );
            return None;
        }
        let tick = ns / self.period_ns;
        self.last_tick = self.last_tick.max(tick);
        Some(tick)
    }

    fn output(&mut self, path: &str, location: &str) -> Option<usize> {
        match self.fixture.outputs.iter().position(|o| o.path == path) {
            Some(i) => {
                for f in self.fixture.outputs[i].missing.clone() {
                    add_feature(&mut self.missing, f);
                }
                Some(i)
            }
            None => {
                self.sink
                    .push("J0844", location, format!("unknown output `{path}`"));
                None
            }
        }
    }

    fn input(&mut self, path: &str, location: &str) -> Option<usize> {
        match self.fixture.signals.iter().position(|s| s.path == path) {
            Some(i) => {
                for f in self.fixture.signals[i].missing.clone() {
                    add_feature(&mut self.missing, f);
                }
                Some(i)
            }
            None => {
                self.sink
                    .push("J0844", location, format!("unknown input `{path}`"));
                None
            }
        }
    }

    fn writer(&mut self, table: &Table, location: &str) -> Option<usize> {
        let id = require_str(&mut self.sink, table, "writer", location)?;
        let found = self.fixture.writers.iter().position(|w| w.id == id);
        if found.is_none() {
            self.sink
                .push("J0844", location, format!("unknown writer `{id}`"));
        }
        found
    }

    fn action(&mut self, a: &Table, location: &str) -> Option<Action> {
        let kinds: Vec<&str> = ACTION_KINDS
            .iter()
            .copied()
            .filter(|k| a.contains_key(*k))
            .collect();
        let [kind] = kinds.as_slice() else {
            self.sink.push(
                "J0145",
                location,
                "an action has exactly one of input, propose, method, release, reset, clock, generation",
            );
            return None;
        };
        let extra: &[&str] = match *kind {
            "input" => &["value", "quality"],
            "propose" => &["writer", "value"],
            "method" => &["writer", "args"],
            "release" => &["writer"],
            _ => &[],
        };
        let allowed: Vec<&str> = ["at", *kind].iter().chain(extra).copied().collect();
        check_keys(&mut self.sink, a, &allowed, location);
        let at = require_str(&mut self.sink, a, "at", location);
        let tick = self.tick(at?, &format!("{location}.at"))?;
        let target = get_str(&mut self.sink, a, kind, location);
        let kind = match *kind {
            "input" => {
                let signal = self.input(target?, location)?;
                let quality = match get_str(&mut self.sink, a, "quality", location) {
                    None => QualityLit {
                        code: 0,
                        reason: None,
                    },
                    Some(q) => match literal::quality(q) {
                        Ok(q) => q,
                        Err(e) => {
                            self.sink.literal(&format!("{location}.quality"), e);
                            return None;
                        }
                    },
                };
                let def = &self.fixture.signals[signal];
                let bits = match a.get("value") {
                    None if quality.code == 2 => 0,
                    None => {
                        self.sink
                            .push("J0142", location, "an input without `value` must be `bad`");
                        return None;
                    }
                    Some(v) => match expected_value(v, def.ty, &def.unit) {
                        Ok(e) => encode(e, def.ty),
                        Err(e) => {
                            self.sink.literal(&format!("{location}.value"), e);
                            return None;
                        }
                    },
                };
                ActionKind::Input {
                    signal,
                    bits,
                    quality,
                }
            }
            "propose" => {
                let output = self.output(target?, location);
                let writer = self.writer(a, location);
                let (output, writer) = (output?, writer?);
                let def = &self.fixture.outputs[output];
                let Some(v) = a.get("value") else {
                    self.sink
                        .push("J0142", location, "a proposal requires `value`");
                    return None;
                };
                let bits = match v {
                    Value::Float(f) => encode(Expected::Number(*f), def.ty),
                    _ => match expected_value(v, def.ty, &def.unit) {
                        Ok(e) => encode(e, def.ty),
                        Err(e) if e.code == "J0143" => match v {
                            Value::Boolean(b) => u64::from(*b),
                            Value::Integer(i) => *i as u64,
                            _ => {
                                self.sink.literal(&format!("{location}.value"), e);
                                return None;
                            }
                        },
                        Err(e) => {
                            self.sink.literal(&format!("{location}.value"), e);
                            return None;
                        }
                    },
                };
                ActionKind::Propose {
                    output,
                    writer,
                    bits,
                }
            }
            "release" => {
                let output = self.output(target?, location);
                let writer = self.writer(a, location);
                ActionKind::Release {
                    output: output?,
                    writer: writer?,
                }
            }
            "reset" => ActionKind::Reset {
                output: self.output(target?, location)?,
            },
            "clock" => ActionKind::Clock(match target? {
                "synced" => ClockQuality::Synced,
                "holdover" => ClockQuality::Holdover,
                "unsynced" => ClockQuality::Unsynced,
                other => {
                    self.sink.push(
                        "J0806",
                        location,
                        format!("malformed clock quality {other:?}"),
                    );
                    return None;
                }
            }),
            "generation" => {
                if target? != "same" {
                    self.sink
                        .push("J0144", location, "`generation` takes `\"same\"`");
                    return None;
                }
                ActionKind::Generation
            }
            _ => {
                let instance = target?;
                let name = instance.split('.').next().unwrap_or_default();
                if !self.fixture.instances.iter().any(|i| i == name) {
                    self.sink
                        .push("J0844", location, format!("unknown instance `{name}`"));
                }
                add_feature(&mut self.missing, METHODS);
                return None;
            }
        };
        Some(Action { tick, kind })
    }

    fn expect(&mut self, item: &Value, location: &str) -> Option<Expect> {
        let Value::Table(e) = item else {
            self.sink
                .push("J0143", location, "an expectation is an inline table");
            return None;
        };
        for (key, any) in [("any_of", true), ("all_of", false)] {
            if let Some(list) = e.get(key) {
                if e.len() != 1 {
                    self.sink.push(
                        "J0145",
                        location,
                        format!("`{key}` stands alone in its expectation"),
                    );
                    return None;
                }
                let Value::Array(items) = list else {
                    self.sink
                        .push("J0143", location, format!("`{key}` is a list"));
                    return None;
                };
                let inner: Vec<Expect> = items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| self.expect(v, &format!("{location}.{key}[{i}]")))
                    .collect();
                if inner.len() != items.len() {
                    return None;
                }
                return Some(if any {
                    Expect::AnyOf(inner)
                } else {
                    Expect::AllOf(inner)
                });
            }
        }
        self.check(e, location)
    }

    fn check(&mut self, e: &Table, location: &str) -> Option<Expect> {
        for key in ["within", "before"] {
            if e.contains_key(key) {
                self.sink.push(
                    "J0145",
                    location,
                    format!("`{key}` is not a core timing (core uses `at` and `during`)"),
                );
            }
        }
        for key in [
            "member",
            "query",
            "health",
            "event",
            "action",
            "instance",
            "result",
            "count",
            "fields",
            "toggles",
            "toggles_every",
            "pattern",
        ] {
            if e.contains_key(key) {
                self.sink.push(
                    "J0145",
                    location,
                    format!("`{key}` does not apply to a core expectation"),
                );
            }
        }
        if e.contains_key("metric") {
            self.sink
                .push("J0807", location, "`metric` is reserved for 1.1");
        }
        let allowed: Vec<&str> = ["at", "during", "output", "signal", "alarm"]
            .into_iter()
            .chain(PREDICATES)
            .chain([
                "within",
                "before",
                "member",
                "query",
                "health",
                "event",
                "action",
                "instance",
                "result",
                "count",
                "fields",
                "toggles",
                "toggles_every",
                "pattern",
                "metric",
            ])
            .collect();
        check_keys(&mut self.sink, e, &allowed, location);
        let when = match (e.get("at"), e.get("during")) {
            (Some(_), Some(_)) => {
                self.sink
                    .push("J0145", location, "`at` and `during` are exclusive");
                return None;
            }
            (None, None) => {
                self.sink
                    .push("J0142", location, "an expectation needs `at` or `during`");
                return None;
            }
            (Some(_), None) => {
                let at = get_str(&mut self.sink, e, "at", location)?;
                When::At(self.tick(at, &format!("{location}.at"))?)
            }
            (None, Some(_)) => {
                let text = get_str(&mut self.sink, e, "during", location)?;
                let (a, b) = match literal::interval_ns(text) {
                    Ok(v) => v,
                    Err(err) => {
                        self.sink.literal(&format!("{location}.during"), err);
                        return None;
                    }
                };
                let at = format!("{location}.during");
                let a = self.tick_ns(a, text, &at)?;
                let b = self.tick_ns(b, text, &at)?;
                When::During(a, b)
            }
        };
        let subjects: Vec<&str> = ["output", "signal", "alarm"]
            .into_iter()
            .filter(|k| e.contains_key(*k))
            .collect();
        let [subject_key] = subjects.as_slice() else {
            self.sink.push(
                "J0145",
                location,
                "an expectation has exactly one subject: output, signal or alarm",
            );
            return None;
        };
        let has = |k: &str| e.contains_key(k);
        if !PREDICATES.iter().any(|p| *p != "tolerance" && has(p)) {
            self.sink
                .push("J0840", location, "an expectation needs a predicate");
            return None;
        }
        if has("equals") && (has("min") || has("max")) {
            self.sink.push(
                "J0840",
                location,
                "`equals` does not combine with `min`/`max`",
            );
            return None;
        }
        if has("tolerance") && !has("equals") {
            self.sink
                .push("J0840", location, "`tolerance` goes with `equals`");
            return None;
        }
        let target = get_str(&mut self.sink, e, subject_key, location)?;
        if *subject_key == "alarm" {
            if ["equals", "min", "max", "quality", "outcome", "level"]
                .iter()
                .any(|p| has(p))
            {
                self.sink.push(
                    "J0840",
                    location,
                    "an alarm takes only `state` and `active`",
                );
                return None;
            }
            if let Some(state) = get_str(&mut self.sink, e, "state", location)
                && let Err(err) = literal::alarm_state(state)
            {
                self.sink.literal(&format!("{location}.state"), err);
            }
            if !self.fixture.alarms.iter().any(|a| a == target) {
                self.sink
                    .push("J0844", location, format!("unknown alarm `{target}`"));
                return None;
            }
            add_feature(&mut self.missing, ALARMS);
            return None;
        }
        if has("state") || has("active") {
            self.sink
                .push("J0840", location, "`state` and `active` apply to alarms");
            return None;
        }
        let subject = if *subject_key == "output" {
            Subject::Output(self.output(target, location)?)
        } else {
            if has("outcome") || has("level") {
                self.sink.push(
                    "J0840",
                    location,
                    "`outcome` and `level` apply to an `output` subject",
                );
                return None;
            }
            self.signal_subject(target, location)?
        };
        let (ty, unit) = match &subject {
            Subject::Output(i) | Subject::OutputSignal(i) => {
                let o = &self.fixture.outputs[*i];
                (o.ty, o.unit.clone())
            }
            Subject::Input(i) => {
                let s = &self.fixture.signals[*i];
                (s.ty, s.unit.clone())
            }
        };
        let mut p = Predicates::default();
        let mut bad = false;
        if let Some(v) = e.get("equals") {
            match expected_value(v, ty, &unit) {
                Ok(x) => p.equals = Some(x),
                Err(err) => {
                    self.sink.literal(&format!("{location}.equals"), err);
                    bad = true;
                }
            }
        }
        for (key, slot) in [("min", &mut p.min), ("max", &mut p.max)] {
            if let Some(v) = e.get(key) {
                match number_in(v, &unit, false) {
                    Ok(x) => *slot = Some(x),
                    Err(err) => {
                        self.sink.literal(&format!("{location}.{key}"), err);
                        bad = true;
                    }
                }
            }
        }
        if let Some(v) = e.get("tolerance") {
            match number_in(v, &unit, true) {
                Ok(x) if x >= 0.0 => p.tolerance = Some(x),
                Ok(_) => {
                    self.sink
                        .push("J0806", location, "a tolerance is not negative");
                    bad = true;
                }
                Err(err) => {
                    self.sink.literal(&format!("{location}.tolerance"), err);
                    bad = true;
                }
            }
        }
        if let Some(q) = get_str(&mut self.sink, e, "quality", location) {
            match literal::quality(q) {
                Ok(q) => p.quality = Some(q),
                Err(err) => {
                    self.sink.literal(&format!("{location}.quality"), err);
                    bad = true;
                }
            }
        }
        if let Some(o) = get_str(&mut self.sink, e, "outcome", location) {
            match literal::outcome(o) {
                Ok(o) => p.outcome = Some(o),
                Err(err) => {
                    self.sink.literal(&format!("{location}.outcome"), err);
                    bad = true;
                }
            }
        }
        if let Some(l) = get_str(&mut self.sink, e, "level", location) {
            match literal::level(l) {
                Ok(l) => p.level = Some(l),
                Err(err) => {
                    self.sink.literal(&format!("{location}.level"), err);
                    bad = true;
                }
            }
        }
        if bad {
            return None;
        }
        Some(Expect::Check {
            when,
            subject,
            predicates: p,
        })
    }

    fn signal_subject(&mut self, path: &str, location: &str) -> Option<Subject> {
        if path.contains('*') {
            add_feature(&mut self.missing, SELECTORS);
            return None;
        }
        if self.fixture.calcs.iter().any(|c| c == path) {
            add_feature(&mut self.missing, CALC);
            return None;
        }
        if self.fixture.signals.iter().any(|s| s.path == path) {
            return self.input(path, location).map(Subject::Input);
        }
        if self.fixture.outputs.iter().any(|o| o.path == path) {
            return self.output(path, location).map(Subject::OutputSignal);
        }
        self.sink
            .push("J0844", location, format!("unknown signal `{path}`"));
        None
    }
}
