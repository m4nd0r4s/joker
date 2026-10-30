//! JSON forms shared by the API and the logs (ref/00 §3–§4, ref/09 §3.3).

use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use joker_core::{Quality, QualityCode, Sample, ValueType, WideSample};
use joker_units::{Duration, Literal, Unit};
use serde_json::{Value, json};

use crate::plan::{Plan, SignalPlan};

/// An instant as RFC 3339 UTC with nanoseconds (ref/00 §3).
pub fn timestamp(utc_ns: i64) -> String {
    let t = if utc_ns >= 0 {
        UNIX_EPOCH + StdDuration::from_nanos(utc_ns as u64)
    } else {
        UNIX_EPOCH
    };
    humantime::format_rfc3339_nanos(t).to_string()
}

/// The versions of every surface this build supports (`GET /v1/version`,
/// ref/09 §5.1, ADR 0098). A surface this build does not carry (tier W
/// drivers, JDP, Joker Link) lists nothing.
pub fn versions() -> Value {
    let runtime = joker_manifest::context::Context::default().runtime;
    json!({
        "runtime": env!("CARGO_PKG_VERSION"),
        "api": {
            "current": joker_api::server::API_VERSION,
            "supported": [joker_api::server::API_VERSION],
        },
        "wit": {
            "current": joker_host::WORLD,
            "supported": joker_host::component::SUPPORTED_WORLDS,
        },
        "driver_wit": [],
        "jdp": [],
        "manifest": [format!("{}.{}", runtime.major, runtime.minor)],
        "lock": [1],
        "agent": "agent/1",
        "link": {"supported": []},
        "abi": {"major": joker_host::abi::ABI_VERSION, "minor": 0},
        "trace": [1],
    })
}

/// The current UTC time in ns.
pub fn utc_now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// A duration in canonical form (ref/00 §3.1).
pub fn duration(ns: u64) -> String {
    Duration::from_nanos(ns).to_string()
}

/// Parses a duration literal (`J-E1012` on error, with the parameter).
pub fn parse_duration(text: &str) -> Result<u64, String> {
    Duration::parse(text)
        .map(Duration::as_nanos)
        .map_err(|e| format!("`{text}`: {e}"))
}

/// The JSON value of slot bits for a signal (ref/00 §4.1).
pub fn value(signal: &SignalPlan, bits: u64) -> Value {
    match signal.ty {
        ValueType::Bool => json!(bits != 0),
        ValueType::F64 | ValueType::F32 => {
            let v = f64::from_bits(bits);
            if v.is_finite() { json!(v) } else { Value::Null }
        }
        ValueType::I64 => {
            let v = bits as i64;
            if v.unsigned_abs() < (1 << 53) {
                json!(v)
            } else {
                json!(v.to_string())
            }
        }
        ValueType::U64 | ValueType::Bits => {
            if bits < (1 << 53) {
                json!(bits)
            } else {
                json!(bits.to_string())
            }
        }
        ValueType::Enum => signal
            .variants
            .get(bits as usize)
            .map_or_else(|| json!(bits), |v| json!(v)),
        _ => Value::Null,
    }
}

/// The JSON value of a wide signal's bytes (ref/00 §4.1, ADR 0062): the
/// text of a `string`, padded standard base64 (RFC 4648 §4) of a `bytes`.
/// A `string` slot only ever holds valid UTF-8, so `null` there is a
/// corrupt slot, never a value.
pub fn wide_value(signal: &SignalPlan, bytes: &[u8]) -> Value {
    match signal.ty {
        ValueType::Bytes => json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
        ValueType::String => core::str::from_utf8(bytes).map_or(Value::Null, |s| json!(s)),
        _ => Value::Null,
    }
}

/// The canonical type name of a signal (ref/00 §6).
pub fn type_name(ty: ValueType) -> &'static str {
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

/// The lower-case name of a quality code.
pub fn quality_name(q: Quality) -> &'static str {
    match q.code() {
        QualityCode::Good => "good",
        QualityCode::Uncertain => "uncertain",
        QualityCode::Bad => "bad",
    }
}

/// The lower-case name of a quality reason (ref/00 §4.3).
pub fn reason_name(q: Quality) -> &'static str {
    q.reason().name().unwrap_or("none")
}

/// The JSON sample of a signal (ref/00 §4.2): ages it like the latch does
/// (docs/01 §3.3) at `now_mono_ns`, and translates `ts_acq` to UTC with
/// `utc_origin_ns` (the UTC time at monotonic zero).
pub fn sample(signal: &SignalPlan, s: &Sample, now_mono_ns: u64, utc_origin_ns: i64) -> Value {
    let q = graded(signal, s.quality(), s.seq(), s.ts_acq(), now_mono_ns);
    let shown = if q.is_bad() || s.seq() == 0 {
        Value::Null
    } else {
        value(signal, s.transport_bits())
    };
    sample_object(
        signal,
        shown,
        q,
        s.seq(),
        s.ts_acq(),
        now_mono_ns,
        utc_origin_ns,
    )
}

/// [`sample`]'s counterpart for a wide (`string`/`bytes`) signal, with the
/// same `max_age` grading and `null` for a bad or never written sample
/// (ADR 0062, decision 13).
pub fn wide_sample(
    signal: &SignalPlan,
    s: &WideSample,
    now_mono_ns: u64,
    utc_origin_ns: i64,
) -> Value {
    let q = graded(signal, s.quality(), s.seq(), s.ts_acq(), now_mono_ns);
    let shown = if q.is_bad() || s.seq() == 0 {
        Value::Null
    } else {
        wide_value(signal, s.as_slice())
    };
    sample_object(
        signal,
        shown,
        q,
        s.seq(),
        s.ts_acq(),
        now_mono_ns,
        utc_origin_ns,
    )
}

/// The quality a sample reads with at `now_mono_ns`: `uncertain(stale)`
/// past `max_age` and `bad(stale)` past twice it, as the latch ages it
/// (docs/01 §3.3).
fn graded(signal: &SignalPlan, q: Quality, seq: u64, ts_acq: u64, now_mono_ns: u64) -> Quality {
    let Some(max_age) = signal.max_age_ns else {
        return q;
    };
    if seq == 0 || q.is_bad() {
        return q;
    }
    let age = now_mono_ns.saturating_sub(ts_acq);
    if age > max_age.saturating_mul(2) {
        Quality::new(QualityCode::Bad, joker_core::QualityReason::Stale)
    } else if age > max_age && q.is_good() {
        Quality::new(QualityCode::Uncertain, joker_core::QualityReason::Stale)
    } else {
        q
    }
}

/// The JSON object of a sample (ref/00 §4.2) from its shown value.
fn sample_object(
    signal: &SignalPlan,
    shown: Value,
    q: Quality,
    seq: u64,
    ts_acq: u64,
    now_mono_ns: u64,
    utc_origin_ns: i64,
) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("path".into(), json!(signal.path));
    m.insert("value".into(), shown);
    if signal.unit != "1" {
        m.insert("unit".into(), json!(signal.unit));
    }
    m.insert("quality".into(), json!(quality_name(q)));
    m.insert("reason".into(), json!(reason_name(q)));
    m.insert(
        "ts_acq".into(),
        json!(timestamp(utc_origin_ns.saturating_add(ts_acq as i64))),
    );
    m.insert("seq".into(), json!(seq));
    m.insert(
        "age".into(),
        json!(duration(now_mono_ns.saturating_sub(ts_acq))),
    );
    Value::Object(m)
}

/// Why an input value does not fit an output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    /// The code (`J-E1036`, `J0107`, `J0108`).
    pub code: &'static str,
    /// What went wrong.
    pub detail: String,
}

/// Converts an input value (ref/09 §3.3) to the bits of an output: a number
/// in the signal's unit, a quantity literal (converted), a boolean, a
/// variant, or a decimal string for integers.
pub fn input_bits(signal: &SignalPlan, input: &Value) -> Result<u64, InputError> {
    let wrong = |detail: String| InputError {
        code: "J-E1036",
        detail,
    };
    match (signal.ty, input) {
        (ValueType::Bool, Value::Bool(b)) => Ok(u64::from(*b)),
        (ValueType::Bool, other) => Err(wrong(format!("`{other}` is not a bool"))),
        (ValueType::F64 | ValueType::F32, Value::Number(n)) => n
            .as_f64()
            .filter(|v| v.is_finite())
            .map(f64::to_bits)
            .ok_or_else(|| wrong(format!("`{n}` is not a finite number"))),
        (ValueType::F64 | ValueType::F32, Value::String(s)) => {
            let unit = Unit::parse(&signal.unit).map_err(|e| wrong(e.to_string()))?;
            match Literal::parse(s) {
                Ok(Literal::Quantity(q)) => {
                    q.value_in(&unit).map(f64::to_bits).map_err(|e| InputError {
                        code: e.code(),
                        detail: format!("`{s}` does not fit `{}`: {e}", signal.unit),
                    })
                }
                Ok(Literal::Number(n)) => Ok(n.to_bits()),
                Err(e) => Err(wrong(format!("`{s}`: {e}"))),
            }
        }
        (ValueType::I64, Value::Number(n)) => n
            .as_i64()
            .map(|v| v as u64)
            .ok_or_else(|| wrong(format!("`{n}` is not an i64"))),
        (ValueType::I64, Value::String(s)) => s
            .parse::<i64>()
            .map(|v| v as u64)
            .map_err(|_| wrong(format!("`{s}` is not an i64"))),
        (ValueType::U64 | ValueType::Bits, Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| wrong(format!("`{n}` is not a u64"))),
        (ValueType::U64 | ValueType::Bits, Value::String(s)) => s
            .parse::<u64>()
            .map_err(|_| wrong(format!("`{s}` is not a u64"))),
        (ValueType::Enum, Value::String(s)) => {
            let variant = s.rsplit('.').next().unwrap_or(s);
            signal
                .variants
                .iter()
                .position(|v| v == variant)
                .map(|i| i as u64)
                .ok_or_else(|| wrong(format!("no variant `{s}`")))
        }
        (_, other) => Err(wrong(format!(
            "`{other}` does not fit a {}",
            type_name(signal.ty)
        ))),
    }
}

/// Whether a path matches a selector (ref/01 §3.9).
pub fn matches(selector: &str, path: &str) -> bool {
    fn segment(p: &[u8], s: &[u8]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((b'*', rest)) => (0..=s.len()).any(|i| segment(rest, &s[i..])),
            Some((c, rest)) => s.first() == Some(c) && segment(rest, &s[1..]),
        }
    }
    fn go(sel: &[&str], segs: &[&str]) -> bool {
        match (sel.split_first(), segs.split_first()) {
            (None, None) => true,
            (Some((&"**", rest)), _) => (1..=segs.len()).any(|n| go(rest, &segs[n..])),
            (Some((p, rest)), Some((s, more))) => {
                segment(p.as_bytes(), s.as_bytes()) && go(rest, more)
            }
            _ => false,
        }
    }
    let sel: Vec<&str> = selector.split('.').collect();
    let segs: Vec<&str> = path.split('.').collect();
    go(&sel, &segs)
}

/// The signals of a plan that match any of `selectors` (all when empty),
/// in path order, with aliases resolved to their canonical signal.
pub fn select<'p>(plan: &'p Plan, selectors: &[String]) -> Vec<&'p SignalPlan> {
    let mut out: Vec<&SignalPlan> = Vec::new();
    for s in &plan.signals {
        let hit = selectors.is_empty()
            || selectors.iter().any(|sel| {
                matches(sel, &s.path)
                    || plan
                        .aliases
                        .iter()
                        .any(|(a, t)| *t == s.path && matches(sel, a))
            });
        if hit {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn selectors() {
        assert!(matches("inv*.ac.p", "inv1.ac.p"));
        assert!(matches("inv*.ac.p", "inv12.ac.p"));
        assert!(!matches("inv*.ac.p", "inv1.ac.q"));
        assert!(matches("meter.**", "meter.l1.v"));
        assert!(!matches("meter.**", "meter"));
        assert!(matches("**", "a.b.c"));
        assert!(matches("p1.acc.*", "p1.acc.x"));
        assert!(!matches("p1.acc.*", "p1.acc.x.y"));
    }

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000000000Z");
        assert_eq!(timestamp(1_500_000_000), "1970-01-01T00:00:01.500000000Z");
    }

    fn wide_signal(ty: ValueType, max_age_ns: Option<u64>) -> SignalPlan {
        SignalPlan {
            path: "sensor.temp".into(),
            ty,
            unit: "1".into(),
            direction: joker_core::registry::Direction::Input,
            group: None,
            max_age_ns,
            device: None,
            variants: Vec::new(),
            enum_name: None,
        }
    }

    #[test]
    fn wide_value_encodes_bytes_as_base64_and_string_as_plain_text() {
        let bytes_signal = wide_signal(ValueType::Bytes, None);
        assert_eq!(wide_value(&bytes_signal, &[0xAB, 0xCD]), json!("q80="));
        assert_eq!(wide_value(&bytes_signal, &[]), json!(""));

        let string_signal = wide_signal(ValueType::String, None);
        assert_eq!(wide_value(&string_signal, b"hello"), json!("hello"));
    }

    #[test]
    fn wide_sample_follows_the_same_shape_as_scalar_sample() {
        let signal = wide_signal(ValueType::Bytes, None);
        let slot = joker_core::WideSlot::new();
        slot.write(
            &[0xAB, 0xCD],
            Quality::GOOD,
            joker_core::value::NO_TS_SRC,
            5,
        );
        let s = slot.read(0).unwrap();
        let v = wide_sample(&signal, &s, 10, 0);
        assert_eq!(v["path"], json!("sensor.temp"));
        assert_eq!(v["value"], json!("q80="));
        assert_eq!(v["quality"], json!("good"));
        assert_eq!(v["seq"], json!(1));
    }

    #[test]
    fn a_wide_sample_never_written_or_bad_shows_null() {
        let signal = wide_signal(ValueType::String, None);
        let slot = joker_core::WideSlot::new();
        let v = wide_sample(&signal, &slot.read(0).unwrap(), 10, 0);
        assert_eq!(v["value"], Value::Null);

        let bad = Quality::new(QualityCode::Bad, joker_core::QualityReason::CommFailure);
        slot.write(&[], bad, joker_core::value::NO_TS_SRC, 5);
        let v = wide_sample(&signal, &slot.read(0).unwrap(), 10, 0);
        assert_eq!(v["value"], Value::Null);
        assert_eq!(v["reason"], json!("comm_failure"));
    }

    #[test]
    fn a_wide_sample_older_than_twice_max_age_is_bad_stale_with_no_value() {
        let signal = wide_signal(ValueType::String, Some(100));
        let slot = joker_core::WideSlot::new();
        slot.write(b"OK", Quality::GOOD, joker_core::value::NO_TS_SRC, 1_000);
        let s = slot.read(0).unwrap();

        let fresh = wide_sample(&signal, &s, 1_050, 0);
        assert_eq!(fresh["value"], json!("OK"));
        assert_eq!(fresh["quality"], json!("good"));

        let aging = wide_sample(&signal, &s, 1_150, 0);
        assert_eq!(aging["value"], json!("OK"));
        assert_eq!(aging["quality"], json!("uncertain"));
        assert_eq!(aging["reason"], json!("stale"));

        let stale = wide_sample(&signal, &s, 1_201, 0);
        assert_eq!(stale["value"], Value::Null);
        assert_eq!(stale["quality"], json!("bad"));
        assert_eq!(stale["reason"], json!("stale"));
    }
}
