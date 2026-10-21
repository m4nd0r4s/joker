//! Literals of simulation models: numbers, booleans, quantities and
//! durations (ref/00 §3.1, ref/03 §2.4.5).
//!
//! M0 converts only the units the learning track's models use (ADR 0051):
//! a literal in the signal's own unit is taken as is, durations are seconds,
//! and `°C` and `K` share one scale, which is right for absolute values in
//! `°C` and differences in `K` (`45 °C + 30 K`, document 16 §4).

use std::string::String;

/// A value from a model, a parameter or the manifest.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    /// A bare number.
    Number(f64),
    /// A boolean.
    Bool(bool),
    /// A string: a quantity, a duration, a quality or a `$param` reference.
    Text(String),
}

/// A literal could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiteralError(pub String);

/// A number with its unit, as written.
#[derive(Clone, Debug, PartialEq)]
pub struct Quantity<'a> {
    /// The magnitude.
    pub value: f64,
    /// The unit, if any.
    pub unit: Option<&'a str>,
}

const DURATION_UNITS: [(&str, f64); 6] = [
    ("ns", 1e-9),
    ("us", 1e-6),
    ("µs", 1e-6),
    ("ms", 1e-3),
    ("s", 1.0),
    ("min", 60.0),
];

/// Units recognised right after a number in an expression.
pub const KNOWN_UNITS: [&str; 10] = ["ns", "us", "µs", "ms", "s", "min", "h", "°C", "K", "%"];

/// Seconds per unit of a duration unit.
pub fn duration_scale(unit: &str) -> Option<f64> {
    if unit == "h" {
        return Some(3600.0);
    }
    DURATION_UNITS
        .iter()
        .find(|(u, _)| *u == unit)
        .map(|(_, s)| *s)
}

/// Splits `"45 °C"`, `"60s"` or `"-3"` into magnitude and unit.
pub fn quantity(text: &str) -> Result<Quantity<'_>, LiteralError> {
    let t = text.trim();
    let end = t
        .char_indices()
        .find(|&(i, c)| {
            !(c.is_ascii_digit()
                || c == '.'
                || c == '_'
                || ((c == '-' || c == '+') && (i == 0 || t[..i].ends_with(['e', 'E'])))
                || ((c == 'e' || c == 'E') && i > 0))
        })
        .map_or(t.len(), |(i, _)| i);
    let (number, unit) = t.split_at(end);
    let number = number.replace('_', "");
    let value: f64 = number
        .parse()
        .map_err(|_| LiteralError(std::format!("`{text}` is not a quantity")))?;
    let unit = unit.trim();
    Ok(Quantity {
        value,
        unit: (!unit.is_empty() && unit != "1").then_some(unit),
    })
}

/// A duration literal (`"10ms"`, `"2 min"`) in ns.
pub fn duration_ns(text: &str) -> Result<u64, LiteralError> {
    let q = quantity(text)?;
    let scale = q
        .unit
        .and_then(|u| match u {
            "m" => Some(60.0),
            "d" => Some(86_400.0),
            other => duration_scale(other),
        })
        .ok_or_else(|| LiteralError(std::format!("`{text}` is not a duration")))?;
    let ns = q.value * scale * 1e9;
    if !ns.is_finite() || ns < 0.0 || ns > u64::MAX as f64 {
        return Err(LiteralError(std::format!("`{text}` is out of range")));
    }
    Ok(ns.round() as u64)
}

/// The magnitude of `q` in a signal whose unit is `target`.
pub fn convert(q: &Quantity<'_>, target: &str) -> Result<f64, LiteralError> {
    let Some(unit) = q.unit else {
        return Ok(q.value);
    };
    if unit == target {
        return Ok(q.value);
    }
    if let (Some(from), Some(to)) = (duration_scale(unit), duration_scale(target)) {
        return Ok(q.value * from / to);
    }
    if let Some(from) = duration_scale(unit)
        && target == "1"
    {
        return Ok(q.value * from);
    }
    if matches!((unit, target), ("K", "°C")) {
        return Ok(q.value);
    }
    Err(LiteralError(std::format!(
        "`{} {unit}` cannot be converted to `{target}` by the M0 simulator",
        q.value
    )))
}

impl Literal {
    /// The literal as a number in a signal of unit `target`; booleans are
    /// 0 and 1.
    pub fn number(&self, target: &str) -> Result<f64, LiteralError> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Bool(b) => Ok(f64::from(u8::from(*b))),
            Self::Text(t) => convert(&quantity(t)?, target),
        }
    }

    /// The literal as a duration in ns.
    pub fn duration_ns(&self) -> Result<u64, LiteralError> {
        match self {
            Self::Text(t) => duration_ns(t),
            Self::Number(n) if *n >= 0.0 => Ok((*n * 1e9).round() as u64),
            _ => Err(LiteralError(String::from("expected a duration"))),
        }
    }

    /// The literal as text.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantities_and_durations() {
        let q = quantity("45 °C").unwrap();
        assert_eq!((q.value, q.unit), (45.0, Some("°C")));
        assert_eq!(quantity("-3").unwrap().unit, None);
        assert_eq!(quantity("1e3 ms").unwrap().value, 1000.0);
        assert_eq!(quantity("60s").unwrap().unit, Some("s"));
        assert_eq!(duration_ns("10ms").unwrap(), 10_000_000);
        assert_eq!(duration_ns("2 min").unwrap(), 120_000_000_000);
        assert_eq!(duration_ns("1h").unwrap(), 3_600_000_000_000);
        assert_eq!(duration_ns("10m").unwrap(), 600_000_000_000);
        assert_eq!(duration_ns("1d").unwrap(), 86_400_000_000_000);
        assert!(duration_ns("10 °C").is_err());
        assert!(quantity("abc").is_err());
    }

    #[test]
    fn m0_conversions() {
        assert_eq!(Literal::Text("30 K".into()).number("°C").unwrap(), 30.0);
        assert_eq!(Literal::Text("500 ms".into()).number("s").unwrap(), 0.5);
        assert_eq!(Literal::Text("60s".into()).number("1").unwrap(), 60.0);
        assert_eq!(Literal::Text("3 kW".into()).number("kW").unwrap(), 3.0);
        assert!(Literal::Text("3 kW".into()).number("W").is_err());
        assert!(Literal::Text("45 °C".into()).number("K").is_err());
        assert_eq!(Literal::Bool(true).number("1").unwrap(), 1.0);
        assert_eq!(Literal::Number(0.25).duration_ns().unwrap(), 250_000_000);
    }
}
