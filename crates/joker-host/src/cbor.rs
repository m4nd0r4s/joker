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
    fn refuses_what_deterministic_cbor_never_has() {
        assert!(decode_sequence(&[0x9f]).is_err());
        assert!(decode_sequence(&[0xc1, 0x00]).is_err());
        assert!(decode_sequence(&[0x63, b'a']).is_err());
        assert!(decode_sequence(&[0x62, 0xff, 0xfe]).is_err());
        assert!(decode_sequence(&[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err());
    }
}
