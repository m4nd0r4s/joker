//! The cycle view of an app (ref/04 §4.4–§4.6): declaration types, the
//! `CycleIo` view, and the port handles that read the packed image and write
//! the proposal buffer (ref/04 §3).

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;
use core::cell::RefCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::time::Duration;

use crate::error::{Code, Error, QualityError, Result};
use crate::time::{ClockQuality, Instant, MonoNanos, UtcNanos};
use crate::value::{Level, PortValue, Quality, Sample, WriteOutcome};

/// What `#[joker::app]` implements for the app struct. Not implemented by
/// hand.
pub trait App: 'static {
    /// The cycle view: one handle per port.
    type Cycle<'a>;
    /// The app name.
    const NAME: &'static str;
    /// The ports, in declaration order (the port table of ref/04 §2.6.2).
    const PORTS: &'static [PortDecl];
    /// Builds the cycle view over one call's image and buffer.
    fn cycle_view<'a>(cx: &'a CycleCx<'a>) -> Self::Cycle<'a>;
}

/// A port kind of the M0 SDK.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortKind {
    /// `#[input]`.
    Input,
    /// `#[output]`.
    Output,
    /// `#[param]`.
    Param,
}

/// A port as the macro declares it.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PortDecl {
    /// Field name.
    pub name: &'static str,
    /// Kind.
    pub kind: PortKind,
    /// Signal type code.
    pub ty: u8,
}

/// An input port of type `T` (a declaration: the app never builds it).
#[derive(Debug)]
pub struct Input<T>(PhantomData<T>);

/// An output port of type `T`.
#[derive(Debug)]
pub struct Output<T>(PhantomData<T>);

/// A parameter of type `T`.
#[derive(Debug)]
pub struct Param<T>(PhantomData<T>);

/// Where a port is in the image, from `init-context.ports`.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Layout {
    /// Sample entry.
    pub slot: Option<u32>,
    /// Result entry.
    pub outcome: Option<u32>,
    /// Bound in this generation.
    pub bound: bool,
}

const HEADER: usize = 64;
const ENTRY: usize = 24;
const WRITE: u8 = 1;
const NOTE: u8 = 6;
const RELINQUISH: u8 = 1;
const UNBOUND: u16 = 1 << 4;
const CHANGED: u16 = 1 << 3;
const HAS_TS_SRC: u16 = 1 << 2;
const HAS_TS_HW: u16 = 1 << 1;
const MINE_ACTIVE: u8 = 1;

fn u16_at(b: &[u8], at: usize) -> u16 {
    match b.get(at..at + 2) {
        Some(s) => u16::from_le_bytes([s[0], s[1]]),
        None => 0,
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    match b.get(at..at + 4) {
        Some(s) => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        None => 0,
    }
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    if let Some(s) = b.get(at..at + 8) {
        w.copy_from_slice(s);
    }
    u64::from_le_bytes(w)
}

/// The proposal buffer of one call (ref/04 §3.3).
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct ProposalBuffer {
    bytes: Vec<u8>,
    records: u16,
    capacity: usize,
    full: bool,
    pending: Vec<Option<u64>>,
}

impl ProposalBuffer {
    /// An empty buffer of `capacity` bytes for `ports` ports.
    pub fn new(capacity: usize, ports: usize) -> Self {
        let mut b = Self {
            bytes: Vec::with_capacity(capacity),
            records: 0,
            capacity,
            full: false,
            pending: alloc::vec![None; ports],
        };
        b.reset();
        b
    }

    fn reset(&mut self) {
        self.bytes.clear();
        self.bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]);
        self.records = 0;
        self.full = false;
        self.pending.fill(None);
    }

    fn push(&mut self, record: &[u8]) {
        if self.full || self.bytes.len() + record.len() > self.capacity || self.records == u16::MAX
        {
            self.full = true;
            return;
        }
        self.bytes.extend_from_slice(record);
        self.records += 1;
    }

    fn write(&mut self, port: u32, flags: u8, value: u64) {
        let mut r = [0u8; 14];
        r[0] = WRITE;
        r[1] = flags;
        r[2..6].copy_from_slice(&port.to_le_bytes());
        r[6..14].copy_from_slice(&value.to_le_bytes());
        self.push(&r);
        if !self.full
            && let Some(p) = self.pending.get_mut(port as usize)
        {
            *p = (flags & RELINQUISH == 0).then_some(value);
        }
    }

    fn note(&mut self, port: u32) {
        let mut r = [0u8; 6];
        r[0] = NOTE;
        r[2..6].copy_from_slice(&port.to_le_bytes());
        self.push(&r);
    }

    /// True when a record did not fit (`J-E0203`).
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// The finished buffer, handed to the host. The buffer starts again empty.
    pub fn finish(&mut self) -> Vec<u8> {
        let len = self.bytes.len() as u32;
        self.bytes[2..4].copy_from_slice(&self.records.to_le_bytes());
        self.bytes[4..8].copy_from_slice(&len.to_le_bytes());
        let out = core::mem::replace(&mut self.bytes, Vec::with_capacity(self.capacity));
        self.reset();
        out
    }
}

/// One `on-cycle` call: the image, the port layout and the proposal buffer.
#[doc(hidden)]
#[derive(Debug)]
pub struct CycleCx<'a> {
    image: &'a [u8],
    layout: &'a [Layout],
    n_slots: usize,
    task: &'static str,
    buffer: RefCell<ProposalBuffer>,
}

struct Raw {
    value: u64,
    quality: Quality,
    flags: u16,
    seq: u32,
    ts_acq: u64,
}

impl<'a> CycleCx<'a> {
    /// A call over `image`, with the layout from `init` and the instance's
    /// buffer (taken back with [`CycleCx::into_buffer`]).
    pub fn new(
        image: &'a [u8],
        layout: &'a [Layout],
        task: &'static str,
        mut buffer: ProposalBuffer,
    ) -> Self {
        buffer.reset();
        Self {
            image,
            layout,
            n_slots: u32_at(image, 44) as usize,
            task,
            buffer: RefCell::new(buffer),
        }
    }

    /// Gives the buffer back.
    pub fn into_buffer(self) -> ProposalBuffer {
        self.buffer.into_inner()
    }

    fn layout(&self, port: u32) -> Layout {
        self.layout.get(port as usize).copied().unwrap_or_default()
    }

    fn raw(&self, port: u32) -> Raw {
        let l = self.layout(port);
        let Some(slot) = l.slot.filter(|_| l.bound) else {
            return Raw {
                value: 0,
                quality: Quality::bad(crate::value::QualityReason::NotConnected),
                flags: UNBOUND,
                seq: 0,
                ts_acq: 0,
            };
        };
        let at = HEADER + ENTRY * slot as usize;
        Raw {
            value: u64_at(self.image, at),
            quality: Quality::from_codes(
                self.image.get(at + 8).copied().unwrap_or(2),
                self.image.get(at + 9).copied().unwrap_or(0),
            ),
            flags: u16_at(self.image, at + 10),
            seq: u32_at(self.image, at + 12),
            ts_acq: u64_at(self.image, at + 16),
        }
    }

    fn sample<T: PortValue>(&self, port: u32) -> Sample<T> {
        let raw = self.raw(port);
        let (ts_src, ts_hw) = if raw.flags & (HAS_TS_SRC | HAS_TS_HW) != 0 {
            let s = crate::runtime::read(port);
            (s.0.map(UtcNanos), s.1.map(MonoNanos))
        } else {
            (None, None)
        };
        Sample {
            value: T::from_bits(raw.value),
            quality: raw.quality,
            ts_src,
            ts_hw,
            ts_acq: MonoNanos(raw.ts_acq),
            seq: u64::from(raw.seq),
        }
    }

    fn result(&self, port: u32) -> Option<(u8, u8, u8, u8, u64, u64)> {
        let o = self.layout(port).outcome?;
        let at = HEADER + ENTRY * (self.n_slots + o as usize);
        let e = self.image.get(at..at + ENTRY)?;
        Some((e[0], e[1], e[2], e[3], u64_at(e, 8), u64_at(e, 16)))
    }

    fn flags(&self) -> u16 {
        u16_at(self.image, 2)
    }
}

/// The context a cycle task runs in.
#[doc(hidden)]
pub struct TaskCx<'a> {
    cx: &'a CycleCx<'a>,
    state: &'a mut Option<Box<dyn Any + Send>>,
}

impl core::fmt::Debug for TaskCx<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TaskCx")
            .field("task", &self.cx.task)
            .finish_non_exhaustive()
    }
}

impl<'a> TaskCx<'a> {
    /// A context over `cx`, with the task's state slot.
    pub fn new(cx: &'a CycleCx<'a>, state: &'a mut Option<Box<dyn Any + Send>>) -> Self {
        Self { cx, state }
    }

    /// Runs a task with state `S`, created with `Default` on the first tick.
    pub fn run<A: App, S: Default + Send + 'static>(
        &mut self,
        f: fn(&mut CycleIo<'_, A>, &mut S) -> Result<()>,
    ) -> Result<()> {
        let state = self.state.get_or_insert_with(|| Box::new(S::default()));
        let st = state.downcast_mut::<S>().ok_or_else(|| {
            Error::new(Code::from_static("J-E0008"), "task state of another type")
        })?;
        let mut io = CycleIo {
            cx: self.cx,
            view: A::cycle_view(self.cx),
        };
        f(&mut io, st)
    }

    /// Runs a stateless task.
    pub fn run_stateless<A: App>(
        &mut self,
        f: fn(&mut CycleIo<'_, A>) -> Result<()>,
    ) -> Result<()> {
        let mut io = CycleIo {
            cx: self.cx,
            view: A::cycle_view(self.cx),
        };
        f(&mut io)
    }
}

/// The view of a cycle task (ref/04 §4.5): the ports, through `Deref`, and
/// the tick facts.
pub struct CycleIo<'a, A: App> {
    cx: &'a CycleCx<'a>,
    view: A::Cycle<'a>,
}

impl<'a, A: App> core::fmt::Debug for CycleIo<'a, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CycleIo")
            .field("app", &A::NAME)
            .field("task", &self.cx.task)
            .finish_non_exhaustive()
    }
}

impl<'a, A: App> Deref for CycleIo<'a, A> {
    type Target = A::Cycle<'a>;
    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

impl<'a, A: App> DerefMut for CycleIo<'a, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.view
    }
}

impl<'a, A: App> CycleIo<'a, A> {
    /// The latch instant of the tick.
    pub fn now(&self) -> Instant {
        Instant {
            mono: MonoNanos(u64_at(self.cx.image, 16)),
            utc: UtcNanos(u64_at(self.cx.image, 24) as i64),
            clock: ClockQuality::from_u8(self.cx.image.get(40).copied().unwrap_or(2)),
        }
    }

    /// The effective period of the group (ADR 0032).
    pub fn dt(&self) -> Duration {
        Duration::from_nanos(u64_at(self.cx.image, 32))
    }

    /// The tick number of the group.
    pub fn tick(&self) -> u64 {
        u64_at(self.cx.image, 8)
    }

    /// First tick of this instance in this generation.
    pub fn first_tick(&self) -> bool {
        self.cx.flags() & 1 != 0
    }

    /// The previous tick was skipped by `overrun = "skip"`.
    pub fn after_skip(&self) -> bool {
        self.cx.flags() & (1 << 3) != 0
    }

    /// The app runs in shadow mode (M16).
    pub fn shadow(&self) -> bool {
        self.cx.flags() & (1 << 1) != 0
    }

    /// The task name.
    pub fn task(&self) -> &'static str {
        self.cx.task
    }

    /// A number of the instance's deterministic generator (ref/04 §2.7).
    pub fn random(&self) -> u64 {
        crate::runtime::random()
    }
}

/// An input in the cycle view.
#[derive(Debug, Clone, Copy)]
pub struct CycleInput<'a, T> {
    cx: &'a CycleCx<'a>,
    port: u32,
    name: &'static str,
    _t: PhantomData<T>,
}

impl<'a, T: PortValue> CycleInput<'a, T> {
    #[doc(hidden)]
    pub fn __new(cx: &'a CycleCx<'a>, port: u32, name: &'static str) -> Self {
        Self {
            cx,
            port,
            name,
            _t: PhantomData,
        }
    }

    fn refused(&self, quality: Quality) -> QualityError {
        QualityError {
            port: self.name,
            quality,
        }
    }

    /// The value if the sample is `Good`; otherwise `Err` (docs/16 §3.1).
    pub fn get(&self) -> Result<T, QualityError> {
        let raw = self.cx.raw(self.port);
        if raw.quality.is_good() {
            Ok(T::from_bits(raw.value))
        } else {
            Err(self.refused(raw.quality))
        }
    }

    /// The value if the sample is `Good` or `Uncertain`; `Err` when `Bad`.
    pub fn get_uncertain(&self) -> Result<T, QualityError> {
        let raw = self.cx.raw(self.port);
        if raw.quality.is_bad() {
            Err(self.refused(raw.quality))
        } else {
            Ok(T::from_bits(raw.value))
        }
    }

    /// The value if `Good`; otherwise `default`, and the substitution is
    /// recorded in the trace (a `NOTE` record).
    pub fn get_or(&self, default: T) -> T {
        match self.get() {
            Ok(v) => v,
            Err(_) => {
                self.cx.buffer.borrow_mut().note(self.port);
                default
            }
        }
    }

    /// The complete sample.
    pub fn sample(&self) -> Sample<T> {
        self.cx.sample(self.port)
    }

    /// False for an optional port left unbound.
    pub fn is_bound(&self) -> bool {
        self.cx.layout(self.port).bound
    }

    /// The sequence number changed since the previous latch of this group.
    pub fn changed(&self) -> bool {
        self.cx.raw(self.port).flags & CHANGED != 0
    }
}

/// An output in the cycle view.
#[derive(Debug, Clone, Copy)]
pub struct CycleOutput<'a, T> {
    cx: &'a CycleCx<'a>,
    port: u32,
    name: &'static str,
    _t: PhantomData<T>,
}

impl<'a, T: PortValue> CycleOutput<'a, T> {
    #[doc(hidden)]
    pub fn __new(cx: &'a CycleCx<'a>, port: u32, name: &'static str) -> Self {
        Self {
            cx,
            port,
            name,
            _t: PhantomData,
        }
    }

    /// Proposes `v` (renews the lease, docs/16 A2).
    pub fn set(&self, v: T) {
        self.cx.buffer.borrow_mut().write(self.port, 0, v.to_bits());
    }

    /// Releases this writer's level now.
    pub fn relinquish(&self) {
        self.cx.buffer.borrow_mut().write(self.port, RELINQUISH, 0);
    }

    /// The latest value this task proposed in this tick.
    pub fn pending(&self) -> Option<T> {
        self.cx
            .buffer
            .borrow()
            .pending
            .get(self.port as usize)
            .copied()
            .flatten()
            .map(T::from_bits)
    }

    /// The outcome of this writer's proposal in the owner group's latest
    /// commit; `None` if it had none.
    pub fn last_outcome(&self) -> Option<WriteOutcome<T>> {
        let (code, detail, _, _, applied, _) = self.cx.result(self.port)?;
        WriteOutcome::from_entry(code, detail, applied)
    }

    /// The applied value, if its sample is `Good`.
    pub fn get(&self) -> Result<T, QualityError> {
        let raw = self.cx.raw(self.port);
        if raw.quality.is_good() {
            Ok(T::from_bits(raw.value))
        } else {
            Err(QualityError {
                port: self.name,
                quality: raw.quality,
            })
        }
    }

    /// The sample of the applied value.
    pub fn sample(&self) -> Sample<T> {
        self.cx.sample(self.port)
    }

    /// This writer's active proposal (`mine()`, docs/17 §11).
    pub fn mine(&self) -> Option<T> {
        let (_, _, _, flags, _, mine) = self.cx.result(self.port)?;
        (flags & MINE_ACTIVE != 0).then(|| T::from_bits(mine))
    }

    /// The level that won the last commit; `None` when the output is at
    /// `safe`.
    pub fn winner(&self) -> Option<Level> {
        let (_, _, winner, _, _, _) = self.cx.result(self.port)?;
        Level::new(winner)
    }

    /// False for an optional port left unbound.
    pub fn is_bound(&self) -> bool {
        self.cx.layout(self.port).bound
    }
}

/// A parameter in the cycle view.
#[derive(Debug, Clone, Copy)]
pub struct ParamHandle<'a, T> {
    cx: &'a CycleCx<'a>,
    port: u32,
    _t: PhantomData<T>,
}

impl<'a, T: PortValue> ParamHandle<'a, T> {
    #[doc(hidden)]
    pub fn __new(cx: &'a CycleCx<'a>, port: u32, _name: &'static str) -> Self {
        Self {
            cx,
            port,
            _t: PhantomData,
        }
    }

    /// The current value: a parameter always has one.
    pub fn get(&self) -> T {
        T::from_bits(self.cx.raw(self.port).value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{QualityCode, QualityReason};

    fn image(samples: &[(u64, u8, u8, u16)], results: &[(u8, u8, u8, u8, u64, u64)]) -> Vec<u8> {
        let mut b = alloc::vec![0u8; HEADER + ENTRY * (samples.len() + results.len())];
        b[0] = 1;
        b[1] = 64;
        b[2] = 1;
        b[32..40].copy_from_slice(&20_000_000u64.to_le_bytes());
        b[44..48].copy_from_slice(&(samples.len() as u32).to_le_bytes());
        for (i, &(v, c, r, f)) in samples.iter().enumerate() {
            let at = HEADER + ENTRY * i;
            b[at..at + 8].copy_from_slice(&v.to_le_bytes());
            b[at + 8] = c;
            b[at + 9] = r;
            b[at + 10..at + 12].copy_from_slice(&f.to_le_bytes());
            b[at + 12] = 7;
        }
        for (i, &(o, d, w, f, a, m)) in results.iter().enumerate() {
            let at = HEADER + ENTRY * (samples.len() + i);
            b[at] = o;
            b[at + 1] = d;
            b[at + 2] = w;
            b[at + 3] = f;
            b[at + 8..at + 16].copy_from_slice(&a.to_le_bytes());
            b[at + 16..at + 24].copy_from_slice(&m.to_le_bytes());
        }
        b
    }

    const LAYOUT: [Layout; 3] = [
        Layout {
            slot: Some(0),
            outcome: None,
            bound: true,
        },
        Layout {
            slot: Some(1),
            outcome: Some(0),
            bound: true,
        },
        Layout {
            slot: Some(2),
            outcome: None,
            bound: false,
        },
    ];

    #[test]
    fn reads_follow_the_quality() {
        let img = image(
            &[
                (21.5f64.to_bits(), 1, 9, CHANGED),
                (1, 0, 0, 0),
                (0, 2, 16, UNBOUND),
            ],
            &[(2, 0, 12, MINE_ACTIVE, 1, 1)],
        );
        let cx = CycleCx::new(&img, &LAYOUT, "t", ProposalBuffer::new(1024, 3));
        let t: CycleInput<'_, f64> = CycleInput::__new(&cx, 0, "temp");
        assert_eq!(
            t.get().unwrap_err().quality,
            Quality::uncertain(QualityReason::Initial)
        );
        assert_eq!(t.get_uncertain(), Ok(21.5));
        assert_eq!(t.get_or(0.0), 0.0);
        assert!(t.changed());
        assert_eq!(t.sample().seq, 7);
        let off: CycleInput<'_, f64> = CycleInput::__new(&cx, 2, "spare");
        assert!(!off.is_bound());
        assert_eq!(off.get().unwrap_err().quality.code, QualityCode::Bad);
        let led: CycleOutput<'_, bool> = CycleOutput::__new(&cx, 1, "led");
        assert_eq!(led.get(), Ok(true));
        assert_eq!(led.winner().map(Level::get), Some(12));
        assert_eq!(led.mine(), Some(true));
        assert!(matches!(
            led.last_outcome(),
            Some(WriteOutcome::Clamped { applied: true, .. })
        ));
        assert_eq!(led.pending(), None);
        led.set(false);
        assert_eq!(led.pending(), Some(false));
        led.relinquish();
        assert_eq!(led.pending(), None);
        let mut buffer = cx.into_buffer();
        let out = buffer.finish();
        assert_eq!(&out[..8], &[1, 0, 3, 0, 42, 0, 0, 0]);
        assert_eq!(&out[8..14], &[NOTE, 0, 0, 0, 0, 0]);
        assert_eq!(out[14], WRITE);
        assert_eq!(out[28..30], [WRITE, RELINQUISH]);
    }

    #[test]
    fn a_full_buffer_drops_and_says_so() {
        let img = image(&[(0, 0, 0, 0)], &[]);
        let layout = [Layout {
            slot: Some(0),
            outcome: Some(0),
            bound: true,
        }];
        let cx = CycleCx::new(&img, &layout, "t", ProposalBuffer::new(8 + 14 * 2, 1));
        let o: CycleOutput<'_, u64> = CycleOutput::__new(&cx, 0, "o");
        o.set(1);
        o.set(2);
        o.set(3);
        let mut buffer = cx.into_buffer();
        assert!(buffer.is_full());
        assert_eq!(buffer.finish().len(), 36);
        assert!(!buffer.is_full());
    }
}
