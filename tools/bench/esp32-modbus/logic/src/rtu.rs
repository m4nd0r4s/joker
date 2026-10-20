//! Modbus RTU framing: CRC-16, frame check and the inter-frame silence.

/// Largest RTU frame: address, 253-byte PDU and CRC.
pub const MAX_ADU: usize = 256;

/// Length of the CRC that ends every frame.
pub const CRC_LEN: usize = 2;

/// Silence that ends a frame, in microseconds.
///
/// The Modbus serial line specification fixes the 3.5-character silence at
/// 1750 µs for every baud rate above 19 200.
pub const FRAME_GAP_US: u64 = 1_750;

/// Computes the Modbus CRC-16 (polynomial 0xA001, initial value 0xFFFF).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// Checks a received frame and splits it into unit address and PDU.
///
/// Returns `None` for a frame shorter than address, function code and CRC,
/// or whose CRC does not match.
pub fn open(frame: &[u8]) -> Option<(u8, &[u8])> {
    if frame.len() < 2 + CRC_LEN {
        return None;
    }
    let (body, crc) = frame.split_at(frame.len() - CRC_LEN);
    if crc16(body).to_le_bytes() != [crc[0], crc[1]] {
        return None;
    }
    let (unit, pdu) = body.split_first()?;
    Some((*unit, pdu))
}

/// Appends the CRC to the first `len` bytes of `buf` and returns the frame
/// length.
///
/// `len` is clamped so that the CRC always fits in `buf`.
pub fn seal(buf: &mut [u8; MAX_ADU], len: usize) -> usize {
    let len = len.min(MAX_ADU - CRC_LEN);
    let crc = crc16(&buf[..len]).to_le_bytes();
    buf[len] = crc[0];
    buf[len + 1] = crc[1];
    len + CRC_LEN
}

/// Splits the received byte stream into frames by the inter-frame silence.
///
/// Bytes are pushed as they leave the UART FIFO, stamped with the time they
/// were read. A frame is complete when no byte arrives for
/// [`FRAME_GAP_US`]. A frame longer than [`MAX_ADU`] is discarded whole.
#[derive(Debug, Clone)]
pub struct Framer {
    buf: [u8; MAX_ADU],
    len: usize,
    discard: bool,
    last_byte_us: Option<u64>,
}

impl Default for Framer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framer {
    /// Creates an empty framer.
    pub const fn new() -> Self {
        Self {
            buf: [0; MAX_ADU],
            len: 0,
            discard: false,
            last_byte_us: None,
        }
    }

    /// Appends bytes read at `now_us`.
    ///
    /// If the silence since the previous byte already ended a frame that was
    /// not collected with [`Framer::poll`], that frame is dropped and the new
    /// bytes start a new one.
    pub fn push(&mut self, bytes: &[u8], now_us: u64) {
        if bytes.is_empty() {
            return;
        }
        if self.gap_elapsed(now_us) {
            self.reset();
        }
        for &byte in bytes {
            if self.len < MAX_ADU {
                self.buf[self.len] = byte;
                self.len += 1;
            } else {
                self.discard = true;
            }
        }
        self.last_byte_us = Some(now_us);
    }

    /// Records a receive error (framing, parity or FIFO overflow) at
    /// `now_us`.
    ///
    /// The frame being received is discarded when the silence ends, even if
    /// its CRC happens to match.
    pub fn mark_error(&mut self, now_us: u64) {
        if self.gap_elapsed(now_us) {
            self.reset();
        }
        self.discard = true;
        self.last_byte_us = Some(now_us);
    }

    /// Returns the pending frame if the line has been silent for
    /// [`FRAME_GAP_US`] at `now_us`.
    ///
    /// A returned frame is consumed: the next call returns `None` until new
    /// bytes arrive. An overflowed frame is dropped and never returned.
    pub fn poll(&mut self, now_us: u64) -> Option<&[u8]> {
        if !self.gap_elapsed(now_us) {
            return None;
        }
        let len = self.len;
        let discard = self.discard;
        self.reset();
        if discard || len == 0 {
            return None;
        }
        Some(&self.buf[..len])
    }

    fn gap_elapsed(&self, now_us: u64) -> bool {
        self.last_byte_us
            .is_some_and(|last| now_us.saturating_sub(last) >= FRAME_GAP_US)
    }

    fn reset(&mut self) {
        self.len = 0;
        self.discard = false;
        self.last_byte_us = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_specification_example() {
        assert_eq!(crc16(&[0x02, 0x07]), 0x1241);
        assert_eq!(
            crc16(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x01]).to_le_bytes(),
            [0x84, 0x0A]
        );
    }

    #[test]
    fn seal_then_open_round_trips() {
        let mut buf = [0u8; MAX_ADU];
        buf[..6].copy_from_slice(&[1, 4, 0, 0, 0, 4]);
        let len = seal(&mut buf, 6);
        assert_eq!(len, 8);
        assert_eq!(open(&buf[..len]), Some((1, &[4u8, 0, 0, 0, 4][..])));
    }

    #[test]
    fn open_rejects_short_and_corrupt_frames() {
        assert_eq!(open(&[1, 3, 0]), None);
        assert_eq!(
            open(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x01, 0x84, 0x0B]),
            None
        );
    }

    #[test]
    fn frame_ends_after_the_silence() {
        let mut framer = Framer::new();
        framer.push(&[1, 3, 0], 0);
        framer.push(&[0, 0, 1], 500);
        assert_eq!(framer.poll(2_000), None);
        assert_eq!(framer.poll(2_250), Some(&[1u8, 3, 0, 0, 0, 1][..]));
        assert_eq!(framer.poll(10_000), None);
    }

    #[test]
    fn uncollected_frame_is_replaced_by_the_next() {
        let mut framer = Framer::new();
        framer.push(&[0xAA, 0xBB], 0);
        framer.push(&[1, 2], 5_000);
        assert_eq!(framer.poll(7_000), Some(&[1u8, 2][..]));
    }

    #[test]
    fn overflowed_frame_is_dropped() {
        let mut framer = Framer::new();
        framer.push(&[0x55; MAX_ADU], 0);
        framer.push(&[0x55], 10);
        assert_eq!(framer.poll(5_000), None);
        framer.push(&[1, 2, 3], 6_000);
        assert_eq!(framer.poll(8_000), Some(&[1u8, 2, 3][..]));
    }

    #[test]
    fn receive_error_discards_the_frame() {
        let mut framer = Framer::new();
        framer.push(&[1, 1, 0, 0], 0);
        framer.mark_error(100);
        framer.push(&[0, 1], 200);
        assert_eq!(framer.poll(5_000), None);
        framer.push(&[1, 2], 6_000);
        assert_eq!(framer.poll(8_000), Some(&[1u8, 2][..]));
    }

    #[test]
    fn empty_push_does_not_start_a_frame() {
        let mut framer = Framer::new();
        framer.push(&[], 0);
        assert_eq!(framer.poll(5_000), None);
    }
}
