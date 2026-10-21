use core::fmt;

/// One axis of the dimension vector.
///
/// The seven SI base dimensions plus `Percent`, which keeps `%` apart from
/// the plain dimensionless `1` (docs/02 §3, docs/16 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BaseDimension {
    /// Time, base unit `s`.
    Time,
    /// Length, base unit `m`.
    Length,
    /// Mass, base unit `kg` (the catalogue names `g`).
    Mass,
    /// Electric current, base unit `A`.
    Current,
    /// Thermodynamic temperature, base unit `K`.
    Temperature,
    /// Amount of substance, base unit `mol`.
    Amount,
    /// Luminous intensity, base unit `cd`.
    Luminosity,
    /// Percent, the `%` axis that never mixes with `1`.
    Percent,
}

impl BaseDimension {
    /// Every axis, in vector order.
    pub const ALL: [BaseDimension; 8] = [
        BaseDimension::Time,
        BaseDimension::Length,
        BaseDimension::Mass,
        BaseDimension::Current,
        BaseDimension::Temperature,
        BaseDimension::Amount,
        BaseDimension::Luminosity,
        BaseDimension::Percent,
    ];

    /// The symbol used when printing a dimension vector.
    pub const fn symbol(self) -> &'static str {
        match self {
            BaseDimension::Time => "T",
            BaseDimension::Length => "L",
            BaseDimension::Mass => "M",
            BaseDimension::Current => "I",
            BaseDimension::Temperature => "Θ",
            BaseDimension::Amount => "N",
            BaseDimension::Luminosity => "J",
            BaseDimension::Percent => "%",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// A dimension: one integer exponent per [`BaseDimension`].
///
/// Two units can be converted into each other exactly when their dimensions
/// are equal.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Dimension {
    exponents: [i8; 8],
}

impl Dimension {
    /// The dimensionless dimension of `1`.
    pub const NONE: Dimension = Dimension { exponents: [0; 8] };

    /// Builds a dimension from its exponents, in [`BaseDimension::ALL`] order.
    pub const fn from_exponents(exponents: [i8; 8]) -> Dimension {
        Dimension { exponents }
    }

    /// The dimension of a single base axis raised to the first power.
    pub const fn base(axis: BaseDimension) -> Dimension {
        let mut exponents = [0; 8];
        exponents[axis.index()] = 1;
        Dimension { exponents }
    }

    /// The exponents, in [`BaseDimension::ALL`] order.
    pub const fn exponents(self) -> [i8; 8] {
        self.exponents
    }

    /// The exponent of one axis.
    pub const fn exponent(self, axis: BaseDimension) -> i8 {
        self.exponents[axis.index()]
    }

    /// True for the dimension of `1` (and not for `%`).
    pub fn is_dimensionless(self) -> bool {
        self == Dimension::NONE
    }

    /// The product of two dimensions, or `None` if an exponent overflows.
    pub fn checked_mul(self, other: Dimension) -> Option<Dimension> {
        let mut exponents = [0i8; 8];
        for (i, slot) in exponents.iter_mut().enumerate() {
            *slot = self.exponents[i].checked_add(other.exponents[i])?;
        }
        Some(Dimension { exponents })
    }

    /// The quotient of two dimensions, or `None` if an exponent overflows.
    pub fn checked_div(self, other: Dimension) -> Option<Dimension> {
        self.checked_mul(other.checked_recip()?)
    }

    /// The reciprocal dimension, or `None` if an exponent overflows.
    pub fn checked_recip(self) -> Option<Dimension> {
        let mut exponents = [0i8; 8];
        for (i, slot) in exponents.iter_mut().enumerate() {
            *slot = self.exponents[i].checked_neg()?;
        }
        Some(Dimension { exponents })
    }

    /// The dimension raised to an integer power, or `None` on overflow.
    pub fn checked_pow(self, n: i8) -> Option<Dimension> {
        let mut exponents = [0i8; 8];
        for (i, slot) in exponents.iter_mut().enumerate() {
            *slot = self.exponents[i].checked_mul(n)?;
        }
        Some(Dimension { exponents })
    }
}

impl fmt::Debug for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Dimension({self})")
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_dimensionless() {
            return f.write_str("1");
        }
        let mut first = true;
        for axis in BaseDimension::ALL {
            let e = self.exponent(axis);
            if e == 0 {
                continue;
            }
            if !first {
                f.write_str("·")?;
            }
            first = false;
            f.write_str(axis.symbol())?;
            if e != 1 {
                write!(f, "^{e}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        let l = Dimension::base(BaseDimension::Length);
        let t = Dimension::base(BaseDimension::Time);
        let v = l.checked_div(t).unwrap();
        assert_eq!(v.exponent(BaseDimension::Length), 1);
        assert_eq!(v.exponent(BaseDimension::Time), -1);
        assert_eq!(v.checked_mul(t).unwrap(), l);
        assert_eq!(l.checked_pow(3).unwrap().exponent(BaseDimension::Length), 3);
        assert!(Dimension::NONE.is_dimensionless());
        assert!(!Dimension::base(BaseDimension::Percent).is_dimensionless());
    }

    #[test]
    fn overflow_is_reported() {
        let big = Dimension::from_exponents([120, 0, 0, 0, 0, 0, 0, 0]);
        assert!(big.checked_mul(big).is_none());
        assert!(big.checked_pow(2).is_none());
        let min = Dimension::from_exponents([i8::MIN, 0, 0, 0, 0, 0, 0, 0]);
        assert!(min.checked_recip().is_none());
    }

    #[test]
    fn display() {
        extern crate std;
        use std::string::ToString;
        let w = Dimension::from_exponents([-3, 2, 1, 0, 0, 0, 0, 0]);
        assert_eq!(w.to_string(), "T^-3·L^2·M");
        assert_eq!(Dimension::NONE.to_string(), "1");
    }
}
