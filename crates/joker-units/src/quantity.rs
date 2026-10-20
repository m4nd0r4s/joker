use core::fmt;
use core::str::FromStr;

use crate::error::{Error, ErrorKind};
use crate::number;
use crate::unit::Unit;

/// A number with a unit, as written in a quantity literal (ref/00 §3.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    /// The number, in [`Quantity::unit`].
    pub value: f64,
    /// The unit the number is expressed in.
    pub unit: Unit,
}

/// A literal that may or may not carry a unit.
///
/// A bare number is only valid where the expected unit is dimensionless or
/// the signal has none; the caller decides and reports `J0151` otherwise
/// (ref/01 §3.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Literal {
    /// A number without a unit.
    Number(f64),
    /// A number with a unit.
    Quantity(Quantity),
}

impl Literal {
    /// Parses `"-"? number exponent? " "? unit?`.
    pub fn parse(text: &str) -> Result<Literal, Error> {
        if text.is_empty() {
            return Err(Error::at(ErrorKind::Empty, 0, 0));
        }
        let scanned = number::scan(text, true, true).ok_or(Error::at(
            ErrorKind::InvalidNumber,
            0,
            text.len(),
        ))?;
        let digits = text.get(..scanned.end).unwrap_or("");
        let value = digits
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or(Error::at(ErrorKind::InvalidNumber, 0, scanned.end))?;
        let mut unit_start = scanned.end;
        if text.get(unit_start..unit_start + 1) == Some(" ") {
            unit_start += 1;
        }
        let unit_text = text.get(unit_start..).unwrap_or("");
        if unit_text.is_empty() {
            if unit_start != scanned.end {
                return Err(Error::at(
                    ErrorKind::InvalidUnitSyntax,
                    scanned.end,
                    text.len(),
                ));
            }
            return Ok(Literal::Number(value));
        }
        if unit_text.starts_with(' ') {
            return Err(Error::at(
                ErrorKind::InvalidUnitSyntax,
                unit_start,
                text.len(),
            ));
        }
        let unit = Unit::parse(unit_text).map_err(|e| Error {
            span: e
                .span
                .shifted(u32::try_from(unit_start).unwrap_or(u32::MAX)),
            ..e
        })?;
        Ok(Literal::Quantity(Quantity { value, unit }))
    }
}

impl Quantity {
    /// Builds a quantity.
    pub const fn new(value: f64, unit: Unit) -> Quantity {
        Quantity { value, unit }
    }

    /// Parses a quantity literal that must carry a unit (`"125 kW"`,
    /// `"-20 kW/s"`, `"5 %"`). A bare number is [`ErrorKind::MissingUnit`].
    pub fn parse(text: &str) -> Result<Quantity, Error> {
        match Literal::parse(text)? {
            Literal::Quantity(q) => Ok(q),
            Literal::Number(_) => Err(Error::at(ErrorKind::MissingUnit, 0, text.len())),
        }
    }

    /// The value converted to `unit` (`J0107` or `J0108` if incompatible).
    pub fn value_in(&self, unit: &Unit) -> Result<f64, Error> {
        self.unit.convert(self.value, unit)
    }

    /// The same quantity expressed in `unit`.
    pub fn to_unit(&self, unit: &Unit) -> Result<Quantity, Error> {
        Ok(Quantity {
            value: self.value_in(unit)?,
            unit: *unit,
        })
    }
}

impl FromStr for Quantity {
    type Err = Error;

    fn from_str(s: &str) -> Result<Quantity, Error> {
        Quantity::parse(s)
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.value, self.unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> Quantity {
        Quantity::parse(s).unwrap()
    }

    #[test]
    fn literals() {
        assert_eq!(q("125 kW").value, 125.0);
        assert_eq!(q("125kW").unit, Unit::parse("kW").unwrap());
        assert_eq!(q("-20 kW/s").value, -20.0);
        assert_eq!(q("49.8 Hz").value, 49.8);
        assert_eq!(q("5 %").unit, Unit::PERCENT);
        assert_eq!(q("1.5e3 W").value, 1500.0);
        assert_eq!(q("20 °C").unit.offset(), 273.15);
        assert_eq!(q("0 kW").value, 0.0);
    }

    #[test]
    fn bare_numbers() {
        assert_eq!(Literal::parse("0.5"), Ok(Literal::Number(0.5)));
        let e = Quantity::parse("200").unwrap_err();
        assert_eq!(e.kind, ErrorKind::MissingUnit);
        assert_eq!(e.code(), "J0151");
    }

    #[test]
    fn malformed() {
        for s in [
            "", "kW", "1.kW", ".5 kW", "1e kW", "1  kW", "1 ", "+1 kW", "--1 kW", "1e999 kW",
        ] {
            assert!(Quantity::parse(s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn unit_errors_are_shifted() {
        let e = Quantity::parse("12 kWW").unwrap_err();
        assert_eq!(e.kind, ErrorKind::UnknownUnit);
        assert_eq!(e.span.range(), 3..6);
    }

    #[test]
    fn conversion() {
        let p = q("125000 W");
        assert_eq!(p.value_in(&Unit::parse("kW").unwrap()), Ok(125.0));
        assert_eq!(
            p.value_in(&Unit::parse("kvar").unwrap())
                .unwrap_err()
                .code(),
            "J0108"
        );
        let t = q("25 °C").to_unit(&Unit::KELVIN).unwrap();
        assert!((t.value - 298.15).abs() < 1e-9);
    }
}
