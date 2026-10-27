//! The 32-byte last-value slot with its seqlock (docs/01 §3.2).
//!
//! Every field is split into 32-bit atomic words so that the slot is exactly
//! 32 bytes, needs no `unsafe` and never falls back to a critical section on
//! MCUs without 64-bit atomics. The seqlock makes the words of one sample
//! consistent; the atomics only rule out data races.

use crate::quality::Quality;
use crate::sync::{AtomicU32, Ordering, fence};
use crate::value::Sample;

/// A last-value slot with a single writer and any number of wait-free readers.
///
/// The version word is odd while a write is in progress; the sequence number of
/// the stored sample is `version / 2`, 31 bits wide, which readers extend to
/// 64 bits with a [`SeqExtender`] kept outside the slot (docs/01 §3.2).
#[derive(Debug)]
#[repr(C, align(32))]
pub struct Slot {
    version: AtomicU32,
    quality: AtomicU32,
    value_lo: AtomicU32,
    value_hi: AtomicU32,
    ts_src_lo: AtomicU32,
    ts_src_hi: AtomicU32,
    ts_acq_lo: AtomicU32,
    ts_acq_hi: AtomicU32,
}

/// A read that met a write in progress on every attempt (docs/01 §3.2).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Collision;

impl Default for Slot {
    fn default() -> Self {
        Self::new()
    }
}

impl Slot {
    /// An empty slot: sequence 0, value 0, `uncertain(initial)` (ref/04 §3.2).
    pub fn new() -> Self {
        let initial = Sample::INITIAL;
        Self {
            version: AtomicU32::new(0),
            quality: AtomicU32::new(initial.quality().to_bits()),
            value_lo: AtomicU32::new(0),
            value_hi: AtomicU32::new(0),
            ts_src_lo: AtomicU32::new(lo(initial.ts_src_raw() as u64)),
            ts_src_hi: AtomicU32::new(hi(initial.ts_src_raw() as u64)),
            ts_acq_lo: AtomicU32::new(0),
            ts_acq_hi: AtomicU32::new(0),
        }
    }

    /// Stores a new sample and returns its 31-bit sequence number.
    ///
    /// Only the slot's single writer may call this (the acquisition stage of
    /// the owning driver, or the owning cycle group). Concurrent writers do not
    /// corrupt memory but may lose samples.
    pub fn write(&self, bits: u64, quality: Quality, ts_src: i64, ts_acq: u64) -> u32 {
        let version = self.version.load(Ordering::Relaxed);
        let busy = version.wrapping_add(1);
        self.version.store(busy, Ordering::Relaxed);
        fence(Ordering::Release);
        self.quality.store(quality.to_bits(), Ordering::Relaxed);
        self.value_lo.store(lo(bits), Ordering::Relaxed);
        self.value_hi.store(hi(bits), Ordering::Relaxed);
        self.ts_src_lo.store(lo(ts_src as u64), Ordering::Relaxed);
        self.ts_src_hi.store(hi(ts_src as u64), Ordering::Relaxed);
        self.ts_acq_lo.store(lo(ts_acq), Ordering::Relaxed);
        self.ts_acq_hi.store(hi(ts_acq), Ordering::Relaxed);
        let done = busy.wrapping_add(1);
        self.version.store(done, Ordering::Release);
        done >> 1
    }

    /// One read attempt. Returns the sample with its 31-bit sequence number in
    /// [`Sample::seq`], or [`Collision`] if a write was in progress.
    pub fn try_read(&self) -> Result<Sample, Collision> {
        let before = self.version.load(Ordering::Acquire);
        if before & 1 == 1 {
            return Err(Collision);
        }
        let quality = self.quality.load(Ordering::Relaxed);
        let value_lo = self.value_lo.load(Ordering::Relaxed);
        let value_hi = self.value_hi.load(Ordering::Relaxed);
        let ts_src_lo = self.ts_src_lo.load(Ordering::Relaxed);
        let ts_src_hi = self.ts_src_hi.load(Ordering::Relaxed);
        let ts_acq_lo = self.ts_acq_lo.load(Ordering::Relaxed);
        let ts_acq_hi = self.ts_acq_hi.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        let after = self.version.load(Ordering::Relaxed);
        if before != after {
            return Err(Collision);
        }
        Ok(Sample::new(
            join(value_lo, value_hi),
            Quality::from_bits(quality),
            join(ts_src_lo, ts_src_hi) as i64,
            join(ts_acq_lo, ts_acq_hi),
            u64::from(before >> 1),
        ))
    }

    /// Reads with at most `retries` extra attempts after the first. Never
    /// waits for the writer: on persistent collision it gives up, and the
    /// caller keeps its previous complete sample and counts the collision
    /// (docs/01 §3.2). A single-core MCU reader passes `retries = 0`
    /// (ref/08 §2.4.1).
    pub fn read(&self, retries: u32) -> Result<Sample, Collision> {
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

/// Extends the 31-bit sequence numbers of a slot (or the 32-bit ones of the
/// trace) to 64 bits, for one reader of one signal (docs/01 §3.2, docs/16 §7).
///
/// Sequence numbers never go backwards, so it assumes that the reader observes
/// the signal at least once per wrap of the truncated counter.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SeqExtender {
    last: u64,
    modulus_log2: u8,
}

impl SeqExtender {
    /// An extender for the 31-bit sequence numbers of a slot.
    pub const fn for_slot() -> Self {
        Self {
            last: 0,
            modulus_log2: 31,
        }
    }

    /// An extender for the 32-bit sequence numbers of a trace.
    pub const fn for_trace() -> Self {
        Self {
            last: 0,
            modulus_log2: 32,
        }
    }

    /// The last extended value.
    pub const fn last(&self) -> u64 {
        self.last
    }

    /// Extends a truncated sequence number to the first 64-bit value, not
    /// older than the previous one, with the same low bits.
    pub fn extend(&mut self, truncated: u64) -> u64 {
        let mask = (1u64 << self.modulus_log2) - 1;
        let forward = (truncated & mask).wrapping_sub(self.last & mask) & mask;
        self.last = self.last.wrapping_add(forward);
        self.last
    }
}

pub(crate) const fn lo(word: u64) -> u32 {
    word as u32
}

pub(crate) const fn hi(word: u64) -> u32 {
    (word >> 32) as u32
}

pub(crate) const fn join(lo: u32, hi: u32) -> u64 {
    (lo as u64) | ((hi as u64) << 32)
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use crate::quality::{QualityCode, QualityReason};
    use crate::value::NO_TS_SRC;

    extern crate std;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    #[test]
    fn slot_is_32_bytes_and_aligned() {
        assert_eq!(core::mem::size_of::<Slot>(), 32);
        assert_eq!(core::mem::align_of::<Slot>(), 32);
    }

    #[test]
    fn empty_slot_reads_initial() {
        assert_eq!(Slot::new().try_read(), Ok(Sample::INITIAL));
    }

    #[test]
    fn write_then_read() {
        let slot = Slot::new();
        let q = Quality::new(QualityCode::Bad, QualityReason::CommFailure);
        assert_eq!(slot.write(u64::MAX - 1, q, -5, 1 << 40), 1);
        assert_eq!(slot.write(9, Quality::GOOD, NO_TS_SRC, 3), 2);
        let s = slot.read(0);
        assert_eq!(s, Ok(Sample::new(9, Quality::GOOD, NO_TS_SRC, 3, 2)));
    }

    #[test]
    fn concurrent_readers_never_see_torn_samples() {
        let slot = Arc::new(Slot::new());
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let slot = Arc::clone(&slot);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut k: u64 = 1;
                while !stop.load(core::sync::atomic::Ordering::Relaxed) {
                    let word = k.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                    slot.write(word, Quality::from_bits(word as u32), word as i64, word);
                    k += 1;
                }
            })
        };
        let mut ok = 0u32;
        let mut extender = SeqExtender::for_slot();
        let mut last = 0;
        for _ in 0..200_000 {
            if let Ok(s) = slot.read(4) {
                if s.seq() == 0 {
                    assert_eq!(s, Sample::INITIAL);
                    continue;
                }
                let word = s.transport_bits();
                assert_eq!(s.ts_acq(), word);
                assert_eq!(s.ts_src_raw(), word as i64);
                assert_eq!(s.quality().to_bits(), word as u32);
                assert_eq!(word, s.seq().wrapping_mul(0x9E37_79B9_7F4A_7C15));
                let seq = extender.extend(s.seq());
                assert!(seq >= last);
                last = seq;
                ok += 1;
            }
        }
        stop.store(true, core::sync::atomic::Ordering::Relaxed);
        assert!(writer.join().is_ok());
        assert!(ok > 0);
    }

    #[test]
    fn extender_handles_wrap() {
        let mut e = SeqExtender::for_slot();
        assert_eq!(e.extend(5), 5);
        assert_eq!(e.extend(5), 5);
        assert_eq!(e.extend((1 << 31) - 1), (1 << 31) - 1);
        assert_eq!(e.extend(3), (1 << 31) + 3);
        assert_eq!(e.last(), (1 << 31) + 3);
    }

    #[test]
    fn trace_extender_uses_32_bits() {
        let mut e = SeqExtender::for_trace();
        assert_eq!(e.extend(u64::from(u32::MAX) - 1), u64::from(u32::MAX) - 1);
        assert_eq!(e.extend(1), (1 << 32) + 1);
    }
}

#[cfg(all(test, loom))]
mod loom_tests {
    use super::*;
    use loom::sync::Arc;
    use loom::thread;

    fn pattern(k: u64) -> u64 {
        k.wrapping_mul(0x0001_0001_0001_0001) | (k << 60)
    }

    #[test]
    fn reader_sees_whole_samples_only() {
        loom::model(|| {
            let slot = Arc::new(Slot::new());
            let writer = {
                let slot = Arc::clone(&slot);
                thread::spawn(move || {
                    for k in 1..=2u64 {
                        let w = pattern(k);
                        slot.write(w, Quality::from_bits(w as u32), w as i64, w);
                    }
                })
            };
            if let Ok(s) = slot.try_read() {
                let k = s.seq();
                let expected = if k == 0 { 0 } else { pattern(k) };
                if k == 0 {
                    assert_eq!(s, Sample::INITIAL);
                } else {
                    assert_eq!(s.transport_bits(), expected);
                    assert_eq!(s.ts_acq(), expected);
                    assert_eq!(s.ts_src_raw(), expected as i64);
                    assert_eq!(s.quality().to_bits(), expected as u32);
                }
            }
            assert!(writer.join().is_ok());
        });
    }

    #[test]
    fn two_reads_are_monotonic() {
        loom::model(|| {
            let slot = Arc::new(Slot::new());
            let writer = {
                let slot = Arc::clone(&slot);
                thread::spawn(move || {
                    for k in 1..=2u64 {
                        slot.write(k, Quality::GOOD, 0, k);
                    }
                })
            };
            let first = slot.try_read().ok();
            let second = slot.try_read().ok();
            if let (Some(a), Some(b)) = (first, second) {
                assert!(a.seq() <= b.seq());
                assert_eq!(b.transport_bits(), b.seq());
            }
            assert!(writer.join().is_ok());
        });
    }
}
