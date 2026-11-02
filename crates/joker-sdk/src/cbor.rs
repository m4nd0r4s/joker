//! Deterministic CBOR (RFC 8949 §4.2.1; ref/00 §7) without allocation: the
//! encoding of records, events and persisted state (ref/04 §4.10).

const UINT: u8 = 0;
const NINT: u8 = 1;
const BYTES: u8 = 2;
const TEXT: u8 = 3;
const ARRAY: u8 = 4;
const MAP: u8 = 5;
const SIMPLE: u8 = 7;

/// The most bytes the header of an item takes.
pub const HEADER_MAX: usize = 9;

/// The bytes the header of an item with argument `n` takes.
pub const fn header_len(n: u64) -> usize {
    if n < 24 {
        1
    } else if n <= 0xff {
        2
    } else if n <= 0xffff {
        3
    } else if n <= 0xffff_ffff {
        5
    } else {
        9
    }
}

/// Writes CBOR into a fixed buffer. When an item does not fit, the writer
/// stops and remembers it: [`Writer::finish`] then returns `None`.
#[derive(Debug)]
pub struct Writer<'a> {
    buf: &'a mut [u8],
    len: usize,
    full: bool,
}

impl<'a> Writer<'a> {
    /// A writer over `buf`.
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            len: 0,
            full: false,
        }
    }

    /// The length written, or `None` if something did not fit.
    pub fn finish(&self) -> Option<usize> {
        (!self.full).then_some(self.len)
    }

    fn put(&mut self, bytes: &[u8]) {
        let end = self.len + bytes.len();
        match self.buf.get_mut(self.len..end) {
            Some(dst) if !self.full => {
                dst.copy_from_slice(bytes);
                self.len = end;
            }
            _ => self.full = true,
        }
    }

    fn head(&mut self, major: u8, n: u64) {
        let m = major << 5;
        match header_len(n) {
            1 => self.put(&[m | n as u8]),
            2 => self.put(&[m | 24, n as u8]),
            3 => {
                let b = (n as u16).to_be_bytes();
                self.put(&[m | 25, b[0], b[1]]);
            }
            5 => {
                let b = (n as u32).to_be_bytes();
                self.put(&[m | 26, b[0], b[1], b[2], b[3]]);
            }
            _ => {
                let b = n.to_be_bytes();
                self.put(&[m | 27, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
            }
        }
    }

    /// An unsigned integer.
    pub fn uint(&mut self, n: u64) {
        self.head(UINT, n);
    }

    /// A signed integer.
    pub fn int(&mut self, n: i64) {
        if n >= 0 {
            self.head(UINT, n as u64);
        } else {
            self.head(NINT, !(n as u64));
        }
    }

    /// A boolean.
    pub fn bool(&mut self, b: bool) {
        self.put(&[(SIMPLE << 5) | if b { 21 } else { 20 }]);
    }

    /// A float, in the shortest form that keeps the value (RFC 8949
    /// §4.2.2); every NaN is `0xf97e00`.
    pub fn float(&mut self, v: f64) {
        if let Some(h) = f16_exact(v) {
            let b = h.to_be_bytes();
            self.put(&[0xf9, b[0], b[1]]);
        } else if f64::from(v as f32) == v {
            let b = (v as f32).to_bits().to_be_bytes();
            self.put(&[0xfa, b[0], b[1], b[2], b[3]]);
        } else {
            let b = v.to_bits().to_be_bytes();
            self.put(&[0xfb, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        }
    }

    /// A text string.
    pub fn text(&mut self, s: &str) {
        self.head(TEXT, s.len() as u64);
        self.put(s.as_bytes());
    }

    /// A byte string.
    pub fn bytes(&mut self, b: &[u8]) {
        self.head(BYTES, b.len() as u64);
        self.put(b);
    }

    /// The header of an array of `n` items.
    pub fn array(&mut self, n: usize) {
        self.head(ARRAY, n as u64);
    }

    /// The header of a map of `n` pairs.
    pub fn map(&mut self, n: usize) {
        self.head(MAP, n as u64);
    }
}

fn f16_exact(v: f64) -> Option<u16> {
    if v.is_nan() {
        return Some(0x7e00);
    }
    let sign: u16 = if v.is_sign_negative() { 0x8000 } else { 0 };
    let a = v.abs();
    if a == 0.0 {
        return Some(sign);
    }
    if a.is_infinite() {
        return Some(sign | 0x7c00);
    }
    let bits = a.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32 - 1023;
    let frac = bits & ((1u64 << 52) - 1);
    if (-14..=15).contains(&exp) {
        if frac & ((1u64 << 42) - 1) != 0 {
            return None;
        }
        Some(sign | (((exp + 15) as u16) << 10) | (frac >> 42) as u16)
    } else if (-24..-14).contains(&exp) {
        let mantissa = (1u64 << 52) | frac;
        let shift = 42 + (-14 - exp) as u32;
        if mantissa & ((1u64 << shift) - 1) != 0 {
            return None;
        }
        Some(sign | (mantissa >> shift) as u16)
    } else {
        None
    }
}

fn f16_value(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f64::from(h & 0x3ff);
    sign * match exp {
        0 => frac * pow2(-24),
        31 if frac == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        e => (1024.0 + frac) * pow2(e - 25),
    }
}

fn pow2(e: i32) -> f64 {
    f64::from_bits(((1023 + e) as u64) << 52)
}

/// Reads CBOR from a slice. Every method returns `None` on a malformed or
/// unexpected item.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// A reader over `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, at: 0 }
    }

    /// True when everything was read.
    pub fn is_empty(&self) -> bool {
        self.at >= self.buf.len()
    }

    fn byte(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.buf.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn peek_major(&self) -> Option<u8> {
        self.buf.get(self.at).map(|b| b >> 5)
    }

    fn head(&mut self) -> Option<(u8, u8, u64)> {
        let b = self.byte()?;
        let (major, info) = (b >> 5, b & 0x1f);
        let n = match info {
            0..=23 => u64::from(info),
            24 => u64::from(self.byte()?),
            25 => u64::from(u16::from_be_bytes(self.take(2)?.try_into().ok()?)),
            26 => u64::from(u32::from_be_bytes(self.take(4)?.try_into().ok()?)),
            27 => u64::from_be_bytes(self.take(8)?.try_into().ok()?),
            _ => return None,
        };
        Some((major, info, n))
    }

    fn expect(&mut self, major: u8) -> Option<u64> {
        let (m, _, n) = self.head()?;
        (m == major).then_some(n)
    }

    /// An unsigned integer.
    pub fn uint(&mut self) -> Option<u64> {
        self.expect(UINT)
    }

    /// A signed integer.
    pub fn int(&mut self) -> Option<i64> {
        let (m, _, n) = self.head()?;
        match m {
            UINT => i64::try_from(n).ok(),
            NINT => i64::try_from(n).ok().map(|n| -1 - n),
            _ => None,
        }
    }

    /// A boolean.
    pub fn bool(&mut self) -> Option<bool> {
        match self.byte()? {
            0xf4 => Some(false),
            0xf5 => Some(true),
            _ => None,
        }
    }

    /// A float, from any of the three widths, or an integer.
    pub fn float(&mut self) -> Option<f64> {
        match self.peek_major()? {
            UINT | NINT => return self.int().map(|n| n as f64),
            _ => {}
        }
        match self.byte()? {
            0xf9 => Some(f16_value(u16::from_be_bytes(
                self.take(2)?.try_into().ok()?,
            ))),
            0xfa => Some(f64::from(f32::from_bits(u32::from_be_bytes(
                self.take(4)?.try_into().ok()?,
            )))),
            0xfb => Some(f64::from_bits(u64::from_be_bytes(
                self.take(8)?.try_into().ok()?,
            ))),
            _ => None,
        }
    }

    /// A text string.
    pub fn text(&mut self) -> Option<&'a str> {
        let n = usize::try_from(self.expect(TEXT)?).ok()?;
        core::str::from_utf8(self.take(n)?).ok()
    }

    /// A byte string.
    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.expect(BYTES)?).ok()?;
        self.take(n)
    }

    /// The header of an array: its length.
    pub fn array(&mut self) -> Option<usize> {
        usize::try_from(self.expect(ARRAY)?).ok()
    }

    /// The header of a map: its number of pairs.
    pub fn map(&mut self) -> Option<usize> {
        usize::try_from(self.expect(MAP)?).ok()
    }

    /// Skips one item, with everything inside it.
    pub fn skip(&mut self) -> Option<()> {
        self.skip_depth(0)
    }

    fn skip_depth(&mut self, depth: u32) -> Option<()> {
        if depth > 32 {
            return None;
        }
        let (major, info, n) = self.head()?;
        match major {
            UINT | NINT => {}
            BYTES | TEXT => {
                self.take(usize::try_from(n).ok()?)?;
            }
            ARRAY => {
                for _ in 0..n {
                    self.skip_depth(depth + 1)?;
                }
            }
            MAP => {
                for _ in 0..n.checked_mul(2)? {
                    self.skip_depth(depth + 1)?;
                }
            }
            6 => self.skip_depth(depth + 1)?,
            _ if info <= 27 => {}
            _ => return None,
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(f: impl FnOnce(&mut Writer<'_>)) -> std::vec::Vec<u8> {
        let mut buf = [0u8; 64];
        let mut w = Writer::new(&mut buf);
        f(&mut w);
        let n = w.finish().unwrap();
        buf[..n].to_vec()
    }

    #[test]
    fn integers_and_floats_take_their_shortest_form() {
        assert_eq!(enc(|w| w.uint(23)), [0x17]);
        assert_eq!(enc(|w| w.uint(24)), [0x18, 24]);
        assert_eq!(enc(|w| w.int(-1)), [0x20]);
        assert_eq!(enc(|w| w.int(-500)), [0x39, 0x01, 0xf3]);
        assert_eq!(enc(|w| w.float(400.0)), [0xf9, 0x5e, 0x40]);
        assert_eq!(enc(|w| w.float(0.5)), [0xf9, 0x38, 0x00]);
        assert_eq!(enc(|w| w.float(f64::NAN)), [0xf9, 0x7e, 0x00]);
        assert_eq!(enc(|w| w.float(100_000.0)), [0xfa, 0x47, 0xc3, 0x50, 0x00]);
        assert_eq!(enc(|w| w.float(0.62)).len(), 9);
        assert_eq!(enc(|w| w.float(5.960464477539063e-8)), [0xf9, 0x00, 0x01]);
        for v in [400.0, 0.62, -3.25, 1e300, 5.960464477539063e-8, 65504.0] {
            let b = enc(|w| w.float(v));
            assert_eq!(Reader::new(&b).float(), Some(v));
        }
    }

    #[test]
    fn a_reader_skips_nested_items_and_refuses_truncated_ones() {
        let b = enc(|w| {
            w.map(2);
            w.uint(0);
            w.array(2);
            w.text("ab");
            w.bytes(&[1, 2]);
            w.uint(1);
            w.int(-7);
        });
        let mut r = Reader::new(&b);
        r.skip().unwrap();
        assert!(r.is_empty());
        assert_eq!(Reader::new(&b[..b.len() - 1]).skip(), None);
    }

    #[test]
    fn a_writer_that_runs_out_of_room_says_so() {
        let mut buf = [0u8; 2];
        let mut w = Writer::new(&mut buf);
        w.text("abc");
        assert_eq!(w.finish(), None);
    }
}
