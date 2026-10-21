//! The `*.sim.toml` model format (ref/03 §2.4, ADR 0051, ADR 0054):
//! `[model]` (with `profile` reuse), `[params]`, `[signals]` with every
//! generator, noise, resolution, bounce, latency, limits and panels,
//! `[dynamics]`, `[faults]` (scheduled faults and fault keys), `[enums]`,
//! `[actions]`, `[implements]` and `[plant]`.

use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::vec::Vec;

use joker_hal::types::{Code, RawStatus, Reason};
use joker_hal::{Cbor, QualityCode};
use joker_units::{QuantityKind, Unit};
use toml::{Table, Value};

use crate::codes::*;
use crate::expr::{self, Dynamics};
use crate::literal::{self, Literal, LiteralError};
use crate::profile::{self, PointType};
use crate::replay::{self, Series};

/// The runtime version this simulator implements, for the `joker` key.
pub const RUNTIME: (u32, u32) = (1, 0);
/// Signals per simulation model (ref/03 §2.1.5).
pub const MAX_SIGNALS: usize = 1024;
/// Variants per enum.
pub const MAX_VARIANTS: usize = 256;
/// Entries in `[implements]`.
pub const MAX_IMPLEMENTS: usize = 16;
/// Steps per action.
pub const MAX_STEPS: usize = 32;

/// An error in a model, with its code and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelError {
    /// Registry code.
    pub code: Code,
    /// Key path, such as `signals.cpu_temp.gen`.
    pub path: String,
    /// For humans.
    pub message: String,
}

impl core::fmt::Display for ModelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}: {}", self.code.as_str(), self.path, self.message)
    }
}

impl std::error::Error for ModelError {}

pub(crate) fn err(code: Code, path: &str, message: impl Into<String>) -> ModelError {
    ModelError {
        code,
        path: String::from(path),
        message: message.into(),
    }
}

fn lit_err(path: &str) -> impl Fn(LiteralError) -> ModelError + '_ {
    move |e| err(e.code, path, e.message)
}

/// Type of a simulated signal (document 02 §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SimType {
    /// `bool`.
    Bool,
    /// `i64`.
    I64,
    /// `u64`.
    U64,
    /// `f64`.
    F64,
    /// `f32`.
    F32,
    /// `enum<Name>`: the index of the enum in [`Model::enums`].
    Enum(usize),
    /// `bits<N>`, with `N` in 16, 32 and 64.
    Bits(u8),
    /// `string`, at most 64 bytes.
    String,
    /// `bytes`, at most 64 bytes.
    Bytes,
}

impl SimType {
    /// True for the types held as a number.
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::I64 | Self::U64 | Self::F64 | Self::F32)
    }

    /// True for `string` and `bytes`.
    pub const fn is_text(self) -> bool {
        matches!(self, Self::String | Self::Bytes)
    }
}

/// Largest `string` or `bytes` value (document 02 §2).
pub const MAX_TEXT: usize = 64;

/// Direction of a simulated signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SimDirection {
    /// Produced by the model.
    Input,
    /// Written by the core: the applied value.
    Output,
    /// Parameter.
    Param,
}

/// A named enum (ref/03 §2.2.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enum {
    /// Name.
    pub name: String,
    /// Variants and values, in file order.
    pub variants: Vec<(String, i64)>,
}

impl Enum {
    /// The value of `variant`.
    pub fn value(&self, variant: &str) -> Option<i64> {
        self.variants
            .iter()
            .find(|(n, _)| n == variant)
            .map(|(_, v)| *v)
    }

    /// The variant with `value`.
    pub fn variant(&self, value: i64) -> Option<&str> {
        self.variants
            .iter()
            .find(|(_, v)| *v == value)
            .map(|(n, _)| n.as_str())
    }
}

/// A generator (ref/03 §2.4.5); times in s, values in the signal's unit.
#[derive(Clone, Debug, PartialEq)]
pub enum Gen {
    /// `value`.
    Constant(f64),
    /// `from` before `at`, `to` afterwards.
    Step {
        /// Before.
        from: f64,
        /// After.
        to: f64,
        /// Switch time.
        at: f64,
    },
    /// Linear from `from` to `to` between `at` and `at + over`.
    Ramp {
        /// Start.
        from: f64,
        /// End.
        to: f64,
        /// Duration.
        over: f64,
        /// Start time.
        at: f64,
    },
    /// `mean + amp · sin(2π t/period + phase)`.
    Sine {
        /// Mean.
        mean: f64,
        /// Amplitude.
        amp: f64,
        /// Period.
        period: f64,
        /// Phase in degrees.
        phase: f64,
    },
    /// `high` for `duty` of each period, then `low`.
    Square {
        /// Low.
        low: f64,
        /// High.
        high: f64,
        /// Period.
        period: f64,
        /// Fraction high.
        duty: f64,
    },
    /// Gaussian noise.
    Noise {
        /// Mean.
        mean: f64,
        /// Standard deviation.
        std: f64,
    },
    /// Bounded random walk.
    Walk {
        /// Start.
        start: f64,
        /// Standard deviation per step.
        std: f64,
        /// Lower bound.
        min: f64,
        /// Upper bound.
        max: f64,
    },
    /// Recorded values.
    Replay(Series),
}

/// Kind of a panel control (ref/03 §2.4.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanelKind {
    /// Shows.
    Lamp,
    /// `true` for `pulse` on each press.
    Momentary,
    /// Inverts on each press.
    Toggle,
    /// Keys add `± step` and replace the generator.
    Slider,
    /// Shows; keys add an offset.
    Gauge,
}

/// A panel control.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    /// Kind.
    pub kind: PanelKind,
    /// Label, at most 24 bytes.
    pub label: Option<String>,
    /// Key of `momentary` and `toggle`.
    pub key: Option<String>,
    /// Decrease and increase keys.
    pub keys: Option<(String, String)>,
    /// Increment per key.
    pub step: f64,
    /// Lower bound of a slider.
    pub min: Option<f64>,
    /// Upper bound of a slider.
    pub max: Option<f64>,
    /// Time at `true` of a momentary press.
    pub pulse_ns: u64,
    /// Position in the panel; the file order without one.
    pub order: Option<i64>,
}

/// One simulated signal.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    /// Name, relative to the device.
    pub name: String,
    /// Type.
    pub ty: SimType,
    /// Unit, `1` without one.
    pub unit: Unit,
    /// Direction.
    pub direction: SimDirection,
    /// Initial value of a numeric, `bool`, enum or bits signal.
    pub init: f64,
    /// Initial value of a `string` or `bytes` signal.
    pub init_text: Vec<u8>,
    /// Generator.
    pub generator: Option<Gen>,
    /// Standard deviation of the additive noise.
    pub noise: Option<f64>,
    /// Quantization step.
    pub resolution: Option<f64>,
    /// Contact bounce of a `bool`, in ns.
    pub bounce_ns: Option<u64>,
    /// Latency of this signal, in ns; the model's without one.
    pub latency_ns: Option<u64>,
    /// Physical limits: outside them, `bad(out_of_range)`.
    pub limits: Option<(f64, f64)>,
    /// Panel control.
    pub panel: Option<Panel>,
    /// Label.
    pub label: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// A named bit of a profile's bits signal: the parent and the bit.
    pub bit_of: Option<(usize, u8)>,
}

impl Signal {
    /// A signal of type `ty` and unit `unit` with every other key at its
    /// default.
    pub fn new(name: &str, ty: SimType, unit: Unit) -> Self {
        Self {
            name: String::from(name),
            ty,
            unit,
            direction: SimDirection::Input,
            init: 0.0,
            init_text: Vec::new(),
            generator: None,
            noise: None,
            resolution: None,
            bounce_ns: None,
            latency_ns: None,
            limits: None,
            panel: None,
            label: None,
            description: None,
            bit_of: None,
        }
    }
}

/// The effect of a fault (ref/03 §2.4.8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// The signal has this quality; the value stays the last good one.
    Quality(RawStatus),
    /// The value stops changing, with quality `good`.
    Freeze,
    /// Forced value, quality `good`.
    Value(f64),
    /// Added offset.
    Offset(f64),
    /// The whole device is down.
    Disconnect,
    /// Extra latency in ns.
    Delay(u64),
}

/// A scheduled fault.
#[derive(Clone, Debug, PartialEq)]
pub struct Fault {
    /// Signal, or `None` for the whole device (`"*"`).
    pub signal: Option<usize>,
    /// Start, ns after the model starts.
    pub at_ns: u64,
    /// Duration in ns.
    pub for_ns: u64,
    /// Effect.
    pub effect: Effect,
}

/// A fault key of the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelFault {
    /// Key.
    pub key: String,
    /// Effect while toggled on: `Quality`, `Freeze` or `Disconnect`.
    pub effect: Effect,
    /// Signals affected.
    pub signals: Vec<usize>,
}

/// A value in an action step: a constant or an argument.
#[derive(Clone, Debug, PartialEq)]
pub enum StepValue {
    /// A number, a boolean as 0 or 1, or an enum value, in the signal's unit.
    Num(f64),
    /// A `string` or `bytes` value.
    Text(Vec<u8>),
    /// `"$name"`: the argument with that name.
    Arg(String),
}

/// A step of a simulated action (ref/03 §2.4.9).
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Waits, in ns.
    Wait(u64),
    /// Ends the action with `message` when `signal` equals `equals`.
    FailIf {
        /// The signal.
        signal: usize,
        /// The value it is compared with.
        equals: StepValue,
        /// The error message.
        message: String,
    },
    /// Fixes signals until a `set` with `release = true`.
    Set {
        /// Signals and values.
        values: Vec<(usize, StepValue)>,
        /// Release the signals instead of fixing them.
        release: bool,
    },
}

/// The type of an action argument (`"f64 %"`, `"bool"`, `"Mode"`).
#[derive(Clone, Debug, PartialEq)]
pub enum ArgType {
    /// `bool`.
    Bool,
    /// A number, with its unit.
    Num(SimType, Unit),
    /// An enum of the model.
    Enum(usize),
    /// A string.
    String,
}

/// A simulated action.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    /// Name.
    pub name: String,
    /// Arguments in declaration order.
    pub args: Vec<(String, ArgType)>,
    /// Deadline in ns.
    pub timeout_ns: u64,
    /// Steps.
    pub steps: Vec<Step>,
}

/// `[plant]`: a plant in Wasm (ref/03 §2.4.10).
#[derive(Clone, Debug, PartialEq)]
pub struct PlantSpec {
    /// The component, as written (`.wasm` path or `oci://`).
    pub component: String,
    /// Output signals delivered to the plant, in order.
    pub inputs: Vec<usize>,
    /// Input signals the plant returns, in order.
    pub outputs: Vec<usize>,
    /// The `config` table in deterministic CBOR.
    pub config: Vec<u8>,
}

/// A parsed model.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// Description.
    pub description: Option<String>,
    /// Integration and sampling step in ns.
    pub step_ns: u64,
    /// Delay between the plant and the delivery of a sample.
    pub latency_ns: u64,
    /// Seed of the noise; derived from the device when absent.
    pub seed: Option<u64>,
    /// The profile whose signals the model reuses.
    pub profile: Option<String>,
    /// Signals, in name order.
    pub signals: Vec<Signal>,
    /// Enums, the profile's first.
    pub enums: Vec<Enum>,
    /// The `[dynamics]`, compiled.
    pub dynamics: Dynamics,
    /// Scheduled faults.
    pub faults: Vec<Fault>,
    /// Panel fault keys.
    pub panel_faults: Vec<PanelFault>,
    /// Actions, in name order.
    pub actions: Vec<Action>,
    /// `[implements]`, the profile's then the model's: class and table.
    pub implements: Vec<(String, Table)>,
    /// `[plant]`.
    pub plant: Option<PlantSpec>,
}

impl Model {
    /// Index of signal `name`.
    pub fn signal(&self, name: &str) -> Option<usize> {
        self.signals
            .binary_search_by(|s| s.name.as_str().cmp(name))
            .ok()
    }

    /// Index of action `name`.
    pub fn action(&self, name: &str) -> Option<usize> {
        self.actions.iter().position(|a| a.name == name)
    }
}

/// What a model may need besides its own text, all provided by the core
/// (ADR 0051, ADR 0054).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inputs {
    /// The device's `params`, which override the model's defaults.
    pub params: BTreeMap<String, Literal>,
    /// The source of the profile named by `[model] profile`.
    pub profile: Option<String>,
    /// The bytes of the files the model names (`replay` files, the
    /// `[plant]` component), by the path as written in the model.
    pub files: BTreeMap<String, Vec<u8>>,
}

fn pascal_to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses a quality literal: `good`, `uncertain(stale)`, `bad(comm_failure)`
/// or the display forms `Good`, `Bad(CommFailure)` (ref/03 §2.1.3).
pub fn quality(text: &str) -> Option<RawStatus> {
    let t = text.trim();
    let (head, reason) = match t.split_once('(') {
        Some((h, r)) => (h, Some(r.strip_suffix(')')?)),
        None => (t, None),
    };
    let display = head.starts_with(|c: char| c.is_ascii_uppercase());
    let code = match pascal_to_snake(head).as_str() {
        "good" => QualityCode::Good,
        "uncertain" => QualityCode::Uncertain,
        "bad" => QualityCode::Bad,
        _ => return None,
    };
    let Some(reason) = reason else {
        return (code == QualityCode::Good).then_some(RawStatus::OK);
    };
    let name = if display {
        pascal_to_snake(reason)
    } else {
        String::from(reason)
    };
    let r = (0..=19u8)
        .find(|&r| joker_core::QualityReason::from_u8(r).name() == Some(name.as_str()))?;
    Some(RawStatus {
        code,
        reason: Reason(r),
    })
}

fn literal(v: &Value) -> Option<Literal> {
    match v {
        Value::Integer(i) => Some(Literal::Number(*i as f64)),
        Value::Float(f) => Some(Literal::Number(*f)),
        Value::Boolean(b) => Some(Literal::Bool(*b)),
        Value::String(s) => Some(Literal::Text(s.clone())),
        _ => None,
    }
}

fn table<'a>(v: &'a Value, path: &str, code: Code) -> Result<&'a Table, ModelError> {
    v.as_table()
        .ok_or_else(|| err(code, path, "expected a table"))
}

fn only(t: &Table, keys: &[&str], path: &str) -> Result<(), ModelError> {
    for k in t.keys() {
        if k.starts_with("x_") || keys.contains(&k.as_str()) {
            continue;
        }
        let code = if k.starts_with('_') { J0141 } else { J0140 };
        return Err(err(code, &std::format!("{path}.{k}"), "unknown key"));
    }
    Ok(())
}

fn text<'a>(t: &'a Table, key: &str, path: &str) -> Result<Option<&'a str>, ModelError> {
    match t.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(err(
            J0806,
            &std::format!("{path}.{key}"),
            "expected a string",
        )),
    }
}

fn ident(s: &str, max: usize) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|c| c.is_ascii_lowercase())
        && c.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && s.len() <= max
}

fn signal_path(s: &str) -> bool {
    let segs: Vec<&str> = s.split('.').collect();
    segs.len() <= 7
        && segs.first() != Some(&"actions")
        && segs.iter().all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

struct Ctx<'a> {
    params: &'a BTreeMap<String, Literal>,
}

/// How a value is read: absolute, or as a difference (`K` on `°C`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scale {
    Absolute,
    Difference,
}

impl Ctx<'_> {
    fn resolve(&self, v: &Value, path: &str) -> Result<Literal, ModelError> {
        let lit = literal(v)
            .ok_or_else(|| err(J0806, path, "expected a number, a boolean or a string"))?;
        if let Literal::Text(t) = &lit
            && let Some(name) = t.strip_prefix('$')
        {
            return self
                .params
                .get(name)
                .cloned()
                .ok_or_else(|| err(J0836, path, std::format!("unknown parameter `{name}`")));
        }
        Ok(lit)
    }

    fn number(&self, v: &Value, unit: &Unit, scale: Scale, path: &str) -> Result<f64, ModelError> {
        let lit = self.resolve(v, path)?;
        let r = match scale {
            Scale::Absolute => lit.number(unit),
            Scale::Difference => lit.difference(unit),
        };
        r.map_err(lit_err(path))
    }

    fn duration(&self, v: &Value, path: &str) -> Result<u64, ModelError> {
        self.resolve(v, path)?.duration_ns().map_err(lit_err(path))
    }

    fn value(
        &self,
        v: &Value,
        ty: SimType,
        unit: &Unit,
        enums: &[Enum],
        scale: Scale,
        path: &str,
    ) -> Result<f64, ModelError> {
        let lit = self.resolve(v, path)?;
        match ty {
            SimType::Bool => match lit {
                Literal::Bool(b) => Ok(f64::from(u8::from(b))),
                Literal::Number(n) if n == 0.0 || n == 1.0 => Ok(n),
                _ => Err(err(J0806, path, "a `bool` value is true or false")),
            },
            SimType::Enum(e) => {
                let en = &enums[e];
                let value = match &lit {
                    Literal::Text(t) => {
                        let variant = t.strip_prefix(&std::format!("{}.", en.name)).unwrap_or(t);
                        en.value(variant)
                    }
                    Literal::Number(n) if n.fract() == 0.0 => {
                        en.variant(*n as i64).map(|_| *n as i64)
                    }
                    _ => None,
                };
                value
                    .map(|v| v as f64)
                    .ok_or_else(|| err(J0303, path, std::format!("not a variant of `{}`", en.name)))
            }
            SimType::Bits(_) => match lit {
                Literal::Number(n) if n.fract() == 0.0 && n >= 0.0 => Ok(n),
                _ => Err(err(J0806, path, "a bits value is a non-negative integer")),
            },
            SimType::String | SimType::Bytes => Err(err(
                J0806,
                path,
                "a string or bytes signal has no numeric value",
            )),
            _ => {
                let r = match scale {
                    Scale::Absolute => lit.number(unit),
                    Scale::Difference => lit.difference(unit),
                };
                r.map_err(lit_err(path))
            }
        }
    }
}

fn check_header(source: &str, doc: &Table) -> Result<(), ModelError> {
    let first = source
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'));
    if !first.is_some_and(|l| {
        l.strip_prefix("schema")
            .is_some_and(|r| r.trim_start().starts_with('='))
    }) {
        return Err(err(
            J0800,
            "schema",
            "a simulation model starts with `schema = 1`",
        ));
    }
    match doc.get("schema") {
        Some(Value::Integer(1)) => {}
        _ => return Err(err(J0801, "schema", "only `schema = 1` is supported")),
    }
    if let Some(v) = doc.get("joker") {
        let wanted = v
            .as_str()
            .and_then(|s| s.split_once('.'))
            .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)))
            .ok_or_else(|| err(J0806, "joker", "expected \"M.m\""))?;
        if wanted > RUNTIME {
            return Err(err(
                J0802,
                "joker",
                std::format!("the model needs runtime {}.{}", wanted.0, wanted.1),
            ));
        }
    }
    const OTHERS: [&str; 6] = [
        "profile", "board", "scenario", "suite", "fixture", "package",
    ];
    if let Some(t) = OTHERS.iter().find(|k| doc.contains_key(**k)) {
        return Err(err(
            J0803,
            t,
            std::format!("`[{t}]` belongs to another file type, not to a `.sim.toml`"),
        ));
    }
    if !["model", "signals", "plant"]
        .iter()
        .any(|k| doc.contains_key(*k))
    {
        return Err(err(
            J0803,
            "",
            "a simulation model has at least one of `[model]`, `[signals]` and `[plant]`",
        ));
    }
    Ok(())
}

/// Parses `source` with the device's `params` only (no profile, no files).
pub fn parse(source: &str, device_params: &BTreeMap<String, Literal>) -> Result<Model, ModelError> {
    parse_with(
        source,
        &Inputs {
            params: device_params.clone(),
            ..Inputs::default()
        },
    )
}

fn params(
    doc: &Table,
    device: &BTreeMap<String, Literal>,
) -> Result<BTreeMap<String, Literal>, ModelError> {
    let mut out = BTreeMap::new();
    let declared = match doc.get("params") {
        Some(v) => table(v, "params", J0836)?.clone(),
        None => Table::new(),
    };
    for (name, p) in &declared {
        let path = std::format!("params.{name}");
        if !ident(name, 32) {
            return Err(err(J0805, &path, "a parameter name is snake_case"));
        }
        let p = table(p, &path, J0836)?;
        only(p, &["default", "unit", "description"], &path)?;
        let value = match (device.get(name), p.get("default")) {
            (Some(v), _) => v.clone(),
            (None, Some(d)) => literal(d).ok_or_else(|| err(J0836, &path, "invalid default"))?,
            (None, None) => return Err(err(J0836, &path, "required parameter without a value")),
        };
        if let Some(u) = text(p, "unit", &path)? {
            let unit = literal::unit(u).map_err(lit_err(&path))?;
            let ok = match &value {
                Literal::Text(t) => match literal::quantity(t) {
                    Ok((_, Some(q))) => q.check_compatible(&unit).is_ok(),
                    Ok((_, None)) => true,
                    Err(_) => literal::duration_ns(t).is_ok() && expr::is_time(&unit),
                },
                Literal::Number(_) => true,
                Literal::Bool(_) => unit.is_dimensionless(),
            };
            if !ok {
                return Err(err(J0836, &path, std::format!("the value is not in `{u}`")));
            }
        }
        out.insert(name.clone(), value);
    }
    if let Some(name) = device.keys().find(|k| !declared.contains_key(*k)) {
        return Err(err(
            J0836,
            &std::format!("params.{name}"),
            "unknown parameter",
        ));
    }
    Ok(out)
}

fn enums(t: &Table, path: &str, out: &mut Vec<Enum>) -> Result<(), ModelError> {
    for (name, v) in t {
        let p = std::format!("{path}.{name}");
        let upper = name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && name.chars().all(|c| c.is_ascii_alphanumeric())
            && name.len() <= 32;
        if !upper {
            return Err(err(
                J0805,
                &p,
                "an enum name is PascalCase, at most 32 characters",
            ));
        }
        if out.iter().any(|e| &e.name == name) {
            return Err(err(J0814, &p, "enum declared twice"));
        }
        let vt = table(v, &p, J0814)?;
        if vt.is_empty() || vt.len() > MAX_VARIANTS {
            return Err(err(J0814, &p, "an enum has 1 to 256 variants"));
        }
        let mut variants: Vec<(String, i64)> = Vec::new();
        for (variant, value) in vt {
            if !ident(variant, 32) {
                return Err(err(
                    J0805,
                    &p,
                    std::format!("variant `{variant}` is not snake_case"),
                ));
            }
            let value = value
                .as_integer()
                .filter(|v| *v >= 0)
                .ok_or_else(|| err(J0814, &p, "a variant value is a non-negative integer"))?;
            if variants.iter().any(|(_, v)| *v == value) {
                return Err(err(J0814, &p, "two variants with the same value"));
            }
            variants.push((variant.clone(), value));
        }
        out.push(Enum {
            name: name.clone(),
            variants,
        });
    }
    Ok(())
}

fn sim_type(
    name: &str,
    enum_name: Option<&str>,
    enums: &[Enum],
    path: &str,
) -> Result<SimType, ModelError> {
    if let Some(e) = enum_name {
        if !matches!(name, "" | "enum") {
            return Err(err(J0815, path, "an enum signal omits `type`"));
        }
        let i = enums
            .iter()
            .position(|x| x.name == e)
            .ok_or_else(|| err(J0814, path, std::format!("unknown enum `{e}`")))?;
        return Ok(SimType::Enum(i));
    }
    Ok(match name {
        "bool" => SimType::Bool,
        "i64" => SimType::I64,
        "u64" => SimType::U64,
        "f64" => SimType::F64,
        "f32" => SimType::F32,
        "bits16" => SimType::Bits(16),
        "bits32" => SimType::Bits(32),
        "bits64" => SimType::Bits(64),
        "string" => SimType::String,
        "bytes" => SimType::Bytes,
        "" => return Err(err(J0815, path, "missing `type`")),
        other => {
            return Err(err(
                J0815,
                path,
                std::format!("unknown signal type `{other}`"),
            ));
        }
    })
}

fn direction(text: &str, path: &str) -> Result<SimDirection, ModelError> {
    match text {
        "input" => Ok(SimDirection::Input),
        "output" => Ok(SimDirection::Output),
        "param" => Ok(SimDirection::Param),
        other => Err(err(
            J0806,
            path,
            std::format!("unknown direction `{other}`"),
        )),
    }
}

type Inherited = (String, Base, Vec<(u8, String)>);

struct Base {
    ty: SimType,
    unit: String,
    quantity: Option<String>,
    direction: String,
    enum_name: Option<String>,
    limits: Option<Value>,
}

fn limits(ctx: &Ctx<'_>, v: &Value, unit: &Unit, path: &str) -> Result<(f64, f64), ModelError> {
    match v.as_array().map(Vec::as_slice) {
        Some([a, b]) => {
            let a = ctx.number(a, unit, Scale::Absolute, path)?;
            let b = ctx.number(b, unit, Scale::Absolute, path)?;
            if a < b {
                Ok((a, b))
            } else {
                Err(err(J0816, path, "`limits` needs min < max"))
            }
        }
        _ => Err(err(J0816, path, "`limits` is [min, max]")),
    }
}

fn text_init(v: &Value, ty: SimType, path: &str) -> Result<Vec<u8>, ModelError> {
    let bytes = match (v, ty) {
        (Value::String(s), _) => s.as_bytes().to_vec(),
        (Value::Array(a), SimType::Bytes) => a
            .iter()
            .map(|b| b.as_integer().and_then(|b| u8::try_from(b).ok()))
            .collect::<Option<Vec<u8>>>()
            .ok_or_else(|| err(J0806, path, "bytes are integers 0–255"))?,
        _ => return Err(err(J0806, path, "expected a string")),
    };
    if bytes.len() > MAX_TEXT {
        return Err(err(J0806, path, "at most 64 bytes"));
    }
    Ok(bytes)
}

fn signal(
    ctx: &Ctx<'_>,
    inputs: &Inputs,
    enums: &[Enum],
    name: &str,
    t: &Table,
    base: Option<&Base>,
) -> Result<Signal, ModelError> {
    let path = std::format!("signals.{name}");
    if !signal_path(name) {
        return Err(err(J0805, &path, "invalid signal path"));
    }
    match base {
        None => only(
            t,
            &[
                "type",
                "enum",
                "unit",
                "quantity",
                "direction",
                "init",
                "gen",
                "noise",
                "resolution",
                "bounce",
                "latency",
                "limits",
                "panel",
                "label",
                "description",
            ],
            &path,
        )?,
        Some(b) => {
            only(
                t,
                &[
                    "type",
                    "enum",
                    "unit",
                    "direction",
                    "init",
                    "gen",
                    "noise",
                    "resolution",
                    "bounce",
                    "latency",
                    "panel",
                    "label",
                    "description",
                ],
                &path,
            )?;
            let same = |key: &str, have: Option<&str>| -> Result<(), ModelError> {
                match text(t, key, &path)? {
                    Some(v) if Some(v) != have => Err(err(
                        J0815,
                        &std::format!("{path}.{key}"),
                        "differs from the profile",
                    )),
                    _ => Ok(()),
                }
            };
            let ty_name = match b.ty {
                SimType::Bool => Some("bool"),
                SimType::I64 => Some("i64"),
                SimType::U64 => Some("u64"),
                SimType::F64 => Some("f64"),
                SimType::F32 => Some("f32"),
                _ => None,
            };
            same("type", ty_name)?;
            same("unit", Some(&b.unit))?;
            same("direction", Some(&b.direction))?;
            same("enum", b.enum_name.as_deref())?;
        }
    }
    let ty = match base {
        Some(b) => b.ty,
        None => sim_type(
            text(t, "type", &path)?.unwrap_or(""),
            text(t, "enum", &path)?,
            enums,
            &path,
        )?,
    };
    let unit_text = match base {
        Some(b) => b.unit.clone(),
        None => String::from(text(t, "unit", &path)?.unwrap_or("1")),
    };
    let mut unit = literal::unit(&unit_text).map_err(lit_err(&path))?;
    if !ty.is_numeric() && unit_text != "1" {
        return Err(err(J0815, &path, "only numeric signals have a unit"));
    }
    let quantity = match base {
        Some(b) => b.quantity.clone(),
        None => text(t, "quantity", &path)?.map(String::from),
    };
    if let Some(q) = quantity {
        let kind: QuantityKind = q
            .parse()
            .map_err(|_| err(J0108, &path, std::format!("unknown quantity `{q}`")))?;
        if unit.kind().is_some_and(|k| k != kind) {
            return Err(err(J0108, &path, "the quantity does not match the unit"));
        }
        unit = unit.with_kind(Some(kind));
    }
    let direction = direction(
        match base {
            Some(b) => b.direction.as_str(),
            None => text(t, "direction", &path)?.unwrap_or("input"),
        },
        &path,
    )?;
    let mut s = Signal::new(name, ty, unit);
    s.direction = direction;
    let numeric = ty.is_numeric();
    if let Some(v) = t.get("init") {
        let p = std::format!("{path}.init");
        if ty.is_text() {
            s.init_text = text_init(v, ty, &p)?;
        } else {
            s.init = ctx.value(v, ty, &unit, enums, Scale::Absolute, &p)?;
        }
    } else if let SimType::Enum(e) = ty {
        s.init = enums[e].variants.first().map_or(0.0, |(_, v)| *v as f64);
    }
    if let Some(g) = t.get("gen") {
        let p = std::format!("{path}.gen");
        if ty.is_text() {
            return Err(err(J0831, &p, "string and bytes signals have no generator"));
        }
        if direction == SimDirection::Output {
            return Err(err(
                J0832,
                &path,
                "an output is written by the core and has no generator",
            ));
        }
        s.generator = Some(generator(ctx, inputs, enums, g, &s, &p)?);
    }
    if let Some(v) = t.get("bounce") {
        let p = std::format!("{path}.bounce");
        if ty != SimType::Bool {
            return Err(err(J0815, &p, "`bounce` is only for `bool`"));
        }
        s.bounce_ns = Some(ctx.duration(v, &p)?).filter(|b| *b > 0);
    }
    if let Some(v) = t.get("latency") {
        s.latency_ns = Some(ctx.duration(v, &std::format!("{path}.latency"))?);
    }
    for (key, slot) in [("noise", &mut s.noise), ("resolution", &mut s.resolution)] {
        if let Some(v) = t.get(key) {
            let p = std::format!("{path}.{key}");
            if !numeric {
                return Err(err(
                    J0815,
                    &p,
                    std::format!("`{key}` is only for numeric signals"),
                ));
            }
            let x = ctx.number(v, &unit, Scale::Difference, &p)?;
            if x < 0.0 {
                return Err(err(J0806, &p, "must not be negative"));
            }
            *slot = Some(x).filter(|x| *x > 0.0);
        }
    }
    let lim = match base {
        Some(b) => b.limits.as_ref(),
        None => t.get("limits"),
    };
    if let Some(v) = lim {
        let p = std::format!("{path}.limits");
        if !numeric {
            return Err(err(J0816, &p, "`limits` is only for numeric signals"));
        }
        s.limits = Some(limits(ctx, v, &unit, &p)?);
    }
    s.label = text(t, "label", &path)?.map(String::from);
    s.description = text(t, "description", &path)?.map(String::from);
    if s.label.as_ref().is_some_and(|l| l.len() > 64) {
        return Err(err(J0806, &path, "a label has at most 64 bytes"));
    }
    if let Some(p) = t.get("panel") {
        s.panel = Some(panel(ctx, p, &s, &std::format!("{path}.panel"))?);
    }
    Ok(s)
}

fn generator(
    ctx: &Ctx<'_>,
    inputs: &Inputs,
    enums: &[Enum],
    v: &Value,
    s: &Signal,
    path: &str,
) -> Result<Gen, ModelError> {
    let t = table(v, path, J0831)?;
    let kind = text(t, "kind", path)?.ok_or_else(|| err(J0831, path, "missing `kind`"))?;
    let keys: &[&str] = match kind {
        "constant" => &["kind", "value"],
        "step" => &["kind", "from", "to", "at"],
        "ramp" => &["kind", "from", "to", "over", "at"],
        "sine" => &["kind", "mean", "amp", "period", "phase"],
        "square" => &["kind", "low", "high", "period", "duty"],
        "noise" => &["kind", "mean", "std"],
        "walk" => &["kind", "start", "std", "min", "max"],
        "replay" => &["kind", "file", "column", "time_column", "loop"],
        other => {
            return Err(err(
                J0831,
                path,
                std::format!("unknown generator `{other}`"),
            ));
        }
    };
    if let Some(k) = t
        .keys()
        .find(|k| !k.starts_with("x_") && !keys.contains(&k.as_str()))
    {
        return Err(err(
            J0831,
            &std::format!("{path}.{k}"),
            "unknown generator key",
        ));
    }
    let continuous = matches!(kind, "ramp" | "sine" | "noise" | "walk");
    if continuous && !s.ty.is_numeric() {
        return Err(err(
            J0831,
            path,
            std::format!("`{kind}` needs a numeric signal"),
        ));
    }
    let get = |key: &str, scale: Scale| -> Result<Option<f64>, ModelError> {
        t.get(key)
            .map(|v| {
                ctx.value(
                    v,
                    s.ty,
                    &s.unit,
                    enums,
                    scale,
                    &std::format!("{path}.{key}"),
                )
            })
            .transpose()
    };
    let req = |key: &str, scale: Scale| -> Result<f64, ModelError> {
        get(key, scale)?.ok_or_else(|| err(J0831, path, std::format!("missing `{key}`")))
    };
    let secs = |key: &str, required: bool| -> Result<f64, ModelError> {
        match t.get(key) {
            Some(v) => Ok(ctx.duration(v, &std::format!("{path}.{key}"))? as f64 / 1e9),
            None if required => Err(err(J0831, path, std::format!("missing `{key}`"))),
            None => Ok(0.0),
        }
    };
    let bare = |key: &str, default: f64| -> Result<f64, ModelError> {
        match t.get(key) {
            Some(v) => ctx.number(
                v,
                &Unit::ONE,
                Scale::Absolute,
                &std::format!("{path}.{key}"),
            ),
            None => Ok(default),
        }
    };
    use Scale::{Absolute as A, Difference as D};
    let g = match kind {
        "constant" => Gen::Constant(req("value", A)?),
        "step" => Gen::Step {
            from: req("from", A)?,
            to: req("to", A)?,
            at: secs("at", true)?,
        },
        "ramp" => Gen::Ramp {
            from: req("from", A)?,
            to: req("to", A)?,
            over: secs("over", true)?,
            at: secs("at", false)?,
        },
        "sine" => Gen::Sine {
            mean: req("mean", A)?,
            amp: req("amp", D)?,
            period: secs("period", true)?,
            phase: bare("phase", 0.0)?,
        },
        "square" => Gen::Square {
            low: req("low", A)?,
            high: req("high", A)?,
            period: secs("period", true)?,
            duty: bare("duty", 0.5)?,
        },
        "noise" => Gen::Noise {
            mean: req("mean", A)?,
            std: req("std", D)?,
        },
        "walk" => Gen::Walk {
            start: req("start", A)?,
            std: req("std", D)?,
            min: get("min", A)?.unwrap_or(f64::NEG_INFINITY),
            max: get("max", A)?.unwrap_or(f64::INFINITY),
        },
        _ => Gen::Replay(replay_series(ctx, inputs, enums, t, s, path)?),
    };
    let bad = match &g {
        Gen::Sine { period, .. } | Gen::Square { period, .. } => *period <= 0.0,
        Gen::Walk { std, min, max, .. } => *std < 0.0 || min > max,
        Gen::Noise { std, .. } => *std < 0.0,
        _ => false,
    } || matches!(&g, Gen::Square { duty, .. } if !(0.0..=1.0).contains(duty));
    if bad {
        return Err(err(
            J0831,
            path,
            "invalid period, duty, deviation or bounds",
        ));
    }
    Ok(g)
}

fn replay_series(
    ctx: &Ctx<'_>,
    inputs: &Inputs,
    enums: &[Enum],
    t: &Table,
    s: &Signal,
    path: &str,
) -> Result<Series, ModelError> {
    let file = text(t, "file", path)?.ok_or_else(|| err(J0831, path, "missing `file`"))?;
    let column = text(t, "column", path)?.ok_or_else(|| err(J0831, path, "missing `column`"))?;
    let time_column = text(t, "time_column", path)?.unwrap_or("t");
    let looped = match t.get("loop") {
        None => false,
        Some(Value::Boolean(b)) => *b,
        Some(_) => {
            return Err(err(
                J0806,
                &std::format!("{path}.loop"),
                "expected a boolean",
            ));
        }
    };
    if file.to_ascii_lowercase().ends_with(".parquet") {
        return Err(err(
            J0807,
            &std::format!("{path}.file"),
            "Parquet replay is reserved for a later version; export the series as CSV",
        ));
    }
    let bytes = inputs.files.get(file).ok_or_else(|| {
        err(
            J0831,
            &std::format!("{path}.file"),
            std::format!("`{file}` was not provided with the model"),
        )
    })?;
    let source = core::str::from_utf8(bytes)
        .map_err(|_| err(J0831, path, std::format!("`{file}` is not UTF-8 CSV")))?;
    let csv =
        replay::parse_csv(source).map_err(|m| err(J0831, path, std::format!("`{file}`: {m}")))?;
    let times = csv.column(time_column).ok_or_else(|| {
        err(
            J0831,
            path,
            std::format!("`{file}` has no column `{time_column}`"),
        )
    })?;
    let values = csv.column(column).ok_or_else(|| {
        err(
            J0831,
            path,
            std::format!("`{file}` has no column `{column}`"),
        )
    })?;
    if times.is_empty() {
        return Err(err(J0831, path, std::format!("`{file}` has no rows")));
    }
    let mut series = Series {
        times: Vec::with_capacity(times.len()),
        values: Vec::with_capacity(times.len()),
        looped,
    };
    for (k, (tc, vc)) in times.iter().zip(values.iter()).enumerate() {
        let row = std::format!("{path}: `{file}` row {}", k + 1);
        let tn = match tc.parse::<f64>() {
            Ok(secs) if secs.is_finite() && secs >= 0.0 => (secs * 1e9).round() as u64,
            Ok(_) => return Err(err(J0831, &row, "negative time")),
            Err(_) => literal::duration_ns(tc).map_err(lit_err(&row))?,
        };
        if series.times.last().is_some_and(|last| *last >= tn) {
            return Err(err(J0831, &row, "times must increase"));
        }
        let cell = match vc.parse::<f64>() {
            Ok(n) => Value::Float(n),
            Err(_) if *vc == "true" || *vc == "false" => Value::Boolean(*vc == "true"),
            Err(_) => Value::String((*vc).to_string()),
        };
        let v = ctx.value(&cell, s.ty, &s.unit, enums, Scale::Absolute, &row)?;
        series.times.push(tn);
        series.values.push(v);
    }
    Ok(series)
}

const NAMED_KEYS: [&str; 19] = [
    "space", "enter", "tab", "up", "down", "left", "right", "f1", "f2", "f3", "f4", "f5", "f6",
    "f7", "f8", "f9", "f10", "f11", "f12",
];
const RESERVED_KEYS: [&str; 4] = ["q", "r", "t", "?"];

fn valid_key(k: &str) -> bool {
    let mut c = k.chars();
    let single =
        matches!((c.next(), c.next()), (Some(ch), None) if !ch.is_control() && !ch.is_whitespace());
    single || NAMED_KEYS.contains(&k)
}

fn panel(ctx: &Ctx<'_>, v: &Value, s: &Signal, path: &str) -> Result<Panel, ModelError> {
    let t = table(v, path, J0835)?;
    if let Some(k) = t.keys().find(|k| {
        !k.starts_with("x_")
            && ![
                "kind", "label", "key", "keys", "step", "min", "max", "pulse", "order",
            ]
            .contains(&k.as_str())
    }) {
        return Err(err(J0835, &std::format!("{path}.{k}"), "unknown panel key"));
    }
    let kind = match text(t, "kind", path)? {
        Some("lamp") => PanelKind::Lamp,
        Some("momentary") => PanelKind::Momentary,
        Some("toggle") => PanelKind::Toggle,
        Some("slider") => PanelKind::Slider,
        Some("gauge") => PanelKind::Gauge,
        _ => {
            return Err(err(
                J0835,
                path,
                "`kind` must be lamp, momentary, toggle, slider or gauge",
            ));
        }
    };
    let keys = match t.get("keys") {
        Some(Value::Array(a)) => match a.as_slice() {
            [Value::String(down), Value::String(up)] => Some((down.clone(), up.clone())),
            _ => return Err(err(J0835, path, "`keys` is [decrease, increase]")),
        },
        Some(_) => return Err(err(J0835, path, "`keys` is [decrease, increase]")),
        None => None,
    };
    let num = |key: &str, scale: Scale| {
        t.get(key)
            .map(|v| ctx.number(v, &s.unit, scale, &std::format!("{path}.{key}")))
            .transpose()
    };
    let numeric = s.ty.is_numeric();
    let p = Panel {
        kind,
        label: text(t, "label", path)?.map(String::from),
        key: text(t, "key", path)?.map(String::from),
        keys,
        step: if numeric {
            num("step", Scale::Difference)?.unwrap_or(0.0)
        } else {
            0.0
        },
        min: if numeric {
            num("min", Scale::Absolute)?.or(s.limits.map(|l| l.0))
        } else {
            None
        },
        max: if numeric {
            num("max", Scale::Absolute)?.or(s.limits.map(|l| l.1))
        } else {
            None
        },
        pulse_ns: match t.get("pulse") {
            Some(v) => ctx.duration(v, &std::format!("{path}.pulse"))?,
            None => 150_000_000,
        },
        order: match t.get("order") {
            None => None,
            Some(Value::Integer(i)) => Some(*i),
            Some(_) => return Err(err(J0835, path, "`order` is an integer")),
        },
    };
    if p.label.as_ref().is_some_and(|l| l.len() > 24) {
        return Err(err(J0835, path, "a panel label has at most 24 bytes"));
    }
    let needs_key = matches!(p.kind, PanelKind::Momentary | PanelKind::Toggle);
    let ok = match p.kind {
        PanelKind::Lamp => p.keys.is_none() && p.key.is_none(),
        PanelKind::Gauge => p.key.is_none(),
        PanelKind::Momentary | PanelKind::Toggle => p.key.is_some() && p.keys.is_none(),
        PanelKind::Slider => p.keys.is_some() && p.key.is_none(),
    };
    if !ok || (needs_key && p.key.is_none()) || (p.keys.is_some() && p.step <= 0.0) {
        return Err(err(
            J0835,
            path,
            "missing or extra key, keys or step for this kind",
        ));
    }
    let every = p.key.iter().chain(p.keys.iter().flat_map(|(a, b)| [a, b]));
    if let Some(k) = every.into_iter().find(|k| !valid_key(k)) {
        return Err(err(J0835, path, std::format!("`{k}` is not a key")));
    }
    let fits = match p.kind {
        PanelKind::Lamp => matches!(s.ty, SimType::Bool | SimType::Enum(_)),
        PanelKind::Momentary | PanelKind::Toggle => {
            s.ty == SimType::Bool && s.direction == SimDirection::Input
        }
        PanelKind::Slider => numeric && s.direction == SimDirection::Input,
        PanelKind::Gauge => numeric,
    };
    if !fits {
        return Err(err(
            J0835,
            path,
            "kind incompatible with the type or direction",
        ));
    }
    Ok(p)
}

fn effect(
    ctx: &Ctx<'_>,
    t: &Table,
    signal: Option<&Signal>,
    enums: &[Enum],
    path: &str,
    keys: &[&str],
) -> Result<Effect, ModelError> {
    let present: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|k| t.contains_key(*k))
        .collect();
    let [key] = present.as_slice() else {
        return Err(err(J0834, path, "exactly one effect is required"));
    };
    let v = &t[*key];
    let p = std::format!("{path}.{key}");
    Ok(match *key {
        "quality" | "toggle" => {
            let q = v
                .as_str()
                .and_then(quality)
                .ok_or_else(|| err(J0834, &p, "invalid quality"))?;
            Effect::Quality(q)
        }
        "freeze" if v.as_bool() == Some(true) => Effect::Freeze,
        "disconnect" if v.as_bool() == Some(true) => Effect::Disconnect,
        "value" | "offset" => {
            let Some(s) = signal else {
                return Err(err(J0834, &p, "`value` and `offset` need a signal"));
            };
            if key == &"offset" {
                if !s.ty.is_numeric() {
                    return Err(err(J0834, &p, "`offset` is only for numeric signals"));
                }
                Effect::Offset(ctx.number(v, &s.unit, Scale::Difference, &p)?)
            } else {
                Effect::Value(ctx.value(v, s.ty, &s.unit, enums, Scale::Absolute, &p)?)
            }
        }
        "delay" => Effect::Delay(ctx.duration(v, &p)?),
        _ => return Err(err(J0834, &p, "invalid effect")),
    })
}

fn overlaps(a: &Fault, b: &Fault) -> bool {
    a.at_ns < b.at_ns + b.for_ns && b.at_ns < a.at_ns + a.for_ns
}

fn faults(ctx: &Ctx<'_>, model: &mut Model, t: &Table) -> Result<(), ModelError> {
    for (key, v) in t {
        let path = std::format!("faults.{key}");
        if key.starts_with("x_") {
            continue;
        }
        let fault_keys = key == "panel"
            && match v {
                Value::Array(a) => a.iter().all(|e| e.get("key").is_some()),
                _ => true,
            };
        if fault_keys {
            let entries: Vec<&Value> = match v {
                Value::Array(a) => a.iter().collect(),
                other => std::vec![other],
            };
            for e in entries {
                let e = table(e, &path, J0834)?;
                only(
                    e,
                    &["key", "toggle", "freeze", "disconnect", "signals"],
                    &path,
                )?;
                let key =
                    text(e, "key", &path)?.ok_or_else(|| err(J0834, &path, "missing `key`"))?;
                if !valid_key(key) {
                    return Err(err(J0835, &path, std::format!("`{key}` is not a key")));
                }
                let effect = effect(
                    ctx,
                    e,
                    None,
                    &model.enums,
                    &path,
                    &["toggle", "freeze", "disconnect"],
                )?;
                let signals = match e.get("signals") {
                    None => model
                        .signals
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.direction == SimDirection::Input)
                        .map(|(i, _)| i)
                        .collect(),
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|s| {
                            s.as_str()
                                .and_then(|n| model.signal(n))
                                .ok_or_else(|| err(J0834, &path, "unknown signal"))
                        })
                        .collect::<Result<_, _>>()?,
                    Some(_) => return Err(err(J0834, &path, "`signals` is a list of names")),
                };
                model.panel_faults.push(PanelFault {
                    key: String::from(key),
                    effect,
                    signals,
                });
            }
            continue;
        }
        if key == "panel" {
            return Err(err(
                J0834,
                &path,
                "`panel` is reserved for fault keys: a signal named `panel` has no scheduled faults",
            ));
        }
        let signal = if key == "*" {
            None
        } else {
            let i = model
                .signal(key)
                .ok_or_else(|| err(J0834, &path, "unknown signal"))?;
            if model.signals[i].direction == SimDirection::Output {
                return Err(err(
                    J0834,
                    &path,
                    "outputs are not acquired and have no faults",
                ));
            }
            Some(i)
        };
        let list = v
            .as_array()
            .ok_or_else(|| err(J0834, &path, "expected a list of faults"))?;
        for f in list {
            let f = table(f, &path, J0834)?;
            only(
                f,
                &[
                    "at",
                    "for",
                    "quality",
                    "freeze",
                    "value",
                    "offset",
                    "disconnect",
                    "delay",
                ],
                &path,
            )?;
            let at_ns = ctx.duration(
                f.get("at")
                    .ok_or_else(|| err(J0834, &path, "missing `at`"))?,
                &std::format!("{path}.at"),
            )?;
            let for_ns = ctx.duration(
                f.get("for")
                    .ok_or_else(|| err(J0834, &path, "missing `for`"))?,
                &std::format!("{path}.for"),
            )?;
            if for_ns == 0 {
                return Err(err(J0834, &path, "`for` must be positive"));
            }
            let effect = effect(
                ctx,
                f,
                signal.map(|i| &model.signals[i]),
                &model.enums,
                &path,
                &[
                    "quality",
                    "freeze",
                    "value",
                    "offset",
                    "disconnect",
                    "delay",
                ],
            )?;
            if effect == Effect::Disconnect && signal.is_some() {
                return Err(err(J0834, &path, "`disconnect` is only for `\"*\"`"));
            }
            let fault = Fault {
                signal,
                at_ns,
                for_ns,
                effect,
            };
            let clash = model.faults.iter().any(|m| {
                let same = m.signal == fault.signal;
                let device_wide = |x: &Fault| x.signal.is_none() && x.effect != Effect::Disconnect;
                (same || device_wide(m) || device_wide(&fault)) && overlaps(m, &fault)
            });
            if clash {
                return Err(err(J0834, &path, "overlapping faults on the same signal"));
            }
            model.faults.push(fault);
        }
    }
    Ok(())
}

fn check_keys(model: &Model) -> Result<(), ModelError> {
    let repeated = |k: &str| {
        err(
            J0835,
            "panel",
            std::format!("key `{k}` is reserved or repeated"),
        )
    };
    let mut seen: Vec<&str> = Vec::new();
    for s in &model.signals {
        if let Some(p) = &s.panel {
            for k in p.key.iter().chain(p.keys.iter().flat_map(|(a, b)| [a, b])) {
                if RESERVED_KEYS.contains(&k.as_str()) || seen.contains(&k.as_str()) {
                    return Err(repeated(k));
                }
                seen.push(k);
            }
        }
    }
    for f in &model.panel_faults {
        if RESERVED_KEYS.contains(&f.key.as_str()) || seen.contains(&f.key.as_str()) {
            return Err(repeated(&f.key));
        }
        seen.push(&f.key);
    }
    Ok(())
}

fn arg_type(text: &str, enums: &[Enum], path: &str) -> Result<ArgType, ModelError> {
    let (ty, unit) = match text.split_once(' ') {
        Some((t, u)) => (t, u.trim()),
        None => (text, "1"),
    };
    let bad = || err(J0821, path, std::format!("unknown argument type `{text}`"));
    let unit = literal::unit(unit).map_err(lit_err(path))?;
    Ok(match ty {
        "bool" => ArgType::Bool,
        "string" => ArgType::String,
        "i64" => ArgType::Num(SimType::I64, unit),
        "u64" => ArgType::Num(SimType::U64, unit),
        "f64" => ArgType::Num(SimType::F64, unit),
        "f32" => ArgType::Num(SimType::F32, unit),
        e => ArgType::Enum(enums.iter().position(|x| x.name == e).ok_or_else(bad)?),
    })
}

fn step_value(
    ctx: &Ctx<'_>,
    v: &Value,
    s: &Signal,
    enums: &[Enum],
    args: &[(String, ArgType)],
    path: &str,
) -> Result<StepValue, ModelError> {
    if let Some(name) = v.as_str().and_then(|t| t.strip_prefix('$'))
        && !ctx.params.contains_key(name)
    {
        if !args.iter().any(|(a, _)| a == name) {
            return Err(err(
                J0821,
                path,
                std::format!("undeclared argument `${name}`"),
            ));
        }
        return Ok(StepValue::Arg(String::from(name)));
    }
    if s.ty.is_text() {
        return Ok(StepValue::Text(text_init(v, s.ty, path)?));
    }
    Ok(StepValue::Num(ctx.value(
        v,
        s.ty,
        &s.unit,
        enums,
        Scale::Absolute,
        path,
    )?))
}

fn actions(ctx: &Ctx<'_>, model: &Model, t: &Table) -> Result<Vec<Action>, ModelError> {
    let mut out = Vec::new();
    for (name, a) in t {
        let path = std::format!("actions.{name}");
        if !ident(name, 32) {
            return Err(err(J0805, &path, "an action name is snake_case"));
        }
        let a = table(a, &path, J0821)?;
        only(
            a,
            &["args", "timeout", "steps", "description", "long", "result"],
            &path,
        )?;
        if a.contains_key("result") {
            return Err(err(
                J0821,
                &path,
                "simulated actions have no `read` steps, so no `result`",
            ));
        }
        let mut args = Vec::new();
        if let Some(v) = a.get("args") {
            for (arg, ty) in table(v, &path, J0821)? {
                let ty = ty
                    .as_str()
                    .ok_or_else(|| err(J0821, &path, "an argument type is a string"))?;
                args.push((arg.clone(), arg_type(ty, &model.enums, &path)?));
            }
        }
        let list = a
            .get("steps")
            .and_then(Value::as_array)
            .ok_or_else(|| err(J0821, &path, "a simulated action has `steps`"))?;
        if list.len() > MAX_STEPS {
            return Err(err(J0821, &path, "at most 32 steps"));
        }
        let mut steps = Vec::new();
        let mut waits = 0u64;
        for (k, step) in list.iter().enumerate() {
            let p = std::format!("{path}.steps[{k}]");
            let st = table(step, &p, J0821)?;
            let [(kind, body)] = st.iter().collect::<Vec<_>>()[..] else {
                return Err(err(J0821, &p, "a step has exactly one key"));
            };
            let signal_of = |n: &str| {
                model
                    .signal(n)
                    .ok_or_else(|| err(J0821, &p, std::format!("unknown signal `{n}`")))
            };
            steps.push(match kind.as_str() {
                "wait" => {
                    let ns = ctx.duration(body, &p)?;
                    waits = waits.saturating_add(ns);
                    Step::Wait(ns)
                }
                "fail_if" => {
                    let b = table(body, &p, J0821)?;
                    only(b, &["signal", "equals", "message"], &p)?;
                    let sname =
                        text(b, "signal", &p)?.ok_or_else(|| err(J0821, &p, "missing `signal`"))?;
                    let i = signal_of(sname)?;
                    let equals = b
                        .get("equals")
                        .ok_or_else(|| err(J0821, &p, "missing `equals`"))?;
                    Step::FailIf {
                        signal: i,
                        equals: step_value(
                            ctx,
                            equals,
                            &model.signals[i],
                            &model.enums,
                            &args,
                            &p,
                        )?,
                        message: String::from(
                            text(b, "message", &p)?.unwrap_or("simulated failure"),
                        ),
                    }
                }
                "set" => {
                    let b = table(body, &p, J0821)?;
                    let release = match b.get("release") {
                        None => false,
                        Some(Value::Boolean(r)) => *r,
                        Some(_) => return Err(err(J0821, &p, "`release` is a boolean")),
                    };
                    let mut values = Vec::new();
                    for (sname, v) in b.iter().filter(|(k, _)| *k != "release") {
                        let i = signal_of(sname)?;
                        let s = &model.signals[i];
                        if s.direction == SimDirection::Output || s.bit_of.is_some() {
                            return Err(err(
                                J0821,
                                &p,
                                std::format!("`{sname}` is written by the core or derived"),
                            ));
                        }
                        let value = if release {
                            StepValue::Num(0.0)
                        } else {
                            step_value(ctx, v, s, &model.enums, &args, &p)?
                        };
                        values.push((i, value));
                    }
                    if values.is_empty() {
                        return Err(err(J0821, &p, "`set` names at least one signal"));
                    }
                    Step::Set { values, release }
                }
                other => {
                    return Err(err(
                        J0821,
                        &p,
                        std::format!("`{other}` is not a simulated step (wait, fail_if, set)"),
                    ));
                }
            });
        }
        let default = waits.saturating_add(1_000_000_000 * steps.len() as u64);
        let timeout_ns = match a.get("timeout") {
            Some(v) => ctx.duration(v, &std::format!("{path}.timeout"))?,
            None => default,
        };
        if waits > timeout_ns {
            return Err(err(J0821, &path, "the waits exceed the timeout"));
        }
        out.push(Action {
            name: name.clone(),
            args,
            timeout_ns,
            steps,
        });
    }
    Ok(out)
}

fn class_ref(s: &str) -> bool {
    let Some((path, major)) = s.rsplit_once('@') else {
        return false;
    };
    !major.is_empty()
        && major.chars().all(|c| c.is_ascii_digit())
        && path.split('/').all(|seg| ident(seg, 64))
}

fn implements_map(model: &Model, t: &Table, path: &str, depth: usize) -> Result<(), ModelError> {
    only(t, &["capabilities", "map", "members"], path)?;
    if let Some(c) = t.get("capabilities")
        && !c.as_array().is_some_and(|a| a.iter().all(Value::is_str))
    {
        return Err(err(J0824, path, "`capabilities` is a list of names"));
    }
    let signal = |n: &str, p: &str| -> Result<(), ModelError> {
        model
            .signal(n)
            .map(|_| ())
            .ok_or_else(|| err(J0101, p, std::format!("unknown signal `{n}`")))
    };
    let action = |n: &str, p: &str| -> Result<(), ModelError> {
        model
            .action(n)
            .map(|_| ())
            .ok_or_else(|| err(J0824, p, std::format!("unknown action `{n}`")))
    };
    let target = |s: &str, p: &str| -> Result<(), ModelError> {
        if let Some(a) = s.strip_prefix("actions.") {
            action(a, p)
        } else if s.starts_with("const:") {
            Ok(())
        } else {
            signal(s, p)
        }
    };
    if let Some(m) = t.get("map") {
        for (member, v) in table(m, path, J0824)? {
            let p = std::format!("{path}.map.{member}");
            match v {
                Value::String(s) => target(s, &p)?,
                Value::Table(adapter) => {
                    for key in ["signal", "on_rise", "on_change"] {
                        if let Some(s) = adapter.get(key).and_then(Value::as_str) {
                            signal(s, &p)?;
                        }
                    }
                    if let Some(edge) = adapter.get("edge").and_then(Value::as_table) {
                        for a in edge.values().filter_map(Value::as_str) {
                            target(a, &p)?;
                        }
                    }
                    if let Some(fields) = adapter.get("fields").and_then(Value::as_table) {
                        for s in fields.values().filter_map(Value::as_str) {
                            signal(s, &p)?;
                        }
                    }
                }
                _ => return Err(err(J0824, &p, "a target is a path or an adapter table")),
            }
        }
    }
    if let Some(members) = t.get("members") {
        if depth >= 2 {
            return Err(err(J0824, path, "at most two levels of `members`"));
        }
        for (member, sub) in table(members, path, J0824)? {
            let p = std::format!("{path}.members.{member}");
            implements_map(model, table(sub, &p, J0824)?, &p, depth + 1)?;
        }
    }
    Ok(())
}

fn implements(model: &Model, list: &[(String, Table)]) -> Result<(), ModelError> {
    if list.len() > MAX_IMPLEMENTS {
        return Err(err(J0824, "implements", "at most 16 classes"));
    }
    for (class, t) in list {
        let path = std::format!("implements.\"{class}\"");
        if !class_ref(class) {
            return Err(err(
                J0823,
                &path,
                "a class reference is `<package>/<name>@<major>`",
            ));
        }
        if t.contains_key("instances") {
            return Err(err(
                J0807,
                &std::format!("{path}.instances"),
                "several implicit instances per device arrive in 1.x",
            ));
        }
        implements_map(model, t, &path, 0)?;
    }
    Ok(())
}

fn toml_cbor(v: &Value) -> joker_hal::cbor::Value {
    use joker_hal::cbor::Value as C;
    match v {
        Value::String(s) => C::Text(s.clone()),
        Value::Integer(i) => C::Int(*i),
        Value::Float(f) => C::Float(*f),
        Value::Boolean(b) => C::Bool(*b),
        Value::Datetime(d) => C::Text(d.to_string()),
        Value::Array(a) => C::Array(a.iter().map(toml_cbor).collect()),
        Value::Table(t) => C::Map(t.iter().map(|(k, v)| (k.clone(), toml_cbor(v))).collect()),
    }
}

fn plant(model: &Model, t: &Table) -> Result<PlantSpec, ModelError> {
    let path = "plant";
    only(t, &["component", "inputs", "outputs", "config"], path)?;
    let component =
        text(t, "component", path)?.ok_or_else(|| err(J0837, path, "missing `component`"))?;
    let list = |key: &str, want: SimDirection| -> Result<Vec<usize>, ModelError> {
        let a = t
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| err(J0837, path, std::format!("`{key}` is a list of signals")))?;
        a.iter()
            .map(|n| {
                let i = n
                    .as_str()
                    .and_then(|n| model.signal(n))
                    .ok_or_else(|| err(J0837, path, std::format!("unknown signal in `{key}`")))?;
                let s = &model.signals[i];
                if s.direction != want || s.ty.is_text() || s.bit_of.is_some() {
                    return Err(err(
                        J0837,
                        path,
                        std::format!("`{}` has the wrong direction or type for `{key}`", s.name),
                    ));
                }
                Ok(i)
            })
            .collect()
    };
    let inputs = list("inputs", SimDirection::Output)?;
    let outputs = list("outputs", SimDirection::Input)?;
    for &i in &outputs {
        if model.signals[i].generator.is_some() {
            return Err(err(
                J0832,
                path,
                std::format!(
                    "`{}` has a generator and is a plant output",
                    model.signals[i].name
                ),
            ));
        }
    }
    let config = match t.get("config") {
        None => joker_hal::cbor::Value::Map(Vec::new()).encode(),
        Some(c) => {
            table(c, path, J0837)?;
            toml_cbor(c).encode()
        }
    };
    if Cbor::new(&config).validate().is_err() {
        return Err(err(J0837, path, "the config does not encode"));
    }
    Ok(PlantSpec {
        component: String::from(component),
        inputs,
        outputs,
        config,
    })
}

fn bases(
    ctx: &Ctx<'_>,
    parts: &profile::ProfileParts,
    enums: &[Enum],
) -> Result<Vec<Inherited>, ModelError> {
    let _ = ctx;
    let mut out = Vec::new();
    for p in &parts.points {
        let path = std::format!("model.profile.signals.{}", p.name);
        let (ty, enum_name) = match &p.ty {
            PointType::Plain(t) => (sim_type(t, None, enums, &path)?, None),
            PointType::Enum(e) => (sim_type("", Some(e), enums, &path)?, Some(e.clone())),
        };
        if let SimType::Bits(w) = ty
            && p.bits.iter().any(|(i, _)| *i >= w)
        {
            return Err(err(J0814, &path, "a bit outside the width"));
        }
        out.push((
            p.name.clone(),
            Base {
                ty,
                unit: p.unit.clone(),
                quantity: p.quantity.clone(),
                direction: p.direction.clone(),
                enum_name,
                limits: p.limits.clone(),
            },
            p.bits.clone(),
        ));
    }
    Ok(out)
}

/// Parses `source` with everything the core passes along (ADR 0054).
pub fn parse_with(source: &str, inputs: &Inputs) -> Result<Model, ModelError> {
    let doc: Table = source
        .parse()
        .map_err(|e: toml::de::Error| err(J0806, "", e.message().to_string()))?;
    check_header(source, &doc)?;
    for key in doc.keys() {
        match key.as_str() {
            "schema" | "joker" | "model" | "params" | "signals" | "dynamics" | "faults"
            | "enums" | "actions" | "implements" | "plant" => {}
            k if k.starts_with("x_") => {}
            k if k.starts_with('_') => return Err(err(J0141, k, "reserved key")),
            other => return Err(err(J0140, other, "unknown section")),
        }
    }

    let empty = Table::new();
    let model_t = match doc.get("model") {
        Some(v) => table(v, "model", J0140)?,
        None => &empty,
    };
    only(
        model_t,
        &["description", "step", "latency", "seed", "profile"],
        "model",
    )?;
    let params = params(&doc, &inputs.params)?;
    let ctx = Ctx { params: &params };

    let step_ns = match model_t.get("step") {
        Some(v) => ctx.duration(v, "model.step")?,
        None => 10_000_000,
    };
    if step_ns == 0 {
        return Err(err(J0806, "model.step", "the step must be positive"));
    }
    let latency_ns = match model_t.get("latency") {
        Some(v) => ctx.duration(v, "model.latency")?,
        None => 0,
    };
    let seed = match model_t.get("seed") {
        Some(Value::Integer(i)) if *i >= 0 => Some(*i as u64),
        Some(_) => return Err(err(J0806, "model.seed", "expected a non-negative integer")),
        None => None,
    };
    let profile_ref = text(model_t, "profile", "model")?.map(String::from);

    let mut enum_list = Vec::new();
    let mut implements_list: Vec<(String, Table)> = Vec::new();
    let mut profile_parts = None;
    if let Some(r) = &profile_ref {
        let ok = r.rsplit_once('@').is_some_and(|(n, m)| {
            !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()) && n.contains('/')
        });
        if !ok {
            return Err(err(
                J0805,
                "model.profile",
                "a profile reference is `<name>@<major>`",
            ));
        }
        let src = inputs.profile.as_deref().ok_or_else(|| {
            err(
                J0803,
                "model.profile",
                std::format!("`{r}` does not resolve to a profile"),
            )
        })?;
        let parts = profile::read(src)?;
        if let Some(e) = &parts.enums {
            enums(e, "model.profile.enums", &mut enum_list)?;
        }
        if let Some(i) = &parts.implements {
            for (class, t) in i {
                implements_list.push((
                    class.clone(),
                    table(t, "model.profile.implements", J0824)?.clone(),
                ));
            }
        }
        profile_parts = Some(parts);
    }
    if let Some(v) = doc.get("enums") {
        enums(table(v, "enums", J0814)?, "enums", &mut enum_list)?;
    }

    let declared = match doc.get("signals") {
        Some(v) => table(v, "signals", J0140)?.clone(),
        None => Table::new(),
    };
    let mut signals = Vec::new();
    let mut bits: Vec<(String, String, u8)> = Vec::new();
    let mut has_init: BTreeMap<String, bool> = BTreeMap::new();
    if let Some(parts) = &profile_parts {
        for (name, base, named_bits) in bases(&ctx, parts, &enum_list)? {
            let t = match declared.get(&name) {
                Some(v) => table(v, &std::format!("signals.{name}"), J0140)?.clone(),
                None => Table::new(),
            };
            has_init.insert(name.clone(), t.contains_key("init"));
            signals.push(signal(&ctx, inputs, &enum_list, &name, &t, Some(&base))?);
            for (bit, bname) in named_bits {
                bits.push((name.clone(), std::format!("{name}.{bname}"), bit));
            }
        }
    }
    for (name, s) in &declared {
        if signals.iter().any(|x: &Signal| &x.name == name) || name.starts_with("x_") {
            continue;
        }
        let t = table(s, &std::format!("signals.{name}"), J0140)?;
        has_init.insert(name.clone(), t.contains_key("init"));
        signals.push(signal(&ctx, inputs, &enum_list, name, t, None)?);
    }
    for (_, bname, _) in &bits {
        if signals.iter().any(|x| &x.name == bname) {
            return Err(err(
                J0811,
                &std::format!("signals.{bname}"),
                "a named bit collides with a signal",
            ));
        }
        signals.push(Signal::new(bname, SimType::Bool, Unit::ONE));
    }
    signals.sort_by(|a, b| a.name.cmp(&b.name));
    if signals.len() > MAX_SIGNALS {
        return Err(err(J0811, "signals", "a model has at most 1,024 signals"));
    }
    for (parent, bname, bit) in &bits {
        let p = signals.binary_search_by(|s| s.name.as_str().cmp(parent));
        let b = signals.binary_search_by(|s| s.name.as_str().cmp(bname));
        if let (Ok(p), Ok(b)) = (p, b) {
            signals[b].direction = signals[p].direction;
            signals[b].bit_of = Some((p, *bit));
        }
    }

    let mut dyn_src = Vec::new();
    if let Some(v) = doc.get("dynamics") {
        for (name, e) in table(v, "dynamics", J0833)? {
            let path = std::format!("dynamics.{name}");
            let e = e
                .as_str()
                .ok_or_else(|| err(J0833, &path, "expected an expression"))?;
            if signals
                .iter()
                .any(|s| &s.name == name && s.bit_of.is_some())
            {
                return Err(err(J0832, &path, "a named bit follows its signal"));
            }
            dyn_src.push((name.clone(), String::from(e)));
        }
    }
    let init_flags: Vec<bool> = signals
        .iter()
        .map(|s| has_init.get(&s.name).copied().unwrap_or(false))
        .collect();
    let dynamics = expr::compile(
        &signals,
        &enum_list,
        &params,
        &dyn_src,
        step_ns,
        &init_flags,
    )?;

    let mut model = Model {
        description: text(model_t, "description", "model")?.map(String::from),
        step_ns,
        latency_ns,
        seed,
        profile: profile_ref,
        signals,
        enums: enum_list,
        dynamics,
        faults: Vec::new(),
        panel_faults: Vec::new(),
        actions: Vec::new(),
        implements: Vec::new(),
        plant: None,
    };
    if let Some(v) = doc.get("faults") {
        faults(&ctx, &mut model, table(v, "faults", J0834)?)?;
    }
    check_keys(&model)?;
    if let Some(v) = doc.get("actions") {
        model.actions = actions(&ctx, &model, table(v, "actions", J0821)?)?;
    }
    if let Some(v) = doc.get("implements") {
        for (class, t) in table(v, "implements", J0824)? {
            if implements_list.iter().any(|(c, _)| c == class) {
                return Err(err(
                    J0824,
                    "implements",
                    std::format!("`{class}` is implemented twice"),
                ));
            }
            implements_list.push((class.clone(), table(t, "implements", J0824)?.clone()));
        }
    }
    implements(&model, &implements_list)?;
    model.implements = implements_list;
    if let Some(v) = doc.get("plant") {
        if doc.contains_key("dynamics") {
            return Err(err(
                J0832,
                "plant",
                "`[plant]` and `[dynamics]` are exclusive",
            ));
        }
        model.plant = Some(plant(&model, table(v, "plant", J0837)?)?);
    }
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PI: &str = include_str!("../tests/models/pi.sim.toml");
    const ESP: &str = include_str!("../tests/models/esp.sim.toml");

    fn ok(src: &str) -> Model {
        parse(src, &BTreeMap::new()).unwrap_or_else(|e| panic!("{e}"))
    }

    fn code(src: &str) -> Code {
        parse(src, &BTreeMap::new()).map(|_| ()).unwrap_err().code
    }

    #[test]
    fn the_track_models_parse() {
        let pi = ok(PI);
        let names: Vec<&str> = pi.signals.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["act_led", "cpu_temp", "load"]);
        let temp = &pi.signals[pi.signal("cpu_temp").unwrap()];
        assert_eq!(
            (temp.ty, temp.unit.symbol(), temp.init),
            (SimType::F64, "°C", 45.0)
        );
        assert_eq!(pi.step_ns, 10_000_000);
        let esp = ok(ESP);
        assert_eq!(esp.panel_faults.len(), 1);
        assert_eq!(esp.panel_faults[0].effect, Effect::Disconnect);
        let rssi = &esp.signals[esp.signal("rssi").unwrap()];
        assert_eq!(
            rssi.generator,
            Some(Gen::Noise {
                mean: -55.0,
                std: 3.0
            })
        );
        assert_eq!(
            esp.signals[esp.signal("button").unwrap()].bounce_ns,
            Some(5_000_000)
        );
    }

    #[test]
    fn qualities_with_display_forms() {
        assert_eq!(
            quality("bad(comm_failure)"),
            Some(RawStatus::bad(Reason::COMM_FAILURE))
        );
        assert_eq!(
            quality("Bad(CommFailure)"),
            Some(RawStatus::bad(Reason::COMM_FAILURE))
        );
        assert_eq!(
            quality("uncertain(stale)"),
            Some(RawStatus::uncertain(Reason::STALE))
        );
        assert_eq!(quality("good"), Some(RawStatus::OK));
        assert_eq!(quality("Good"), Some(RawStatus::OK));
        assert_eq!(quality("bad(nope)"), None);
        assert_eq!(quality("bad"), None);
    }

    #[test]
    fn params_substitute_and_are_checked() {
        let src = "schema = 1\n[params]\nmean = { default = \"22 °C\" }\namp = { unit = \"K\" }\n[signals]\nt = { type = \"f64\", unit = \"°C\", gen = { kind = \"sine\", mean = \"$mean\", amp = \"$amp\", period = \"10m\" } }\n";
        assert_eq!(code(src), J0836);
        let mut p = BTreeMap::new();
        p.insert(String::from("amp"), Literal::Text(String::from("3 K")));
        let m = parse(src, &p).unwrap();
        assert_eq!(
            m.signals[0].generator,
            Some(Gen::Sine {
                mean: 22.0,
                amp: 3.0,
                period: 600.0,
                phase: 0.0
            })
        );
        p.insert(String::from("amp"), Literal::Text(String::from("3 kW")));
        assert_eq!(parse(src, &p).unwrap_err().code, J0836);
        p.insert(String::from("amp"), Literal::Text(String::from("3 K")));
        p.insert(String::from("other"), Literal::Number(1.0));
        assert_eq!(parse(src, &p).unwrap_err().code, J0836);
    }

    #[test]
    fn enums_strings_bits_and_limits() {
        let m = ok(r#"schema = 1
[enums]
Mode = { off = 0, heat = 1, boost = 3 }
[signals]
mode = { enum = "Mode", gen = { kind = "step", from = "off", to = "Mode.boost", at = "1s" } }
idle = { enum = "Mode" }
serial = { type = "string", init = "SN-42" }
blob = { type = "bytes", init = [1, 2, 255] }
word = { type = "bits16", init = 5 }
p = { type = "f64", unit = "kW", quantity = "active_power", limits = ["-10 kW", "10000 W"], init = "1500 W" }
"#);
        let s = |n: &str| &m.signals[m.signal(n).unwrap()];
        assert_eq!(
            s("mode").generator,
            Some(Gen::Step {
                from: 0.0,
                to: 3.0,
                at: 1.0
            })
        );
        assert_eq!(s("idle").init, 0.0);
        assert_eq!(s("serial").init_text, b"SN-42");
        assert_eq!(s("blob").init_text, [1, 2, 255]);
        assert_eq!(s("word").ty, SimType::Bits(16));
        assert_eq!((s("p").limits, s("p").init), (Some((-10.0, 10.0)), 1.5));
    }

    #[test]
    fn actions_and_implements() {
        let m = ok(r#"schema = 1
[enums]
Mode = { off = 0, on = 1 }
[signals]
fault = { type = "bool" }
mode = { enum = "Mode", direction = "param" }
temp = { type = "f64", unit = "°C", gen = { kind = "constant", value = "20 °C" } }
cmd = { type = "bool", direction = "output" }
[actions.reset]
args = { to = "Mode", t = "f64 °C" }
steps = [{ fail_if = { signal = "fault", equals = true, message = "still faulted" } }, { wait = "500ms" }, { set = { mode = "$to", temp = "$t" } }, { set = { temp = true, release = true } }]
[implements."std/thing@1"]
map = { on = "cmd", fault = "fault", reset = "actions.reset", scaled = { signal = "temp", scale = 0.1 } }
"#);
        let a = &m.actions[0];
        assert_eq!(a.timeout_ns, 500_000_000 + 4_000_000_000);
        assert_eq!(a.steps.len(), 4);
        assert!(matches!(&a.steps[3], Step::Set { release: true, .. }));
        assert_eq!(m.implements.len(), 1);
        let base = "schema = 1\n[signals]\nx = { type = \"bool\" }\n";
        assert_eq!(
            code(&std::format!(
                "{base}[implements.\"std/a@1\"]\nmap = {{ y = \"nope\" }}\n"
            )),
            J0101
        );
        assert_eq!(
            code(&std::format!(
                "{base}[implements.\"std/a@1\"]\nmap = {{ y = \"actions.nope\" }}\n"
            )),
            J0824
        );
        assert_eq!(
            code(&std::format!("{base}[implements.\"Std/A\"]\nmap = {{}}\n")),
            J0823
        );
        assert_eq!(
            code(&std::format!(
                "{base}[actions.a]\nsteps = [{{ write = {{ at = 1 }} }}]\n"
            )),
            J0821
        );
        assert_eq!(
            code(&std::format!(
                "{base}[actions.a]\nsteps = [{{ set = {{ x = \"$v\" }} }}]\n"
            )),
            J0821
        );
        assert_eq!(
            code(&std::format!(
                "{base}[actions.a]\ntimeout = \"1s\"\nsteps = [{{ wait = \"2s\" }}]\n"
            )),
            J0821
        );
    }

    #[test]
    fn a_profile_gives_its_signals_enums_and_classes() {
        let profile = r#"schema = 1
[profile]
name = "local/bench-esp32"
version = "1.1.0"
driver = "modbus"
[signals]
led    = { coil = 0, type = "bool", direction = "output" }
button = { discrete = 0, type = "bool" }
touch  = { input = 0, type = "u16" }
status = { input = 1, type = "bits16", bits = { 0 = "running", 3 = "fault" } }
mode   = { holding = 2, type = "u16", enum = "Mode", direction = "param" }
[enums]
Mode = { auto = 0, manual = 1 }
[implements."std/led@1"]
map = { on = "led" }
"#;
        let src = r#"schema = 1
[model]
profile = "local/bench-esp32@1"
[signals]
button = { init = false, bounce = "5ms", panel = { kind = "momentary", key = "b" } }
touch = { type = "u64", init = 60 }
status = { init = 9 }
uptime = { type = "f64", unit = "s", init = 0 }
[dynamics]
uptime = "integrate(1)"
"#;
        let inputs = Inputs {
            profile: Some(String::from(profile)),
            ..Inputs::default()
        };
        let m = parse_with(src, &inputs).unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<&str> = m.signals.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "button",
                "led",
                "mode",
                "status",
                "status.fault",
                "status.running",
                "touch",
                "uptime"
            ]
        );
        let s = |n: &str| &m.signals[m.signal(n).unwrap()];
        assert_eq!(s("led").direction, SimDirection::Output);
        assert_eq!(s("touch").ty, SimType::U64);
        assert_eq!(s("mode").ty, SimType::Enum(0));
        assert_eq!(
            s("status.fault").bit_of,
            Some((m.signal("status").unwrap(), 3))
        );
        assert_eq!(m.implements.len(), 1);
        let clash = src.replace("touch = { type = \"u64\"", "touch = { type = \"f64\"");
        assert_eq!(parse_with(&clash, &inputs).unwrap_err().code, J0815);
        assert_eq!(parse(src, &BTreeMap::new()).unwrap_err().code, J0803);
        let not_profile = Inputs {
            profile: Some(String::from("schema = 1\n[signals]\n")),
            ..Inputs::default()
        };
        assert_eq!(parse_with(src, &not_profile).unwrap_err().code, J0803);
    }

    #[test]
    fn replay_reads_the_provided_csv() {
        let src = "schema = 1\n[signals]\np = { type = \"f64\", unit = \"kW\", gen = { kind = \"replay\", file = \"data/load.csv\", column = \"p\", loop = true } }\n";
        let mut inputs = Inputs::default();
        assert_eq!(parse_with(src, &inputs).unwrap_err().code, J0831);
        inputs.files.insert(
            String::from("data/load.csv"),
            b"t,p\n0,1.5\n1s,2000 W\n2.5,3\n".to_vec(),
        );
        let m = parse_with(src, &inputs).unwrap();
        let Some(Gen::Replay(series)) = &m.signals[0].generator else {
            panic!("not a replay");
        };
        assert_eq!(series.times, [0, 1_000_000_000, 2_500_000_000]);
        assert_eq!(series.values, [1.5, 2.0, 3.0]);
        assert!(series.looped);
        let parquet = src.replace("load.csv", "load.parquet");
        assert_eq!(parse_with(&parquet, &inputs).unwrap_err().code, J0807);
    }

    #[test]
    fn plant_spec_and_its_exclusions() {
        let m = ok(r#"schema = 1
[signals]
valve = { type = "f64", unit = "%", direction = "output" }
flow = { type = "f64", unit = "m3/h" }
[plant]
component = "plants/valve.wasm"
inputs = ["valve"]
outputs = ["flow"]
config = { cv = 12.5, name = "v1" }
"#);
        let p = m.plant.unwrap();
        assert_eq!((p.inputs, p.outputs), (std::vec![1], std::vec![0]));
        assert!(!p.config.is_empty());
        let base = "schema = 1\n[signals]\nv = { type = \"f64\", direction = \"output\" }\nf = { type = \"f64\" }\n";
        assert_eq!(
            code(&std::format!(
                "{base}[plant]\ncomponent = \"a.wasm\"\ninputs = [\"f\"]\noutputs = [\"f\"]\n"
            )),
            J0837
        );
        assert_eq!(
            code(&std::format!(
                "{base}[dynamics]\nf = \"v\"\n[plant]\ncomponent = \"a.wasm\"\ninputs = [\"v\"]\noutputs = [\"f\"]\n"
            )),
            J0832
        );
    }

    #[test]
    fn errors_carry_their_codes() {
        let cases = [
            ("schema = 2\n[signals]\n", J0801),
            ("[signals]\nschema = 1\n", J0800),
            ("schema = 1\njoker = \"9.0\"\n[signals]\n", J0802),
            ("schema = 1\n[profile]\n", J0803),
            ("schema = 1\n", J0803),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", colour = 1 }\n",
                J0140,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", _colour = 1 }\n",
                J0141,
            ),
            ("schema = 1\n[signals]\nX = { type = \"f64\" }\n", J0805),
            ("schema = 1\n[signals]\nx = { type = \"text\" }\n", J0815),
            (
                "schema = 1\n[signals]\nx = { type = \"bool\", unit = \"V\" }\n",
                J0815,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", unit = \"furlong\" }\n",
                J0109,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", gen = { kind = \"chaos\" } }\n",
                J0831,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"bool\", gen = { kind = \"sine\", mean = 0, amp = 1, period = \"1s\" } }\n",
                J0831,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"bool\", direction = \"output\", gen = { kind = \"constant\", value = true } }\n",
                J0832,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", direction = \"output\" }\n[dynamics]\nx = \"1\"\n",
                J0832,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", gen = { kind = \"constant\", value = 1 } }\n[dynamics]\nx = \"1\"\n",
                J0832,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"bool\", panel = { kind = \"toggle\", key = \"q\" } }\n",
                J0835,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"bool\", panel = { kind = \"slider\", keys = [\"a\", \"b\"], step = 1 } }\n",
                J0835,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"bool\", panel = { kind = \"toggle\", key = \"ctrl\" } }\n",
                J0835,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"bool\", panel = { kind = \"lamp\", label = \"a label far longer than allowed\" } }\n",
                J0835,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\" }\n[faults]\nx = [{ at = \"1s\", for = \"1s\", disconnect = true }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\" }\n[faults]\nx = [{ at = \"1s\", for = \"2s\", freeze = true }, { at = \"2s\", for = \"1s\", value = 1 }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\" }\n[faults]\nx = [{ at = \"1s\", for = \"2s\", freeze = true }]\n\"*\" = [{ at = \"2s\", for = \"1s\", quality = \"bad(device_fault)\" }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\" }\n[faults]\ny = [{ at = \"1s\", for = \"1s\", freeze = true }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\" }\n[faults]\nx = [{ at = \"1s\", for = \"0s\", freeze = true }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", direction = \"output\" }\n[faults]\nx = [{ at = \"1s\", for = \"1s\", freeze = true }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\npanel = { type = \"f64\" }\n[faults]\npanel = [{ at = \"1s\", for = \"1s\", freeze = true }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", limits = [2, 1] }\n",
                J0816,
            ),
            (
                "schema = 1\n[enums]\nMode = { a = 0, b = 0 }\n[signals]\nx = { enum = \"Mode\" }\n",
                J0814,
            ),
            ("schema = 1\n[signals]\nx = { enum = \"Nope\" }\n", J0814),
            (
                "schema = 1\n[enums]\nMode = { a = 0 }\n[signals]\nx = { enum = \"Mode\", init = \"b\" }\n",
                J0303,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"string\", init = \"0123456789012345678901234567890123456789012345678901234567890123456789\" }\n",
                J0806,
            ),
        ];
        for (src, want) in cases {
            assert_eq!(code(src), want, "{src}");
        }
        let many: String = (0..=MAX_SIGNALS)
            .map(|i| std::format!("s{i} = {{ type = \"bool\" }}\n"))
            .collect();
        assert_eq!(code(&std::format!("schema = 1\n[signals]\n{many}")), J0811);
    }
}
