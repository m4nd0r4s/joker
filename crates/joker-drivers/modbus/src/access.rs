//! Access by address for the step programs of a profile (capability
//! `raw-access`, ref/06 §2.10, ADR 0073): a step's address and `type`
//! become one Modbus request, and the registers or bits of its answer the
//! scalar the step reads.
//!
//! The register count of a type is the table of ref/03 §2.2.7; the words
//! are assembled with the address's `word_order` (big: the first register
//! is the most significant) and `byte_order` (within each register). A
//! step without `type` reads and writes `bool` on the bit function codes
//! and `u16` on the register ones. Everything here is pure: the drivers
//! frame the PDU and move the bytes.

use joker_hal::{BatchFailure, Cbor, RawValue};

use crate::plan::Address;

/// The scalar a step's `type` reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// `bool`: a coil, a discrete input, or a register (one `bit` of it,
    /// or any non-zero value).
    Bool,
    /// `u16`, `u32`, `u64` and `bits16`/`bits32`/`bits64`.
    Unsigned,
    /// `i16`, `i32`, `i64`.
    Signed,
    /// `f32`, `f64`.
    Float,
}

/// One step's access, from its address and `type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    /// The address, base 0.
    pub at: u16,
    /// The function code.
    pub fc: u8,
    /// The bit of a register a `bool` reads.
    pub bit: Option<u8>,
    /// Registers the type spans; 0 on the bit function codes.
    pub words: u16,
    /// The scalar of the type.
    pub form: Form,
    /// `word_order = "little"`.
    pub word_little: bool,
    /// `byte_order = "little"`.
    pub byte_little: bool,
}

/// Largest PDU a step sends: FC 16 with four registers.
pub const MAX_PDU: usize = 6 + 8;

/// A request PDU: function code and data, without unit, CRC or header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pdu {
    bytes: [u8; MAX_PDU],
    len: usize,
}

impl Pdu {
    fn new(head: &[u8]) -> Self {
        let mut bytes = [0u8; MAX_PDU];
        bytes[..head.len()].copy_from_slice(head);
        Self {
            bytes,
            len: head.len(),
        }
    }

    fn push(&mut self, byte: u8) {
        if let Some(slot) = self.bytes.get_mut(self.len) {
            *slot = byte;
            self.len += 1;
        }
    }

    /// The bytes of the PDU.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// The function code.
    pub fn fc(&self) -> u8 {
        self.bytes[0]
    }
}

const TYPE_MISMATCH: BatchFailure =
    BatchFailure::ConfigError("the step's type does not fit its function code");
const OUT_OF_RANGE: BatchFailure =
    BatchFailure::ConfigError("the value does not fit the step's type");

fn shape(type_name: &str, bits: bool) -> Result<(Form, u16), BatchFailure> {
    Ok(match type_name {
        "" if bits => (Form::Bool, 0),
        "" => (Form::Unsigned, 1),
        "bool" if bits => (Form::Bool, 0),
        "bool" => (Form::Bool, 1),
        _ if bits => return Err(TYPE_MISMATCH),
        "u16" | "bits16" => (Form::Unsigned, 1),
        "i16" => (Form::Signed, 1),
        "u32" | "bits32" => (Form::Unsigned, 2),
        "i32" => (Form::Signed, 2),
        "f32" => (Form::Float, 2),
        "u64" | "bits64" => (Form::Unsigned, 4),
        "i64" => (Form::Signed, 4),
        "f64" => (Form::Float, 4),
        _ => {
            return Err(BatchFailure::ConfigError(
                "a step reads and writes scalar types",
            ));
        }
    })
}

impl Access {
    /// The access of a step at `addr` with `type_name` (empty when the
    /// step has none).
    pub fn parse(addr: Cbor<'_>, type_name: &str) -> Result<Self, BatchFailure> {
        let a = Address::parse(addr).map_err(BatchFailure::ConfigError)?;
        let bits = matches!(a.fc, 1 | 2 | 5 | 15);
        if !matches!(a.fc, 1..=6 | 15 | 16) {
            return Err(BatchFailure::ConfigError("unknown function code"));
        }
        let (form, words) = shape(type_name, bits)?;
        if a.bit.is_some() && (bits || form != Form::Bool) {
            return Err(BatchFailure::ConfigError(
                "`bit` is a bool read from a register",
            ));
        }
        if u32::from(a.at) + u32::from(words.max(1)) - 1 > 65_535 {
            return Err(BatchFailure::ConfigError("address past 65535"));
        }
        Ok(Self {
            at: a.at,
            fc: a.fc,
            bit: a.bit,
            words,
            form,
            word_little: a.word_little,
            byte_little: a.byte_little,
        })
    }

    /// The read request: FC 1–4 for one bit or the type's registers.
    pub fn read(&self) -> Result<Pdu, BatchFailure> {
        if !matches!(self.fc, 1..=4) {
            return Err(BatchFailure::ConfigError(
                "a step reads with FC 1, 2, 3 or 4",
            ));
        }
        let [a0, a1] = self.at.to_be_bytes();
        let [c0, c1] = self.words.max(1).to_be_bytes();
        Ok(Pdu::new(&[self.fc, a0, a1, c0, c1]))
    }

    /// Bytes of data the answer to [`Access::read`] carries.
    pub fn read_data_len(&self) -> usize {
        crate::rtu::read_data_len(self.fc, self.words.max(1))
    }

    /// The scalar of the data of an answer to [`Access::read`].
    pub fn decode(&self, data: &[u8]) -> Result<RawValue<'static>, BatchFailure> {
        let short = BatchFailure::CommFailure("wrong byte count");
        if self.words == 0 {
            let byte = data.first().ok_or(short)?;
            return Ok(RawValue::Bool(byte & 1 == 1));
        }
        let n = usize::from(self.words);
        if data.len() != 2 * n {
            return Err(short);
        }
        let mut v = 0u64;
        for i in 0..n {
            let k = if self.word_little { n - 1 - i } else { i };
            let mut w = u16::from_be_bytes([data[2 * k], data[2 * k + 1]]);
            if self.byte_little {
                w = w.swap_bytes();
            }
            v = (v << 16) | u64::from(w);
        }
        let width = 16 * u32::from(self.words);
        Ok(match self.form {
            Form::Bool => RawValue::Bool(match self.bit {
                Some(b) => (v >> b) & 1 == 1,
                None => v != 0,
            }),
            Form::Unsigned => RawValue::Uint(v),
            Form::Signed => {
                let shift = 64 - width;
                RawValue::Int(((v << shift) as i64) >> shift)
            }
            Form::Float if self.words == 2 => RawValue::Float(f64::from(f32::from_bits(v as u32))),
            Form::Float => RawValue::Float(f64::from_bits(v)),
        })
    }

    fn bits_of(&self, value: RawValue<'_>) -> Result<u64, BatchFailure> {
        let width = 16 * u32::from(self.words.max(1));
        let max = if width == 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        match (self.form, value) {
            (Form::Bool, RawValue::Bool(b)) => Ok(u64::from(b)),
            (Form::Bool, RawValue::Uint(u)) if u <= 1 => Ok(u),
            (Form::Bool, RawValue::Int(i)) if (0..=1).contains(&i) => Ok(i as u64),
            (Form::Unsigned, RawValue::Uint(u)) if u <= max => Ok(u),
            (Form::Unsigned, RawValue::Int(i)) if i >= 0 && (i as u64) <= max => Ok(i as u64),
            (Form::Signed, v @ (RawValue::Int(_) | RawValue::Uint(_))) => {
                signed_fits(v, width).ok_or(OUT_OF_RANGE)
            }
            (Form::Float, v) => {
                let f = match v {
                    RawValue::Float(f) => f,
                    RawValue::Int(i) => i as f64,
                    RawValue::Uint(u) => u as f64,
                    _ => return Err(OUT_OF_RANGE),
                };
                Ok(if self.words == 2 {
                    u64::from((f as f32).to_bits())
                } else {
                    f.to_bits()
                })
            }
            _ => Err(OUT_OF_RANGE),
        }
    }

    /// The write request of `value`: FC 5 or 15 for a coil, FC 6 for one
    /// register, FC 16 for the type's registers.
    pub fn write(&self, value: RawValue<'_>) -> Result<Pdu, BatchFailure> {
        if !matches!(self.fc, 5 | 6 | 15 | 16) {
            return Err(BatchFailure::ConfigError(
                "a step writes with FC 5, 6, 15 or 16",
            ));
        }
        if self.bit.is_some() {
            return Err(BatchFailure::ConfigError(
                "a step does not write one bit of a register",
            ));
        }
        let v = self.bits_of(value)?;
        let [a0, a1] = self.at.to_be_bytes();
        Ok(match self.fc {
            5 => Pdu::new(&[5, a0, a1, if v == 1 { 0xFF } else { 0 }, 0]),
            15 => Pdu::new(&[15, a0, a1, 0, 1, 1, v as u8]),
            6 if self.words == 1 => {
                let [w0, w1] = self.word(v, 0).to_be_bytes();
                Pdu::new(&[6, a0, a1, w0, w1])
            }
            6 => return Err(TYPE_MISMATCH),
            _ => {
                let [c0, c1] = self.words.to_be_bytes();
                let mut pdu = Pdu::new(&[16, a0, a1, c0, c1, (2 * self.words) as u8]);
                for i in 0..self.words {
                    let [w0, w1] = self.word(v, i).to_be_bytes();
                    pdu.push(w0);
                    pdu.push(w1);
                }
                pdu
            }
        })
    }

    fn word(&self, v: u64, index: u16) -> u16 {
        let n = self.words.max(1);
        let k = if self.word_little {
            index
        } else {
            n - 1 - index
        };
        let w = (v >> (16 * u32::from(k))) as u16;
        if self.byte_little { w.swap_bytes() } else { w }
    }
}

fn signed_fits(value: RawValue<'_>, width: u32) -> Option<u64> {
    let i = match value {
        RawValue::Int(i) => i,
        RawValue::Uint(u) => i64::try_from(u).ok()?,
        _ => return None,
    };
    if width == 64 {
        return Some(i as u64);
    }
    let lo = -(1i64 << (width - 1));
    let hi = (1i64 << (width - 1)) - 1;
    (lo..=hi)
        .contains(&i)
        .then(|| (i as u64) & ((1u64 << width) - 1))
}

#[cfg(test)]
mod tests {
    use joker_hal::cbor::Value;

    use super::*;

    fn access(entries: &[(&str, Value)], type_name: &str) -> Result<Access, BatchFailure> {
        let addr = Value::Map(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        )
        .encode();
        Access::parse(Cbor::new(&addr), type_name)
    }

    fn at(at: i64, fc: i64) -> Vec<(&'static str, Value)> {
        vec![("at", Value::Int(at)), ("fc", Value::Int(fc))]
    }

    #[test]
    fn a_holding_u16_reads_one_register_and_writes_with_fc_6() {
        let a = access(&at(40300, 3), "u16").unwrap();
        assert_eq!(a.read().unwrap().as_bytes(), [3, 0x9D, 0x6C, 0, 1]);
        assert_eq!(a.decode(&[0x05, 0xAA]).unwrap(), RawValue::Uint(0x05AA));
        let w = access(&at(40300, 6), "u16").unwrap();
        assert_eq!(
            w.write(RawValue::Uint(1)).unwrap().as_bytes(),
            [6, 0x9D, 0x6C, 0, 1]
        );
        assert_eq!(w.write(RawValue::Uint(70_000)), Err(OUT_OF_RANGE));
        assert_eq!(w.write(RawValue::Int(-1)), Err(OUT_OF_RANGE));
    }

    #[test]
    fn word_and_byte_order_decide_how_registers_assemble() {
        let big = access(&at(0, 4), "i32").unwrap();
        assert_eq!(
            big.decode(&[0xFF, 0xFF, 0xFF, 0xFE]).unwrap(),
            RawValue::Int(-2)
        );
        let mut little = at(0, 4);
        little.push(("word_order", Value::text("little")));
        let little = access(&little, "u32").unwrap();
        assert_eq!(
            little.decode(&[0x00, 0x02, 0x00, 0x01]).unwrap(),
            RawValue::Uint(0x0001_0002)
        );
        let mut swapped = at(0, 3);
        swapped.push(("byte_order", Value::text("little")));
        let swapped = access(&swapped, "u16").unwrap();
        assert_eq!(
            swapped.decode(&[0x34, 0x12]).unwrap(),
            RawValue::Uint(0x1234)
        );
        let f = access(&at(0, 3), "f32").unwrap();
        let [b0, b1, b2, b3] = 1.5f32.to_bits().to_be_bytes();
        assert_eq!(f.decode(&[b0, b1, b2, b3]).unwrap(), RawValue::Float(1.5));
        let d = access(&at(0, 3), "f64").unwrap();
        assert_eq!(
            d.decode(&(-2.25f64).to_bits().to_be_bytes()).unwrap(),
            RawValue::Float(-2.25)
        );
    }

    #[test]
    fn multi_register_writes_use_fc_16_in_the_address_order() {
        let mut addr = at(10, 16);
        addr.push(("word_order", Value::text("little")));
        let a = access(&addr, "u32").unwrap();
        assert_eq!(
            a.write(RawValue::Uint(0x0001_0002)).unwrap().as_bytes(),
            [16, 0, 10, 0, 2, 4, 0x00, 0x02, 0x00, 0x01]
        );
        let s = access(&at(10, 16), "i16").unwrap();
        assert_eq!(
            s.write(RawValue::Int(-2)).unwrap().as_bytes(),
            [16, 0, 10, 0, 1, 2, 0xFF, 0xFE]
        );
        assert_eq!(s.write(RawValue::Int(40_000)), Err(OUT_OF_RANGE));
        let f = access(&at(10, 16), "f32").unwrap();
        let [b0, b1, b2, b3] = 2.5f32.to_bits().to_be_bytes();
        assert_eq!(
            f.write(RawValue::Float(2.5)).unwrap().as_bytes(),
            [16, 0, 10, 0, 2, 4, b0, b1, b2, b3]
        );
        assert_eq!(
            access(&at(10, 6), "u32").unwrap().write(RawValue::Uint(1)),
            Err(TYPE_MISMATCH)
        );
    }

    #[test]
    fn coils_and_bits() {
        let c = access(&at(3, 5), "bool").unwrap();
        assert_eq!(
            c.write(RawValue::Bool(true)).unwrap().as_bytes(),
            [5, 0, 3, 0xFF, 0]
        );
        assert_eq!(
            c.write(RawValue::Uint(0)).unwrap().as_bytes(),
            [5, 0, 3, 0, 0]
        );
        let m = access(&at(3, 15), "").unwrap();
        assert_eq!(
            m.write(RawValue::Bool(true)).unwrap().as_bytes(),
            [15, 0, 3, 0, 1, 1, 1]
        );
        let d = access(&at(7, 2), "").unwrap();
        assert_eq!(d.read().unwrap().as_bytes(), [2, 0, 7, 0, 1]);
        assert_eq!(d.decode(&[1]).unwrap(), RawValue::Bool(true));
        let mut bit = at(0, 3);
        bit.push(("bit", Value::Int(4)));
        let b = access(&bit, "bool").unwrap();
        assert_eq!(b.decode(&[0x00, 0x10]).unwrap(), RawValue::Bool(true));
        assert!(b.write(RawValue::Bool(true)).is_err());
        assert_eq!(access(&at(0, 1), "u16"), Err(TYPE_MISMATCH));
        assert!(access(&at(0, 3), "string").is_err());
        assert!(access(&at(0, 7), "u16").is_err());
        assert!(access(&at(65_535, 3), "u32").is_err());
        assert!(access(&at(0, 6), "u16").unwrap().read().is_err());
        assert!(
            access(&at(0, 3), "u16")
                .unwrap()
                .write(RawValue::Uint(1))
                .is_err()
        );
    }
}
