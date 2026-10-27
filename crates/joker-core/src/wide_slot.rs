//! The wide (<=64 byte) last-value slot with its seqlock (docs/02-signal-model.md
//! §2, docs/04-sdk.md's `Internal<T>`).
//!
//! Same seqlock algorithm as [`crate::slot::Slot`], deliberately not a
//! bigger `Slot`: every scalar signal keeps paying zero cost for a
//! feature only `bytes`/`string` signals use.

use crate::quality::{Quality, QualityCode, QualityReason};
use crate::slot::Collision;
use crate::slot::{hi, join, lo};
use crate::sync::{AtomicU32, Ordering, fence};
use crate::value::NO_TS_SRC;

/// Maximum payload bytes a [`WideSlot`] holds (docs/02-signal-model.md §2:
/// `bytes`/`string` are each <=64 bytes).
pub const WIDE_SLOT_CAPACITY: usize = 64;
const PAYLOAD_WORDS: usize = WIDE_SLOT_CAPACITY / 4;

/// A last-value slot for a `bytes`/`string` signal, up to
/// [`WIDE_SLOT_CAPACITY`] bytes: single writer, any number of wait-free
/// readers, same seqlock shape as [`crate::slot::Slot`].
#[derive(Debug)]
#[repr(C, align(32))]
pub struct WideSlot {
    version: AtomicU32,
    quality: AtomicU32,
    ts_src_lo: AtomicU32,
    ts_src_hi: AtomicU32,
    ts_acq_lo: AtomicU32,
    ts_acq_hi: AtomicU32,
    len: AtomicU32,
    payload: [AtomicU32; PAYLOAD_WORDS],
}

/// A read sample of a [`WideSlot`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WideSample {
    bytes: [u8; WIDE_SLOT_CAPACITY],
    len: u8,
    quality: Quality,
    ts_src: i64,
    ts_acq: u64,
    seq: u64,
}

impl WideSample {
    /// The sample of a slot that never received a value:
    /// `uncertain(initial)`, zero bytes, sequence 0.
    pub const INITIAL: Self = Self {
        bytes: [0; WIDE_SLOT_CAPACITY],
        len: 0,
        quality: Quality::INITIAL,
        ts_src: NO_TS_SRC,
        ts_acq: 0,
        seq: 0,
    };

    /// The bytes actually written (`len` of them, never more).
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// The quality.
    pub const fn quality(&self) -> Quality {
        self.quality
    }

    /// The raw `ts_src` field ([`NO_TS_SRC`] when absent).
    pub const fn ts_src_raw(&self) -> i64 {
        self.ts_src
    }

    /// Monotonic ns of acquisition.
    pub const fn ts_acq(&self) -> u64 {
        self.ts_acq
    }

    /// The slot's sequence number at the time of this read (0 means
    /// never written).
    pub const fn seq(&self) -> u64 {
        self.seq
    }
}

impl Default for WideSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl WideSlot {
    /// An empty slot: sequence 0, zero bytes, `uncertain(initial)`.
    pub fn new() -> Self {
        Self {
            version: AtomicU32::new(0),
            quality: AtomicU32::new(Quality::INITIAL.to_bits()),
            ts_src_lo: AtomicU32::new(lo(NO_TS_SRC as u64)),
            ts_src_hi: AtomicU32::new(hi(NO_TS_SRC as u64)),
            ts_acq_lo: AtomicU32::new(0),
            ts_acq_hi: AtomicU32::new(0),
            len: AtomicU32::new(0),
            payload: core::array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    /// Stores a new sample and returns its 31-bit sequence number.
    /// `bytes` longer than [`WIDE_SLOT_CAPACITY`] is never truncated: it
    /// stores zero bytes with `bad(config_error)` (ADR 0062, decision 3).
    /// Producers check the length first; this is a last line of defence.
    ///
    /// Only the slot's single writer may call this (docs/01 §3.2's own
    /// rule for [`crate::slot::Slot::write`] applies here identically).
    pub fn write(&self, bytes: &[u8], quality: Quality, ts_src: i64, ts_acq: u64) -> u32 {
        let (bytes, quality) = if bytes.len() > WIDE_SLOT_CAPACITY {
            (
                &[][..],
                Quality::new(QualityCode::Bad, QualityReason::ConfigError),
            )
        } else {
            (bytes, quality)
        };
        let len = bytes.len();
        let version = self.version.load(Ordering::Relaxed);
        let busy = version.wrapping_add(1);
        self.version.store(busy, Ordering::Relaxed);
        fence(Ordering::Release);
        self.quality.store(quality.to_bits(), Ordering::Relaxed);
        self.len.store(len as u32, Ordering::Relaxed);
        for (i, word) in self.payload.iter().enumerate() {
            let start = i * 4;
            let mut buf = [0u8; 4];
            if start < len {
                let n = (len - start).min(4);
                buf[..n].copy_from_slice(&bytes[start..start + n]);
            }
            word.store(u32::from_le_bytes(buf), Ordering::Relaxed);
        }
        self.ts_src_lo.store(lo(ts_src as u64), Ordering::Relaxed);
        self.ts_src_hi.store(hi(ts_src as u64), Ordering::Relaxed);
        self.ts_acq_lo.store(lo(ts_acq), Ordering::Relaxed);
        self.ts_acq_hi.store(hi(ts_acq), Ordering::Relaxed);
        let done = busy.wrapping_add(1);
        self.version.store(done, Ordering::Release);
        done >> 1
    }

    /// One read attempt. Returns [`Collision`] if a write was in progress.
    pub fn try_read(&self) -> Result<WideSample, Collision> {
        let before = self.version.load(Ordering::Acquire);
        if before & 1 == 1 {
            return Err(Collision);
        }
        let quality = self.quality.load(Ordering::Relaxed);
        let len = self.len.load(Ordering::Relaxed);
        let mut bytes = [0u8; WIDE_SLOT_CAPACITY];
        for (i, word) in self.payload.iter().enumerate() {
            let w = word.load(Ordering::Relaxed).to_le_bytes();
            let start = i * 4;
            bytes[start..start + 4].copy_from_slice(&w);
        }
        let ts_src_lo = self.ts_src_lo.load(Ordering::Relaxed);
        let ts_src_hi = self.ts_src_hi.load(Ordering::Relaxed);
        let ts_acq_lo = self.ts_acq_lo.load(Ordering::Relaxed);
        let ts_acq_hi = self.ts_acq_hi.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        let after = self.version.load(Ordering::Relaxed);
        if before != after {
            return Err(Collision);
        }
        Ok(WideSample {
            bytes,
            len: len.min(WIDE_SLOT_CAPACITY as u32) as u8,
            quality: Quality::from_bits(quality),
            ts_src: join(ts_src_lo, ts_src_hi) as i64,
            ts_acq: join(ts_acq_lo, ts_acq_hi),
            seq: u64::from(before >> 1),
        })
    }

    /// Reads with at most `retries` extra attempts after the first (same
    /// give-up-on-persistent-collision rule as [`crate::slot::Slot::read`]).
    pub fn read(&self, retries: u32) -> Result<WideSample, Collision> {
        let mut attempt = 0;
        loop {
            match self.try_read() {
                Ok(sample) => return Ok(sample),
                Err(Collision) if attempt < retries => attempt += 1,
                Err(Collision) => return Err(Collision),
            }
        }
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use crate::quality::{QualityCode, QualityReason};

    extern crate std;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    #[test]
    fn wide_slot_is_96_bytes_and_aligned() {
        assert_eq!(core::mem::size_of::<WideSlot>(), 96);
        assert_eq!(core::mem::align_of::<WideSlot>(), 32);
    }

    #[test]
    fn empty_wide_slot_reads_initial() {
        assert_eq!(WideSlot::new().try_read(), Ok(WideSample::INITIAL));
    }

    #[test]
    fn write_then_read() {
        let slot = WideSlot::new();
        let q = Quality::new(QualityCode::Bad, QualityReason::CommFailure);
        assert_eq!(slot.write(b"hi", q, -5, 1 << 40), 1);
        assert_eq!(slot.write(b"hello", Quality::GOOD, NO_TS_SRC, 3), 2);
        let s = slot.read(0).unwrap();
        assert_eq!(s.as_slice(), b"hello");
        assert_eq!(s.quality(), Quality::GOOD);
        assert_eq!(s.ts_src_raw(), NO_TS_SRC);
        assert_eq!(s.ts_acq(), 3);
        assert_eq!(s.seq(), 2);
    }

    #[test]
    fn a_payload_longer_than_64_bytes_is_bad_config_error_not_truncated() {
        let slot = WideSlot::new();
        let too_long = [b'x'; 100];
        slot.write(&too_long, Quality::GOOD, NO_TS_SRC, 0);
        let s = slot.read(0).unwrap();
        assert!(s.as_slice().is_empty());
        assert_eq!(s.quality().code(), QualityCode::Bad);
        assert_eq!(s.quality().reason(), QualityReason::ConfigError);
        assert_eq!(s.seq(), 1);
    }

    #[test]
    fn concurrent_readers_never_see_torn_samples() {
        let slot = Arc::new(WideSlot::new());
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let slot = Arc::clone(&slot);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut k: u8 = 1;
                while !stop.load(core::sync::atomic::Ordering::Relaxed) {
                    let buf = [k; WIDE_SLOT_CAPACITY];
                    slot.write(&buf, Quality::GOOD, 0, u64::from(k));
                    k = k.wrapping_add(1).max(1);
                }
            })
        };
        let mut ok = 0u32;
        for _ in 0..200_000 {
            if let Ok(s) = slot.read(4) {
                if s.seq() == 0 {
                    assert_eq!(s, WideSample::INITIAL);
                    continue;
                }
                let bytes = s.as_slice();
                let k = bytes[0];
                assert!(bytes.iter().all(|&b| b == k), "torn wide sample");
                assert_eq!(s.ts_acq(), u64::from(k));
                ok += 1;
            }
        }
        stop.store(true, core::sync::atomic::Ordering::Relaxed);
        assert!(writer.join().is_ok());
        assert!(ok > 0);
    }
}
