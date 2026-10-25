//! Checksummed frames: `len: u32 LE`, `crc32(payload): u32 LE`, `payload`.
//! The WAL, the series catalog and the segment indexes are sequences of
//! frames; a torn or corrupt frame ends the valid prefix of a file.

use crate::crc::crc32;

/// Bytes of a frame header.
pub const HEADER: usize = 8;
/// The largest payload a frame may carry.
pub const MAX_PAYLOAD: usize = 64 << 20;

/// Appends one frame holding `payload` to `out`.
pub fn put(out: &mut Vec<u8>, payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32(payload).to_le_bytes());
    out.extend_from_slice(payload);
}

/// The payloads of the valid frames at the start of `bytes`, and the length
/// of that valid prefix.
pub fn split(bytes: &[u8]) -> (Vec<&[u8]>, usize) {
    let mut frames = Vec::new();
    let mut at = 0;
    while let Some(header) = bytes.get(at..at + HEADER) {
        let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let crc = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        if len > MAX_PAYLOAD {
            break;
        }
        let Some(payload) = bytes.get(at + HEADER..at + HEADER + len) else {
            break;
        };
        if crc32(payload) != crc {
            break;
        }
        frames.push(payload);
        at += HEADER + len;
    }
    (frames, at)
}

/// A cursor over the fields of a payload.
#[derive(Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    /// Reads `bytes` from the start.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// True when every byte was read.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The next `n` bytes.
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.bytes.len() < n {
            return None;
        }
        let (head, tail) = self.bytes.split_at(n);
        self.bytes = tail;
        Some(head)
    }

    /// A byte.
    pub fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    /// A little-endian `u16`.
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    /// A little-endian `u64`.
    pub fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    /// A little-endian `i64`.
    pub fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    /// An optional `i64`: a flag byte, then the value when the flag is 1.
    pub fn opt_i64(&mut self) -> Option<Option<i64>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.i64()?)),
            _ => None,
        }
    }

    /// Bytes preceded by their `u32` length.
    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}

/// Appends an optional `i64` in the form [`Reader::opt_i64`] reads.
pub fn put_opt_i64(out: &mut Vec<u8>, value: Option<i64>) {
    match value {
        None => out.push(0),
        Some(v) => {
            out.push(1);
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
}

/// Appends bytes preceded by their `u32` length.
pub fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_torn_tail_ends_the_valid_prefix() {
        let mut out = Vec::new();
        put(&mut out, b"one");
        put(&mut out, b"two");
        let good = out.len();
        put(&mut out, b"three");
        out.truncate(out.len() - 2);
        let (frames, valid) = split(&out);
        assert_eq!(frames, vec![&b"one"[..], &b"two"[..]]);
        assert_eq!(valid, good);
    }

    #[test]
    fn a_flipped_bit_ends_the_valid_prefix() {
        let mut out = Vec::new();
        put(&mut out, b"one");
        let good = out.len();
        put(&mut out, b"two");
        let last = out.len() - 1;
        out[last] ^= 1;
        assert_eq!(split(&out).1, good);
    }
}
