//! The output stage's conversion of an applied value to the raw value of
//! its point (ref/06 §2.7, ADR 0075): `invert` on a `bool`, the raw
//! integer of an `enum` variant, `(v − offset) / scale` rounded to the
//! nearest integer with ties to even, the range check of the raw type, and
//! the encoding into registers (`word_order`, `byte_order`) when the point
//! is a register point.
//!
//! The converted write is built here, from a [`CommittedWrite`] that came
//! out of the commit channel, so a write still has only two sources: a
//! value applied by the envelope, or the declared `safe` (invariant 3).

use crate::driver::CommittedWrite;
use crate::types::RawValue;

/// The raw type of a point (ref/03 §2.2.7), as the output stage writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawNumber {
    /// `bool`.
    Bool,
    /// A two's complement integer of this many bits.
    Signed(u8),
    /// An unsigned integer of this many bits.
    Unsigned(u8),
    /// `f32`.
    F32,
    /// `f64`.
    F64,
}

/// How a register point lays out its value (ref/06 §2.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegLayout {
    /// Registers, 1 to 4.
    pub words: u8,
    /// `word_order = "little"`: the least significant word first.
    pub words_little: bool,
    /// `byte_order = "little"`: the bytes of each register swapped.
    pub bytes_little: bool,
}

/// The conversion of one output point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputCodec<'a> {
    /// `invert` of a `bool`.
    pub invert: bool,
    /// The raw integer of each `enum` variant, by position; empty for any
    /// other output.
    pub enum_raw: &'a [i64],
    /// The engineering value is `raw × scale + offset` (ref/03 §3).
    pub scale: f64,
    /// See `scale`.
    pub offset: f64,
    /// The raw type.
    pub raw: RawNumber,
    /// The registers of a register point; `None` writes a typed scalar.
    pub regs: Option<RegLayout>,
}

/// Why an applied value cannot be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// The value does not fit the raw type (`Rejected(Range)`, ref/06 §2.7).
    NotRepresentable,
    /// The value's form does not fit this conversion (a configuration
    /// error the plan should have refused).
    Unsupported,
}

/// Rounds to the nearest integer, ties to even, the same on every
/// architecture (ref/06 §2.7).
pub fn round_ties_even(x: f64) -> f64 {
    const EXACT: f64 = 4_503_599_627_370_496.0;
    if x.is_nan() || x >= EXACT || x <= -EXACT {
        return x;
    }
    let t = x as i64;
    let frac = x - t as f64;
    let odd = t % 2 != 0;
    let r = if frac > 0.5 || (frac == 0.5 && odd) {
        t + 1
    } else if frac < -0.5 || (frac == -0.5 && odd) {
        t - 1
    } else {
        t
    };
    r as f64
}

impl RawNumber {
    fn integer_bounds(self) -> Option<(i128, i128)> {
        match self {
            Self::Signed(bits) if (1..=64).contains(&bits) => {
                let half = 1i128 << (bits - 1);
                Some((-half, half - 1))
            }
            Self::Unsigned(bits) if (1..=64).contains(&bits) => Some((0, (1i128 << bits) - 1)),
            _ => None,
        }
    }

    fn bits(self) -> u32 {
        match self {
            Self::Bool => 1,
            Self::Signed(b) | Self::Unsigned(b) => u32::from(b),
            Self::F32 => 32,
            Self::F64 => 64,
        }
    }
}

impl OutputCodec<'_> {
    /// The engineering values every raw value of the type can carry, the
    /// lower bound first; `None` when the type does not bound them (`bool`,
    /// `f64`) or an `enum` maps the value.
    pub fn bounds(&self) -> Option<(f64, f64)> {
        if !self.enum_raw.is_empty() {
            return None;
        }
        let (lo, hi) = match self.raw {
            RawNumber::Bool | RawNumber::F64 => return None,
            RawNumber::F32 => (-f64::from(f32::MAX), f64::from(f32::MAX)),
            raw => {
                let (lo, hi) = raw.integer_bounds()?;
                (lo as f64, hi as f64)
            }
        };
        let (a, b) = (lo * self.scale + self.offset, hi * self.scale + self.offset);
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// True when every raw integer of the `enum` fits the raw type.
    pub fn enum_fits(&self) -> bool {
        match self.raw.integer_bounds() {
            Some((lo, hi)) => self
                .enum_raw
                .iter()
                .all(|v| (lo..=hi).contains(&i128::from(*v))),
            None => self.enum_raw.is_empty(),
        }
    }

    fn integer(&self, value: RawValue<'_>) -> Result<i128, EncodeError> {
        let exact = self.scale == 1.0 && self.offset == 0.0;
        match value {
            RawValue::Uint(index) if !self.enum_raw.is_empty() => usize::try_from(index)
                .ok()
                .and_then(|i| self.enum_raw.get(i))
                .map(|v| i128::from(*v))
                .ok_or(EncodeError::NotRepresentable),
            RawValue::Int(i) if exact => Ok(i128::from(i)),
            RawValue::Uint(u) if exact => Ok(i128::from(u)),
            RawValue::Int(i) => self.rounded(i as f64),
            RawValue::Uint(u) => self.rounded(u as f64),
            RawValue::Float(f) => self.rounded(f),
            _ => Err(EncodeError::Unsupported),
        }
    }

    fn rounded(&self, value: f64) -> Result<i128, EncodeError> {
        let raw = round_ties_even((value - self.offset) / self.scale);
        if !raw.is_finite() || raw >= 1.8e19 || raw <= -1.8e19 {
            return Err(EncodeError::NotRepresentable);
        }
        Ok(raw as i128)
    }

    fn float(&self, value: RawValue<'_>) -> Result<f64, EncodeError> {
        let v = match value {
            RawValue::Int(i) => i as f64,
            RawValue::Uint(u) => u as f64,
            RawValue::Float(f) => f,
            _ => return Err(EncodeError::Unsupported),
        };
        let raw = (v - self.offset) / self.scale;
        if !raw.is_finite() {
            return Err(EncodeError::NotRepresentable);
        }
        Ok(raw)
    }

    /// `write` with its value converted to raw; register values are put in
    /// `words`. A relinquish passes unchanged.
    pub fn encode<'b>(
        &self,
        write: &CommittedWrite<'_>,
        words: &'b mut [u16; 4],
    ) -> Result<CommittedWrite<'b>, EncodeError> {
        let raw_bits: u64 = match (self.raw, write.value) {
            (_, RawValue::None) => {
                return Ok(with(write, RawValue::None));
            }
            (RawNumber::Bool, RawValue::Bool(b)) if self.regs.is_none() => {
                return Ok(with(write, RawValue::Bool(b != self.invert)));
            }
            (RawNumber::Bool, _) => return Err(EncodeError::Unsupported),
            (RawNumber::F32, value) => {
                let raw = self.float(value)?;
                if raw > f64::from(f32::MAX) || raw < -f64::from(f32::MAX) {
                    return Err(EncodeError::NotRepresentable);
                }
                let narrow = raw as f32;
                if self.regs.is_none() {
                    return Ok(with(write, RawValue::Float(f64::from(narrow))));
                }
                u64::from(narrow.to_bits())
            }
            (RawNumber::F64, value) => {
                let raw = self.float(value)?;
                if self.regs.is_none() {
                    return Ok(with(write, RawValue::Float(raw)));
                }
                raw.to_bits()
            }
            (raw, value) => {
                let n = self.integer(value)?;
                let (lo, hi) = raw.integer_bounds().ok_or(EncodeError::Unsupported)?;
                if n < lo || n > hi {
                    return Err(EncodeError::NotRepresentable);
                }
                if self.regs.is_none() {
                    let value = match raw {
                        RawNumber::Signed(_) => RawValue::Int(n as i64),
                        _ => RawValue::Uint(n as u64),
                    };
                    return Ok(with(write, value));
                }
                let mask = if raw.bits() >= 64 {
                    u64::MAX
                } else {
                    (1u64 << raw.bits()) - 1
                };
                (n as i64 as u64) & mask
            }
        };
        let Some(layout) = self.regs else {
            return Err(EncodeError::Unsupported);
        };
        let n = usize::from(layout.words);
        if !(1..=4).contains(&n) || u32::from(layout.words) * 16 < self.raw.bits() {
            return Err(EncodeError::Unsupported);
        }
        for (k, slot) in words.iter_mut().take(n).enumerate() {
            let shift = 16 * (n - 1 - k);
            let word = (raw_bits >> shift) as u16;
            *slot = if layout.bytes_little {
                word.swap_bytes()
            } else {
                word
            };
        }
        let regs: &'b mut [u16] = &mut words[..n];
        if layout.words_little {
            regs.reverse();
        }
        Ok(with(write, RawValue::Regs(regs)))
    }
}

fn with<'b>(write: &CommittedWrite<'_>, value: RawValue<'b>) -> CommittedWrite<'b> {
    CommittedWrite {
        seq: write.seq,
        point: write.point,
        value,
        kind: write.kind,
        at: write.at,
        atomic: write.atomic,
        sealed: write.sealed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::Sealed;
    use crate::types::WriteKind;

    fn write(value: RawValue<'static>) -> CommittedWrite<'static> {
        CommittedWrite {
            seq: 9,
            point: 3,
            value,
            kind: WriteKind::Set,
            at: None,
            atomic: None,
            sealed: Sealed(()),
        }
    }

    fn codec(
        raw: RawNumber,
        scale: f64,
        offset: f64,
        regs: Option<RegLayout>,
    ) -> OutputCodec<'static> {
        OutputCodec {
            invert: false,
            enum_raw: &[],
            scale,
            offset,
            raw,
            regs,
        }
    }

    const ONE: Option<RegLayout> = Some(RegLayout {
        words: 1,
        words_little: false,
        bytes_little: false,
    });

    #[test]
    fn rounding_is_to_the_nearest_with_ties_to_even() {
        let r: [f64; 8] = [0.5, 1.5, 2.5, -0.5, -1.5, -2.5, 2.4999, 2.5001];
        let expected = [0.0, 2.0, 2.0, 0.0, -2.0, -2.0, 2.0, 3.0];
        for (x, e) in r.iter().zip(expected) {
            assert_eq!(round_ties_even(*x), e, "{x}");
        }
    }

    #[test]
    fn a_scaled_value_becomes_its_register() {
        let c = codec(RawNumber::Signed(16), 0.1, 0.0, ONE);
        let mut words = [0; 4];
        let w = c.encode(&write(RawValue::Float(-1.0)), &mut words).unwrap();
        assert_eq!(w.value, RawValue::Regs(&[0xFFF6]));
        assert_eq!((w.seq, w.point, w.kind), (9, 3, WriteKind::Set));
        let c = codec(RawNumber::Unsigned(16), 0.5, 0.0, ONE);
        let mut words = [0; 4];
        let w = c.encode(&write(RawValue::Float(1.25)), &mut words).unwrap();
        assert_eq!(w.value, RawValue::Regs(&[2]), "2.5 rounds to 2");
    }

    #[test]
    fn a_value_outside_the_raw_type_is_not_representable() {
        let c = codec(RawNumber::Unsigned(16), 1.0, 0.0, ONE);
        let mut words = [0; 4];
        assert_eq!(
            c.encode(&write(RawValue::Float(65_535.6)), &mut words),
            Err(EncodeError::NotRepresentable)
        );
        assert_eq!(
            c.encode(&write(RawValue::Float(-0.6)), &mut words),
            Err(EncodeError::NotRepresentable)
        );
        assert_eq!(c.bounds(), Some((0.0, 65_535.0)));
        let c = codec(RawNumber::Signed(16), -0.1, 5.0, None);
        let (lo, hi) = c.bounds().unwrap();
        assert!(
            (lo - -3_271.7).abs() < 1e-9 && (hi - 3_281.8).abs() < 1e-9,
            "{lo} {hi}"
        );
    }

    #[test]
    fn word_and_byte_order_lay_out_the_registers() {
        let layout = |words_little, bytes_little| {
            Some(RegLayout {
                words: 2,
                words_little,
                bytes_little,
            })
        };
        let mut words = [0; 4];
        let c = codec(RawNumber::Unsigned(32), 1.0, 0.0, layout(false, false));
        let w = c
            .encode(&write(RawValue::Uint(0x0001_0203)), &mut words)
            .unwrap();
        assert_eq!(w.value, RawValue::Regs(&[0x0001, 0x0203]));
        let mut words = [0; 4];
        let c = codec(RawNumber::Unsigned(32), 1.0, 0.0, layout(true, true));
        let w = c
            .encode(&write(RawValue::Uint(0x0001_0203)), &mut words)
            .unwrap();
        assert_eq!(w.value, RawValue::Regs(&[0x0302, 0x0100]));
        let mut words = [0; 4];
        let c = codec(RawNumber::F32, 1.0, 0.0, layout(false, false));
        let w = c.encode(&write(RawValue::Float(1.5)), &mut words).unwrap();
        assert_eq!(w.value, RawValue::Regs(&[0x3FC0, 0x0000]));
    }

    #[test]
    fn invert_and_enum_map_to_their_raw_values() {
        let mut words = [0; 4];
        let c = OutputCodec {
            invert: true,
            ..codec(RawNumber::Bool, 1.0, 0.0, None)
        };
        let w = c.encode(&write(RawValue::Bool(true)), &mut words).unwrap();
        assert_eq!(w.value, RawValue::Bool(false));
        let c = OutputCodec {
            enum_raw: &[0, 7, 300],
            ..codec(RawNumber::Unsigned(8), 1.0, 0.0, None)
        };
        let w = c.encode(&write(RawValue::Uint(1)), &mut words).unwrap();
        assert_eq!(w.value, RawValue::Uint(7));
        assert!(!c.enum_fits(), "300 does not fit a u8");
        assert_eq!(
            c.encode(&write(RawValue::Uint(2)), &mut words),
            Err(EncodeError::NotRepresentable)
        );
        assert_eq!(
            c.encode(&write(RawValue::Uint(5)), &mut words),
            Err(EncodeError::NotRepresentable)
        );
        let c = codec(RawNumber::Signed(16), 1.0, 0.0, None);
        let w = c.encode(&write(RawValue::Int(-7)), &mut words).unwrap();
        assert_eq!(w.value, RawValue::Int(-7));
        let w = c.encode(&write(RawValue::None), &mut words).unwrap();
        assert_eq!(w.value, RawValue::None);
    }
}
