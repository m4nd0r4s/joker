//! ULIDs for requests, tickets, traces and decisions (ref/00 §5).

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A ULID from a time in ns: 48 bits of milliseconds, then 80 bits from the
/// sub-millisecond time and a process counter (monotonic within a process).
pub fn ulid(nanos: u128) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ms = (nanos / 1_000_000) as u64;
    let low = ((nanos % 1_000_000) << 48)
        | u128::from(COUNTER.fetch_add(1, Ordering::Relaxed) & 0xFFFF_FFFF) << 16
        | u128::from(std::process::id() as u16);
    ulid_from(ms, low)
}

/// A ULID from its parts: a millisecond time and 80 bits of entropy. The
/// same parts give the same ULID, for ids derived from stored records.
pub fn ulid_from(ms: u64, entropy: u128) -> String {
    let value: u128 = (u128::from(ms & 0xFFFF_FFFF_FFFF) << 80) | (entropy & ((1u128 << 80) - 1));
    (0..26)
        .rev()
        .map(|i| CROCKFORD[((value >> (i * 5)) & 0x1F) as usize] as char)
        .collect()
}
