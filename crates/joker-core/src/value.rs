//! Signal types, value bits and complete samples (ref/00 §4.1, §6).

use crate::quality::{Quality, QualityCode};

/// Signal type with the numeric codes of ref/00 §6.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ValueType {
    /// `bool`: bits 0 or 1.
    Bool = 0,
    /// `i64`: two's complement.
    I64 = 1,
    /// `u64`: as is.
    U64 = 2,
    /// `f64`: IEEE 754 bits.
    F64 = 3,
    /// `f32`: promoted to the IEEE 754 bits of an `f64`.
    F32 = 4,
    /// `enum<N>`: numeric value of the variant.
    Enum = 5,
    /// `bits<N>`: the word.
    Bits = 6,
    /// `string`: lives in the wide arena, not in the slot value.
    String = 7,
    /// `bytes`: lives in the wide arena, not in the slot value.
    Bytes = 8,
    /// `record` (`Internal<T>`): lives in a wide slot.
    Record = 9,
}

impl ValueType {
    /// Decodes a numeric signal type.
    pub const fn from_u8(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Bool,
            1 => Self::I64,
            2 => Self::U64,
            3 => Self::F64,
            4 => Self::F32,
            5 => Self::Enum,
            6 => Self::Bits,
            7 => Self::String,
            8 => Self::Bytes,
            9 => Self::Record,
            _ => return None,
        })
    }

    /// True for the types whose value is a real number (`f64`, `f32`).
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F64 | Self::F32)
    }

    /// True for the numeric types on which ranges and rates apply.
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::I64 | Self::U64 | Self::F64 | Self::F32)
    }

    /// Interprets value bits of this type as an `f64`, the arithmetic domain of
    /// the core (docs/16 §4). Returns `None` for non-numeric types.
    pub fn bits_to_f64(self, bits: u64) -> Option<f64> {
        match self {
            Self::F64 | Self::F32 => Some(f64::from_bits(bits)),
            Self::I64 => Some(bits as i64 as f64),
            Self::U64 => Some(bits as f64),
            _ => None,
        }
    }

    /// Encodes an `f64` as value bits of this type. Integers are rounded to the
    /// nearest value and saturate; `f32` is rounded to `f32` precision and
    /// promoted. Returns `None` for non-numeric types.
    pub fn f64_to_bits(self, value: f64) -> Option<u64> {
        match self {
            Self::F64 => Some(value.to_bits()),
            Self::F32 => Some(f64::from(value as f32).to_bits()),
            Self::I64 => Some(round_half_away(value) as i64 as u64),
            Self::U64 => Some(round_half_away(value) as u64),
            _ => None,
        }
    }
}

fn round_half_away(value: f64) -> f64 {
    let truncated = value as i128 as f64;
    let fraction = value - truncated;
    if fraction >= 0.5 {
        truncated + 1.0
    } else if fraction <= -0.5 {
        truncated - 1.0
    } else {
        truncated
    }
}

/// `ts_src` value meaning that the sample has no source timestamp (docs/01 §3.2).
pub const NO_TS_SRC: i64 = i64::MIN;

/// Error returned when a value is requested from a sample whose quality does
/// not allow it (docs/16 §3.1). It carries the full quality word.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct QualityError {
    /// The quality of the refused sample.
    pub quality: Quality,
}

/// A complete sample: value bits, quality, timestamps and the extended sequence
/// number (docs/02 §5).
///
/// The value is only reachable as a number through accessors that check the
/// quality (invariant 4): a `Bad` sample is never turned into a number.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    bits: u64,
    quality: Quality,
    ts_src: i64,
    ts_acq: u64,
    seq: u64,
}

impl Sample {
    /// The sample of a slot that never received a value: `uncertain(initial)`,
    /// value 0, sequence 0 (ref/04 §3.2).
    pub const INITIAL: Self = Self {
        bits: 0,
        quality: Quality::INITIAL,
        ts_src: NO_TS_SRC,
        ts_acq: 0,
        seq: 0,
    };

    /// Builds a sample from its parts.
    pub const fn new(bits: u64, quality: Quality, ts_src: i64, ts_acq: u64, seq: u64) -> Self {
        Self {
            bits,
            quality,
            ts_src,
            ts_acq,
            seq,
        }
    }

    /// The quality word.
    pub const fn quality(&self) -> Quality {
        self.quality
    }

    /// The source timestamp in ns, or `None` when absent.
    pub const fn ts_src(&self) -> Option<i64> {
        if self.ts_src == NO_TS_SRC {
            None
        } else {
            Some(self.ts_src)
        }
    }

    /// The raw `ts_src` field, with [`NO_TS_SRC`] when absent.
    pub const fn ts_src_raw(&self) -> i64 {
        self.ts_src
    }

    /// Monotonic acquisition instant in ns.
    pub const fn ts_acq(&self) -> u64 {
        self.ts_acq
    }

    /// The 64-bit sequence number.
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// Returns a copy with another sequence number.
    pub const fn with_seq(self, seq: u64) -> Self {
        Self { seq, ..self }
    }

    /// The value bits for transport (slot, process image, trace, link), whatever
    /// the quality. Never interpret them as a number without checking quality:
    /// use [`Sample::get`] and its siblings instead.
    pub const fn transport_bits(&self) -> u64 {
        self.bits
    }

    /// The value bits if the quality is `Good` (`get()` of docs/16 §3.1).
    pub const fn get(&self) -> Result<u64, QualityError> {
        match self.quality.code() {
            QualityCode::Good => Ok(self.bits),
            _ => Err(QualityError {
                quality: self.quality,
            }),
        }
    }

    /// The value bits if the quality is `Good` or `Uncertain`
    /// (`get_uncertain()` of docs/16 §3.1).
    pub const fn get_uncertain(&self) -> Result<u64, QualityError> {
        match self.quality.code() {
            QualityCode::Good | QualityCode::Uncertain => Ok(self.bits),
            QualityCode::Bad => Err(QualityError {
                quality: self.quality,
            }),
        }
    }

    /// The value bits if `Good`, otherwise `default` (`get_or()` of docs/16 §3.1).
    pub const fn get_or(&self, default: u64) -> u64 {
        match self.get() {
            Ok(bits) => bits,
            Err(_) => default,
        }
    }

    /// The value as an `f64` of type `ty` if the quality is `Good`.
    pub fn get_f64(&self, ty: ValueType) -> Result<Option<f64>, QualityError> {
        self.get().map(|bits| ty.bits_to_f64(bits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::QualityReason;

    #[test]
    fn bad_sample_never_yields_a_number() {
        let s = Sample::new(
            42.0f64.to_bits(),
            Quality::new(QualityCode::Bad, QualityReason::CommFailure),
            NO_TS_SRC,
            7,
            1,
        );
        assert!(s.get().is_err());
        assert!(s.get_uncertain().is_err());
        assert!(s.get_f64(ValueType::F64).is_err());
        assert_eq!(s.get_or(5), 5);
    }

    #[test]
    fn uncertain_sample_only_through_get_uncertain() {
        let s = Sample::new(3, Quality::INITIAL, NO_TS_SRC, 0, 0);
        assert!(s.get().is_err());
        assert_eq!(s.get_uncertain(), Ok(3));
    }

    #[test]
    fn numeric_conversions() {
        assert_eq!(ValueType::I64.bits_to_f64((-3i64) as u64), Some(-3.0));
        assert_eq!(ValueType::I64.f64_to_bits(-2.5), Some((-3i64) as u64));
        assert_eq!(ValueType::U64.f64_to_bits(2.5), Some(3));
        assert_eq!(
            ValueType::F32.f64_to_bits(0.1),
            Some(f64::from(0.1f32).to_bits())
        );
        assert_eq!(ValueType::Bool.bits_to_f64(1), None);
    }

    #[test]
    fn type_codes_round_trip() {
        for code in 0..10 {
            assert_eq!(ValueType::from_u8(code).map(|t| t as u8), Some(code));
        }
        assert_eq!(ValueType::from_u8(10), None);
    }
}
