//! The deterministic CBOR subset of ref/06 §2.4.3: what the core puts in
//! `config`, `profile-config`, `quirks`, `addr` and `params`.
//!
//! Reading borrows and never allocates, so a driver on a Joker Micro decodes
//! from flash. Writing ([`Value`], with `alloc`) gives the deterministic
//! encoding of RFC 8949 §4.2.1: shortest integer heads, definite lengths and
//! map keys sorted by their encoded bytes. Floats are always 64-bit, the form
//! ref/06 §2.4.3 gives to quantities.
//!
//! The subset covers unsigned and negative integers, byte and text strings,
//! arrays, maps, `false`, `true`, `null` and floats (16, 32 and 64 bits on
//! read). Tags and indefinite lengths are refused.

/// Deepest nesting accepted on read.
pub const MAX_DEPTH: usize = 16;

/// Why a CBOR item could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CborError {
    /// The bytes end inside an item.
    Truncated,
    /// The item is not of the requested type.
    Type,
    /// The item uses a form outside the subset (tags, indefinite lengths).
    Unsupported,
    /// Nesting deeper than [`MAX_DEPTH`].
    Depth,
    /// A text string is not UTF-8.
    Utf8,
    /// An integer does not fit the requested type.
    Range,
}

/// A view over one deterministic CBOR item (ref/06 §3.2).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Cbor<'a>(&'a [u8]);

impl core::fmt::Debug for Cbor<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Cbor({} bytes)", self.0.len())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Head {
    major: u8,
    info: u8,
    arg: u64,
    len: usize,
}

fn head(bytes: &[u8]) -> Result<Head, CborError> {
    let first = *bytes.first().ok_or(CborError::Truncated)?;
    let major = first >> 5;
    let info = first & 0x1f;
    let (arg, len) = match info {
        0..=23 => (u64::from(info), 1),
        24..=27 => {
            let n = 1usize << (info - 24);
            let body = bytes.get(1..1 + n).ok_or(CborError::Truncated)?;
            let mut arg = 0u64;
            for b in body {
                arg = (arg << 8) | u64::from(*b);
            }
            (arg, 1 + n)
        }
        _ => return Err(CborError::Unsupported),
    };
    if major == 6 {
        return Err(CborError::Unsupported);
    }
    Ok(Head {
        major,
        info,
        arg,
        len,
    })
}

fn item_len(bytes: &[u8], depth: usize) -> Result<usize, CborError> {
    if depth > MAX_DEPTH {
        return Err(CborError::Depth);
    }
    let h = head(bytes)?;
    match h.major {
        0 | 1 | 7 => Ok(h.len),
        2 | 3 => {
            let n = usize::try_from(h.arg).map_err(|_| CborError::Truncated)?;
            let end = h.len.checked_add(n).ok_or(CborError::Truncated)?;
            if end > bytes.len() {
                return Err(CborError::Truncated);
            }
            Ok(end)
        }
        4 | 5 => {
            let items = if h.major == 5 {
                h.arg.saturating_mul(2)
            } else {
                h.arg
            };
            let mut pos = h.len;
            for _ in 0..items {
                let rest = bytes.get(pos..).ok_or(CborError::Truncated)?;
                pos += item_len(rest, depth + 1)?;
            }
            Ok(pos)
        }
        _ => Err(CborError::Unsupported),
    }
}

impl<'a> Cbor<'a> {
    /// The CBOR of an empty map, the value of an absent table.
    pub const EMPTY_MAP: Cbor<'static> = Cbor(&[0xa0]);

    /// A view over `bytes`; nothing is checked until it is read.
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    /// The bytes.
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.0
    }

    /// Splits the first item off `bytes`.
    pub fn split(bytes: &'a [u8]) -> Result<(Self, &'a [u8]), CborError> {
        let len = item_len(bytes, 0)?;
        Ok((Self(&bytes[..len]), &bytes[len..]))
    }

    /// Checks that the view holds exactly one well-formed item.
    pub fn validate(&self) -> Result<(), CborError> {
        if item_len(self.0, 0)? == self.0.len() {
            Ok(())
        } else {
            Err(CborError::Unsupported)
        }
    }

    /// True for `null` or an empty view.
    pub fn is_null(&self) -> bool {
        self.0.is_empty() || self.0 == [0xf6]
    }

    /// The value as `u64`.
    pub fn as_u64(&self) -> Result<u64, CborError> {
        let h = head(self.0)?;
        match h.major {
            0 => Ok(h.arg),
            1 => Err(CborError::Range),
            _ => Err(CborError::Type),
        }
    }

    /// The value as `i64`.
    pub fn as_i64(&self) -> Result<i64, CborError> {
        let h = head(self.0)?;
        match h.major {
            0 => i64::try_from(h.arg).map_err(|_| CborError::Range),
            1 => i64::try_from(h.arg)
                .map(|n| -1 - n)
                .map_err(|_| CborError::Range),
            _ => Err(CborError::Type),
        }
    }

    /// The value as `f64`; integers are converted.
    pub fn as_f64(&self) -> Result<f64, CborError> {
        let h = head(self.0)?;
        match (h.major, h.info) {
            (0, _) => Ok(h.arg as f64),
            (1, _) => Ok(-1.0 - h.arg as f64),
            (7, 25) => Ok(f16_to_f64(h.arg as u16)),
            (7, 26) => Ok(f64::from(f32::from_bits(h.arg as u32))),
            (7, 27) => Ok(f64::from_bits(h.arg)),
            _ => Err(CborError::Type),
        }
    }

    /// The value as `bool`.
    pub fn as_bool(&self) -> Result<bool, CborError> {
        match self.0.first() {
            Some(0xf4) => Ok(false),
            Some(0xf5) => Ok(true),
            Some(_) => Err(CborError::Type),
            None => Err(CborError::Truncated),
        }
    }

    /// The value as text.
    pub fn as_str(&self) -> Result<&'a str, CborError> {
        let h = head(self.0)?;
        if h.major != 3 {
            return Err(CborError::Type);
        }
        let end = item_len(self.0, 0)?;
        core::str::from_utf8(&self.0[h.len..end]).map_err(|_| CborError::Utf8)
    }

    /// The value as a byte string.
    pub fn as_byte_string(&self) -> Result<&'a [u8], CborError> {
        let h = head(self.0)?;
        if h.major != 2 {
            return Err(CborError::Type);
        }
        let end = item_len(self.0, 0)?;
        Ok(&self.0[h.len..end])
    }

    /// The items of an array.
    pub fn array(&self) -> Result<Items<'a>, CborError> {
        let h = head(self.0)?;
        if h.major != 4 {
            return Err(CborError::Type);
        }
        Ok(Items {
            rest: &self.0[h.len..],
            left: h.arg,
        })
    }

    /// The entries of a map with text keys.
    pub fn entries(&self) -> Result<Entries<'a>, CborError> {
        let h = head(self.0)?;
        if h.major != 5 {
            return Err(CborError::Type);
        }
        Ok(Entries {
            rest: &self.0[h.len..],
            left: h.arg,
        })
    }

    /// The value of `key` in a map with text keys; `Ok(None)` when absent
    /// (or when the view is empty or `null`).
    pub fn get(&self, key: &str) -> Result<Option<Cbor<'a>>, CborError> {
        if self.is_null() {
            return Ok(None);
        }
        for entry in self.entries()? {
            let (k, v) = entry?;
            if k == key {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }
}

/// Iterator over the items of an array.
#[derive(Clone, Debug)]
pub struct Items<'a> {
    rest: &'a [u8],
    left: u64,
}

impl<'a> Iterator for Items<'a> {
    type Item = Result<Cbor<'a>, CborError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        match Cbor::split(self.rest) {
            Ok((item, rest)) => {
                self.rest = rest;
                Some(Ok(item))
            }
            Err(e) => {
                self.left = 0;
                Some(Err(e))
            }
        }
    }
}

/// Iterator over the entries of a map with text keys.
#[derive(Clone, Debug)]
pub struct Entries<'a> {
    rest: &'a [u8],
    left: u64,
}

impl<'a> Iterator for Entries<'a> {
    type Item = Result<(&'a str, Cbor<'a>), CborError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let step = || -> Result<(&'a str, Cbor<'a>, &'a [u8]), CborError> {
            let (key, rest) = Cbor::split(self.rest)?;
            let (value, rest) = Cbor::split(rest)?;
            Ok((key.as_str()?, value, rest))
        };
        match step() {
            Ok((key, value, rest)) => {
                self.rest = rest;
                Some(Ok((key, value)))
            }
            Err(e) => {
                self.left = 0;
                Some(Err(e))
            }
        }
    }
}

fn f16_to_f64(bits: u16) -> f64 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((bits >> 10) & 0x1f);
    let frac = f64::from(bits & 0x3ff);
    let magnitude = match exp {
        0 => frac * pow2(-24),
        31 if frac == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        e => (1024.0 + frac) * pow2(e - 25),
    };
    sign * magnitude
}

fn pow2(e: i32) -> f64 {
    f64::from_bits(((1023 + e) as u64) << 52)
}

#[cfg(feature = "alloc")]
pub use writer::Value;

#[cfg(feature = "alloc")]
mod writer {
    use alloc::string::String;
    use alloc::vec::Vec;

    /// An owned CBOR value, written in deterministic encoding.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Value {
        /// Unsigned integer.
        Uint(u64),
        /// Signed integer; non-negative values are written as unsigned.
        Int(i64),
        /// 64-bit float.
        Float(f64),
        /// Boolean.
        Bool(bool),
        /// `null`.
        Null,
        /// Text string.
        Text(String),
        /// Byte string.
        Bytes(Vec<u8>),
        /// Array.
        Array(Vec<Value>),
        /// Map with text keys; the order given does not matter.
        Map(Vec<(String, Value)>),
    }

    fn write_head(out: &mut Vec<u8>, major: u8, arg: u64) {
        let m = major << 5;
        if arg < 24 {
            out.push(m | arg as u8);
        } else if arg <= 0xff {
            out.extend_from_slice(&[m | 24, arg as u8]);
        } else if arg <= 0xffff {
            out.push(m | 25);
            out.extend_from_slice(&(arg as u16).to_be_bytes());
        } else if arg <= 0xffff_ffff {
            out.push(m | 26);
            out.extend_from_slice(&(arg as u32).to_be_bytes());
        } else {
            out.push(m | 27);
            out.extend_from_slice(&arg.to_be_bytes());
        }
    }

    impl Value {
        /// A text value.
        pub fn text(text: &str) -> Self {
            Self::Text(String::from(text))
        }

        /// A map from `(key, value)` pairs.
        pub fn map<const N: usize>(entries: [(&str, Value); N]) -> Self {
            Self::Map(
                entries
                    .into_iter()
                    .map(|(k, v)| (String::from(k), v))
                    .collect(),
            )
        }

        /// The deterministic encoding.
        pub fn encode(&self) -> Vec<u8> {
            let mut out = Vec::new();
            self.encode_into(&mut out);
            out
        }

        /// Appends the deterministic encoding to `out`.
        pub fn encode_into(&self, out: &mut Vec<u8>) {
            match self {
                Self::Uint(n) => write_head(out, 0, *n),
                Self::Int(n) if *n >= 0 => write_head(out, 0, *n as u64),
                Self::Int(n) => write_head(out, 1, (-1 - *n) as u64),
                Self::Float(x) => {
                    out.push(0xfb);
                    out.extend_from_slice(&x.to_bits().to_be_bytes());
                }
                Self::Bool(false) => out.push(0xf4),
                Self::Bool(true) => out.push(0xf5),
                Self::Null => out.push(0xf6),
                Self::Text(t) => {
                    write_head(out, 3, t.len() as u64);
                    out.extend_from_slice(t.as_bytes());
                }
                Self::Bytes(b) => {
                    write_head(out, 2, b.len() as u64);
                    out.extend_from_slice(b);
                }
                Self::Array(items) => {
                    write_head(out, 4, items.len() as u64);
                    for item in items {
                        item.encode_into(out);
                    }
                }
                Self::Map(entries) => {
                    let mut encoded: Vec<(Vec<u8>, Vec<u8>)> = entries
                        .iter()
                        .map(|(k, v)| (Value::Text(k.clone()).encode(), v.encode()))
                        .collect();
                    encoded.sort_by(|a, b| a.0.cmp(&b.0));
                    encoded.dedup_by(|a, b| a.0 == b.0);
                    write_head(out, 5, encoded.len() as u64);
                    for (k, v) in encoded {
                        out.extend_from_slice(&k);
                        out.extend_from_slice(&v);
                    }
                }
            }
        }
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn maps_are_sorted_by_encoded_key_and_round_trip() {
        let v = Value::map([
            ("fc", Value::Uint(4)),
            ("at", Value::Uint(300)),
            ("word_order", Value::text("big")),
            ("neg", Value::Int(-5)),
            ("x", Value::Float(1.5)),
            ("on", Value::Bool(true)),
        ]);
        let bytes = v.encode();
        let c = Cbor::new(&bytes);
        c.validate().unwrap();
        let keys: vec::Vec<&str> = c.entries().unwrap().map(|e| e.unwrap().0).collect();
        assert_eq!(keys, ["x", "at", "fc", "on", "neg", "word_order"]);
        assert_eq!(c.get("at").unwrap().unwrap().as_u64(), Ok(300));
        assert_eq!(c.get("neg").unwrap().unwrap().as_i64(), Ok(-5));
        assert_eq!(c.get("x").unwrap().unwrap().as_f64(), Ok(1.5));
        assert_eq!(c.get("on").unwrap().unwrap().as_bool(), Ok(true));
        assert_eq!(c.get("word_order").unwrap().unwrap().as_str(), Ok("big"));
        assert_eq!(c.get("missing").unwrap(), None);
        assert_eq!(v.encode(), bytes);
    }

    #[test]
    fn known_encodings() {
        assert_eq!(Value::Uint(23).encode(), [0x17]);
        assert_eq!(Value::Uint(24).encode(), [0x18, 24]);
        assert_eq!(Value::Uint(1000).encode(), [0x19, 0x03, 0xe8]);
        assert_eq!(Value::Int(-1).encode(), [0x20]);
        assert_eq!(Value::text("a").encode(), [0x61, b'a']);
        assert_eq!(Value::Map(vec![]).encode(), Cbor::EMPTY_MAP.as_bytes());
        assert_eq!(Cbor::new(&[0xf9, 0x3c, 0x00]).as_f64(), Ok(1.0));
        assert_eq!(Cbor::new(&[0xfa, 0x3f, 0xc0, 0, 0]).as_f64(), Ok(1.5));
    }

    #[test]
    fn errors_are_reported_without_panicking() {
        assert_eq!(Cbor::new(&[0x19, 0x03]).as_u64(), Err(CborError::Truncated));
        assert_eq!(Cbor::new(&[0x61]).as_str(), Err(CborError::Truncated));
        assert_eq!(
            Cbor::new(&[0xc0, 0]).validate(),
            Err(CborError::Unsupported)
        );
        assert_eq!(Cbor::new(&[0x9f]).validate(), Err(CborError::Unsupported));
        assert_eq!(Cbor::new(&[0x61, 0xff]).as_str(), Err(CborError::Utf8));
        assert_eq!(Cbor::new(&[0x01]).as_str(), Err(CborError::Type));
        assert_eq!(Cbor::new(&[0x20]).as_u64(), Err(CborError::Range));
        let deep = [0x81u8; 40];
        assert_eq!(Cbor::new(&deep).validate(), Err(CborError::Depth));
        assert!(Cbor::new(&[]).is_null());
        assert_eq!(Cbor::new(&[]).get("a"), Ok(None));
        let arr = Value::Array(vec![Value::Uint(1), Value::Bool(false)]).encode();
        let items: vec::Vec<_> = Cbor::new(&arr).array().unwrap().collect();
        assert_eq!(items.len(), 2);
    }
}
