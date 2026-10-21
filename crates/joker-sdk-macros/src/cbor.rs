//! Deterministic CBOR (RFC 8949 §4.2.1) for the `joker:meta` fragments:
//! shortest integer heads, the smallest float that keeps the value, and map
//! keys in ascending order (the callers write them so).

/// A CBOR item.
#[derive(Clone, Debug, PartialEq)]
pub enum Cbor {
    /// Unsigned integer.
    Uint(u64),
    /// Text.
    Text(String),
    /// Float.
    Float(f64),
    /// Boolean.
    Bool(bool),
    /// Array.
    Array(Vec<Cbor>),
    /// Map with integer keys, already in ascending order.
    Map(Vec<(u64, Cbor)>),
}

fn head(out: &mut Vec<u8>, major: u8, n: u64) {
    let m = major << 5;
    if n < 24 {
        out.push(m | n as u8);
    } else if n <= 0xff {
        out.extend_from_slice(&[m | 24, n as u8]);
    } else if n <= 0xffff {
        out.push(m | 25);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n <= 0xffff_ffff {
        out.push(m | 26);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

fn f16_bits(v: f64) -> Option<u16> {
    let f = v as f32;
    if f64::from(f) != v {
        return None;
    }
    let bits = f.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let frac = bits & 0x7f_ffff;
    if exp == 0 && frac == 0 {
        return Some(sign);
    }
    if exp == 0xff {
        return (frac == 0).then_some(sign | 0x7c00);
    }
    let e = exp - 127 + 15;
    if (1..31).contains(&e) && frac & 0x1fff == 0 {
        return Some(sign | ((e as u16) << 10) | (frac >> 13) as u16);
    }
    if (-10..1).contains(&e) {
        let shift = 14 - e;
        let mant = frac | 0x80_0000;
        if mant & ((1u32 << shift) - 1) == 0 {
            return Some(sign | (mant >> shift) as u16);
        }
    }
    None
}

impl Cbor {
    /// Encodes the item.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Cbor::Uint(n) => head(out, 0, *n),
            Cbor::Text(s) => {
                head(out, 3, s.len() as u64);
                out.extend_from_slice(s.as_bytes());
            }
            Cbor::Float(v) => {
                if let Some(h) = f16_bits(*v) {
                    out.push(0xf9);
                    out.extend_from_slice(&h.to_be_bytes());
                } else if f64::from(*v as f32) == *v {
                    out.push(0xfa);
                    out.extend_from_slice(&(*v as f32).to_be_bytes());
                } else {
                    out.push(0xfb);
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Cbor::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
            Cbor::Array(items) => {
                head(out, 4, items.len() as u64);
                for item in items {
                    item.encode(out);
                }
            }
            Cbor::Map(entries) => {
                head(out, 5, entries.len() as u64);
                for (k, v) in entries {
                    head(out, 0, *k);
                    v.encode(out);
                }
            }
        }
    }

    /// The encoded bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_take_the_smallest_lossless_width() {
        assert_eq!(Cbor::Float(1.5).to_bytes(), [0xf9, 0x3e, 0x00]);
        assert_eq!(Cbor::Float(500.0).to_bytes(), [0xf9, 0x5f, 0xd0]);
        assert_eq!(Cbor::Float(80.0).to_bytes(), [0xf9, 0x55, 0x00]);
        assert_eq!(Cbor::Float(0.0).to_bytes(), [0xf9, 0x00, 0x00]);
        assert_eq!(
            Cbor::Float(100000.0).to_bytes(),
            [0xfa, 0x47, 0xc3, 0x50, 0x00]
        );
        assert_eq!(Cbor::Float(0.1).to_bytes()[0], 0xfb);
        assert_eq!(Cbor::Float(2f64.powi(-24)).to_bytes(), [0xf9, 0x00, 0x01]);
    }

    #[test]
    fn integers_take_the_shortest_head() {
        assert_eq!(Cbor::Uint(23).to_bytes(), [0x17]);
        assert_eq!(Cbor::Uint(24).to_bytes(), [0x18, 24]);
        assert_eq!(
            Cbor::Uint(100_000_000).to_bytes(),
            [0x1a, 0x05, 0xf5, 0xe1, 0x00]
        );
        assert_eq!(
            Cbor::Map(vec![(0, Cbor::Text("a".into())), (1, Cbor::Bool(true))]).to_bytes(),
            [0xa2, 0x00, 0x61, b'a', 0x01, 0xf5]
        );
    }
}
