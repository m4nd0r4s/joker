//! Literals of simulation models: numbers, booleans, quantities and
//! durations (ref/00 §3.1–§3.2, ref/03 §2.4.5), read with `joker-units`.
//!
//! A bare number is in the unit of the signal; a quantity is converted to
//! it. Amplitudes, deviations, steps and offsets are differences, so they
//! convert to the signal's difference unit (`"3 K"` on a `°C` signal is
//! 3, not −270.15; document 16 §4).

use std::string::String;

use joker_hal::types::Code;
use joker_units::{Duration, Literal as UnitLiteral, Unit};

use crate::codes::{J0806, of_units};

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
pub struct LiteralError {
    /// Registry code.
    pub code: Code,
    /// For humans.
    pub message: String,
}

impl LiteralError {
    fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Parses a signal unit; an absent unit is `1`.
pub fn unit(text: &str) -> Result<Unit, LiteralError> {
    Unit::parse(text).map_err(|e| {
        LiteralError::new(of_units(&e), std::format!("`{text}`: {}", e.kind.message()))
    })
}

/// A duration literal (`"10ms"`, `"2 min"`, `"10m"`) in ns.
pub fn duration_ns(text: &str) -> Result<u64, LiteralError> {
    Duration::parse(text.trim())
        .map(Duration::as_nanos)
        .map_err(|e| LiteralError::new(J0806, std::format!("`{text}`: {}", e.kind.message())))
}

/// A number and the unit it was written in, if any.
pub fn quantity(text: &str) -> Result<(f64, Option<Unit>), LiteralError> {
    match UnitLiteral::parse(text.trim()) {
        Ok(UnitLiteral::Number(v)) => Ok((v, None)),
        Ok(UnitLiteral::Quantity(q)) => Ok((q.value, Some(q.unit))),
        Err(e) => Err(LiteralError::new(
            of_units(&e),
            std::format!("`{text}`: {}", e.kind.message()),
        )),
    }
}

fn convert(value: f64, from: Option<Unit>, to: &Unit) -> Result<f64, LiteralError> {
    match from {
        None => Ok(value),
        Some(u) => u.convert(value, to).map_err(|e| {
            LiteralError::new(
                of_units(&e),
                std::format!("`{value} {u}` cannot be converted to `{to}`"),
            )
        }),
    }
}

impl Literal {
    /// The literal as an absolute value in a signal of unit `target`;
    /// booleans are 0 and 1.
    pub fn number(&self, target: &Unit) -> Result<f64, LiteralError> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Bool(b) => Ok(f64::from(u8::from(*b))),
            Self::Text(t) => {
                let (v, u) = quantity(t)?;
                convert(v, u, target)
            }
        }
    }

    /// The literal as a difference in a signal of unit `target`: `K` on a
    /// `°C` signal.
    pub fn difference(&self, target: &Unit) -> Result<f64, LiteralError> {
        let diff = target.difference();
        match self {
            Self::Text(t) => {
                let (v, u) = quantity(t)?;
                convert(v, u.map(|u| u.difference()), &diff)
            }
            other => other.number(&diff),
        }
    }

    /// The literal as a duration in ns.
    pub fn duration_ns(&self) -> Result<u64, LiteralError> {
        match self {
            Self::Text(t) => duration_ns(t),
            _ => Err(LiteralError::new(
                J0806,
                "expected a duration such as \"10ms\"",
            )),
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

    fn u(s: &str) -> Unit {
        unit(s).unwrap()
    }

    #[test]
    fn quantities_and_durations() {
        let (v, q) = quantity("45 °C").unwrap();
        assert_eq!(
            (v, q.map(|u| u.symbol().to_owned())),
            (45.0, Some("°C".into()))
        );
        assert_eq!(quantity("-3").unwrap(), (-3.0, None));
        assert_eq!(duration_ns("10ms").unwrap(), 10_000_000);
        assert_eq!(duration_ns("2 min").unwrap(), 120_000_000_000);
        assert_eq!(duration_ns("1h").unwrap(), 3_600_000_000_000);
        assert_eq!(duration_ns("10m").unwrap(), 600_000_000_000);
        assert_eq!(duration_ns("1d").unwrap(), 86_400_000_000_000);
        assert_eq!(duration_ns("10 °C").unwrap_err().code, J0806);
        assert!(quantity("abc").is_err());
    }

    #[test]
    fn conversions_follow_the_catalogue() {
        let t = |s: &str| Literal::Text(s.into());
        assert_eq!(t("3 K").difference(&u("°C")).unwrap(), 3.0);
        assert!((t("300 K").number(&u("°C")).unwrap() - 26.85).abs() < 1e-9);
        assert_eq!(t("22 °C").number(&u("°C")).unwrap(), 22.0);
        assert_eq!(t("500 ms").number(&u("s")).unwrap(), 0.5);
        assert_eq!(t("3 kW").number(&u("W")).unwrap(), 3000.0);
        assert!((t("1 m3/h").number(&u("l/min")).unwrap() - 1000.0 / 60.0).abs() < 1e-9);
        assert_eq!(
            t("3 kW").number(&u("var")).unwrap_err().code.as_str(),
            "J0108"
        );
        assert_eq!(
            t("3 kW").number(&u("m")).unwrap_err().code.as_str(),
            "J0107"
        );
        assert_eq!(t("5 %").number(&u("1")).unwrap_err().code.as_str(), "J0107");
        assert_eq!(Literal::Bool(true).number(&Unit::ONE).unwrap(), 1.0);
        assert_eq!(Literal::Number(0.25).duration_ns().unwrap_err().code, J0806);
    }
}
