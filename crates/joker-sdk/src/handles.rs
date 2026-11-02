//! Port handles (ref/04 §4.6): one generic type per declaration, used by
//! both views; the aliases `CycleInput`, `TaskInput`, … name them per view.

use alloc::vec::Vec;
use core::marker::PhantomData;
use core::ops::Deref;
use core::time::Duration;

use joker_sdk_sys::joker::runtime::async_io::HistoryQuery;
use joker_sdk_sys::joker::runtime::types;

use crate::error::{Code, Error, QualityError, Result};
use crate::host::{self, Agg};
use crate::io::{CHANGED, MINE_ACTIVE, RELINQUISH, Src};
use crate::record::{Event, Record};
use crate::time::Instant;
use crate::value::{AlarmState, Level, PortValue, Quality, QualityReason, Sample, WriteOutcome};
use crate::view::{AsyncApi, AsyncWriter, Cycle, CycleApi, Task, View, Writes, api};

fn refused(port: &'static str, quality: Quality) -> QualityError {
    QualityError { port, quality }
}

fn error(e: types::Error) -> Error {
    Error::new(Code::new(&e.code), e.message)
}

/// An input port.
#[derive(Debug, Clone, Copy)]
pub struct InputHandle<'a, T, V> {
    src: Src<'a>,
    port: u32,
    name: &'static str,
    _t: PhantomData<(T, V)>,
}

/// An input in the cycle view.
pub type CycleInput<'a, T> = InputHandle<'a, T, Cycle>;
/// An input in the async view.
pub type TaskInput<'a, T> = InputHandle<'a, T, Task>;

/// A series read from the historian; dereferences to the samples.
#[derive(Debug, Clone, PartialEq)]
pub struct History<T>(Vec<Sample<T>>);

impl<T> Deref for History<T> {
    type Target = [Sample<T>];
    fn deref(&self) -> &[Sample<T>] {
        &self.0
    }
}

async fn history<T: PortValue>(
    src: Src<'_>,
    port: u32,
    from: Instant,
    to: Instant,
    step: Duration,
    agg: Agg,
) -> Result<History<T>> {
    let Src::Task(cx) = src else {
        return Err(Error::new(
            Code::from_static("J-E0901"),
            "history in a cycle task",
        ));
    };
    let q = HistoryQuery {
        port,
        from_utc_ns: from.utc.0,
        to_utc_ns: to.utc.0,
        step_ns: if agg == Agg::Raw {
            0
        } else {
            u64::try_from(step.as_nanos()).unwrap_or(u64::MAX)
        },
        agg: agg.wire(),
    };
    let samples = cx.host.history(q).await.map_err(error)?;
    Ok(History(
        samples
            .iter()
            .map(|s| host::sample(s, |v| T::from_bits(host::bits(v))))
            .collect(),
    ))
}

impl<'a, T: PortValue, V: View> InputHandle<'a, T, V> {
    #[doc(hidden)]
    pub fn __new(src: Src<'a>, port: u32, name: &'static str) -> Self {
        Self {
            src,
            port,
            name,
            _t: PhantomData,
        }
    }

    /// The value if the sample is `Good`; otherwise `Err` (docs/16 §3.1).
    pub fn get(&self) -> Result<T, QualityError> {
        let raw = self.src.raw(self.port);
        if raw.quality.is_good() {
            Ok(T::from_bits(raw.value))
        } else {
            Err(refused(self.name, raw.quality))
        }
    }

    /// The value if the sample is `Good` or `Uncertain`; `Err` when `Bad`.
    pub fn get_uncertain(&self) -> Result<T, QualityError> {
        let raw = self.src.raw(self.port);
        if raw.quality.is_bad() {
            Err(refused(self.name, raw.quality))
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
                match self.src {
                    Src::Cycle(cx) => cx.buffer.borrow_mut().note(self.port),
                    Src::Task(_) => {}
                }
                default
            }
        }
    }

    /// The complete sample.
    pub fn sample(&self) -> Sample<T> {
        sample(self.src, self.port)
    }

    /// False for an optional port left unbound.
    pub fn is_bound(&self) -> bool {
        self.src.bound(self.port)
    }

    /// The sequence number changed since the previous latch of this group.
    pub fn changed(&self) -> bool
    where
        V: CycleApi<api::changed>,
    {
        self.src.raw(self.port).flags & CHANGED != 0
    }

    /// The mean per `step` of the samples in `[from, to]`.
    pub async fn history(&self, from: Instant, to: Instant, step: Duration) -> Result<History<T>>
    where
        V: AsyncApi<api::history>,
    {
        history(self.src, self.port, from, to, step, Agg::Mean).await
    }

    /// The aggregate `agg` per `step` of the samples in `[from, to]`.
    pub async fn history_agg(
        &self,
        from: Instant,
        to: Instant,
        step: Duration,
        agg: Agg,
    ) -> Result<History<T>>
    where
        V: AsyncApi<api::history>,
    {
        history(self.src, self.port, from, to, step, agg).await
    }
}

fn sample<T: PortValue>(src: Src<'_>, port: u32) -> Sample<T> {
    match src {
        Src::Cycle(cx) => cx.sample(port, |raw| T::from_bits(raw.value)),
        Src::Task(cx) if cx.bound(port) => {
            host::sample(&cx.host.read(port), |v| T::from_bits(host::bits(v)))
        }
        Src::Task(_) => Sample {
            value: T::from_bits(0),
            quality: Quality::bad(QualityReason::NotConnected),
            ts_src: None,
            ts_hw: None,
            ts_acq: Default::default(),
            seq: 0,
        },
    }
}

fn outcome<T: PortValue>(o: &types::WriteOutcome) -> WriteOutcome<T> {
    use types::WriteOutcome as W;
    match o {
        W::Accepted => WriteOutcome::Accepted,
        W::Clamped(c) => WriteOutcome::Clamped {
            by: match c.by {
                types::ClampKind::Range => crate::value::ClampKind::Range,
                types::ClampKind::Rate => crate::value::ClampKind::Rate,
            },
            applied: T::from_bits(host::bits(&c.applied)),
        },
        W::Suppressed => WriteOutcome::Suppressed,
        W::Overridden(level) => match Level::new(*level) {
            Some(level) => WriteOutcome::Overridden { level },
            None => WriteOutcome::Suppressed,
        },
        W::Deferred => WriteOutcome::Deferred,
        W::Shadowed => WriteOutcome::Shadowed,
        W::Rejected(r) => WriteOutcome::Rejected(crate::value::RejectReason::from_u8(*r as u8 + 1)),
        W::Tripped => WriteOutcome::Tripped,
    }
}

/// An output port.
#[derive(Debug, Clone, Copy)]
pub struct OutputHandle<'a, T, V> {
    src: Src<'a>,
    port: u32,
    name: &'static str,
    _t: PhantomData<(T, V)>,
}

/// An output in the cycle view.
pub type CycleOutput<'a, T> = OutputHandle<'a, T, Cycle>;
/// An output in the async view.
pub type TaskOutput<'a, T> = OutputHandle<'a, T, Task>;

/// A proposal that resolves after the commit of the owning group:
/// `.await` gives the outcome.
#[derive(Debug)]
pub struct WriteTicket<'a, T> {
    src: Src<'a>,
    port: u32,
    value: T,
}

impl<'a, T: PortValue> core::future::IntoFuture for WriteTicket<'a, T> {
    type Output = Result<WriteOutcome<T>>;
    type IntoFuture =
        core::pin::Pin<alloc::boxed::Box<dyn core::future::Future<Output = Self::Output> + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        alloc::boxed::Box::pin(async move {
            let Src::Task(cx) = self.src else {
                return Err(Error::new(
                    Code::from_static("J-E0901"),
                    "write in a cycle task",
                ));
            };
            let o = cx
                .host
                .write(self.port, host::wire(T::TYPE, self.value.to_bits()))
                .await
                .map_err(error)?;
            Ok(outcome(&o))
        })
    }
}

impl<'a, T: PortValue, V: View> OutputHandle<'a, T, V> {
    #[doc(hidden)]
    pub fn __new(src: Src<'a>, port: u32, name: &'static str) -> Self {
        Self {
            src,
            port,
            name,
            _t: PhantomData,
        }
    }

    /// Proposes `v` (renews the lease, docs/16 A2); from an async task,
    /// without waiting (`deferred` until the commit).
    pub fn set(&self, v: T) {
        match self.src {
            Src::Cycle(cx) => cx.buffer.borrow_mut().write(self.port, 0, v.to_bits()),
            Src::Task(cx) => cx.host.set(self.port, host::wire(T::TYPE, v.to_bits())),
        }
    }

    /// Proposes `v` and waits for the commit of the owning group.
    pub fn write(&self, v: T) -> WriteTicket<'a, T>
    where
        V: AsyncApi<api::write>,
    {
        WriteTicket {
            src: self.src,
            port: self.port,
            value: v,
        }
    }

    /// Releases this writer's level now.
    pub fn relinquish(&self) {
        match self.src {
            Src::Cycle(cx) => cx.buffer.borrow_mut().write(self.port, RELINQUISH, 0),
            Src::Task(cx) => cx.host.relinquish(self.port),
        }
    }

    /// The latest value this task proposed in this tick.
    pub fn pending(&self) -> Option<T>
    where
        V: CycleApi<api::pending>,
    {
        match self.src {
            Src::Cycle(cx) => cx.buffer.borrow().pending(self.port).map(T::from_bits),
            Src::Task(_) => None,
        }
    }

    /// The outcome of this writer's proposal in the owner group's latest
    /// commit; `None` if it had none.
    pub fn last_outcome(&self) -> Option<WriteOutcome<T>> {
        match self.src {
            Src::Cycle(cx) => {
                let (code, detail, _, _, applied, _) = cx.result(self.port)?;
                WriteOutcome::from_entry(code, detail, applied)
            }
            Src::Task(cx) => cx.host.last_outcome(self.port).map(|o| outcome(&o)),
        }
    }

    /// The applied value, if its sample is `Good`.
    pub fn get(&self) -> Result<T, QualityError> {
        let raw = self.src.raw(self.port);
        if raw.quality.is_good() {
            Ok(T::from_bits(raw.value))
        } else {
            Err(refused(self.name, raw.quality))
        }
    }

    /// The sample of the applied value.
    pub fn sample(&self) -> Sample<T> {
        sample(self.src, self.port)
    }

    /// This writer's active proposal (`mine()`, docs/17 §11).
    pub fn mine(&self) -> Option<T> {
        let Src::Cycle(cx) = self.src else {
            return None;
        };
        let (_, _, _, flags, _, mine) = cx.result(self.port)?;
        (flags & MINE_ACTIVE != 0).then(|| T::from_bits(mine))
    }

    /// The level that won the last commit; `None` when the output is at
    /// `safe`.
    pub fn winner(&self) -> Option<Level> {
        let Src::Cycle(cx) = self.src else {
            return None;
        };
        let (_, _, winner, _, _, _) = cx.result(self.port)?;
        Level::new(winner)
    }

    /// False for an optional port left unbound.
    pub fn is_bound(&self) -> bool {
        self.src.bound(self.port)
    }

    /// The mean per `step` of the applied values in `[from, to]`.
    pub async fn history(&self, from: Instant, to: Instant, step: Duration) -> Result<History<T>>
    where
        V: AsyncApi<api::history>,
    {
        history(self.src, self.port, from, to, step, Agg::Mean).await
    }
}

/// A parameter: the same handle in both views.
#[derive(Debug, Clone, Copy)]
pub struct ParamHandle<'a, T> {
    src: Src<'a>,
    port: u32,
    _t: PhantomData<T>,
}

impl<'a, T: PortValue> ParamHandle<'a, T> {
    #[doc(hidden)]
    pub fn __new(src: Src<'a>, port: u32, _name: &'static str) -> Self {
        Self {
            src,
            port,
            _t: PhantomData,
        }
    }

    /// The current value: a parameter always has one.
    pub fn get(&self) -> T {
        match self.src {
            Src::Cycle(cx) => T::from_bits(cx.raw(self.port).value),
            Src::Task(cx) => T::from_bits(host::bits(&cx.host.param(self.port))),
        }
    }
}

/// An `Internal<T>`, written by the domain `W`.
#[derive(Debug, Clone, Copy)]
pub struct InternalHandle<'a, T, V, W = AsyncWriter> {
    src: Src<'a>,
    port: u32,
    name: &'static str,
    _t: PhantomData<(T, V, W)>,
}

/// An `Internal<T>` in the cycle view.
pub type CycleInternal<'a, T, W = AsyncWriter> = InternalHandle<'a, T, Cycle, W>;
/// An `Internal<T>` in the async view.
pub type TaskInternal<'a, T, W = AsyncWriter> = InternalHandle<'a, T, Task, W>;

/// The largest encoded `Internal<T>` (ref/00 §10).
pub const INTERNAL_MAX: usize = 64;

impl<'a, T: Record, V: View, W> InternalHandle<'a, T, V, W> {
    #[doc(hidden)]
    pub fn __new(src: Src<'a>, port: u32, name: &'static str) -> Self {
        Self {
            src,
            port,
            name,
            _t: PhantomData,
        }
    }

    fn read(&self) -> (Quality, Option<T>) {
        match self.src {
            Src::Cycle(cx) => {
                let raw = cx.raw(self.port);
                let value = cx.wide(&raw).and_then(T::decode_from);
                (raw.quality, value)
            }
            Src::Task(cx) if cx.bound(self.port) => {
                let s = cx.host.read(self.port);
                let value = match &s.value {
                    types::Value::Blob(b) => T::decode_from(b),
                    _ => None,
                };
                (host::quality(&s.quality), value)
            }
            Src::Task(_) => (Quality::bad(QualityReason::NotConnected), None),
        }
    }

    /// The value if `Good`; `Uncertain(Initial)` until the first write.
    pub fn get(&self) -> Result<T, QualityError> {
        match self.read() {
            (q, Some(v)) if q.is_good() => Ok(v),
            (q, _) if !q.is_good() => Err(refused(self.name, q)),
            _ => Err(refused(self.name, Quality::bad(QualityReason::ConfigError))),
        }
    }

    /// The value if `Good`; otherwise `default`.
    pub fn get_or(&self, default: T) -> T {
        self.get().unwrap_or(default)
    }

    /// The value if `Good`; otherwise `T::default()`.
    pub fn get_or_default(&self) -> T
    where
        T: Default,
    {
        self.get().unwrap_or_default()
    }

    /// The sample; its value is `T::default()` without a `Good` one.
    pub fn sample(&self) -> Sample<T>
    where
        T: Default,
    {
        let (quality, value) = self.read();
        let mut s = match self.src {
            Src::Cycle(cx) => cx.sample(self.port, |_| ()),
            Src::Task(cx) => host::sample(&cx.host.read(self.port), |_| ()),
        };
        s.quality = quality;
        Sample {
            value: value.unwrap_or_default(),
            quality: s.quality,
            ts_src: s.ts_src,
            ts_hw: s.ts_hw,
            ts_acq: s.ts_acq,
            seq: s.seq,
        }
    }

    /// Writes the value, from the view of the declared writer.
    pub fn set(&self, v: T)
    where
        V: Writes<W>,
    {
        let mut buf = [0u8; INTERNAL_MAX];
        let Some(n) = v.encode_into(&mut buf) else {
            return;
        };
        match self.src {
            Src::Cycle(cx) => cx.buffer.borrow_mut().publish_inline(self.port, &buf[..n]),
            Src::Task(cx) => cx
                .host
                .publish(self.port, types::Value::Blob(buf[..n].to_vec())),
        }
    }
}

/// A `Published<T>`: the same handle in both views.
#[derive(Debug, Clone, Copy)]
pub struct PublishedHandle<'a, T> {
    src: Src<'a>,
    port: u32,
    _t: PhantomData<T>,
}

impl<'a, T: PortValue> PublishedHandle<'a, T> {
    #[doc(hidden)]
    pub fn __new(src: Src<'a>, port: u32, _name: &'static str) -> Self {
        Self {
            src,
            port,
            _t: PhantomData,
        }
    }

    /// Publishes `v`.
    pub fn set(&self, v: T) {
        match self.src {
            Src::Cycle(cx) => cx.buffer.borrow_mut().publish(self.port, v.to_bits()),
            Src::Task(cx) => cx.host.publish(self.port, host::wire(T::TYPE, v.to_bits())),
        }
    }
}

/// A `Counter`: the same handle in both views.
#[derive(Debug, Clone, Copy)]
pub struct CounterHandle<'a> {
    src: Src<'a>,
    port: u32,
}

impl<'a> CounterHandle<'a> {
    #[doc(hidden)]
    pub fn __new(src: Src<'a>, port: u32, _name: &'static str) -> Self {
        Self { src, port }
    }

    /// Adds one.
    pub fn inc(&self) {
        self.add(1);
    }

    /// Adds `n`.
    pub fn add(&self, n: u64) {
        match self.src {
            Src::Cycle(cx) => cx.buffer.borrow_mut().counter_add(self.port, n),
            Src::Task(cx) => cx.host.counter_add(self.port, n),
        }
    }
}

/// An `Alarm`: the same handle in both views.
#[derive(Debug, Clone, Copy)]
pub struct AlarmHandle<'a> {
    src: Src<'a>,
    port: u32,
}

impl<'a> AlarmHandle<'a> {
    #[doc(hidden)]
    pub fn __new(src: Src<'a>, port: u32, _name: &'static str) -> Self {
        Self { src, port }
    }

    /// The condition is true.
    pub fn raise(&self) {
        self.set(true);
    }

    /// The condition is false.
    pub fn clear(&self) {
        self.set(false);
    }

    /// Sets the condition: `raise()` and `clear()` in a single call.
    pub fn set(&self, active: bool) {
        match self.src {
            Src::Cycle(cx) => cx.buffer.borrow_mut().publish(self.port, u64::from(active)),
            Src::Task(cx) => cx.host.publish(self.port, types::Value::Boolean(active)),
        }
    }

    /// The state of the alarm (`alarm.app.<app>.<name>.state`), from the
    /// latch in a cycle task and from the slot in an async one.
    pub fn state(&self) -> AlarmState {
        let raw = self.src.raw(self.port);
        if raw.quality.is_bad() {
            AlarmState::Unknown
        } else {
            AlarmState::from_code(raw.value)
        }
    }
}

/// An `EventOutput<E>`: the same handle in both views.
#[derive(Debug, Clone, Copy)]
pub struct EventOutputHandle<'a, E> {
    src: Src<'a>,
    port: u32,
    _e: PhantomData<E>,
}

impl<'a, E: Event> EventOutputHandle<'a, E> {
    #[doc(hidden)]
    pub fn __new(src: Src<'a>, port: u32, _name: &'static str) -> Self {
        Self {
            src,
            port,
            _e: PhantomData,
        }
    }

    /// Emits `e`. In a cycle task it is encoded in place in the proposal
    /// buffer, without allocating.
    pub fn emit(&self, e: E) {
        match self.src {
            Src::Cycle(cx) => cx.buffer.borrow_mut().emit_with(self.port, |w| e.encode(w)),
            Src::Task(cx) => {
                let mut buf = alloc::vec![0u8; E::MAX_LEN];
                if let Some(n) = e.encode_into(&mut buf) {
                    buf.truncate(n);
                    cx.host.emit(self.port, buf);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::{CycleCx, ENTRY, HEADER, Layout, ProposalBuffer};
    use crate::value::{QualityCode, QualityReason};
    use alloc::vec::Vec;

    const NOTE: u8 = 6;
    const WRITE: u8 = 1;

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
                (0, 2, 16, crate::io::UNBOUND),
            ],
            &[(2, 0, 12, MINE_ACTIVE, 1, 1)],
        );
        let mut bytes = [0u8; 1024];
        let mut pending = [None; 3];
        let cx = CycleCx::new(
            &img,
            &LAYOUT,
            "t",
            ProposalBuffer::new(&mut bytes, &mut pending),
        );
        let t: CycleInput<'_, f64> = InputHandle::__new(Src::Cycle(&cx), 0, "temp");
        assert_eq!(
            t.get().unwrap_err().quality,
            Quality::uncertain(QualityReason::Initial)
        );
        assert_eq!(t.get_uncertain(), Ok(21.5));
        assert_eq!(t.get_or(0.0), 0.0);
        assert!(t.changed());
        assert_eq!(t.sample().seq, 7);
        let off: CycleInput<'_, f64> = InputHandle::__new(Src::Cycle(&cx), 2, "spare");
        assert!(!off.is_bound());
        assert_eq!(off.get().unwrap_err().quality.code, QualityCode::Bad);
        let led: CycleOutput<'_, bool> = OutputHandle::__new(Src::Cycle(&cx), 1, "led");
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
        let (len, full) = cx.finish();
        assert!(!full);
        let out = &bytes[..len];
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
        let mut bytes = [0u8; 8 + 14 * 2];
        let mut pending = [None; 1];
        let cx = CycleCx::new(
            &img,
            &layout,
            "t",
            ProposalBuffer::new(&mut bytes, &mut pending),
        );
        let o: CycleOutput<'_, u64> = OutputHandle::__new(Src::Cycle(&cx), 0, "o");
        o.set(1);
        o.set(2);
        o.set(3);
        assert_eq!(cx.finish(), (36, true));
        assert_eq!(&bytes[..8], &[1, 0, 2, 0, 36, 0, 0, 0]);
    }

    #[test]
    fn a_buffer_without_room_for_its_header_holds_nothing() {
        let img = image(&[(0, 0, 0, 0)], &[]);
        let layout = [Layout {
            slot: Some(0),
            outcome: Some(0),
            bound: true,
        }];
        let mut pending = [None; 1];
        let cx = CycleCx::new(
            &img,
            &layout,
            "t",
            ProposalBuffer::new(&mut [], &mut pending),
        );
        let o: CycleOutput<'_, u64> = OutputHandle::__new(Src::Cycle(&cx), 0, "o");
        o.set(1);
        assert_eq!(o.pending(), None);
        assert_eq!(cx.finish(), (0, true));
    }

    #[test]
    fn publish_counter_and_alarm_records() {
        let img = image(&[(1, 0, 0, 0)], &[]);
        let layout = [Layout {
            slot: Some(0),
            outcome: None,
            bound: true,
        }];
        let mut bytes = [0u8; 128];
        let mut pending = [None; 1];
        let cx = CycleCx::new(
            &img,
            &layout,
            "t",
            ProposalBuffer::new(&mut bytes, &mut pending),
        );
        let a = AlarmHandle::__new(Src::Cycle(&cx), 0, "stale");
        assert_eq!(a.state(), AlarmState::ActiveUnacked);
        a.raise();
        CounterHandle::__new(Src::Cycle(&cx), 0, "n").add(5);
        let (len, full) = cx.finish();
        assert!(!full);
        assert_eq!(&bytes[..4], &[1, 0, 2, 0]);
        assert_eq!(&bytes[8..14], &[3, 0, 0, 0, 0, 0]);
        assert_eq!(bytes[14], 1);
        assert_eq!(bytes[22], 4);
        assert_eq!(bytes[28], 5);
        assert_eq!(len, 36);
    }
}
