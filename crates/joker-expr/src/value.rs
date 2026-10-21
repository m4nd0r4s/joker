use crate::quality::{Quality, QualityCode};
use crate::types::SignalType;

/// A value on the VM stack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// A boolean.
    Bool(bool),
    /// A float.
    F64(f64),
    /// A signed integer, kept exact for comparisons.
    I64(i64),
    /// An unsigned integer (also enum values and bit words), kept exact for comparisons.
    U64(u64),
}

impl Value {
    /// Decodes the `u64` of a slot (ref/00 §4.1) for a signal type.
    pub fn decode(raw: u64, ty: SignalType) -> Value {
        match ty {
            SignalType::Bool => Value::Bool(raw != 0),
            SignalType::I64 => Value::I64(raw as i64),
            SignalType::F64 | SignalType::F32 => Value::F64(f64::from_bits(raw)),
            SignalType::U64
            | SignalType::Bits
            | SignalType::Enum(_)
            | SignalType::String
            | SignalType::Bytes
            | SignalType::Record => Value::U64(raw),
        }
    }

    /// The value as `f64` (`true` is 1).
    pub fn as_f64(self) -> f64 {
        match self {
            Value::Bool(b) => f64::from(u8::from(b)),
            Value::F64(v) => v,
            Value::I64(v) => v as f64,
            Value::U64(v) => v as f64,
        }
    }

    /// The value as a boolean (non-zero is `true`).
    pub fn as_bool(self) -> bool {
        match self {
            Value::Bool(b) => b,
            Value::F64(v) => v != 0.0,
            Value::I64(v) => v != 0,
            Value::U64(v) => v != 0,
        }
    }

    /// The value as a 64-bit word, for `bit`.
    pub fn as_word(self) -> u64 {
        match self {
            Value::Bool(b) => u64::from(b),
            Value::F64(v) => v as u64,
            Value::I64(v) => v as u64,
            Value::U64(v) => v,
        }
    }

    /// The `u64` slot encoding of the value (ref/00 §4.1).
    pub fn to_raw(self) -> u64 {
        match self {
            Value::Bool(b) => u64::from(b),
            Value::F64(v) => v.to_bits(),
            Value::I64(v) => v as u64,
            Value::U64(v) => v,
        }
    }

    pub(crate) fn is_finite(self) -> bool {
        match self {
            Value::F64(v) => v.is_finite(),
            _ => true,
        }
    }
}

/// The value is not usable at the requested quality; carries the quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityError(pub Quality);

impl core::fmt::Display for QualityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "value has quality {}", self.0)
    }
}

impl core::error::Error for QualityError {}

/// The result of an evaluation: a value and its quality.
///
/// The value of a `Bad` result is never handed out as a number (AGENTS.md
/// invariant 4): [`Output::get`] and [`Output::get_uncertain`] return
/// `Err` for it, as the SDK accessors of docs/16 §3.1 do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Output {
    value: Value,
    quality: Quality,
}

impl Output {
    pub(crate) fn new(value: Value, quality: Quality) -> Output {
        Output { value, quality }
    }

    /// The quality.
    pub fn quality(&self) -> Quality {
        self.quality
    }

    /// The value if `Good`.
    pub fn get(&self) -> Result<Value, QualityError> {
        match self.quality.code {
            QualityCode::Good => Ok(self.value),
            _ => Err(QualityError(self.quality)),
        }
    }

    /// The value if `Good` or `Uncertain`.
    pub fn get_uncertain(&self) -> Result<Value, QualityError> {
        match self.quality.code {
            QualityCode::Bad => Err(QualityError(self.quality)),
            _ => Ok(self.value),
        }
    }

    /// The value if `Good`, else `default`.
    pub fn get_or(&self, default: Value) -> Value {
        self.get().unwrap_or(default)
    }

    /// Whether a permissive passes: `true` and `Good`, or `Uncertain` when
    /// `permit_uncertain` is set (docs/16 §3.2).
    pub fn permits(&self, permit_uncertain: bool) -> bool {
        let usable = match self.quality.code {
            QualityCode::Good => true,
            QualityCode::Uncertain => permit_uncertain,
            QualityCode::Bad => false,
        };
        usable && self.value.as_bool()
    }
}
