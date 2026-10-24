//! The committed-write channel from the cycle domain to the drivers
//! (docs/01 §3.4, ref/06 §2.9, ADR 0005, ADR 0047).
//!
//! Each output bound to a driver point has a **latest-wins box**: the last
//! applied value replaces the previous one not yet sent, so slow equipment
//! never wakes up to a queue of stale values. The cycle side
//! ([`CommitTx`]) publishes after each commit, reading the applied values
//! from the group's [`Outputs`]; it never allocates, blocks or waits. The io
//! side ([`CommitRx`]) turns new values into [`CommittedWrite`]s. Each box
//! also has the supervisor's `force_safe` bit ([`ForceSafe`]), which sends
//! the output's declared `safe` without going through the group.
//!
//! [`CommittedWrite`] can only be built here, and only from those two
//! sources: a value applied by the envelope, or the declared `safe`. That is
//! the type-level form of invariant 3.
//!
//! M0 converts only to the raw kinds that need no profile scaling: `bool`,
//! `int`, `uint` and `float`. Scaled register outputs (ref/06 §2.7) come
//! with the acquisition and output stages.

use joker_core::{Applied, Outputs, SafeValue, SignalId, ValueType};
use portable_atomic::{AtomicU32, fence};

use core::sync::atomic::Ordering;

use crate::driver::CommittedWrite;
use crate::types::{PointId, RawValue, WriteKind};

/// Proof that a [`CommittedWrite`] was built by this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sealed(pub(crate) ());

/// How an applied value becomes raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RawEncoding {
    /// `bool` output to a `boolean` point.
    Bool,
    /// `i64` output to an `int` point.
    Int,
    /// `u64`, `enum` or `bits` output to a `uint` point.
    Uint,
    /// `f64` or `f32` output to a `float` point.
    Float,
}

impl RawEncoding {
    /// True when an output of type `ty` can use this encoding.
    pub const fn accepts(self, ty: ValueType) -> bool {
        matches!(
            (self, ty),
            (Self::Bool, ValueType::Bool)
                | (Self::Int, ValueType::I64)
                | (
                    Self::Uint,
                    ValueType::U64 | ValueType::Enum | ValueType::Bits
                )
                | (Self::Float, ValueType::F64 | ValueType::F32)
        )
    }

    fn raw(self, bits: u64) -> RawValue<'static> {
        match self {
            Self::Bool => RawValue::Bool(bits != 0),
            Self::Int => RawValue::Int(bits as i64),
            Self::Uint => RawValue::Uint(bits),
            Self::Float => RawValue::Float(f64::from_bits(bits)),
        }
    }
}

/// An output bound to a driver point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutputBinding {
    /// The output signal.
    pub signal: SignalId,
    /// The driver's point.
    pub point: PointId,
    /// Raw conversion.
    pub encoding: RawEncoding,
}

/// Why a channel could not be set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindError {
    /// The bindings, boxes and state slices differ in length.
    StorageMismatch,
    /// The signal is not an output of these `Outputs`.
    UnknownOutput(SignalId),
    /// The encoding does not fit the output's type.
    EncodingMismatch(SignalId),
}

const RELINQUISH: u32 = 1;

/// One latest-wins box: a seqlock over the last published value, the
/// declared `safe` and the supervisor's `force_safe` bit. 32-bit atomics
/// only, as in the core's slots.
#[derive(Debug)]
pub struct CommitBox {
    version: AtomicU32,
    seq_lo: AtomicU32,
    seq_hi: AtomicU32,
    bits_lo: AtomicU32,
    bits_hi: AtomicU32,
    flags: AtomicU32,
    safe_lo: AtomicU32,
    safe_hi: AtomicU32,
    safe_flags: AtomicU32,
    force: AtomicU32,
}

impl Default for CommitBox {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snapshot {
    seq: u64,
    applied: Applied,
}

fn applied_parts(applied: Applied) -> (u64, u32) {
    match applied {
        Applied::Value(bits) => (bits, 0),
        Applied::Relinquish => (0, RELINQUISH),
    }
}

fn applied_from(bits: u64, flags: u32) -> Applied {
    if flags & RELINQUISH != 0 {
        Applied::Relinquish
    } else {
        Applied::Value(bits)
    }
}

fn join(lo: u32, hi: u32) -> u64 {
    u64::from(lo) | (u64::from(hi) << 32)
}

impl CommitBox {
    /// An empty box: nothing published, not forced.
    pub const fn new() -> Self {
        Self {
            version: AtomicU32::new(0),
            seq_lo: AtomicU32::new(0),
            seq_hi: AtomicU32::new(0),
            bits_lo: AtomicU32::new(0),
            bits_hi: AtomicU32::new(0),
            flags: AtomicU32::new(0),
            safe_lo: AtomicU32::new(0),
            safe_hi: AtomicU32::new(0),
            safe_flags: AtomicU32::new(0),
            force: AtomicU32::new(0),
        }
    }

    fn store(&self, seq: u64, applied: Applied) {
        let (bits, flags) = applied_parts(applied);
        let version = self.version.load(Ordering::Relaxed);
        self.version
            .store(version.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.seq_lo.store(seq as u32, Ordering::Relaxed);
        self.seq_hi.store((seq >> 32) as u32, Ordering::Relaxed);
        self.bits_lo.store(bits as u32, Ordering::Relaxed);
        self.bits_hi.store((bits >> 32) as u32, Ordering::Relaxed);
        self.flags.store(flags, Ordering::Relaxed);
        self.version
            .store(version.wrapping_add(2), Ordering::Release);
    }

    fn try_load(&self) -> Option<Snapshot> {
        let before = self.version.load(Ordering::Acquire);
        if before & 1 == 1 {
            return None;
        }
        let seq = join(
            self.seq_lo.load(Ordering::Relaxed),
            self.seq_hi.load(Ordering::Relaxed),
        );
        let bits = join(
            self.bits_lo.load(Ordering::Relaxed),
            self.bits_hi.load(Ordering::Relaxed),
        );
        let flags = self.flags.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        if self.version.load(Ordering::Relaxed) != before {
            return None;
        }
        Some(Snapshot {
            seq,
            applied: applied_from(bits, flags),
        })
    }

    fn load(&self) -> Option<Snapshot> {
        (0..4).find_map(|_| self.try_load())
    }

    fn set_safe(&self, safe: SafeValue) {
        let (bits, flags) = applied_parts(Applied::from_safe(safe));
        self.safe_lo.store(bits as u32, Ordering::Relaxed);
        self.safe_hi.store((bits >> 32) as u32, Ordering::Relaxed);
        self.safe_flags.store(flags, Ordering::Release);
    }

    fn safe(&self) -> Applied {
        let flags = self.safe_flags.load(Ordering::Acquire);
        let bits = join(
            self.safe_lo.load(Ordering::Relaxed),
            self.safe_hi.load(Ordering::Relaxed),
        );
        applied_from(bits, flags)
    }

    fn forced(&self) -> bool {
        self.force.load(Ordering::Acquire) != 0
    }
}

/// The cycle side of the channel, one per group.
#[derive(Debug)]
pub struct CommitTx<'a> {
    bindings: &'a [OutputBinding],
    boxes: &'a [CommitBox],
    last: &'a mut [Option<Applied>],
    next_seq: u64,
}

fn check(outputs: &Outputs<'_>, bindings: &[OutputBinding]) -> Result<(), BindError> {
    for b in bindings {
        let config = outputs
            .configs()
            .iter()
            .find(|c| c.signal == b.signal)
            .ok_or(BindError::UnknownOutput(b.signal))?;
        if !b.encoding.accepts(config.ty) {
            return Err(BindError::EncodingMismatch(b.signal));
        }
    }
    Ok(())
}

impl<'a> CommitTx<'a> {
    /// Binds `boxes` to the outputs of `outputs` and stores each output's
    /// declared `safe` in its box. Call before the io side starts.
    pub fn new(
        outputs: &Outputs<'_>,
        bindings: &'a [OutputBinding],
        boxes: &'a [CommitBox],
        last: &'a mut [Option<Applied>],
    ) -> Result<Self, BindError> {
        if boxes.len() != bindings.len() || last.len() != bindings.len() {
            return Err(BindError::StorageMismatch);
        }
        check(outputs, bindings)?;
        for (b, bx) in bindings.iter().zip(boxes) {
            if let Some(config) = outputs.configs().iter().find(|c| c.signal == b.signal) {
                bx.set_safe(config.safe);
            }
        }
        last.fill(None);
        Ok(Self {
            bindings,
            boxes,
            last,
            next_seq: 1,
        })
    }

    /// Step 7 for the drivers: publishes every applied value that changed
    /// since the last call (and every value on the first call). Returns the
    /// number of boxes written. Call right after `Outputs::commit`.
    pub fn publish(&mut self, outputs: &Outputs<'_>) -> usize {
        let mut written = 0;
        for ((binding, bx), last) in self
            .bindings
            .iter()
            .zip(self.boxes)
            .zip(self.last.iter_mut())
        {
            let Some(view) = outputs.view(binding.signal) else {
                continue;
            };
            if *last == Some(view.applied) {
                continue;
            }
            bx.store(self.next_seq, view.applied);
            self.next_seq += 1;
            *last = Some(view.applied);
            written += 1;
        }
        written
    }
}

/// The supervisor's handle on the `force_safe` bits (docs/09, attack 4).
#[derive(Clone, Copy, Debug)]
pub struct ForceSafe<'a> {
    boxes: &'a [CommitBox],
}

impl<'a> ForceSafe<'a> {
    /// A handle on `boxes`.
    pub fn new(boxes: &'a [CommitBox]) -> Self {
        Self { boxes }
    }

    /// Sets or clears the bit of box `index`; false when out of range.
    pub fn set(&self, index: usize, on: bool) -> bool {
        match self.boxes.get(index) {
            Some(bx) => {
                bx.force.store(u32::from(on), Ordering::Release);
                true
            }
            None => false,
        }
    }
}

/// What the io side remembers of one box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RxState {
    seen: u64,
    forced: bool,
    resync: bool,
    keepalive: bool,
}

/// The io side of the channel: turns new values into committed writes.
#[derive(Debug)]
pub struct CommitRx<'a> {
    bindings: &'a [OutputBinding],
    boxes: &'a [CommitBox],
    state: &'a mut [RxState],
    next_seq: u64,
}

impl<'a> CommitRx<'a> {
    /// The receiver of `boxes`.
    pub fn new(
        bindings: &'a [OutputBinding],
        boxes: &'a [CommitBox],
        state: &'a mut [RxState],
    ) -> Result<Self, BindError> {
        if boxes.len() != bindings.len() || state.len() != bindings.len() {
            return Err(BindError::StorageMismatch);
        }
        state.fill(RxState::default());
        Ok(Self {
            bindings,
            boxes,
            state,
            next_seq: 1,
        })
    }

    /// After each `connect`: the next calls resend the applied value of every
    /// box that has one, as `resync` (ref/06 §2.1, step 6).
    pub fn resync(&mut self) {
        for s in self.state.iter_mut() {
            s.resync = true;
        }
    }

    /// Marks box `index` due for a device-fallback keepalive (docs/01 §8,
    /// M1): the next `take()` resends its applied value as `Keepalive`,
    /// even though it did not change. Deadband and `min_interval` never
    /// suppress this (they live upstream of the box, at the envelope).
    /// `false` when `index` is out of range. Scheduling *when* each output
    /// is due (from the profile's `keepalive`, default timeout/5) is not
    /// this type's job; a caller with the manifest's fallback declarations
    /// calls this on the boxes that are due each tick.
    pub fn keepalive(&mut self, index: usize) -> bool {
        match self.state.get_mut(index) {
            Some(s) => {
                s.keepalive = true;
                true
            }
            None => false,
        }
    }

    /// The declared `safe` of box `index` as a write of `kind`: the end of
    /// a pulse (ref/06 §2.9, the second of its two writes), or a resync or
    /// keepalive of a pulse output, which renews the state the equipment
    /// rests in. `None` when `index` is out of range.
    pub fn safe_write(&mut self, index: usize, kind: WriteKind) -> Option<CommittedWrite<'static>> {
        let binding = *self.bindings.get(index)?;
        let safe = self.boxes.get(index)?.safe();
        Some(self.write(binding.point, binding.encoding, safe, kind))
    }

    /// True when `write` carries the declared `safe` of box `index`.
    pub fn is_safe(&self, index: usize, write: &CommittedWrite<'_>) -> bool {
        let (Some(binding), Some(bx)) = (self.bindings.get(index), self.boxes.get(index)) else {
            return false;
        };
        match bx.safe() {
            Applied::Value(bits) => write.value == binding.encoding.raw(bits),
            Applied::Relinquish => write.value == RawValue::None,
        }
    }

    fn write(
        &mut self,
        point: PointId,
        encoding: RawEncoding,
        applied: Applied,
        kind: WriteKind,
    ) -> CommittedWrite<'static> {
        let (value, kind) = match applied {
            Applied::Value(bits) => (encoding.raw(bits), kind),
            Applied::Relinquish => (RawValue::None, WriteKind::Relinquish),
        };
        let seq = self.next_seq;
        self.next_seq += 1;
        CommittedWrite {
            seq,
            point,
            value,
            kind,
            at: None,
            atomic: None,
            sealed: Sealed(()),
        }
    }

    /// The next pending write, if any. Each box gives at most one write per
    /// new value, per `resync` and per rising `force_safe` bit.
    pub fn take(&mut self) -> Option<CommittedWrite<'static>> {
        for i in 0..self.bindings.len() {
            let binding = self.bindings[i];
            let bx = &self.boxes[i];
            let forced = bx.forced();
            let state = self.state[i];
            if forced {
                if let Some(snap) = bx.load() {
                    self.state[i].seen = snap.seq;
                }
                if !state.forced || state.resync {
                    self.state[i].forced = true;
                    self.state[i].resync = false;
                    return Some(self.write(
                        binding.point,
                        binding.encoding,
                        bx.safe(),
                        WriteKind::ForceSafe,
                    ));
                }
                continue;
            }
            self.state[i].forced = false;
            let Some(snap) = bx.load() else {
                continue;
            };
            if snap.seq == 0 {
                self.state[i].resync = false;
                self.state[i].keepalive = false;
                continue;
            }
            let fresh = snap.seq != state.seen;
            if fresh || state.resync || state.keepalive || state.forced {
                self.state[i].seen = snap.seq;
                self.state[i].resync = false;
                self.state[i].keepalive = false;
                let kind = if fresh {
                    WriteKind::Set
                } else if state.resync || state.forced {
                    WriteKind::Resync
                } else {
                    WriteKind::Keepalive
                };
                return Some(self.write(binding.point, binding.encoding, snap.applied, kind));
            }
        }
        None
    }

    /// Appends every pending write to `out`; returns how many.
    #[cfg(feature = "alloc")]
    pub fn drain_into(&mut self, out: &mut alloc::vec::Vec<CommittedWrite<'static>>) -> usize {
        let before = out.len();
        while let Some(w) = self.take() {
            out.push(w);
        }
        out.len() - before
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;
    use crate::testing::OutputHarness;
    use alloc::vec::Vec;

    #[test]
    fn only_envelope_values_and_safe_reach_the_driver() {
        let mut h = OutputHarness::new(&[("pi.act_led", ValueType::Bool, 0, 7)]).unwrap();
        let bindings = h.bindings(&[RawEncoding::Bool]);
        let boxes = [CommitBox::new()];
        let mut last = [None];
        let mut state = [RxState::default()];
        let mut tx = CommitTx::new(h.outputs(), &bindings, &boxes, &mut last).unwrap();
        let mut rx = CommitRx::new(&bindings, &boxes, &mut state).unwrap();
        assert_eq!(rx.take(), None);
        tx.publish(h.outputs());
        let w = rx.take().unwrap();
        assert_eq!(
            (w.point, w.value, w.kind),
            (7, RawValue::Bool(false), WriteKind::Set)
        );
        assert_eq!(rx.take(), None);
        h.manual("pi.act_led", 1);
        assert_eq!(tx.publish(h.outputs()), 1);
        assert_eq!(tx.publish(h.outputs()), 0);
        let w = rx.take().unwrap();
        assert_eq!((w.value, w.kind), (RawValue::Bool(true), WriteKind::Set));
        rx.resync();
        let w = rx.take().unwrap();
        assert_eq!((w.value, w.kind), (RawValue::Bool(true), WriteKind::Resync));
        assert_eq!(rx.take(), None);
        let force = ForceSafe::new(&boxes);
        assert!(force.set(0, true));
        let w = rx.take().unwrap();
        assert_eq!(
            (w.value, w.kind),
            (RawValue::Bool(false), WriteKind::ForceSafe)
        );
        h.manual("pi.act_led", 1);
        tx.publish(h.outputs());
        assert_eq!(rx.take(), None);
        assert!(force.set(0, false));
        let w = rx.take().unwrap();
        assert_eq!((w.value, w.kind), (RawValue::Bool(true), WriteKind::Resync));
        assert!(!force.set(1, true));
    }

    #[test]
    fn keepalive_resends_the_applied_value_without_a_new_commit() {
        let h = OutputHarness::new(&[("pi.act_led", ValueType::Bool, 0, 7)]).unwrap();
        let bindings = h.bindings(&[RawEncoding::Bool]);
        let boxes = [CommitBox::new()];
        let mut last = [None];
        let mut state = [RxState::default()];
        let mut tx = CommitTx::new(h.outputs(), &bindings, &boxes, &mut last).unwrap();
        let mut rx = CommitRx::new(&bindings, &boxes, &mut state).unwrap();
        tx.publish(h.outputs());
        let w = rx.take().unwrap();
        assert_eq!((w.value, w.kind), (RawValue::Bool(false), WriteKind::Set));
        assert_eq!(rx.take(), None);

        assert!(rx.keepalive(0));
        let w = rx.take().unwrap();
        assert_eq!(
            (w.point, w.value, w.kind),
            (7, RawValue::Bool(false), WriteKind::Keepalive)
        );
        assert_eq!(rx.take(), None, "one keepalive per call, not forever");
        assert!(!rx.keepalive(1), "out of range");
    }

    #[test]
    fn the_declared_safe_is_a_write_of_its_own() {
        let mut h = OutputHarness::new(&[("pi.relay", ValueType::Bool, 0, 4)]).unwrap();
        let bindings = h.bindings(&[RawEncoding::Bool]);
        let boxes = [CommitBox::new()];
        let mut last = [None];
        let mut state = [RxState::default()];
        let mut tx = CommitTx::new(h.outputs(), &bindings, &boxes, &mut last).unwrap();
        let mut rx = CommitRx::new(&bindings, &boxes, &mut state).unwrap();
        h.manual("pi.relay", 1);
        tx.publish(h.outputs());
        let set = rx.take().unwrap();
        assert!(!rx.is_safe(0, &set));
        let end = rx.safe_write(0, WriteKind::Set).unwrap();
        assert_eq!(
            (end.point, end.value, end.kind),
            (4, RawValue::Bool(false), WriteKind::Set)
        );
        assert!(end.seq > set.seq);
        assert!(rx.is_safe(0, &end));
        assert!(rx.safe_write(1, WriteKind::Set).is_none());
        assert!(!rx.is_safe(1, &end));
    }

    #[test]
    fn latest_value_wins_and_relinquish_has_no_value() {
        let mut h = OutputHarness::new(&[("x.p", ValueType::F64, 0f64.to_bits(), 3)]).unwrap();
        let bindings = h.bindings(&[RawEncoding::Float]);
        let boxes = [CommitBox::new()];
        let mut last = [None];
        let mut state = [RxState::default()];
        let mut tx = CommitTx::new(h.outputs(), &bindings, &boxes, &mut last).unwrap();
        let mut rx = CommitRx::new(&bindings, &boxes, &mut state).unwrap();
        for v in [1.0f64, 2.0, 3.0] {
            h.manual("x.p", v.to_bits());
            tx.publish(h.outputs());
        }
        let mut out = Vec::new();
        assert_eq!(rx.drain_into(&mut out), 1);
        assert_eq!(out[0].value, RawValue::Float(3.0));
        let seqs: Vec<u64> = out.iter().map(|w| w.seq).collect();
        assert_eq!(seqs, [1]);
        assert_eq!(CommitBox::new().safe(), Applied::Value(0));
        let bx = CommitBox::new();
        bx.store(9, Applied::Relinquish);
        assert_eq!(bx.load().map(|s| s.applied), Some(Applied::Relinquish));
    }

    #[test]
    fn bindings_are_checked_against_the_outputs() {
        let h = OutputHarness::new(&[("x.p", ValueType::F64, 0, 0)]).unwrap();
        let boxes = [CommitBox::new()];
        let mut last = [None];
        let wrong = h.bindings(&[RawEncoding::Bool]);
        assert_eq!(
            CommitTx::new(h.outputs(), &wrong, &boxes, &mut last).unwrap_err(),
            BindError::EncodingMismatch(wrong[0].signal)
        );
        let unknown = [OutputBinding {
            signal: SignalId(99),
            point: 0,
            encoding: RawEncoding::Float,
        }];
        assert_eq!(
            CommitTx::new(h.outputs(), &unknown, &boxes, &mut last).unwrap_err(),
            BindError::UnknownOutput(SignalId(99))
        );
        let mut two = [None, None];
        assert_eq!(
            CommitTx::new(
                h.outputs(),
                &h.bindings(&[RawEncoding::Float]),
                &boxes,
                &mut two
            )
            .unwrap_err(),
            BindError::StorageMismatch
        );
    }
}
