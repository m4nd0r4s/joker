//! Literals of ref/03 §2.1.3 and ref/00 §3–§6: durations, instants,
//! intervals, quantities, qualities, write results, alarm states and
//! arbiter levels, with the display forms accepted as exact synonyms.
//!
//! The parsed predicates ([`QualityLit`], [`OutcomeLit`]) are the plain data
//! of `joker-case-runner`, which the conformance firmware embeds as is
//! (docs/13 §3).

use alloc::format;
use alloc::string::{String, ToString};

pub use joker_case_runner::model::{OutcomeLit, QualityLit};

/// A literal that does not follow its grammar (`J0806`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiteralError {
    /// The code of the problem (`J0806`, `J0109`, `J0107` or `J0108`).
    pub code: &'static str,
    /// What was wrong, in English.
    pub message: String,
}

impl LiteralError {
    fn malformed(kind: &str, text: &str) -> Self {
        Self {
            code: "J0806",
            message: format!("malformed {kind} literal {text:?}"),
        }
    }
}

fn digits(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    i
}

/// Splits `-1.5e3 kW` into the number and the unit after it.
fn split_number(text: &str) -> Option<(f64, &str)> {
    let text = text.trim();
    let bytes = text.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'-' | b'+')));
    let int_end = digits(bytes, i);
    let mut end = int_end;
    if bytes.get(end) == Some(&b'.') {
        end = digits(bytes, end + 1);
    }
    if end == i || (int_end == i && end == i + 1) {
        return None;
    }
    i = end;
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(bytes.get(j), Some(b'-' | b'+')) {
            j += 1;
        }
        let exp_end = digits(bytes, j);
        if exp_end > j {
            end = exp_end;
        }
    }
    let value = text.get(..end)?.parse::<f64>().ok()?;
    Some((value, text.get(end..)?.trim_start()))
}

/// Parses a duration (ref/00 §3.1) and rounds it to nanoseconds.
pub fn duration_ns(text: &str) -> Result<u64, LiteralError> {
    let err = || LiteralError::malformed("duration", text);
    let (value, unit) = split_number(text).ok_or_else(err)?;
    let scale = match unit {
        "ns" => 1.0,
        "us" | "µs" => 1e3,
        "ms" => 1e6,
        "s" => 1e9,
        "m" | "min" => 60e9,
        "h" => 3600e9,
        "d" => 86_400e9,
        _ => return Err(err()),
    };
    let ns = value * scale;
    if !ns.is_finite() || ns < 0.0 || ns > u64::MAX as f64 {
        return Err(err());
    }
    Ok(round_half_away(ns) as u64)
}

fn round_half_away(x: f64) -> f64 {
    let floor = x - x % 1.0;
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// Parses an instant of a timeline: `duration ("+" duration)*` (ref/03
/// §2.1.3). The `timeout` term belongs to driver suites and is refused here.
pub fn instant_ns(text: &str) -> Result<u64, LiteralError> {
    let mut total: u64 = 0;
    for term in text.split('+') {
        let ns = duration_ns(term).map_err(|_| LiteralError::malformed("instant", text))?;
        total = total
            .checked_add(ns)
            .ok_or_else(|| LiteralError::malformed("instant", text))?;
    }
    Ok(total)
}

/// Parses an interval `instant ".." instant`, closed at both bounds, with
/// `start ≤ end` (ref/03 §2.1.3).
pub fn interval_ns(text: &str) -> Result<(u64, u64), LiteralError> {
    let err = || LiteralError::malformed("interval", text);
    let (start, end) = text.split_once("..").ok_or_else(err)?;
    let start = instant_ns(start).map_err(|_| err())?;
    let end = instant_ns(end).map_err(|_| err())?;
    if start > end {
        return Err(err());
    }
    Ok((start, end))
}

/// The kind of physical quantity of a unit, for the dimension checks
/// (`J0107`, `J0108`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Kind {
    ActivePower,
    ReactivePower,
    ApparentPower,
    Energy,
    Frequency,
    Voltage,
    Current,
    Temperature,
    Percent,
    One,
    Time,
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct UnitDef {
    kind: Kind,
    scale: f64,
    offset: f64,
}

const PREFIXES: [(&str, f64); 6] = [
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("m", 1e-3),
    ("µ", 1e-6),
    ("u", 1e-6),
];

const BASES: [(&str, Kind); 9] = [
    ("W", Kind::ActivePower),
    ("var", Kind::ReactivePower),
    ("VA", Kind::ApparentPower),
    ("Wh", Kind::Energy),
    ("Hz", Kind::Frequency),
    ("V", Kind::Voltage),
    ("A", Kind::Current),
    ("K", Kind::Temperature),
    ("s", Kind::Time),
];

fn unit_def(unit: &str) -> Option<UnitDef> {
    let plain = |kind, scale| {
        Some(UnitDef {
            kind,
            scale,
            offset: 0.0,
        })
    };
    match unit {
        "" | "1" => return plain(Kind::One, 1.0),
        "%" => return plain(Kind::Percent, 1.0),
        "°C" => {
            return Some(UnitDef {
                kind: Kind::Temperature,
                scale: 1.0,
                offset: 273.15,
            });
        }
        "min" => return plain(Kind::Time, 60.0),
        "h" => return plain(Kind::Time, 3600.0),
        _ => {}
    }
    if let Some(&(_, kind)) = BASES.iter().find(|(b, _)| *b == unit) {
        return plain(kind, 1.0);
    }
    for (prefix, factor) in PREFIXES {
        if let Some(base) = unit.strip_prefix(prefix)
            && let Some(&(_, kind)) = BASES.iter().find(|(b, _)| *b == base)
            && kind != Kind::Temperature
        {
            return plain(kind, factor);
        }
    }
    None
}

fn split_rate(unit: &str) -> Option<(&str, f64)> {
    let (num, den) = unit.rsplit_once('/')?;
    let seconds = match den {
        "s" => 1.0,
        "min" => 60.0,
        "h" => 3600.0,
        _ => return None,
    };
    Some((num, seconds))
}

fn convert(value: f64, from: &str, to: &str, difference: bool) -> Result<f64, LiteralError> {
    let unknown = |u: &str| LiteralError {
        code: "J0109",
        message: format!("unit {u:?} is not in the catalog subset of this runner"),
    };
    let from_def = unit_def(from).ok_or_else(|| unknown(from))?;
    let to_def = unit_def(to).ok_or_else(|| unknown(to))?;
    if from_def.kind != to_def.kind {
        let power = [Kind::ActivePower, Kind::ReactivePower, Kind::ApparentPower];
        let code = if power.contains(&from_def.kind) && power.contains(&to_def.kind) {
            "J0108"
        } else {
            "J0107"
        };
        return Err(LiteralError {
            code,
            message: format!("unit {from:?} does not convert to {to:?}"),
        });
    }
    let base = if difference {
        value * from_def.scale
    } else {
        value * from_def.scale + from_def.offset
    };
    Ok(if difference {
        base / to_def.scale
    } else {
        (base - to_def.offset) / to_def.scale
    })
}

/// Parses a quantity literal (ref/00 §3.2) and converts it to `unit`, the
/// declared unit of the signal (ref/03 §2.1.3).
pub fn quantity_in(text: &str, unit: &str) -> Result<f64, LiteralError> {
    let (value, from) =
        split_number(text).ok_or_else(|| LiteralError::malformed("quantity", text))?;
    convert(value, from, unit, false)
}

/// Parses a difference quantity (a tolerance or a deadband): affine units
/// convert without their offset.
pub fn difference_in(text: &str, unit: &str) -> Result<f64, LiteralError> {
    let (value, from) =
        split_number(text).ok_or_else(|| LiteralError::malformed("quantity", text))?;
    convert(value, from, unit, true)
}

/// Parses a rate (`"10 kW/s"`) and returns it in `unit` per second.
pub fn rate_in(text: &str, unit: &str) -> Result<f64, LiteralError> {
    let (value, from) = split_number(text).ok_or_else(|| LiteralError::malformed("rate", text))?;
    let (num, seconds) = split_rate(from).ok_or_else(|| LiteralError::malformed("rate", text))?;
    Ok(convert(value, num, unit, true)? / seconds)
}

const QUALITY_CODES: [&str; 3] = ["good", "uncertain", "bad"];

/// The quality reasons of ref/00 §4.3, in the order of their numeric value.
pub const QUALITY_REASONS: [&str; 20] = [
    "none",
    "overridden",
    "local_override",
    "stale",
    "not_confirmed",
    "substituted",
    "out_of_distribution",
    "sensor_cal",
    "clock_unsynced",
    "initial",
    "frozen",
    "partial_write",
    "comm_failure",
    "device_fault",
    "out_of_range",
    "config_error",
    "not_connected",
    "nan",
    "tripped",
    "deadline",
];

/// The write results of ref/00 §4.4, in the order of their numeric value.
pub const OUTCOMES: [&str; 10] = [
    "none",
    "accepted",
    "clamped_range",
    "clamped_rate",
    "suppressed",
    "overridden",
    "deferred",
    "shadowed",
    "rejected",
    "tripped",
];

/// The reasons of `rejected` (ref/00 §4.4), from value 1.
pub const REJECT_REASONS: [&str; 8] = [
    "wrong_type",
    "interlock",
    "quality",
    "range",
    "atomic_group",
    "not_owner",
    "clock_unsynced",
    "budget",
];

/// The alarm states of ref/00 §6.
pub const ALARM_STATES: [&str; 5] = [
    "normal",
    "active_unacked",
    "active_acked",
    "rtn_unacked",
    "unknown",
];

fn pascal_to_snake(text: &str) -> Option<String> {
    let mut chars = text.chars();
    if !chars.next()?.is_ascii_uppercase() || !text.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn split_call(text: &str) -> Option<(&str, Option<&str>)> {
    match text.split_once('(') {
        None => Some((text, None)),
        Some((head, rest)) => {
            let arg = rest.strip_suffix(')')?;
            (!arg.contains('(') && !arg.contains(')')).then_some((head, Some(arg)))
        }
    }
}

/// Translates the display form of a `quality`, `outcome` or `state` literal
/// to the canonical one (ref/03 §2.1.3, "Accepted display forms"); a
/// canonical literal is returned unchanged.
pub fn canonical(text: &str) -> Option<String> {
    let (head, arg) = split_call(text)?;
    if !head.starts_with(|c: char| c.is_ascii_uppercase()) {
        return Some(text.to_string());
    }
    let head = pascal_to_snake(head)?;
    Some(match (head.as_str(), arg) {
        ("clamped", Some("Range")) => "clamped_range".to_string(),
        ("clamped", Some("Rate")) => "clamped_rate".to_string(),
        ("overridden", Some(level)) => format!("overridden({level})"),
        (_, None) => head,
        (_, Some(arg)) => format!("{head}({})", pascal_to_snake(arg)?),
    })
}

/// Parses a quality literal.
pub fn quality(text: &str) -> Result<QualityLit, LiteralError> {
    let err = || LiteralError::malformed("quality", text);
    let canonical = canonical(text).ok_or_else(err)?;
    let (head, arg) = split_call(&canonical).ok_or_else(err)?;
    let code = QUALITY_CODES
        .iter()
        .position(|c| *c == head)
        .ok_or_else(err)?;
    let reason = match arg {
        None => None,
        Some(r) => Some(
            QUALITY_REASONS
                .iter()
                .position(|q| *q == r)
                .ok_or_else(err)?,
        ),
    };
    Ok(QualityLit {
        code: code as u8,
        reason: reason.map(|r| r as u8),
    })
}

/// The canonical text of an outcome predicate (`"rejected(range)"`).
pub fn outcome_text(o: &OutcomeLit) -> String {
    let name = OUTCOMES
        .get(usize::from(o.code))
        .copied()
        .unwrap_or("unknown");
    match (name, o.detail) {
        (_, None) => name.to_string(),
        ("overridden", Some(level)) => format!("{name}({})", level_name(level)),
        (_, Some(d)) => match REJECT_REASONS.get(usize::from(d).wrapping_sub(1)) {
            Some(reason) => format!("{name}({reason})"),
            None => format!("{name}({d})"),
        },
    }
}

/// Parses a write result literal.
pub fn outcome(text: &str) -> Result<OutcomeLit, LiteralError> {
    let err = || LiteralError::malformed("write result", text);
    let canonical = canonical(text).ok_or_else(err)?;
    let (head, arg) = split_call(&canonical).ok_or_else(err)?;
    let code = OUTCOMES.iter().position(|o| *o == head).ok_or_else(err)?;
    let detail = match (head, arg) {
        (_, None) => None,
        ("rejected", Some(r)) => {
            let index = REJECT_REASONS
                .iter()
                .position(|x| *x == r)
                .ok_or_else(err)?;
            Some(index as u8 + 1)
        }
        ("overridden", Some(l)) => Some(level(l).map_err(|_| err())?.ok_or_else(err)?),
        _ => return Err(err()),
    };
    Ok(OutcomeLit {
        code: code as u8,
        detail,
    })
}

/// Parses an alarm state literal.
pub fn alarm_state(text: &str) -> Result<&'static str, LiteralError> {
    let err = || LiteralError::malformed("alarm state", text);
    let canonical = canonical(text).ok_or_else(err)?;
    ALARM_STATES
        .iter()
        .find(|s| **s == canonical)
        .copied()
        .ok_or_else(err)
}

const BANDS: [(&str, u8, u8); 6] = [
    ("safety", 1, 2),
    ("manual", 3, 5),
    ("supervisor", 6, 8),
    ("control", 9, 12),
    ("optimizer", 13, 15),
    ("schedule", 16, 16),
];

/// Parses an arbiter level `"band:n"` (ref/00 §5), or `"none"` as `None`.
pub fn level(text: &str) -> Result<Option<u8>, LiteralError> {
    let err = || LiteralError::malformed("arbiter level", text);
    if text == "none" {
        return Ok(None);
    }
    let (band, number) = text.split_once(':').ok_or_else(err)?;
    let number: u8 = number.parse().map_err(|_| err())?;
    let &(_, low, high) = BANDS.iter().find(|(b, _, _)| *b == band).ok_or_else(err)?;
    if (low..=high).contains(&number) {
        Ok(Some(number))
    } else {
        Err(err())
    }
}

/// The text form of an absolute level (`13` → `"optimizer:13"`).
pub fn level_name(level: u8) -> String {
    BANDS
        .iter()
        .find(|(_, low, high)| (*low..=*high).contains(&level))
        .map_or_else(
            || format!("level:{level}"),
            |(b, _, _)| format!("{b}:{level}"),
        )
}

/// The valid ids of cases and suites: `[a-z0-9_]+ ("." [a-z0-9_]+)*`,
/// at most 96 characters (ref/03 §2.1.2).
pub fn is_valid_id(id: &str) -> bool {
    id.len() <= 96
        && id.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// Formats nanoseconds as the shortest exact duration (`"2200ms"`).
pub fn format_ns(ns: u64) -> String {
    let units: [(u64, &str); 4] = [
        (1_000_000_000, "s"),
        (1_000_000, "ms"),
        (1_000, "us"),
        (1, "ns"),
    ];
    for (scale, unit) in units {
        if ns.is_multiple_of(scale) && (ns != 0 || unit == "ms") {
            return format!("{}{unit}", ns / scale);
        }
    }
    format!("{ns}ns")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_instants() {
        assert_eq!(duration_ns("500ms"), Ok(500_000_000));
        assert_eq!(duration_ns("1.5s"), Ok(1_500_000_000));
        assert_eq!(duration_ns("10m"), Ok(600_000_000_000));
        assert_eq!(duration_ns("2 h"), Ok(7_200_000_000_000));
        assert!(duration_ns("1h30m").is_err());
        assert!(duration_ns("-1s").is_err());
        assert!(duration_ns("ms").is_err());
        assert_eq!(instant_ns("2m+1.2s"), Ok(121_200_000_000));
        assert_eq!(instant_ns("100ms + 50ms"), Ok(150_000_000));
        assert!(instant_ns("100ms + timeout").is_err());
        assert_eq!(interval_ns("0ms .. 400ms"), Ok((0, 400_000_000)));
        assert!(interval_ns("5s .. 2s").is_err());
        assert_eq!(format_ns(2_200_000_000), "2200ms");
        assert_eq!(format_ns(0), "0ms");
        assert_eq!(format_ns(3_000_000_000), "3s");
    }

    #[test]
    fn quantities_convert_to_the_signal_unit() {
        assert_eq!(quantity_in("50 kW", "kW"), Ok(50.0));
        assert_eq!(quantity_in("1.5 MW", "kW"), Ok(1500.0));
        assert_eq!(quantity_in("-20kW", "W"), Ok(-20_000.0));
        assert_eq!(quantity_in("5 %", "%"), Ok(5.0));
        assert_eq!(quantity_in("1e3 W", "kW"), Ok(1.0));
        assert!((quantity_in("25 °C", "K").unwrap_or(0.0) - 298.15).abs() < 1e-9);
        assert_eq!(difference_in("5 K", "°C"), Ok(5.0));
        assert_eq!(rate_in("10 kW/s", "kW"), Ok(10.0));
        assert_eq!(rate_in("60 kW/min", "kW"), Ok(1.0));
        assert_eq!(quantity_in("1 Hz", "kW").map_err(|e| e.code), Err("J0107"));
        assert_eq!(
            quantity_in("1 kvar", "kW").map_err(|e| e.code),
            Err("J0108")
        );
        assert_eq!(
            quantity_in("1 furlong", "kW").map_err(|e| e.code),
            Err("J0109")
        );
        assert_eq!(quantity_in("kW", "kW").map_err(|e| e.code), Err("J0806"));
    }

    #[test]
    fn display_forms_are_exact_synonyms() {
        assert_eq!(
            canonical("Bad(CommFailure)").as_deref(),
            Some("bad(comm_failure)")
        );
        assert_eq!(canonical("Clamped(Rate)").as_deref(), Some("clamped_rate"));
        assert_eq!(
            canonical("Rejected(Interlock)").as_deref(),
            Some("rejected(interlock)")
        );
        assert_eq!(
            canonical("Overridden(manual:4)").as_deref(),
            Some("overridden(manual:4)")
        );
        assert_eq!(
            canonical("ActiveUnacked").as_deref(),
            Some("active_unacked")
        );
        assert_eq!(canonical("Unknown").as_deref(), Some("unknown"));
        assert_eq!(canonical("Clamped_Rate"), None);
        assert!(outcome("Clamped(Rate)").is_ok());
        assert!(outcome("CLAMPED_RATE").is_err());
        assert!(outcome("clamped(rate)").is_err());
        assert!(outcome("rejected(sideways)").is_err());
        assert_eq!(
            outcome("rejected").map(|o| o.detail),
            Ok(None),
            "bare rejected matches any reason"
        );
        assert_eq!(
            quality("uncertain(stale)"),
            Ok(QualityLit {
                code: 1,
                reason: Some(3)
            })
        );
        assert!(quality("Good").is_ok());
        assert!(quality("great").is_err());
        assert_eq!(alarm_state("RtnUnacked"), Ok("rtn_unacked"));
    }

    #[test]
    fn levels_and_ids() {
        assert_eq!(level("optimizer:13"), Ok(Some(13)));
        assert_eq!(level("none"), Ok(None));
        assert!(level("control:13").is_err());
        assert!(level("manual").is_err());
        assert_eq!(level_name(4), "manual:4");
        assert!(is_valid_id("envelope.rate.clamp_then_safe"));
        assert!(!is_valid_id("Envelope.rate"));
        assert!(!is_valid_id("envelope..rate"));
    }
}
