//! The `*.sim.toml` subset of M0 (ref/03 §2.4, ADR 0051): `[model]`,
//! `[params]`, `[signals]` with generators, noise, resolution and panels,
//! `[dynamics]` and `[faults]`.
//!
//! Accepted but without effect in M0: `bounce`, a signal's `latency`,
//! `limits`, `label` and `description` (the panel and the acquisition stage
//! use them). Refused with their code: `[model] profile`, `replay`
//! generators, `[plant]`, `[actions]`, `[enums]` and `[implements]`.

use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::vec::Vec;

use joker_hal::types::{Code, RawStatus, Reason};
use joker_hal::{QualityCode, code};
use toml::{Table, Value};

use crate::expr::{self, Dynamics};
use crate::literal::{Literal, LiteralError};

/// Unknown key.
pub const J0140: Code = code!("J0140");
/// Header or file type wrong.
pub const J0800: Code = code!("J0800");
/// Key reserved for a later version.
pub const J0807: Code = code!("J0807");
/// Invalid literal.
pub const J0806: Code = code!("J0806");
/// Invalid signal type or unit.
pub const J0815: Code = code!("J0815");
/// Unknown generator or invalid parameters.
pub const J0831: Code = code!("J0831");
/// More than one source for a signal.
pub const J0832: Code = code!("J0832");
/// Invalid dynamics.
pub const J0833: Code = code!("J0833");
/// Invalid fault.
pub const J0834: Code = code!("J0834");
/// Invalid panel.
pub const J0835: Code = code!("J0835");
/// Unknown, missing or ill-typed parameter.
pub const J0836: Code = code!("J0836");

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

/// Type of a simulated signal (docs/02 §2).
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
}

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
    /// Label.
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
}

/// One simulated signal.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    /// Name, relative to the device.
    pub name: String,
    /// Type.
    pub ty: SimType,
    /// Unit, `1` without one.
    pub unit: String,
    /// Direction.
    pub direction: SimDirection,
    /// Initial value.
    pub init: f64,
    /// Generator.
    pub generator: Option<Gen>,
    /// Standard deviation of the additive noise.
    pub noise: Option<f64>,
    /// Quantization step.
    pub resolution: Option<f64>,
    /// Panel control.
    pub panel: Option<Panel>,
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
    /// Signals, in name order.
    pub signals: Vec<Signal>,
    /// The `[dynamics]`, compiled.
    pub dynamics: Dynamics,
    /// Scheduled faults.
    pub faults: Vec<Fault>,
    /// Panel fault keys.
    pub panel_faults: Vec<PanelFault>,
}

impl Model {
    /// Index of signal `name`.
    pub fn signal(&self, name: &str) -> Option<usize> {
        self.signals
            .binary_search_by(|s| s.name.as_str().cmp(name))
            .ok()
    }
}

/// Parses a quality literal (`good`, `uncertain(stale)`, `bad(comm_failure)`).
pub fn quality(text: &str) -> Option<RawStatus> {
    let t = text.trim();
    if t == "good" {
        return Some(RawStatus::OK);
    }
    let (code, rest) = if let Some(r) = t.strip_prefix("bad(") {
        (QualityCode::Bad, r)
    } else if let Some(r) = t.strip_prefix("uncertain(") {
        (QualityCode::Uncertain, r)
    } else {
        (QualityCode::Good, t.strip_prefix("good(")?)
    };
    let name = rest.strip_suffix(')')?;
    let reason =
        (0..=19u8).find(|&r| joker_core::QualityReason::from_u8(r).name() == Some(name))?;
    Some(RawStatus {
        code,
        reason: Reason(reason),
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

struct Ctx<'a> {
    params: &'a BTreeMap<String, Literal>,
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

    fn number(&self, v: &Value, unit: &str, path: &str) -> Result<f64, ModelError> {
        self.resolve(v, path)?
            .number(unit)
            .map_err(|LiteralError(m)| err(J0806, path, m))
    }

    fn duration(&self, v: &Value, path: &str) -> Result<u64, ModelError> {
        self.resolve(v, path)?
            .duration_ns()
            .map_err(|LiteralError(m)| err(J0806, path, m))
    }
}

fn table<'a>(v: &'a Value, path: &str, code: Code) -> Result<&'a Table, ModelError> {
    v.as_table()
        .ok_or_else(|| err(code, path, "expected a table"))
}

fn only(t: &Table, keys: &[&str], path: &str) -> Result<(), ModelError> {
    match t.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(k) => Err(err(J0140, &std::format!("{path}.{k}"), "unknown key")),
        None => Ok(()),
    }
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

/// Parses `source` with the device's `params` (the manifest's
/// `[devices.<d>] params`, which override the model's defaults).
pub fn parse(source: &str, device_params: &BTreeMap<String, Literal>) -> Result<Model, ModelError> {
    let doc: Table = source
        .parse()
        .map_err(|e: toml::de::Error| err(J0806, "", e.message().to_string()))?;
    match doc.get("schema") {
        Some(Value::Integer(1)) => {}
        _ => {
            return Err(err(
                J0800,
                "schema",
                "a simulation model starts with `schema = 1`",
            ));
        }
    }
    for key in doc.keys() {
        match key.as_str() {
            "schema" | "model" | "params" | "signals" | "dynamics" | "faults" => {}
            "plant" | "actions" | "enums" | "implements" => {
                return Err(err(J0807, key, "not supported by the M0 simulator"));
            }
            other => return Err(err(J0140, other, "unknown section")),
        }
    }

    let empty = Table::new();
    let model = match doc.get("model") {
        Some(v) => table(v, "model", J0140)?,
        None => &empty,
    };
    only(
        model,
        &["description", "step", "latency", "seed", "profile"],
        "model",
    )?;
    if model.contains_key("profile") {
        return Err(err(
            J0807,
            "model.profile",
            "profile reuse is not supported by the M0 simulator",
        ));
    }

    let mut params = BTreeMap::new();
    if let Some(v) = doc.get("params") {
        for (name, p) in table(v, "params", J0836)? {
            let path = std::format!("params.{name}");
            let p = table(p, &path, J0836)?;
            only(p, &["default", "unit", "description"], &path)?;
            if let Some(d) = p.get("default") {
                let lit = literal(d).ok_or_else(|| err(J0836, &path, "invalid default"))?;
                params.insert(name.clone(), lit);
            } else if !device_params.contains_key(name) {
                return Err(err(J0836, &path, "required parameter without a value"));
            }
        }
    }
    for (name, value) in device_params {
        if !params.contains_key(name) && doc.get("params").and_then(|p| p.get(name)).is_none() {
            return Err(err(
                J0836,
                &std::format!("params.{name}"),
                "unknown parameter",
            ));
        }
        params.insert(name.clone(), value.clone());
    }
    let ctx = Ctx { params: &params };

    let step_ns = match model.get("step") {
        Some(v) => ctx.duration(v, "model.step")?,
        None => 10_000_000,
    };
    if step_ns == 0 {
        return Err(err(J0806, "model.step", "the step must be positive"));
    }
    let latency_ns = match model.get("latency") {
        Some(v) => ctx.duration(v, "model.latency")?,
        None => 0,
    };
    let seed = match model.get("seed") {
        Some(Value::Integer(i)) if *i >= 0 => Some(*i as u64),
        Some(_) => return Err(err(J0806, "model.seed", "expected a non-negative integer")),
        None => None,
    };

    let mut signals = Vec::new();
    if let Some(v) = doc.get("signals") {
        for (name, s) in table(v, "signals", J0140)? {
            signals.push(signal(&ctx, name, s)?);
        }
    }
    signals.sort_by(|a, b| a.name.cmp(&b.name));

    let mut dyn_src = Vec::new();
    if let Some(v) = doc.get("dynamics") {
        for (name, e) in table(v, "dynamics", J0833)? {
            let path = std::format!("dynamics.{name}");
            let e = e
                .as_str()
                .ok_or_else(|| err(J0833, &path, "expected an expression"))?;
            dyn_src.push((name.clone(), String::from(e)));
        }
    }
    let has_init: Vec<bool> = signals
        .iter()
        .map(|s| {
            doc.get("signals")
                .and_then(|t| t.get(&s.name))
                .is_some_and(|t| t.get("init").is_some())
        })
        .collect();
    let dynamics = expr::compile(&signals, &params, &dyn_src, step_ns, &has_init)?;

    let mut model = Model {
        description: text(model, "description", "model")?.map(String::from),
        step_ns,
        latency_ns,
        seed,
        signals,
        dynamics,
        faults: Vec::new(),
        panel_faults: Vec::new(),
    };
    if let Some(v) = doc.get("faults") {
        faults(&ctx, &mut model, table(v, "faults", J0834)?)?;
    }
    check_panels(&model)?;
    Ok(model)
}

fn sim_type(name: &str) -> Option<SimType> {
    Some(match name {
        "bool" => SimType::Bool,
        "i64" => SimType::I64,
        "u64" => SimType::U64,
        "f64" => SimType::F64,
        "f32" => SimType::F32,
        _ => return None,
    })
}

fn signal(ctx: &Ctx<'_>, name: &str, v: &Value) -> Result<Signal, ModelError> {
    let path = std::format!("signals.{name}");
    let t = table(v, &path, J0140)?;
    only(
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
    )?;
    if t.contains_key("enum") {
        return Err(err(
            J0807,
            &std::format!("{path}.enum"),
            "enums are not supported by the M0 simulator",
        ));
    }
    let ty_name = text(t, "type", &path)?.ok_or_else(|| err(J0815, &path, "missing `type`"))?;
    let ty = sim_type(ty_name).ok_or_else(|| {
        err(
            J0815,
            &path,
            std::format!("type `{ty_name}` is not simulated in M0"),
        )
    })?;
    let unit = String::from(text(t, "unit", &path)?.unwrap_or("1"));
    if ty == SimType::Bool && unit != "1" {
        return Err(err(J0815, &path, "a `bool` has no unit"));
    }
    let direction = match text(t, "direction", &path)?.unwrap_or("input") {
        "input" => SimDirection::Input,
        "output" => SimDirection::Output,
        "param" => SimDirection::Param,
        other => {
            return Err(err(
                J0806,
                &path,
                std::format!("unknown direction `{other}`"),
            ));
        }
    };
    let init = match t.get("init") {
        Some(v) => ctx.number(v, &unit, &std::format!("{path}.init"))?,
        None => 0.0,
    };
    let generator = match t.get("gen") {
        Some(g) => Some(generator(ctx, g, &unit, &std::format!("{path}.gen"))?),
        None => None,
    };
    if direction == SimDirection::Output && generator.is_some() {
        return Err(err(
            J0832,
            &path,
            "an output is written by the core and has no generator",
        ));
    }
    if let Some(v) = t.get("bounce") {
        ctx.duration(v, &std::format!("{path}.bounce"))?;
        if ty != SimType::Bool {
            return Err(err(J0806, &path, "`bounce` is only for `bool`"));
        }
    }
    if let Some(v) = t.get("latency") {
        ctx.duration(v, &std::format!("{path}.latency"))?;
    }
    let noise = t
        .get("noise")
        .map(|v| ctx.number(v, &unit, &std::format!("{path}.noise")))
        .transpose()?;
    let resolution = t
        .get("resolution")
        .map(|v| ctx.number(v, &unit, &std::format!("{path}.resolution")))
        .transpose()?
        .filter(|r| *r > 0.0);
    let panel = t
        .get("panel")
        .map(|p| panel(ctx, p, &unit, &std::format!("{path}.panel")))
        .transpose()?;
    Ok(Signal {
        name: String::from(name),
        ty,
        unit,
        direction,
        init,
        generator,
        noise,
        resolution,
        panel,
    })
}

fn generator(ctx: &Ctx<'_>, v: &Value, unit: &str, path: &str) -> Result<Gen, ModelError> {
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
        "replay" => {
            return Err(err(
                J0807,
                path,
                "`replay` is not supported by the M0 simulator",
            ));
        }
        other => {
            return Err(err(
                J0831,
                path,
                std::format!("unknown generator `{other}`"),
            ));
        }
    };
    if let Some(k) = t.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(err(
            J0831,
            &std::format!("{path}.{k}"),
            "unknown generator key",
        ));
    }
    let req = |key: &str| -> Result<f64, ModelError> {
        let v = t
            .get(key)
            .ok_or_else(|| err(J0831, path, std::format!("missing `{key}`")))?;
        ctx.number(v, unit, &std::format!("{path}.{key}"))
    };
    let opt = |key: &str, default: f64| -> Result<f64, ModelError> {
        match t.get(key) {
            Some(v) => ctx.number(v, unit, &std::format!("{path}.{key}")),
            None => Ok(default),
        }
    };
    let secs = |key: &str, required: bool| -> Result<f64, ModelError> {
        match t.get(key) {
            Some(v) => Ok(ctx.duration(v, &std::format!("{path}.{key}"))? as f64 / 1e9),
            None if required => Err(err(J0831, path, std::format!("missing `{key}`"))),
            None => Ok(0.0),
        }
    };
    let g = match kind {
        "constant" => Gen::Constant(req("value")?),
        "step" => Gen::Step {
            from: req("from")?,
            to: req("to")?,
            at: secs("at", true)?,
        },
        "ramp" => Gen::Ramp {
            from: req("from")?,
            to: req("to")?,
            over: secs("over", true)?,
            at: secs("at", false)?,
        },
        "sine" => Gen::Sine {
            mean: req("mean")?,
            amp: req("amp")?,
            period: secs("period", true)?,
            phase: match t.get("phase") {
                Some(v) => ctx.number(v, "1", &std::format!("{path}.phase"))?,
                None => 0.0,
            },
        },
        "square" => Gen::Square {
            low: req("low")?,
            high: req("high")?,
            period: secs("period", true)?,
            duty: match t.get("duty") {
                Some(v) => ctx.number(v, "1", &std::format!("{path}.duty"))?,
                None => 0.5,
            },
        },
        "noise" => Gen::Noise {
            mean: req("mean")?,
            std: req("std")?,
        },
        _ => Gen::Walk {
            start: req("start")?,
            std: req("std")?,
            min: opt("min", f64::NEG_INFINITY)?,
            max: opt("max", f64::INFINITY)?,
        },
    };
    let bad_period =
        matches!(g, Gen::Sine { period, .. } | Gen::Square { period, .. } if period <= 0.0);
    let bad_duty = matches!(g, Gen::Square { duty, .. } if !(0.0..=1.0).contains(&duty));
    if bad_period || bad_duty {
        return Err(err(J0831, path, "invalid period or duty"));
    }
    Ok(g)
}

fn panel(ctx: &Ctx<'_>, v: &Value, unit: &str, path: &str) -> Result<Panel, ModelError> {
    let t = table(v, path, J0835)?;
    if let Some(k) = t.keys().find(|k| {
        ![
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
    let num = |key: &str| {
        t.get(key)
            .map(|v| ctx.number(v, unit, &std::format!("{path}.{key}")))
            .transpose()
    };
    let p = Panel {
        kind,
        label: text(t, "label", path)?.map(String::from),
        key: text(t, "key", path)?.map(String::from),
        keys,
        step: num("step")?.unwrap_or(0.0),
        min: num("min")?,
        max: num("max")?,
        pulse_ns: match t.get("pulse") {
            Some(v) => ctx.duration(v, &std::format!("{path}.pulse"))?,
            None => 150_000_000,
        },
    };
    let ok = match p.kind {
        PanelKind::Lamp | PanelKind::Gauge => true,
        PanelKind::Momentary | PanelKind::Toggle => p.key.is_some(),
        PanelKind::Slider => p.keys.is_some(),
    };
    if !ok || (p.keys.is_some() && p.step <= 0.0) {
        return Err(err(J0835, path, "missing key, keys or step for this kind"));
    }
    Ok(p)
}

fn effect(
    ctx: &Ctx<'_>,
    t: &Table,
    unit: &str,
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
        "value" => Effect::Value(ctx.number(v, unit, &p)?),
        "offset" => Effect::Offset(ctx.number(v, unit, &p)?),
        "delay" => Effect::Delay(ctx.duration(v, &p)?),
        _ => return Err(err(J0834, &p, "invalid effect")),
    })
}

fn faults(ctx: &Ctx<'_>, model: &mut Model, t: &Table) -> Result<(), ModelError> {
    for (key, v) in t {
        let path = std::format!("faults.{key}");
        if key == "panel" {
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
                let effect = effect(ctx, e, "1", &path, &["toggle", "freeze", "disconnect"])?;
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
        let signal = if key == "*" {
            None
        } else {
            Some(
                model
                    .signal(key)
                    .ok_or_else(|| err(J0834, &path, "unknown signal"))?,
            )
        };
        let unit = signal
            .map_or("1", |i| model.signals[i].unit.as_str())
            .to_string();
        let list = v
            .as_array()
            .ok_or_else(|| err(J0834, &path, "expected a list of faults"))?;
        let mut mine: Vec<Fault> = Vec::new();
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
                &path,
            )?;
            let for_ns = ctx.duration(
                f.get("for")
                    .ok_or_else(|| err(J0834, &path, "missing `for`"))?,
                &path,
            )?;
            if for_ns == 0 {
                return Err(err(J0834, &path, "`for` must be positive"));
            }
            let effect = effect(
                ctx,
                f,
                &unit,
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
            if (effect == Effect::Disconnect) != signal.is_none() {
                return Err(err(
                    J0834,
                    &path,
                    "`disconnect` is only for `\"*\"`, and `\"*\"` only disconnects",
                ));
            }
            if mine
                .iter()
                .any(|m| at_ns < m.at_ns + m.for_ns && m.at_ns < at_ns + for_ns)
            {
                return Err(err(J0834, &path, "overlapping faults"));
            }
            mine.push(Fault {
                signal,
                at_ns,
                for_ns,
                effect,
            });
        }
        model.faults.extend(mine);
    }
    Ok(())
}

fn check_panels(model: &Model) -> Result<(), ModelError> {
    const RESERVED: [&str; 4] = ["q", "r", "t", "?"];
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
            let interactive = !matches!(p.kind, PanelKind::Lamp);
            let numeric = s.ty != SimType::Bool;
            let ok = match p.kind {
                PanelKind::Lamp => !numeric,
                PanelKind::Momentary | PanelKind::Toggle => {
                    !numeric && s.direction == SimDirection::Input
                }
                PanelKind::Slider => numeric && s.direction == SimDirection::Input,
                PanelKind::Gauge => numeric,
            };
            if !ok {
                return Err(err(
                    J0835,
                    &std::format!("signals.{}.panel", s.name),
                    "kind incompatible with the type or direction",
                ));
            }
            if interactive {
                let keys = p.key.iter().chain(p.keys.iter().flat_map(|(a, b)| [a, b]));
                for k in keys {
                    if RESERVED.contains(&k.as_str()) || seen.contains(&k.as_str()) {
                        return Err(repeated(k));
                    }
                    seen.push(k);
                }
            }
        }
    }
    for f in &model.panel_faults {
        if RESERVED.contains(&f.key.as_str()) || seen.contains(&f.key.as_str()) {
            return Err(repeated(&f.key));
        }
        seen.push(&f.key);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PI: &str = include_str!("../tests/models/pi.sim.toml");
    const ESP: &str = include_str!("../tests/models/esp.sim.toml");

    #[test]
    fn the_track_models_parse() {
        let pi = parse(PI, &BTreeMap::new()).unwrap();
        let names: Vec<&str> = pi.signals.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["act_led", "cpu_temp", "load"]);
        let temp = &pi.signals[pi.signal("cpu_temp").unwrap()];
        assert_eq!(
            (temp.ty, temp.unit.as_str(), temp.init),
            (SimType::F64, "°C", 45.0)
        );
        assert_eq!(pi.step_ns, 10_000_000);
        let esp = parse(ESP, &BTreeMap::new()).unwrap();
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
    }

    #[test]
    fn qualities() {
        assert_eq!(
            quality("bad(comm_failure)"),
            Some(RawStatus::bad(Reason::COMM_FAILURE))
        );
        assert_eq!(
            quality("uncertain(stale)"),
            Some(RawStatus::uncertain(Reason::STALE))
        );
        assert_eq!(quality("good"), Some(RawStatus::OK));
        assert_eq!(quality("bad(nope)"), None);
    }

    #[test]
    fn params_substitute_and_are_checked() {
        let src = "schema = 1\n[params]\nmean = { default = \"22 °C\" }\namp = {}\n[signals]\nt = { type = \"f64\", unit = \"°C\", gen = { kind = \"sine\", mean = \"$mean\", amp = \"$amp\", period = \"10m\" } }\n";
        assert_eq!(parse(src, &BTreeMap::new()).unwrap_err().code, J0836);
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
        p.insert(String::from("other"), Literal::Number(1.0));
        assert_eq!(parse(src, &p).unwrap_err().code, J0836);
    }

    #[test]
    fn errors_carry_their_codes() {
        let cases = [
            ("schema = 2\n", J0800),
            ("schema = 1\n[plant]\n", J0807),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", colour = 1 }\n",
                J0140,
            ),
            ("schema = 1\n[signals]\nx = { type = \"string\" }\n", J0815),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", gen = { kind = \"chaos\" } }\n",
                J0831,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\", gen = { kind = \"replay\", file = \"a\", column = \"b\" } }\n",
                J0807,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"bool\", direction = \"output\", gen = { kind = \"constant\", value = 1 } }\n",
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
                "schema = 1\n[signals]\nx = { type = \"f64\" }\n[faults]\nx = [{ at = \"1s\", for = \"1s\", disconnect = true }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\" }\n[faults]\nx = [{ at = \"1s\", for = \"2s\", freeze = true }, { at = \"2s\", for = \"1s\", value = 1 }]\n",
                J0834,
            ),
            (
                "schema = 1\n[signals]\nx = { type = \"f64\" }\n[faults]\ny = [{ at = \"1s\", for = \"1s\", freeze = true }]\n",
                J0834,
            ),
        ];
        for (src, code) in cases {
            assert_eq!(
                parse(src, &BTreeMap::new()).map(|_| ()).unwrap_err().code,
                code,
                "{src}"
            );
        }
    }
}
