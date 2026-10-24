//! Modbus RTU framing: CRC-16, requests and the length of each response
//! (Modbus over serial line v1.02 §2.5; application protocol v1.1b3 §6).

/// Largest RTU frame: address, 253-byte PDU and CRC.
pub const MAX_ADU: usize = 256;

/// Computes the Modbus CRC-16 (polynomial 0xA001, initial value 0xFFFF).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// True when the last two bytes of `frame` are the CRC of the rest.
pub fn crc_ok(frame: &[u8]) -> bool {
    frame.len() >= 4 && {
        let (body, crc) = frame.split_at(frame.len() - 2);
        crc16(body).to_le_bytes() == [crc[0], crc[1]]
    }
}

/// An 8-byte request: unit, function, two 16-bit words, CRC.
pub fn request(unit: u8, function: u8, a: u16, b: u16) -> [u8; 8] {
    let [a0, a1] = a.to_be_bytes();
    let [b0, b1] = b.to_be_bytes();
    let mut frame = [unit, function, a0, a1, b0, b1, 0, 0];
    let [c0, c1] = crc16(&frame[..6]).to_le_bytes();
    frame[6] = c0;
    frame[7] = c1;
    frame
}

/// An RTU frame of any request PDU: unit, PDU, CRC. Returns the buffer
/// and the length of the frame in it.
pub fn frame(unit: u8, pdu: &[u8]) -> ([u8; MAX_ADU], usize) {
    let mut out = [0u8; MAX_ADU];
    let n = pdu.len().min(MAX_ADU - 3);
    out[0] = unit;
    out[1..1 + n].copy_from_slice(&pdu[..n]);
    let [c0, c1] = crc16(&out[..1 + n]).to_le_bytes();
    out[1 + n] = c0;
    out[2 + n] = c1;
    (out, n + 3)
}

/// Data bytes of a read response for `count` items of `function`.
pub fn read_data_len(function: u8, count: u16) -> usize {
    match function {
        1 | 2 => usize::from(count.div_ceil(8)),
        _ => 2 * usize::from(count),
    }
}

/// Wire bytes of the response to a read of `count` items.
pub fn read_response_len(function: u8, count: u16) -> usize {
    5 + read_data_len(function, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_of_the_specification_examples() {
        assert_eq!(crc16(&[0x02, 0x07]), 0x1241);
        assert_eq!(
            request(1, 3, 0, 1),
            [0x01, 0x03, 0x00, 0x00, 0x00, 0x01, 0x84, 0x0A]
        );
        assert!(crc_ok(&request(1, 4, 0, 4)));
        let (f, n) = frame(1, &[3, 0, 0, 0, 1]);
        assert_eq!(f[..n], request(1, 3, 0, 1));
        assert!(!crc_ok(&[1, 3, 0]));
        assert_eq!(read_response_len(1, 9), 7);
        assert_eq!(read_response_len(4, 4), 13);
    }
}
