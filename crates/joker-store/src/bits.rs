//! Bit streams for the Gorilla encoding of blocks.

/// Appends bits, most significant first.
#[derive(Debug, Clone, Default)]
pub struct BitWriter {
    bytes: Vec<u8>,
    bits: u64,
}

impl BitWriter {
    /// Bits written so far.
    pub fn len_bits(&self) -> u64 {
        self.bits
    }

    /// Appends one bit.
    pub fn bit(&mut self, bit: bool) {
        let used = (self.bits % 8) as u8;
        if used == 0 {
            self.bytes.push(0);
        }
        if bit && let Some(last) = self.bytes.last_mut() {
            *last |= 0x80 >> used;
        }
        self.bits += 1;
    }

    /// Appends the low `n` bits of `value` (`n` ≤ 64).
    pub fn bits(&mut self, value: u64, n: u32) {
        let mut left = n;
        while left > 0 {
            let used = (self.bits % 8) as u32;
            if used == 0 {
                self.bytes.push(0);
            }
            let room = 8 - used;
            let take = room.min(left);
            let chunk = ((value >> (left - take)) & ((1u64 << take) - 1)) as u8;
            if let Some(last) = self.bytes.last_mut() {
                *last |= chunk << (room - take);
            }
            self.bits += u64::from(take);
            left -= take;
        }
    }

    /// The bytes; the last one is padded with zero bits.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Reads bits written by [`BitWriter`].
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    pos: u64,
    end: u64,
}

impl<'a> BitReader<'a> {
    /// Reads at most `bits` bits of `bytes`.
    pub fn new(bytes: &'a [u8], bits: u64) -> Self {
        Self {
            bytes,
            pos: 0,
            end: bits.min(bytes.len() as u64 * 8),
        }
    }

    /// One bit, or `None` at the end.
    pub fn bit(&mut self) -> Option<bool> {
        if self.pos >= self.end {
            return None;
        }
        let byte = *self.bytes.get((self.pos / 8) as usize)?;
        let bit = byte & (0x80 >> (self.pos % 8)) != 0;
        self.pos += 1;
        Some(bit)
    }

    /// `n` bits (`n` ≤ 64), or `None` past the end.
    pub fn bits(&mut self, n: u32) -> Option<u64> {
        if self.pos + u64::from(n) > self.end {
            return None;
        }
        let mut value = 0u64;
        let mut left = n;
        while left > 0 {
            let used = (self.pos % 8) as u32;
            let byte = *self.bytes.get((self.pos / 8) as usize)?;
            let room = 8 - used;
            let take = room.min(left);
            let chunk = (u64::from(byte) >> (room - take)) & ((1u64 << take) - 1);
            value = (value << take) | chunk;
            self.pos += u64::from(take);
            left -= take;
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_mixed_widths() {
        let mut w = BitWriter::default();
        w.bit(true);
        w.bits(0b1011, 4);
        w.bits(u64::MAX, 64);
        w.bit(false);
        w.bits(0x1234_5678, 32);
        let mut r = BitReader::new(w.as_bytes(), w.len_bits());
        assert_eq!(r.bit(), Some(true));
        assert_eq!(r.bits(4), Some(0b1011));
        assert_eq!(r.bits(64), Some(u64::MAX));
        assert_eq!(r.bit(), Some(false));
        assert_eq!(r.bits(32), Some(0x1234_5678));
        assert_eq!(r.bit(), None);
    }
}
