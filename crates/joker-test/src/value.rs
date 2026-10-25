//! The values the harness reads and writes, untyped so that a test needs
//! no annotation (ref/04 §6.4).

use joker_core::ValueType;

/// A scalar value of a port.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// `bool`.
    Bool(bool),
    /// `i64`, enums and bit sets.
    I64(i64),
    /// `u64`.
    U64(u64),
    /// `f64` and `f32`.
    F64(f64),
}

impl Value {
    /// The value as the bits of a sample of type `ty`.
    pub fn to_bits(self, ty: ValueType) -> u64 {
        match ty {
            ValueType::Bool => u64::from(self.as_f64() != 0.0),
            ValueType::F64 => self.as_f64().to_bits(),
            ValueType::F32 => u64::from((self.as_f64() as f32).to_bits()),
            ValueType::U64 => match self {
                Self::U64(u) => u,
                Self::I64(i) => i as u64,
                Self::Bool(b) => u64::from(b),
                Self::F64(f) => f as u64,
            },
            _ => match self {
                Self::I64(i) => i as u64,
                Self::U64(u) => u,
                Self::Bool(b) => u64::from(b),
                Self::F64(f) => (f as i64) as u64,
            },
        }
    }

    /// The value of the bits of a sample of type `ty`.
    pub fn from_bits(bits: u64, ty: ValueType) -> Self {
        match ty {
            ValueType::Bool => Self::Bool(bits != 0),
            ValueType::F64 => Self::F64(f64::from_bits(bits)),
            ValueType::F32 => Self::F64(f64::from(f32::from_bits(bits as u32))),
            ValueType::U64 => Self::U64(bits),
            _ => Self::I64(bits as i64),
        }
    }

    /// The value as a number (`true` is 1).
    pub fn as_f64(self) -> f64 {
        match self {
            Self::Bool(b) => f64::from(u8::from(b)),
            Self::I64(i) => i as f64,
            Self::U64(u) => u as f64,
            Self::F64(f) => f,
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::I64(i64::from(v))
    }
}

impl PartialEq<bool> for Value {
    fn eq(&self, other: &bool) -> bool {
        matches!(self, Self::Bool(b) if b == other)
    }
}

impl PartialEq<f64> for Value {
    fn eq(&self, other: &f64) -> bool {
        !matches!(self, Self::Bool(_)) && self.as_f64() == *other
    }
}

impl PartialOrd<f64> for Value {
    fn partial_cmp(&self, other: &f64) -> Option<core::cmp::Ordering> {
        if matches!(self, Self::Bool(_)) {
            return None;
        }
        self.as_f64().partial_cmp(other)
    }
}

impl PartialEq<i64> for Value {
    fn eq(&self, other: &i64) -> bool {
        match self {
            Self::I64(i) => i == other,
            Self::U64(u) => i64::try_from(*u).is_ok_and(|u| u == *other),
            Self::F64(f) => *f == *other as f64,
            Self::Bool(_) => false,
        }
    }
}

impl PartialEq<u64> for Value {
    fn eq(&self, other: &u64) -> bool {
        match self {
            Self::U64(u) => u == other,
            Self::I64(i) => u64::try_from(*i).is_ok_and(|i| i == *other),
            Self::F64(f) => *f == *other as f64,
            Self::Bool(_) => false,
        }
    }
}
