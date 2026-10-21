//! The zstd segments of the `.jktrace` files that Linux writes (docs/16 §7,
//! ADR 0037). `joker-core` writes and reads segments uncompressed, as on an
//! MCU; `jokerd` and `jk` compress them, so zstd stays out of the core
//! (policies/dependencies.md §3).

use joker_core::trace::{SEGMENT_HEADER_LEN, TraceError};

/// Compresses the ticks of a [`joker_core::trace::SegmentWriter`] (or a state
/// payload) into a complete segment of `group` with `n_ticks` entries.
pub fn compress_segment(group: u32, n_ticks: u32, payload: &[u8]) -> Result<Vec<u8>, TraceError> {
    let frame = zstd::bulk::compress(payload, 3).map_err(|_| TraceError::Malformed)?;
    let len = u32::try_from(frame.len()).map_err(|_| TraceError::TooMany)?;
    let mut out = Vec::with_capacity(SEGMENT_HEADER_LEN + frame.len());
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&n_ticks.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&frame);
    Ok(out)
}

/// Decompresses a stored segment payload.
pub fn decompress(payload: &[u8]) -> Result<Vec<u8>, TraceError> {
    zstd::stream::decode_all(payload).map_err(|_| TraceError::Malformed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use joker_core::trace::{Item, Items, SegmentWriter, TickRecord, Ticks};
    use joker_core::{Instant, Sample};

    use super::{compress_segment, decompress};

    #[test]
    fn zstd_round_trip() {
        let mut buf = [0u8; 4096];
        let mut w = SegmentWriter::new(2, &mut buf).unwrap();
        let inputs: [Sample; 0] = [];
        for k in 0..50u64 {
            w.push(&TickRecord {
                index: k,
                at: Instant::mono(k * 20_000_000),
                inputs: &inputs,
                proposals: &[],
                results: &[],
                methods: &[],
            })
            .unwrap();
        }
        let segment = compress_segment(2, w.ticks(), w.payload()).unwrap();
        let Some(Ok(Item::Ticks {
            n_ticks, payload, ..
        })) = Items::new(&segment).next()
        else {
            panic!("no segment");
        };
        let raw = decompress(payload).unwrap();
        assert_eq!(raw, w.payload());
        assert_eq!(Ticks::new(&raw, n_ticks, 0).count(), 50);
    }
}
