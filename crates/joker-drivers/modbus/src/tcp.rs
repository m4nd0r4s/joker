//! Modbus TCP framing: the MBAP header (application protocol v1.1b3 §4.1),
//! wrapping the same PDU shape as `modbus-rtu`'s frames, minus the CRC
//! (TCP already guarantees byte integrity).

/// MBAP header (7 bytes) + 253-byte PDU.
pub const MAX_ADU: usize = 260;

/// An MBAP-framed request: transaction id, unit, function, two 16-bit
/// words. No CRC. The length field is 6: the unit byte plus a 5-byte PDU
/// (function and two words).
pub fn request(transaction: u16, unit: u8, function: u8, a: u16, b: u16) -> [u8; 12] {
    let [t0, t1] = transaction.to_be_bytes();
    let [a0, a1] = a.to_be_bytes();
    let [b0, b1] = b.to_be_bytes();
    let [l0, l1] = 6u16.to_be_bytes();
    [t0, t1, 0, 0, l0, l1, unit, function, a0, a1, b0, b1]
}

/// An MBAP frame of any request PDU: transaction id, unit, PDU. Returns
/// the buffer and the length of the frame in it.
pub fn frame(transaction: u16, unit: u8, pdu: &[u8]) -> ([u8; MAX_ADU], usize) {
    let mut out = [0u8; MAX_ADU];
    let n = pdu.len().min(MAX_ADU - 7);
    let [t0, t1] = transaction.to_be_bytes();
    let [l0, l1] = (n as u16 + 1).to_be_bytes();
    out[..7].copy_from_slice(&[t0, t1, 0, 0, l0, l1, unit]);
    out[7..7 + n].copy_from_slice(&pdu[..n]);
    (out, 7 + n)
}

/// The MBAP header's `length` field: how many bytes follow it (unit +
/// PDU).
pub fn header_length(frame: &[u8]) -> Option<u16> {
    (frame.len() >= 6).then(|| u16::from_be_bytes([frame[4], frame[5]]))
}

/// The transaction id a response's header carries.
pub fn transaction_of(frame: &[u8]) -> Option<u16> {
    (frame.len() >= 2).then(|| u16::from_be_bytes([frame[0], frame[1]]))
}

/// Data bytes of a read response for `count` items of `function` (same
/// table as RTU: the PDU shape does not change).
pub fn read_data_len(function: u8, count: u16) -> usize {
    crate::rtu::read_data_len(function, count)
}

/// Wire bytes of the full MBAP frame (header + PDU) of a read response of
/// `count` items.
pub fn read_response_len(function: u8, count: u16) -> usize {
    7 + 2 + read_data_len(function, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_holding_registers_request() {
        assert_eq!(
            request(1, 1, 3, 0, 1),
            [
                0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01, 0x03, 0x00, 0x00, 0x00, 0x01
            ]
        );
    }

    #[test]
    fn a_frame_of_a_pdu_is_the_same_as_a_request() {
        let (f, n) = frame(1, 1, &[3, 0, 0, 0, 1]);
        assert_eq!(f[..n], request(1, 1, 3, 0, 1));
    }

    #[test]
    fn header_length_and_transaction_are_read_from_the_frame() {
        let frame = request(7, 1, 3, 0, 1);
        assert_eq!(transaction_of(&frame), Some(7));
        assert_eq!(header_length(&frame), Some(6));
        assert_eq!(header_length(&[1]), None);
    }

    /// Header (6), unit (1), function (1), byte count (1) and two
    /// registers (4) make 13 bytes.
    #[test]
    fn read_response_len_covers_header_unit_and_pdu() {
        assert_eq!(read_response_len(3, 2), 13);
    }
}
