//! The subset of device profiles (ref/03 §2.2) that `modbus-rtu` needs in M0
//! to serve `profiles/local/bench-esp32@1.profile.toml` (docs/23 §4).
//!
//! Profiles belong to the core: it resolves them, builds the points of the
//! plan request and runs the acquisition stage. Until the manifest front
//! (AUR-201) provides that, this module stands in for it behind a narrow
//! type, [`Profile`]: parsing, the points of a device, and the decoding of
//! raw registers into values (raw type, word and byte order, `bit`,
//! `invert`, `scale`, `offset`) with the acquisition deadband. Nothing in
//! the driver half depends on it.

use std::path::Path;
use std::string::{String, ToString};
use std::vec::Vec;

use joker_hal::acquire::{Deadband, OwnedRaw};
use joker_hal::cbor::Value as Cbor;
use joker_hal::spec::BusKind;
use joker_hal::spec::owned::{OwnedBus, OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::types::{Code, J0809, J0810};
use joker_hal::{Direction, Poll, RawKind, ReadClass, code};
use toml::{Table, Value};

/// Unknown key.
pub const J0140: Code = code!("J0140");
/// Missing `schema`.
pub const J0800: Code = code!("J0800");
/// Name or version inconsistent with the file path.
pub const J0804: Code = code!("J0804");
/// Invalid literal.
pub const J0806: Code = code!("J0806");
/// Reserved for a later version.
pub const J0807: Code = code!("J0807");
/// Scale, offset, unit or signal type inconsistent with the raw type.
pub const J0815: Code = code!("J0815");
/// Invalid `poll`.
pub const J0817: Code = code!("J0817");
/// Ill-declared `[fallback]`.
pub const J0820: Code = code!("J0820");

/// An error in a profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileError {
    /// Registry code.
    pub code: Code,
    /// Key path.
    pub path: String,
    /// For humans.
    pub message: String,
}

impl core::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}: {}", self.code.as_str(), self.path, self.message)
    }
}

impl std::error::Error for ProfileError {}

fn err(code: Code, path: &str, message: impl Into<String>) -> ProfileError {
    ProfileError {
        code,
        path: String::from(path),
        message: message.into(),
    }
}

/// A raw type of ref/03 §2.2.7 (the M0 subset: no strings, bytes or named bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RawType {
    /// `bool`.
    Bool,
    /// `u16`.
    U16,
    /// `i16`.
    I16,
    /// `u32`.
    U32,
    /// `i32`.
    I32,
    /// `u64`.
    U64,
    /// `i64`.
    I64,
    /// `f32`.
    F32,
    /// `f64`.
    F64,
}

impl RawType {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "u16" => Self::U16,
            "i16" => Self::I16,
            "u32" => Self::U32,
            "i32" => Self::I32,
            "u64" => Self::U64,
            "i64" => Self::I64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            _ => return None,
        })
    }

    /// The name in the profile.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::U16 => "u16",
            Self::I16 => "i16",
            Self::U32 => "u32",
            Self::I32 => "i32",
            Self::U64 => "u64",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    /// Registers of the type.
    pub const fn registers(self) -> u16 {
        match self {
            Self::Bool | Self::U16 | Self::I16 => 1,
            Self::U32 | Self::I32 | Self::F32 => 2,
            Self::U64 | Self::I64 | Self::F64 => 4,
        }
    }
}

/// The signal type the acquisition stage gives (ref/03 §2.2.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SignalType {
    /// `bool`.
    Bool,
    /// `i64`.
    I64,
    /// `u64`.
    U64,
    /// `f64`.
    F64,
}

/// One point of the profile.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileSignal {
    /// Relative path.
    pub name: String,
    /// Protocol address, base 0.
    pub at: u16,
    /// Function code.
    pub fc: u8,
    /// Raw type.
    pub ty: RawType,
    /// Direction.
    pub direction: Direction,
    /// Poll period in ns, or `None` for `on_demand`.
    pub poll_ns: Option<u64>,
    /// Unit.
    pub unit: String,
    /// `scale`.
    pub scale: f64,
    /// `offset`.
    pub offset: f64,
    /// Little-endian word order.
    pub words_little: bool,
    /// Little-endian byte order.
    pub bytes_little: bool,
    /// Extracted bit.
    pub bit: Option<u8>,
    /// Inverted logic.
    pub invert: bool,
    /// Acquisition deadband.
    pub deadband: Option<f64>,
    /// Suggested readback point.
    pub readback: Option<String>,
    /// Counter.
    pub monotonic: bool,
    /// Label.
    pub label: Option<String>,
}

impl ProfileSignal {
    /// The signal type (ref/03 §2.2.7).
    pub fn signal_type(&self) -> SignalType {
        let scaled = self.scale != 1.0 || self.offset != 0.0 || self.unit != "1";
        match self.ty {
            RawType::Bool => SignalType::Bool,
            RawType::F32 | RawType::F64 => SignalType::F64,
            _ if scaled => SignalType::F64,
            RawType::I16 | RawType::I32 | RawType::I64 => SignalType::I64,
            _ => SignalType::U64,
        }
    }

    /// The raw form the core expects and its width (ref/06 §2.7).
    pub fn expect(&self) -> (RawKind, u16) {
        if matches!(self.fc, 1 | 2 | 5 | 15) {
            (RawKind::Bool, 0)
        } else {
            (RawKind::Regs, self.ty.registers())
        }
    }

    /// The deadband of this signal.
    pub fn deadband(&self) -> Deadband {
        Deadband::new(self.deadband.unwrap_or(0.0))
    }

    /// Decodes a raw value into the signal's value: raw type, orders, `bit`,
    /// `invert`, `scale` and `offset` (steps 1 and 3 of ref/03 §3). Booleans
    /// are 0 and 1. `None` for a raw value that does not fit.
    pub fn decode(&self, raw: &OwnedRaw) -> Option<f64> {
        let flip = |b: bool| if self.invert { !b } else { b };
        let (kind, width) = self.expect();
        let regs = match raw {
            OwnedRaw::Bool(b) if kind == RawKind::Bool => {
                return Some(f64::from(u8::from(flip(*b))));
            }
            OwnedRaw::Regs(r) if kind == RawKind::Regs && r.len() == usize::from(width) => r,
            _ => return None,
        };
        let mut words: Vec<u16> = regs
            .iter()
            .map(|w| {
                if self.bytes_little {
                    w.swap_bytes()
                } else {
                    *w
                }
            })
            .collect();
        if self.words_little {
            words.reverse();
        }
        let bits = words
            .iter()
            .fold(0u64, |acc, w| (acc << 16) | u64::from(*w));
        let raw = match self.ty {
            RawType::Bool => {
                let on = match self.bit {
                    Some(b) => bits >> b & 1 == 1,
                    None => bits != 0,
                };
                return Some(f64::from(u8::from(flip(on))));
            }
            RawType::U16 | RawType::U32 | RawType::U64 => bits as f64,
            RawType::I16 => f64::from(bits as u16 as i16),
            RawType::I32 => f64::from(bits as u32 as i32),
            RawType::I64 => bits as i64 as f64,
            RawType::F32 => f64::from(f32::from_bits(bits as u32)),
            RawType::F64 => f64::from_bits(bits),
        };
        Some(raw * self.scale + self.offset)
    }
}

/// The profile's own fallback (M1, ref/03 §2.2.11), M0 subset.
#[derive(Clone, Debug, PartialEq)]
pub struct Fallback {
    /// Fixed timeout in ns.
    pub timeout_ns: u64,
    /// Fixed value.
    pub value: f64,
    /// Outputs covered.
    pub applies_to: Vec<String>,
}

/// A parsed profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// `name`.
    pub name: String,
    /// `version`.
    pub version: String,
    /// Description.
    pub description: Option<String>,
    /// Points, in name order.
    pub signals: Vec<ProfileSignal>,
    /// `[fallback]`.
    pub fallback: Option<Fallback>,
    /// The `quirks` table, as CBOR (durations in ns).
    pub quirks: Vec<u8>,
}

fn table<'a>(v: &'a Value, path: &str) -> Result<&'a Table, ProfileError> {
    v.as_table()
        .ok_or_else(|| err(J0806, path, "expected a table"))
}

fn duration(v: &Value, path: &str) -> Result<u64, ProfileError> {
    let text = v
        .as_str()
        .ok_or_else(|| err(J0806, path, "expected a duration"))?;
    let (n, unit) = text.split_at(
        text.find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(text.len()),
    );
    let n: f64 = n
        .parse()
        .map_err(|_| err(J0806, path, "expected a duration"))?;
    let scale = match unit.trim() {
        "ns" => 1.0,
        "us" | "µs" => 1e3,
        "ms" => 1e6,
        "s" => 1e9,
        "m" | "min" => 60e9,
        "h" => 3600e9,
        _ => return Err(err(J0806, path, "expected a duration")),
    };
    Ok((n * scale).round() as u64)
}

fn number(v: &Value, path: &str) -> Result<f64, ProfileError> {
    match v {
        Value::Integer(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        Value::String(s) => s
            .split_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| err(J0806, path, "expected a number or a quantity")),
        _ => Err(err(J0806, path, "expected a number")),
    }
}

const POINT_KEYS: [&str; 22] = [
    "at",
    "fc",
    "type",
    "signal_type",
    "direction",
    "unit",
    "quantity",
    "scale",
    "offset",
    "word_order",
    "byte_order",
    "bit",
    "invert",
    "poll",
    "max_age",
    "deadband",
    "monotonic",
    "readback",
    "safe_hint",
    "range_hint",
    "label",
    "description",
];

impl Profile {
    /// Reads `<root>/profiles/<name>@<major>.profile.toml`, as resolution does
    /// for a project profile (ref/03 §3), and checks the name and major.
    pub fn load(root: &Path, reference: &str) -> Result<Self, ProfileError> {
        let (name, major) = reference
            .split_once('@')
            .ok_or_else(|| err(J0804, reference, "expected <name>@<major>"))?;
        let file = root
            .join("profiles")
            .join(std::format!("{reference}.profile.toml"));
        let text = std::fs::read_to_string(&file)
            .map_err(|e| err(code!("J0803"), &file.display().to_string(), e.to_string()))?;
        let p = Self::parse(&text)?;
        if p.name != name || p.version.split('.').next() != Some(major) {
            return Err(err(
                J0804,
                "profile",
                "name or major version differ from the file path",
            ));
        }
        Ok(p)
    }

    /// Parses a profile.
    pub fn parse(text: &str) -> Result<Self, ProfileError> {
        let doc: Table = text
            .parse()
            .map_err(|e: toml::de::Error| err(J0806, "", e.message().to_string()))?;
        if doc.get("schema").and_then(Value::as_integer) != Some(1) {
            return Err(err(J0800, "schema", "a profile starts with `schema = 1`"));
        }
        for key in doc.keys() {
            match key.as_str() {
                "schema" | "profile" | "defaults" | "signals" | "fallback" => {}
                "enums" | "coherent" | "config" | "actions" | "connect" | "relinquish"
                | "events" | "implements" | "discovery" => {
                    return Err(err(J0807, key, "not supported by the M0 profile subset"));
                }
                other => return Err(err(J0140, other, "unknown section")),
            }
        }
        let head = table(
            doc.get("profile")
                .ok_or_else(|| err(J0140, "profile", "missing [profile]"))?,
            "profile",
        )?;
        for k in head.keys() {
            if ![
                "name",
                "version",
                "driver",
                "vendor",
                "model",
                "description",
                "docs",
                "license",
                "authors",
                "firmware",
                "quirks",
            ]
            .contains(&k.as_str())
            {
                return Err(err(J0140, &std::format!("profile.{k}"), "unknown key"));
            }
        }
        let text_of = |t: &Table, k: &str, path: &str| -> Result<Option<String>, ProfileError> {
            match t.get(k) {
                None => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => Err(err(J0806, &std::format!("{path}.{k}"), "expected a string")),
            }
        };
        let name = text_of(head, "name", "profile")?
            .ok_or_else(|| err(J0804, "profile.name", "missing"))?;
        let version = text_of(head, "version", "profile")?
            .ok_or_else(|| err(J0804, "profile.version", "missing"))?;
        if text_of(head, "driver", "profile")?.as_deref() != Some("modbus") {
            return Err(err(
                code!("J0808"),
                "profile.driver",
                "this subset reads `modbus` profiles",
            ));
        }
        let quirks = match head.get("quirks") {
            None => Cbor::Map(Vec::new()),
            Some(q) => {
                let mut entries = Vec::new();
                for (k, v) in table(q, "profile.quirks")? {
                    let p = std::format!("profile.quirks.{k}");
                    let value = match k.as_str() {
                        "min_interval" | "latency" | "connect_timeout" => {
                            Cbor::Uint(duration(v, &p)?)
                        }
                        "max_regs" | "max_bits" | "max_gap" => Cbor::Uint(number(v, &p)? as u64),
                        _ => return Err(err(code!("J0818"), &p, "unknown quirk in the M0 subset")),
                    };
                    entries.push((k.clone(), value));
                }
                Cbor::Map(entries)
            }
        }
        .encode();

        let empty = Table::new();
        let defaults = match doc.get("defaults") {
            Some(d) => table(d, "defaults")?,
            None => &empty,
        };
        let mut signals = Vec::new();
        if let Some(s) = doc.get("signals") {
            for (sig_name, v) in table(s, "signals")? {
                signals.push(signal(
                    sig_name,
                    table(v, &std::format!("signals.{sig_name}"))?,
                    defaults,
                )?);
            }
        }
        signals.sort_by(|a, b| a.name.cmp(&b.name));
        let fallback = match doc.get("fallback") {
            None => None,
            Some(f) => {
                let f = table(f, "fallback")?;
                for k in f.keys() {
                    if !["timeout", "value", "applies_to"].contains(&k.as_str()) {
                        return Err(err(
                            J0807,
                            &std::format!("fallback.{k}"),
                            "not in the M0 subset (fixed timeout and value)",
                        ));
                    }
                }
                let timeout_ns = duration(
                    f.get("timeout")
                        .ok_or_else(|| err(J0820, "fallback", "missing `timeout`"))?,
                    "fallback.timeout",
                )?;
                let value = match f.get("value") {
                    Some(Value::Boolean(b)) => f64::from(u8::from(*b)),
                    Some(v) => number(v, "fallback.value")?,
                    None => return Err(err(J0820, "fallback", "missing `value`")),
                };
                let outputs: Vec<&ProfileSignal> = signals
                    .iter()
                    .filter(|s| s.direction == Direction::Output)
                    .collect();
                let applies_to = match f.get("applies_to") {
                    None => outputs.iter().map(|s| s.name.clone()).collect(),
                    Some(Value::Array(a)) => {
                        let mut list = Vec::new();
                        for x in a {
                            let n = x.as_str().ok_or_else(|| {
                                err(J0820, "fallback.applies_to", "expected names")
                            })?;
                            if !outputs.iter().any(|s| s.name == n) {
                                return Err(err(
                                    J0820,
                                    "fallback.applies_to",
                                    std::format!("`{n}` is not an output"),
                                ));
                            }
                            list.push(String::from(n));
                        }
                        list
                    }
                    Some(_) => return Err(err(J0820, "fallback.applies_to", "expected a list")),
                };
                Some(Fallback {
                    timeout_ns,
                    value,
                    applies_to,
                })
            }
        };
        for s in &signals {
            if let Some(rb) = &s.readback
                && !signals
                    .iter()
                    .any(|o| &o.name == rb && o.direction != Direction::Output)
            {
                return Err(err(
                    J0810,
                    &std::format!("signals.{}.readback", s.name),
                    "not an input of the profile",
                ));
            }
        }
        Ok(Self {
            name,
            version,
            description: text_of(head, "description", "profile")?,
            signals,
            fallback,
            quirks,
        })
    }

    /// The signal `name`.
    pub fn signal(&self, name: &str) -> Option<&ProfileSignal> {
        self.signals.iter().find(|s| s.name == name)
    }

    /// The plan request of one device using this profile on a serial bus:
    /// points in name order (ids 0..n), the device's `unit_id`, `timeout`
    /// and default `poll` (ref/01 §5.8), and the bus.
    pub fn request(
        &self,
        device: &str,
        unit_id: u8,
        timeout_ns: u64,
        poll_ns: u64,
        bus: OwnedBus,
    ) -> OwnedRequest {
        let config = Cbor::map([("unit_id", Cbor::Uint(u64::from(unit_id)))]).encode();
        let mut dev = OwnedDevice::new(device, config, timeout_ns);
        dev.quirks = self.quirks.clone();
        let points = self
            .signals
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (expect, width) = s.expect();
                let mut entries = std::vec![
                    ("at", Cbor::Uint(u64::from(s.at))),
                    ("fc", Cbor::Uint(u64::from(s.fc)))
                ];
                if s.ty.registers() > 1 {
                    let order = |little| Cbor::text(if little { "little" } else { "big" });
                    entries.push(("word_order", order(s.words_little)));
                    entries.push(("byte_order", order(s.bytes_little)));
                }
                if let Some(b) = s.bit {
                    entries.push(("bit", Cbor::Uint(u64::from(b))));
                }
                let addr = Cbor::Map(
                    entries
                        .into_iter()
                        .map(|(k, v)| (String::from(k), v))
                        .collect(),
                )
                .encode();
                let poll = match (s.direction, s.poll_ns) {
                    (Direction::Output, _) => Poll::OnDemand,
                    (_, Some(ns)) => Poll::Period(ns),
                    (_, None) if s.direction == Direction::Param => Poll::OnDemand,
                    (_, None) => Poll::Period(poll_ns),
                };
                OwnedPoint {
                    id: i as u32,
                    device: 0,
                    path: s.name.clone(),
                    direction: s.direction,
                    expect,
                    width,
                    type_name: String::from(s.ty.name()),
                    addr,
                    poll,
                    class: if s.direction == Direction::Param {
                        ReadClass::Diagnostic
                    } else {
                        ReadClass::Cycle
                    },
                    coherent: None,
                    debounce_ns: 0,
                    timed: false,
                }
            })
            .collect();
        OwnedRequest {
            bus: Some(bus),
            devices: std::vec![dev],
            points,
        }
    }
}

/// The CBOR of a `serial` bus (ref/01 §5.6).
pub fn serial_bus(name: &str, port: &str, baud: u32) -> OwnedBus {
    OwnedBus {
        name: String::from(name),
        kind: BusKind::Serial,
        params: Cbor::map([
            ("kind", Cbor::text("serial")),
            ("port", Cbor::text(port)),
            ("baud", Cbor::Uint(u64::from(baud))),
        ])
        .encode(),
    }
}

fn signal(name: &str, t: &Table, defaults: &Table) -> Result<ProfileSignal, ProfileError> {
    let path = std::format!("signals.{name}");
    for k in t.keys() {
        if !POINT_KEYS.contains(&k.as_str()) {
            let code = if [
                "enum",
                "bits",
                "len",
                "encoding",
                "invalid",
                "limits",
                "stuck_detect",
                "scale_factor",
                "kind",
            ]
            .contains(&k.as_str())
            {
                J0807
            } else {
                J0140
            };
            return Err(err(
                code,
                &std::format!("{path}.{k}"),
                "not in the M0 profile subset",
            ));
        }
    }
    let get = |k: &str| t.get(k).or_else(|| defaults.get(k));
    let p = |k: &str| std::format!("{path}.{k}");
    let int = |k: &str| -> Result<Option<i64>, ProfileError> {
        match get(k) {
            None => Ok(None),
            Some(Value::Integer(i)) => Ok(Some(*i)),
            Some(_) => Err(err(J0806, &p(k), "expected an integer")),
        }
    };
    let text = |k: &str| -> Result<Option<String>, ProfileError> {
        match get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(err(J0806, &p(k), "expected a string")),
        }
    };
    let at = int("at")?
        .and_then(|a| u16::try_from(a).ok())
        .ok_or_else(|| err(J0810, &p("at"), "`at` is 0–65535"))?;
    let fc = int("fc")?
        .and_then(|f| u8::try_from(f).ok())
        .filter(|f| matches!(f, 1..=6 | 15 | 16))
        .ok_or_else(|| err(J0810, &p("fc"), "`fc` is 1–6, 15 or 16"))?;
    let ty_name = text("type")?.ok_or_else(|| err(J0809, &path, "missing `type`"))?;
    let ty = RawType::parse(&ty_name)
        .ok_or_else(|| err(J0807, &p("type"), "type not in the M0 subset"))?;
    let direction = match text("direction")?.as_deref().unwrap_or("input") {
        "input" => Direction::Input,
        "output" => Direction::Output,
        "param" => Direction::Param,
        _ => return Err(err(J0806, &p("direction"), "input, output or param")),
    };
    let fc_ok = match direction {
        Direction::Output => matches!(fc, 5 | 6 | 15 | 16),
        _ => matches!(fc, 1..=4),
    };
    if !fc_ok {
        return Err(err(
            J0810,
            &p("fc"),
            "function code invalid for the direction",
        ));
    }
    let bit = int("bit")?.map(|b| b as u8);
    let bits_fc = matches!(fc, 1 | 2 | 5 | 15);
    if (bits_fc && ty != RawType::Bool)
        || (bit.is_some() && (bits_fc || ty != RawType::Bool))
        || (fc == 6 && ty.registers() > 1)
    {
        return Err(err(J0809, &p("type"), "type invalid for the address"));
    }
    if u32::from(at) + u32::from(ty.registers()) - 1 > 65_535 {
        return Err(err(J0810, &p("at"), "address past 65535"));
    }
    let unit = text("unit")?.unwrap_or_else(|| String::from("1"));
    let scale = get("scale")
        .map(|v| number(v, &p("scale")))
        .transpose()?
        .unwrap_or(1.0);
    let offset = get("offset")
        .map(|v| number(v, &p("offset")))
        .transpose()?
        .unwrap_or(0.0);
    if ty == RawType::Bool && (unit != "1" || scale != 1.0 || offset != 0.0) {
        return Err(err(J0815, &path, "a `bool` has no unit, scale or offset"));
    }
    let order = |k: &str| -> Result<bool, ProfileError> {
        match text(k)?.as_deref() {
            None | Some("big") => Ok(false),
            Some("little") => Ok(true),
            Some(_) => Err(err(J0806, &p(k), "big or little")),
        }
    };
    let poll_ns = match t.get("poll").or_else(|| defaults.get("poll")) {
        None => None,
        Some(Value::String(s)) if s == "on_demand" => {
            if direction != Direction::Param {
                return Err(err(J0817, &p("poll"), "`on_demand` is only for `param`"));
            }
            None
        }
        Some(v) if direction == Direction::Param && t.get("poll").is_none() => {
            let _ = v;
            None
        }
        Some(v) => Some(duration(v, &p("poll"))?),
    };
    let flag = |k: &str| matches!(t.get(k), Some(Value::Boolean(true)));
    Ok(ProfileSignal {
        name: String::from(name),
        at,
        fc,
        ty,
        direction,
        poll_ns,
        unit,
        scale,
        offset,
        words_little: order("word_order")?,
        bytes_little: order("byte_order")?,
        bit,
        invert: flag("invert"),
        deadband: t
            .get("deadband")
            .map(|v| number(v, &p("deadband")))
            .transpose()?,
        readback: text("readback")?.filter(|_| t.contains_key("readback")),
        monotonic: flag("monotonic"),
        label: t.get("label").and_then(Value::as_str).map(String::from),
    })
}

/// The raw values of a profile point, for tests and doubles: what a value
/// becomes on the wire.
pub fn encode_regs(s: &ProfileSignal, value: u64) -> Vec<u16> {
    let n = usize::from(s.ty.registers());
    let mut words: Vec<u16> = (0..n).rev().map(|i| (value >> (16 * i)) as u16).collect();
    if s.words_little {
        words.reverse();
    }
    if s.bytes_little {
        words.iter_mut().for_each(|w| *w = w.swap_bytes());
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn by_direction(p: &Profile) -> BTreeMap<&'static str, Vec<&str>> {
        let mut m: BTreeMap<&'static str, Vec<&str>> = BTreeMap::new();
        for s in &p.signals {
            let k = match s.direction {
                Direction::Input => "input",
                Direction::Output => "output",
                Direction::Param => "param",
            };
            m.entry(k).or_default().push(&s.name);
        }
        m
    }

    const BENCH: &str = include_str!("../tests/project/profiles/local/bench-esp32@1.profile.toml");

    #[test]
    fn the_bench_profile_parses() {
        let p = Profile::parse(BENCH).unwrap();
        assert_eq!(
            (p.name.as_str(), p.version.as_str()),
            ("local/bench-esp32", "1.0.0")
        );
        let dirs = by_direction(&p);
        assert_eq!(dirs["output"], ["led"]);
        assert_eq!(dirs["param"], ["fw"]);
        assert_eq!(
            dirs["input"],
            ["button", "led_rb", "rssi", "touch", "uptime"]
        );
        let touch = p.signal("touch").unwrap();
        assert_eq!(
            (touch.poll_ns, touch.deadband),
            (Some(100_000_000), Some(5.0))
        );
        assert_eq!(touch.signal_type(), SignalType::U64);
        assert_eq!(p.signal("uptime").unwrap().signal_type(), SignalType::F64);
        assert_eq!(p.signal("rssi").unwrap().signal_type(), SignalType::I64);
        assert_eq!(p.signal("fw").unwrap().poll_ns, None);
        assert_eq!(p.signal("led").unwrap().readback.as_deref(), Some("led_rb"));
        let f = p.fallback.clone().unwrap();
        assert_eq!(
            (f.timeout_ns, f.value, f.applies_to),
            (2_000_000_000, 0.0, std::vec![String::from("led")])
        );
    }

    #[test]
    fn decoding_follows_type_and_orders() {
        let p = Profile::parse(BENCH).unwrap();
        let rssi = p.signal("rssi").unwrap();
        assert_eq!(rssi.decode(&OwnedRaw::Regs(std::vec![0xFFBD])), Some(-67.0));
        let uptime = p.signal("uptime").unwrap();
        assert_eq!(
            uptime.decode(&OwnedRaw::Regs(std::vec![0x0001, 0x0002])),
            Some(65_538.0)
        );
        assert_eq!(encode_regs(uptime, 65_538), [1, 2]);
        let button = p.signal("button").unwrap();
        assert_eq!(button.decode(&OwnedRaw::Bool(true)), Some(1.0));
        assert_eq!(button.decode(&OwnedRaw::Regs(std::vec![1])), None);
        let mut le = uptime.clone();
        le.words_little = true;
        le.bytes_little = true;
        let words = encode_regs(&le, 0x0102_0304);
        assert_eq!(
            le.decode(&OwnedRaw::Regs(words)),
            Some(f64::from(0x0102_0304))
        );
        let mut bit = button.clone();
        bit.fc = 3;
        bit.bit = Some(3);
        assert_eq!(bit.decode(&OwnedRaw::Regs(std::vec![0b1000])), Some(1.0));
    }

    #[test]
    fn subset_errors() {
        let head =
            "schema = 1\n[profile]\nname = \"a/b\"\nversion = \"1.0.0\"\ndriver = \"modbus\"\n";
        let cases = [
            (
                "[signals]\nx = { at = 0, fc = 4, type = \"string\", len = 2 }\n",
                J0807,
            ),
            ("[signals]\nx = { at = 0, fc = 1, type = \"u16\" }\n", J0809),
            (
                "[signals]\nx = { at = 0, fc = 5, type = \"bool\" }\n",
                J0810,
            ),
            (
                "[signals]\nx = { at = 0, fc = 4, type = \"u16\", colour = 1 }\n",
                J0140,
            ),
            (
                "[signals]\nx = { at = 0, fc = 1, type = \"bool\", unit = \"V\" }\n",
                J0815,
            ),
            (
                "[signals]\nx = { at = 0, fc = 1, type = \"bool\" }\n[fallback]\ntimeout = \"2s\"\nvalue = false\napplies_to = [\"x\"]\n",
                J0820,
            ),
            ("[enums]\n", J0807),
            (
                "[signals]\nx = { at = 0, fc = 4, type = \"u16\", poll = \"on_demand\" }\n",
                J0817,
            ),
        ];
        for (body, code) in cases {
            let e = Profile::parse(&std::format!("{head}{body}")).unwrap_err();
            assert_eq!(e.code, code, "{body}");
        }
    }
}
