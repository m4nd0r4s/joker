//! The effective manifest in canonical form and its hash (ref/01 §8.2–§8.3):
//! defaults materialised, `x_*` keys dropped, durations in nanoseconds,
//! quantities in coherent SI, free tables in lexicographic order, encoded as
//! deterministic CBOR (RFC 8949 §4.2.1) and hashed with SHA-256.

use std::collections::BTreeMap;
use std::path::Path;

use joker_units::{Duration, Literal};
use sha2::{Digest, Sha256};

use crate::context::Context;
use crate::model::Spanned;
use crate::model::{
    App, Binding, Bus, BusKind, Cycle, Device, Envelope, Kernel, Manifest, Mode, SafeValue,
};
use crate::source::{Sources, normalize};
use crate::value::{Node, Value};

/// A canonical value.
#[derive(Debug, Clone, PartialEq)]
pub enum Canon {
    /// A map with text keys.
    Map(BTreeMap<String, Canon>),
    /// An array.
    Array(Vec<Canon>),
    /// A text string.
    Text(String),
    /// A signed integer.
    Integer(i64),
    /// A float.
    Float(f64),
    /// A boolean.
    Bool(bool),
}

struct MapBuilder(BTreeMap<String, Canon>);

impl MapBuilder {
    fn new() -> Self {
        Self(BTreeMap::new())
    }

    fn set(mut self, key: &str, value: impl Into<Canon>) -> Self {
        self.0.insert(key.to_owned(), value.into());
        self
    }

    fn opt(self, key: &str, value: Option<impl Into<Canon>>) -> Self {
        match value {
            Some(v) => self.set(key, v),
            None => self,
        }
    }

    fn build(self) -> Canon {
        Canon::Map(self.0)
    }
}

impl From<&str> for Canon {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for Canon {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&String> for Canon {
    fn from(value: &String) -> Self {
        Self::Text(value.clone())
    }
}

impl From<bool> for Canon {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for Canon {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<Duration> for Canon {
    fn from(value: Duration) -> Self {
        Self::Integer(i64::try_from(value.as_nanos()).unwrap_or(i64::MAX))
    }
}

/// A quantity or bound as written, normalised: a quantity becomes
/// `[si_value, "dimension", "kind"]` (the kind empty when the unit has
/// none), a bare number a float, anything else (an expression) stays text.
fn quantity(text: &str) -> Canon {
    match Literal::parse(text) {
        Ok(Literal::Quantity(q)) => Canon::Array(vec![
            Canon::Float(q.unit.to_si(q.value)),
            Canon::Text(q.unit.difference().dimension().to_string()),
            Canon::Text(q.unit.kind().map_or("", |k| k.as_str()).to_owned()),
        ]),
        Ok(Literal::Number(n)) => Canon::Float(n),
        Err(_) => Canon::Text(text.to_owned()),
    }
}

fn raw(node: &Node) -> Canon {
    match &node.value {
        Value::String(s) => Canon::Text(s.clone()),
        Value::Integer(i) => Canon::Integer(*i),
        Value::Float(f) => Canon::Float(*f),
        Value::Bool(b) => Canon::Bool(*b),
        Value::Datetime(d) => Canon::Text(d.clone()),
        Value::Array(items) => Canon::Array(items.iter().map(raw).collect()),
        Value::Table(t) => Canon::Map(
            t.iter()
                .filter(|(k, _)| !k.starts_with("x_"))
                .map(|(k, e)| (k.clone(), raw(&e.node)))
                .collect(),
        ),
    }
}

fn raw_map(map: &BTreeMap<String, Node>) -> Canon {
    Canon::Map(
        map.iter()
            .filter(|(k, _)| !k.starts_with("x_"))
            .map(|(k, v)| (k.clone(), raw(v)))
            .collect(),
    )
}

/// Builds the canonical effective manifest. File paths are rewritten
/// relative to the project root (ref/01 §7.5), using the layer each one was
/// written in.
pub fn canonical(manifest: &Manifest, context: &Context, sources: &Sources) -> Canon {
    let mut root = MapBuilder::new();
    let project = &manifest.project;
    let joker = project.joker.as_ref().map_or(context.runtime, |j| j.value);
    root = root.set(
        "project",
        MapBuilder::new()
            .opt("name", project.name.as_ref().map(|n| &n.value))
            .set("joker", format!("{}.{}", joker.major, joker.minor))
            .set(
                "mode",
                if project.mode == Mode::Dev {
                    "dev"
                } else {
                    "prod"
                },
            )
            .build(),
    );
    if let Some(target) = &manifest.target {
        let board = target.board.as_ref().map(|b| {
            let major = b.value.major.or_else(|| {
                context
                    .boards
                    .get(&b.value.name)
                    .and_then(|info| info.majors.iter().max().copied())
            });
            match major {
                Some(m) => format!("{}@{m}", b.value.name),
                None => b.value.name.clone(),
            }
        });
        let arch = target.arch.as_ref().map(|a| a.value.clone()).or_else(|| {
            target
                .board
                .as_ref()
                .and_then(|b| context.boards.get(&b.value.name))
                .map(|info| info.arch.clone())
        });
        root = root.set(
            "target",
            MapBuilder::new()
                .opt("board", board)
                .opt("arch", arch)
                .set(
                    "kernel",
                    if target.kernel == Kernel::PreemptRt {
                        "preempt_rt"
                    } else {
                        "standard"
                    },
                )
                .build(),
        );
    }
    let section = |items: Vec<(String, Canon)>| Canon::Map(items.into_iter().collect());
    root = root
        .set(
            "buses",
            section(
                manifest
                    .buses
                    .iter()
                    .map(|(k, b)| (k.clone(), bus(b)))
                    .collect(),
            ),
        )
        .set(
            "devices",
            section(
                manifest
                    .devices
                    .iter()
                    .map(|(k, d)| (k.clone(), device(d, context)))
                    .collect(),
            ),
        )
        .set(
            "outputs",
            section(
                manifest
                    .outputs
                    .iter()
                    .map(|(k, e)| (k.clone(), envelope(e)))
                    .collect(),
            ),
        )
        .set(
            "cycles",
            section(
                manifest
                    .cycles
                    .iter()
                    .map(|(k, c)| (k.clone(), cycle(c, manifest)))
                    .collect(),
            ),
        )
        .set(
            "apps",
            section(
                manifest
                    .apps
                    .iter()
                    .map(|(k, a)| (k.clone(), app(a, sources)))
                    .collect(),
            ),
        );
    for (key, node) in &manifest.pending {
        root = root.set(key, raw(node));
    }
    root.build()
}

fn bus(bus: &Bus) -> Canon {
    let kind = bus.kind.value;
    let m = MapBuilder::new().set("kind", kind.as_str());
    let text = |s: &Option<crate::model::Spanned<String>>| s.as_ref().map(|v| v.value.clone());
    let int = |s: &Option<crate::model::Spanned<i64>>| s.as_ref().map(|v| v.value);
    match kind {
        BusKind::Ethernet => m.opt("iface", text(&bus.iface)),
        BusKind::Serial => {
            let turnaround = bus.turnaround.as_ref().map(|t| t.value).or_else(|| {
                let baud = u64::try_from(int(&bus.baud)?).ok().filter(|b| *b > 0)?;
                Some(Duration::from_nanos(35 * 1_000_000_000 / baud))
            });
            m.opt("port", text(&bus.port))
                .opt("baud", int(&bus.baud))
                .set("parity", bus.parity.as_str())
                .set("stop", bus.stop)
                .set("data_bits", bus.data_bits)
                .opt("turnaround", turnaround)
                .set("dtr", bus.dtr.as_str())
                .set("rts", bus.rts.as_str())
                .set("duplex", bus.duplex.as_str())
        }
        BusKind::Can => m
            .opt("iface", text(&bus.iface))
            .opt("bitrate", int(&bus.bitrate))
            .set("fd", bus.fd)
            .opt("data_bitrate", int(&bus.data_bitrate)),
        BusKind::Ethercat => m
            .opt("iface", text(&bus.iface))
            .opt("cycle", bus.cycle.as_ref().map(|c| c.value))
            .set("dc", bus.dc),
        BusKind::I2c => m.opt("dev", text(&bus.dev)),
        BusKind::Spi => m.opt("dev", text(&bus.dev)).set("mode", bus.spi_mode).set(
            "speed",
            quantity(bus.speed.as_ref().map_or("1 MHz", |s| s.value.as_str())),
        ),
    }
    .build()
}

fn device(device: &Device, context: &Context) -> Canon {
    let profile = device
        .profile
        .as_ref()
        .and_then(|p| context.profiles.get(&p.value));
    let poll = device
        .poll
        .as_ref()
        .map(|p| p.value)
        .or_else(|| profile.and_then(|p| p.poll))
        .unwrap_or(Duration::from_secs(1));
    let timeout = device
        .timeout
        .as_ref()
        .map(|t| t.value)
        .or_else(|| profile.and_then(|p| p.timeout))
        .unwrap_or(Duration::from_secs(1));
    let mut m = MapBuilder::new()
        .opt("driver", device.driver.as_ref().map(|d| &d.value))
        .opt("bus", device.bus.as_ref().map(|b| &b.value))
        .opt("profile", device.profile.as_ref().map(|p| &p.value))
        .set("poll", poll)
        .set("timeout", timeout)
        .set("retries", device.retries)
        .opt("align", device.align.as_ref().map(|a| &a.value))
        .set(
            "discover",
            device.discover.as_ref().is_some_and(|d| d.value),
        )
        .set(
            "fallback",
            MapBuilder::new()
                .set(
                    "configure",
                    device.fallback_configure.as_ref().is_some_and(|c| c.value),
                )
                .opt(
                    "keepalive",
                    device.fallback_keepalive.as_ref().map(|k| k.value),
                )
                .build(),
        )
        .set(
            "signals",
            Canon::Map(
                device
                    .signals
                    .iter()
                    .map(|(k, s)| (k.clone(), raw_map(&s.value)))
                    .collect(),
            ),
        )
        .set("actions", raw_map(&device.actions));
    for (key, node) in &device.config {
        if !key.starts_with("x_") {
            m = m.set(key, raw(node));
        }
    }
    m.build()
}

fn envelope(e: &Envelope) -> Canon {
    let safe = e.safe.as_ref().map(|s| match &s.value {
        SafeValue::Bool(b) => Canon::Bool(*b),
        SafeValue::Integer(i) => Canon::Integer(*i),
        SafeValue::Float(f) => Canon::Float(*f),
        SafeValue::Text(t) => quantity(t),
        SafeValue::Relinquish => Canon::Text("relinquish".to_owned()),
    });
    let text = |s: &Option<crate::model::Spanned<String>>| s.as_ref().map(|v| v.value.clone());
    let q = |s: &Option<crate::model::Spanned<String>>| s.as_ref().map(|v| quantity(&v.value));
    let mut requires: Vec<String> = e.requires_good.iter().map(|r| r.value.clone()).collect();
    requires.sort();
    requires.dedup();
    MapBuilder::new()
        .opt("safe", safe)
        .set("safer", e.safer.as_str())
        .opt("safe_ramp", q(&e.safe_ramp))
        .opt("interlock", text(&e.interlock))
        .set("permit_uncertain", e.permit_uncertain)
        .set("on_interlock", e.on_interlock.as_str())
        .set(
            "requires_good",
            Canon::Array(requires.into_iter().map(Canon::Text).collect()),
        )
        .set("grace", e.grace)
        .opt(
            "range",
            e.range
                .as_ref()
                .map(|r| Canon::Array(r.value.iter().map(|b| quantity(&b.value)).collect())),
        )
        .set("on_range", e.on_range.as_str())
        .opt("rate", q(&e.rate))
        .opt("deadband", q(&e.deadband))
        .set("min_interval", e.min_interval)
        .opt(
            "max_actuations",
            e.max_actuations.as_ref().map(|b| {
                Canon::Array(vec![
                    Canon::Integer(i64::from(b.value.count)),
                    Canon::from(b.value.window),
                ])
            }),
        )
        .set("max_lease", e.max_lease)
        .set("max_manual", e.max_manual)
        .opt("verify", e.verify.as_ref().map(raw))
        .opt("reset_when", text(&e.reset_when))
        .opt("group", text(&e.group))
        .opt("keepalive", e.keepalive.as_ref().map(|k| k.value))
        .opt("pulse", e.pulse.as_ref().map(|p| p.value))
        .build()
}

fn cycle(c: &Cycle, manifest: &Manifest) -> Canon {
    let has_rt = manifest
        .pending
        .get("resources")
        .and_then(Node::as_table)
        .is_some_and(|t| t.contains_key("rt"));
    let resources = c
        .resources
        .as_ref()
        .map(|r| r.value.clone())
        .or_else(|| has_rt.then(|| "rt".to_owned()));
    MapBuilder::new()
        .opt("period", c.period.as_ref().map(|p| p.value))
        .opt("resources", resources)
        .set("overrun", c.overrun.as_str())
        .opt("trip_after", c.trip_after)
        .set("alarm_after", c.alarm_after)
        .set("recover_after", c.recover_after)
        .set(
            "order",
            Canon::Array(
                c.order
                    .iter()
                    .map(|o| Canon::Text(o.value.clone()))
                    .collect(),
            ),
        )
        .set(
            "trace",
            MapBuilder::new()
                .opt("continuous", c.trace_continuous.as_ref().map(|t| t.value))
                .set("async", c.trace_async)
                .build(),
        )
        .opt("cpu", c.cpu.as_ref().map(|v| v.value))
        .opt("priority", c.priority.as_ref().map(|v| v.value))
        .build()
}

fn rooted(path: &Spanned<String>, sources: &Sources) -> String {
    let declaring = sources.path(path.span.file);
    let joined = declaring
        .parent()
        .unwrap_or(Path::new(""))
        .join(&path.value);
    normalize(&joined).to_string_lossy().replace('\\', "/")
}

fn app(a: &App, sources: &Sources) -> Canon {
    let writer = a.writer.as_ref().map_or_else(
        || {
            MapBuilder::new()
                .set("level", "control:12")
                .set("kind", "direct")
                .build()
        },
        |w| {
            MapBuilder::new()
                .set(
                    "level",
                    format!("{}:{}", w.level.band.as_str(), w.level.priority),
                )
                .opt("lease", w.lease.as_ref().map(|l| l.value))
                .set("kind", "direct")
                .build()
        },
    );
    let bind = a
        .bind
        .iter()
        .map(|(k, b)| {
            let value = match &b.value {
                Binding::Target(t) => Canon::Text(t.value.clone()),
                Binding::List(items) => {
                    Canon::Array(items.iter().map(|i| Canon::Text(i.value.clone())).collect())
                }
                Binding::Signal {
                    signal,
                    conversion,
                    optional,
                } => MapBuilder::new()
                    .set("signal", &signal.value)
                    .opt("as", conversion.as_ref().map(|c| &c.value))
                    .set("optional", *optional)
                    .build(),
                Binding::Each(node) => raw(node),
            };
            (k.clone(), value)
        })
        .collect();
    let params = a
        .params
        .iter()
        .map(|(k, v)| {
            let value = match &v.value.value {
                Value::String(s) => quantity(s),
                _ => raw(&v.value),
            };
            (k.clone(), value)
        })
        .collect();
    let crashloop = a.crashloop.as_ref().map_or_else(
        || {
            MapBuilder::new()
                .set("traps", 3)
                .set("window", Duration::from_secs(10))
                .set("max_backoff", Duration::from_secs(300))
                .build()
        },
        raw,
    );
    MapBuilder::new()
        .opt("source", a.source.as_ref().map(|s| &s.value))
        .opt("path", a.path.as_ref().map(|p| rooted(p, sources)))
        .opt("digest", a.digest.as_ref().map(|d| &d.value))
        .set("kind", a.kind.as_str())
        .set("writer", writer)
        .set("bind", Canon::Map(bind))
        .set("params", Canon::Map(params))
        .set(
            "cycles",
            Canon::Map(
                a.cycles
                    .iter()
                    .map(|(k, g)| (k.clone(), Canon::Text(g.value.clone())))
                    .collect(),
            ),
        )
        .set(
            "period_tolerance",
            quantity(
                a.period_tolerance
                    .as_ref()
                    .map_or("50 %", |p| p.value.as_str()),
            ),
        )
        .opt("resources", a.resources.as_ref().map(raw))
        .set(
            "net",
            MapBuilder::new()
                .set(
                    "allow",
                    Canon::Array({
                        let mut allow: Vec<Canon> = a
                            .net_allow
                            .iter()
                            .map(|h| Canon::Text(h.value.clone()))
                            .collect();
                        allow.sort_by(|x, y| format!("{x:?}").cmp(&format!("{y:?}")));
                        allow.dedup();
                        allow
                    }),
                )
                .build(),
        )
        .set(
            "secrets",
            Canon::Map(
                a.secrets
                    .iter()
                    .map(|(k, s)| (k.clone(), Canon::Text(s.value.clone())))
                    .collect(),
            ),
        )
        .set("requires_clock", a.requires_clock.as_str())
        .set("keep_previous", a.keep_previous)
        .set("crashloop", crashloop)
        .opt("shadow", a.shadow.as_ref().map(raw))
        .build()
}

impl From<i32> for Canon {
    fn from(value: i32) -> Self {
        Self::Integer(i64::from(value))
    }
}

/// Encodes a canonical value as deterministic CBOR (RFC 8949 §4.2.1).
pub fn cbor(value: &Canon) -> Vec<u8> {
    let mut out = Vec::new();
    encode(value, &mut out);
    out
}

fn head(major: u8, value: u64, out: &mut Vec<u8>) {
    let major = major << 5;
    match value {
        0..=23 => out.push(major | value as u8),
        24..=0xff => out.extend([major | 24, value as u8]),
        0x100..=0xffff => {
            out.push(major | 25);
            out.extend((value as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(major | 26);
            out.extend((value as u32).to_be_bytes());
        }
        _ => {
            out.push(major | 27);
            out.extend(value.to_be_bytes());
        }
    }
}

fn encode(value: &Canon, out: &mut Vec<u8>) {
    match value {
        Canon::Integer(i) if *i >= 0 => head(0, i.unsigned_abs(), out),
        Canon::Integer(i) => head(1, i.unsigned_abs() - 1, out),
        Canon::Text(s) => {
            head(3, s.len() as u64, out);
            out.extend(s.as_bytes());
        }
        Canon::Array(items) => {
            head(4, items.len() as u64, out);
            for item in items {
                encode(item, out);
            }
        }
        Canon::Map(map) => {
            let mut entries: Vec<(Vec<u8>, Vec<u8>)> = map
                .iter()
                .map(|(k, v)| (cbor(&Canon::Text(k.clone())), cbor(v)))
                .collect();
            entries.sort();
            head(5, entries.len() as u64, out);
            for (k, v) in entries {
                out.extend(k);
                out.extend(v);
            }
        }
        Canon::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
        Canon::Float(f) => float(*f, out),
    }
}

fn float(value: f64, out: &mut Vec<u8>) {
    if value.is_nan() {
        out.extend([0xf9, 0x7e, 0x00]);
        return;
    }
    if let Some(half) = to_half(value) {
        out.push(0xf9);
        out.extend(half.to_be_bytes());
        return;
    }
    let single = value as f32;
    if f64::from(single) == value {
        out.push(0xfa);
        out.extend(single.to_bits().to_be_bytes());
        return;
    }
    out.push(0xfb);
    out.extend(value.to_bits().to_be_bytes());
}

fn to_half(value: f64) -> Option<u16> {
    let single = value as f32;
    if f64::from(single) != value {
        return None;
    }
    let bits = single.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exponent == 0xff {
        return (mantissa == 0).then_some(sign | 0x7c00);
    }
    if exponent == 0 && mantissa == 0 {
        return Some(sign);
    }
    let e = exponent - 127 + 15;
    if (1..=30).contains(&e) {
        if mantissa & 0x1fff != 0 {
            return None;
        }
        return Some(sign | ((e as u16) << 10) | ((mantissa >> 13) as u16));
    }
    if (-10..=0).contains(&e) {
        let full = mantissa | 0x80_0000;
        let shift = 14 - e;
        if full & ((1 << shift) - 1) != 0 {
            return None;
        }
        return Some(sign | (full >> shift) as u16);
    }
    None
}

/// The hash of the effective manifest, `"sha256:<hex>"`.
pub fn hash(value: &Canon) -> String {
    let digest = Sha256::digest(cbor(value));
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbor_is_deterministic_and_shortest() {
        assert_eq!(cbor(&Canon::Integer(0)), [0x00]);
        assert_eq!(cbor(&Canon::Integer(24)), [0x18, 24]);
        assert_eq!(cbor(&Canon::Integer(-1)), [0x20]);
        assert_eq!(cbor(&Canon::Integer(1000)), [0x19, 0x03, 0xe8]);
        assert_eq!(cbor(&Canon::Float(1.5)), [0xf9, 0x3e, 0x00]);
        assert_eq!(cbor(&Canon::Float(0.0)), [0xf9, 0x00, 0x00]);
        assert_eq!(
            cbor(&Canon::Float(100_000.0)),
            [0xfa, 0x47, 0xc3, 0x50, 0x00]
        );
        assert_eq!(cbor(&Canon::Float(1.1)), {
            let mut v = vec![0xfb];
            v.extend(1.1f64.to_bits().to_be_bytes());
            v
        });
        assert_eq!(
            cbor(&Canon::Float(5.960_464_477_539_063e-8)),
            [0xf9, 0x00, 0x01]
        );
        let map = Canon::Map(
            [
                ("bb", Canon::Bool(true)),
                ("a", Canon::Bool(false)),
                ("c", Canon::Text("x".into())),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        );
        assert_eq!(
            cbor(&map),
            [
                0xa3, 0x61, b'a', 0xf4, 0x61, b'c', 0x61, b'x', 0x62, b'b', b'b', 0xf5
            ]
        );
    }

    #[test]
    fn equal_quantities_normalise_alike() {
        assert_eq!(quantity("125000 W"), quantity("125 kW"));
        assert_ne!(quantity("125 kW"), quantity("125 kvar"));
        assert_eq!(quantity("pcs.p_max"), Canon::Text("pcs.p_max".into()));
    }
}
