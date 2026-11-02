//! A small CBOR (RFC 8949) reader for `joker:meta`: integers, floats, text,
//! bytes, arrays, maps, booleans and null. Indefinite lengths and tags are
//! refused, as deterministic CBOR never uses them (ref/00 §7).

/// A decoded CBOR item.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// An unsigned integer.
    Uint(u64),
    /// A negative integer, as its value.
    Nint(i128),
    /// A float of any width, widened.
    Float(f64),
    /// A text string.
    Text(String),
    /// A byte string.
    Bytes(Vec<u8>),
    /// An array.
    Array(Vec<Item>),
    /// A map, in encoded order.
    Map(Vec<(Item, Item)>),
    /// `true` or `false`.
    Bool(bool),
    /// `null`.
    Null,
}

/// The input is not the CBOR this reader accepts.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CborError {
    /// Offset of the offending byte.
    pub offset: usize,
}

const MAX_DEPTH: usize = 32;

impl Item {
    /// The value of an integer key `key` in a map.
    pub fn get(&self, key: u64) -> Option<&Item> {
        match self {
            Item::Map(entries) => entries
                .iter()
                .find(|(k, _)| *k == Item::Uint(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// The item as an unsigned integer.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Item::Uint(v) => Some(*v),
            _ => None,
        }
    }

    /// The item as text.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Item::Text(s) => Some(s),
            _ => None,
        }
    }

    /// The item as an array.
    pub fn as_array(&self) -> Option<&[Item]> {
        match self {
            Item::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The item as a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Item::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The item as a number (integer or float).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Item::Uint(v) => Some(*v as f64),
            Item::Nint(v) => Some(*v as f64),
            Item::Float(v) => Some(*v),
            _ => None,
        }
    }
}

/// Decodes a CBOR sequence (RFC 8742): every item of `bytes`, in order.
pub fn decode_sequence(bytes: &[u8]) -> Result<Vec<Item>, CborError> {
    let mut r = Reader { bytes, pos: 0 };
    let mut items = Vec::new();
    while r.pos < bytes.len() {
        items.push(r.item(0)?);
    }
    Ok(items)
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn error(&self) -> CborError {
        CborError { offset: self.pos }
    }

    fn take(&mut self, n: usize) -> Result<&[u8], CborError> {
        let end = self.pos.checked_add(n).ok_or(self.error())?;
        let start = self.pos;
        let s = self.bytes.get(start..end).ok_or(self.error())?;
        self.pos = end;
        Ok(s)
    }

    fn argument(&mut self, info: u8) -> Result<u64, CborError> {
        Ok(match info {
            0..=23 => u64::from(info),
            24 => u64::from(self.take(1)?[0]),
            25 => {
                let b = self.take(2)?;
                u64::from(u16::from_be_bytes([b[0], b[1]]))
            }
            26 => {
                let b = self.take(4)?;
                u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            }
            27 => {
                let mut w = [0u8; 8];
                w.copy_from_slice(self.take(8)?);
                u64::from_be_bytes(w)
            }
            _ => return Err(self.error()),
        })
    }

    fn length(&mut self, info: u8) -> Result<usize, CborError> {
        let n = self.argument(info)?;
        let n = usize::try_from(n).map_err(|_| self.error())?;
        if n > self.bytes.len() {
            return Err(self.error());
        }
        Ok(n)
    }

    fn item(&mut self, depth: usize) -> Result<Item, CborError> {
        if depth > MAX_DEPTH {
            return Err(self.error());
        }
        let initial = self.take(1)?[0];
        let (major, info) = (initial >> 5, initial & 0x1f);
        Ok(match major {
            0 => Item::Uint(self.argument(info)?),
            1 => Item::Nint(-1 - i128::from(self.argument(info)?)),
            2 => {
                let n = self.length(info)?;
                Item::Bytes(self.take(n)?.to_vec())
            }
            3 => {
                let n = self.length(info)?;
                let at = self.pos;
                let s =
                    core::str::from_utf8(self.take(n)?).map_err(|_| CborError { offset: at })?;
                Item::Text(s.to_owned())
            }
            4 => {
                let n = self.length(info)?;
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(self.item(depth + 1)?);
                }
                Item::Array(a)
            }
            5 => {
                let n = self.length(info)?;
                let mut m = Vec::with_capacity(n);
                for _ in 0..n {
                    let k = self.item(depth + 1)?;
                    let v = self.item(depth + 1)?;
                    m.push((k, v));
                }
                Item::Map(m)
            }
            7 => match info {
                20 => Item::Bool(false),
                21 => Item::Bool(true),
                22 => Item::Null,
                25 => {
                    let b = self.take(2)?;
                    Item::Float(f16_to_f64(u16::from_be_bytes([b[0], b[1]])))
                }
                26 => {
                    let b = self.take(4)?;
                    Item::Float(f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]])))
                }
                27 => {
                    let mut w = [0u8; 8];
                    w.copy_from_slice(self.take(8)?);
                    Item::Float(f64::from_be_bytes(w))
                }
                _ => {
                    return Err(CborError {
                        offset: self.pos - 1,
                    });
                }
            },
            _ => {
                return Err(CborError {
                    offset: self.pos - 1,
                });
            }
        })
    }
}

fn f16_to_f64(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f64::from(h & 0x3ff);
    let magnitude = match exp {
        0 => frac * 2f64.powi(-24),
        31 if frac == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        e => (1.0 + frac / 1024.0) * 2f64.powi(e - 15),
    };
    sign * magnitude
}

/// Reads the head of a CBOR item at `at`: major type and argument. Only
/// definite lengths.
fn head(bytes: &[u8], at: &mut usize) -> Option<(u8, u64)> {
    let first = *bytes.get(*at)?;
    *at += 1;
    let info = first & 0x1f;
    let n = match info {
        0..=23 => return Some((first >> 5, u64::from(info))),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return None,
    };
    let raw = bytes.get(*at..*at + n)?;
    *at += n;
    let mut word = [0u8; 8];
    word[8 - n..].copy_from_slice(raw);
    Some((first >> 5, u64::from_be_bytes(word)))
}

fn half(bits: u16) -> f64 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((bits >> 10) & 0x1f);
    let frac = f64::from(bits & 0x3ff);
    sign * match exp {
        0 => frac * 2f64.powi(-24),
        31 if frac == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        _ => (1.0 + frac / 1024.0) * 2f64.powi(exp - 15),
    }
}

/// Decodes the arguments of a method record (ref/04 §3.3, §4.10): a map
/// with the keys `0..params.len()` in order, each value in the type of its
/// parameter, into `out` as value bits (ref/00 §4.1). No arguments may also
/// be an empty blob. `None` when the blob is not that: the call is refused
/// with `J-E0504`. Does not allocate.
pub(crate) fn method_args(
    bytes: &[u8],
    params: &[joker_core::ValueType],
    out: &mut [u64],
) -> Option<usize> {
    use joker_core::ValueType as T;
    if bytes.is_empty() && params.is_empty() {
        return Some(0);
    }
    if params.len() > out.len() {
        return None;
    }
    let mut at = 0usize;
    let (major, n) = head(bytes, &mut at)?;
    if major != 5 || n != params.len() as u64 {
        return None;
    }
    for (i, (ty, slot)) in params.iter().zip(out.iter_mut()).enumerate() {
        if head(bytes, &mut at)? != (0, i as u64) {
            return None;
        }
        let first = *bytes.get(at)?;
        let (major, arg) = head(bytes, &mut at)?;
        let float = match (major, first & 0x1f) {
            (7, 25) => Some(half(arg as u16)),
            (7, 26) => Some(f64::from(f32::from_bits(arg as u32))),
            (7, 27) => Some(f64::from_bits(arg)),
            _ => None,
        };
        *slot = match (ty, major, arg) {
            (T::Bool, 7, 20) => 0,
            (T::Bool, 7, 21) => 1,
            (T::I64, 0, v) if i64::try_from(v).is_ok() => v,
            (T::I64, 1, v) if i64::try_from(v).is_ok() => (-1 - v as i64) as u64,
            (T::U64 | T::Bits | T::Enum, 0, v) => v,
            (T::F64 | T::F32, 0, v) => (v as f64).to_bits(),
            (T::F64 | T::F32, 1, v) => (-1.0 - v as f64).to_bits(),
            (T::F64 | T::F32, 7, _) => float.filter(|f| f.is_finite())?.to_bits(),
            _ => return None,
        };
    }
    (at == bytes.len()).then_some(params.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_level_example_fragments() {
        let bytes = [
            0xa3, 0x00, 0x01, 0x01, 0x65, b'l', b'e', b'v', b'e', b'l', 0x08, 0xf9, 0x3e, 0x00,
            0x82, 0x20, 0xf5,
        ];
        let items = decode_sequence(&bytes).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get(0).and_then(Item::as_u64), Some(1));
        assert_eq!(items[0].get(1).and_then(Item::as_str), Some("level"));
        assert_eq!(items[0].get(8).and_then(Item::as_f64), Some(1.5));
        assert_eq!(
            items[1],
            Item::Array(vec![Item::Nint(-1), Item::Bool(true)])
        );
    }

    #[test]
    fn floats_of_every_width() {
        assert_eq!(f16_to_f64(0x3c00), 1.0);
        assert_eq!(f16_to_f64(0xc000), -2.0);
        assert_eq!(f16_to_f64(0x0001), 2f64.powi(-24));
        let items = decode_sequence(&[0xfa, 0x41, 0x20, 0x00, 0x00]).unwrap();
        assert_eq!(items[0], Item::Float(10.0));
    }

    #[test]
    fn method_arguments_decode_into_value_bits() {
        use joker_core::ValueType as T;
        let mut out = [0u64; 8];
        let args = [0xa3, 0x00, 0xf9, 0x3e, 0x00, 0x01, 0x38, 0x09, 0x02, 0xf5];
        let n = method_args(&args, &[T::F64, T::I64, T::Bool], &mut out);
        assert_eq!(n, Some(3));
        assert_eq!(f64::from_bits(out[0]), 1.5);
        assert_eq!(out[1] as i64, -10);
        assert_eq!(out[2], 1);
        let double = [
            0xa1, 0x00, 0xfb, 0x40, 0x09, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x18,
        ];
        assert_eq!(method_args(&double, &[T::F64], &mut out), Some(1));
        assert_eq!(f64::from_bits(out[0]), core::f64::consts::PI);
        assert_eq!(
            method_args(&[0xa1, 0x00, 0x18, 0x2a], &[T::F64], &mut out),
            Some(1)
        );
        assert_eq!(f64::from_bits(out[0]), 42.0);
        assert_eq!(method_args(&[], &[], &mut out), Some(0));
        assert_eq!(method_args(&[0xa0], &[], &mut out), Some(0));
    }

    #[test]
    fn method_arguments_that_do_not_fit_are_refused() {
        use joker_core::ValueType as T;
        let mut out = [0u64; 8];
        assert_eq!(method_args(&[0xa1, 0x00, 0xf5], &[T::F64], &mut out), None);
        assert_eq!(method_args(&[0xa1, 0x01, 0x01], &[T::U64], &mut out), None);
        assert_eq!(method_args(&[0xa0], &[T::U64], &mut out), None);
        assert_eq!(method_args(&[0xa1, 0x00, 0x20], &[T::U64], &mut out), None);
        assert_eq!(
            method_args(&[0xa1, 0x00, 0xf9, 0x7c, 0x00], &[T::F64], &mut out),
            None
        );
        assert_eq!(
            method_args(&[0xa1, 0x00, 0x01, 0x00], &[T::U64], &mut out),
            None
        );
        assert_eq!(method_args(&[0x81, 0x01], &[T::U64], &mut out), None);
    }

    #[test]
    fn refuses_what_deterministic_cbor_never_has() {
        assert!(decode_sequence(&[0x9f]).is_err());
        assert!(decode_sequence(&[0xc1, 0x00]).is_err());
        assert!(decode_sequence(&[0x63, b'a']).is_err());
        assert!(decode_sequence(&[0x62, 0xff, 0xfe]).is_err());
        assert!(decode_sequence(&[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err());
    }
}
