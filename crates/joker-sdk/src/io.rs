//! The views of an app (ref/04 §4.4–§4.5): the declaration types, the
//! `CycleIo` view over the packed image and the proposal buffer (ref/04 §3),
//! and the `TaskIo` view over the `async-io` host.

use alloc::boxed::Box;
use core::any::Any;
use core::cell::RefCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::time::Duration;

use crate::cbor::Writer;
use crate::error::{Code, Error, Result};
use crate::host::{self, TaskHost};
use crate::time::{ClockQuality, Instant, MonoNanos, UtcNanos};
use crate::value::{Quality, QualityReason, Sample};

/// What `#[joker::app]` implements for the app struct. Not implemented by
/// hand.
pub trait App: 'static {
    /// The cycle view: one handle per port.
    type Cycle<'a>;
    /// The async view: one handle per port.
    type Task<'a>;
    /// The app name.
    const NAME: &'static str;
    /// The ports, in declaration order (the port table of ref/04 §2.6.2).
    const PORTS: &'static [PortDecl];
    /// Builds the cycle view over one call's image and buffer.
    fn cycle_view<'a>(cx: &'a CycleCx<'a>) -> Self::Cycle<'a>;
    /// Builds the async view of one task run.
    fn task_view(cx: TaskCx) -> Self::Task<'static>;
}

/// The kind of a port (`port-kind` of `joker:meta`).
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortKind {
    /// `#[input]`.
    Input,
    /// `#[output]`.
    Output,
    /// `#[param]`.
    Param,
    /// `#[internal]`.
    Internal,
    /// `#[publish]` of a `Published<T>`.
    Published,
    /// `#[publish]` of a `Counter`.
    Counter,
    /// `#[output(event)]`.
    EventOut,
    /// `#[alarm]`.
    Alarm,
}

impl PortKind {
    /// True when the port has a sample in the cycle image (ref/04 §3.2).
    pub const fn has_sample(self) -> bool {
        matches!(
            self,
            Self::Input | Self::Output | Self::Param | Self::Internal | Self::Alarm
        )
    }
}

/// A port as the macro declares it.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PortDecl {
    /// Field name.
    pub name: &'static str,
    /// Kind.
    pub kind: PortKind,
    /// Signal type code; 9 for a record.
    pub ty: u8,
}

/// The index of the port `name` of kind in `kinds`, at compile time: the
/// check of `on = "…"` (`J-E0713`).
#[doc(hidden)]
pub const fn port_index(ports: &[PortDecl], name: &str, kinds: &[PortKind]) -> Option<usize> {
    let mut i = 0;
    while i < ports.len() {
        if str_eq(ports[i].name, name) {
            let mut k = 0;
            while k < kinds.len() {
                if kinds[k] as u8 == ports[i].kind as u8 {
                    return Some(i);
                }
                k += 1;
            }
            return None;
        }
        i += 1;
    }
    None
}

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// True when `path` (a `module_path!()`) is the root of its crate: the check
/// of `J-E0711`.
#[doc(hidden)]
pub const fn is_crate_root(path: &str) -> bool {
    let b = path.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b':' && b[i + 1] == b':' {
            return false;
        }
        i += 1;
    }
    true
}

macro_rules! declaration {
    ($(#[$doc:meta])* $name:ident < $($p:ident),* >) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub struct $name<$($p),*>(PhantomData<($($p,)*)>);
    };
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub struct $name;
    };
}

declaration!(
    /// An input port of type `T` (a declaration: the app never builds it).
    Input<T>
);
declaration!(
    /// An output port of type `T`.
    Output<T>
);
declaration!(
    /// A parameter of type `T`.
    Param<T>
);
declaration!(
    /// A record shared between the two instances of the app, with one
    /// declared writer (docs/04 §3.4).
    Internal<T>
);
declaration!(
    /// A signal the app publishes, at `app.<app>.<field>`.
    Published<T>
);
declaration!(
    /// A counter the app publishes.
    Counter
);
declaration!(
    /// An alarm of the app, with the id `app.<app>.<field>`.
    Alarm
);
declaration!(
    /// An event the app emits.
    EventOutput<E>
);

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

pub(crate) const HEADER: usize = 64;
pub(crate) const ENTRY: usize = 24;
const HEADER_LEN: usize = 8;
const WRITE: u8 = 1;
const PUBLISH: u8 = 3;
const COUNTER_ADD: u8 = 4;
const EMIT: u8 = 5;
const NOTE: u8 = 6;
pub(crate) const RELINQUISH: u8 = 1;
const INLINE: u8 = 1 << 1;
const WIDE: u16 = 1;
pub(crate) const UNBOUND: u16 = 1 << 4;
pub(crate) const CHANGED: u16 = 1 << 3;
const HAS_TS_SRC: u16 = 1 << 2;
const HAS_TS_HW: u16 = 1 << 1;
pub(crate) const MINE_ACTIVE: u8 = 1;
const EVENT_MAX: usize = 8 * 1024;

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

/// The proposal buffer of one call (ref/04 §3.3), written in place into the
/// buffer `joker-sdk-sys` reserved at `init` (ref/04 §4.13).
#[doc(hidden)]
#[derive(Debug)]
pub struct ProposalBuffer<'a> {
    bytes: &'a mut [u8],
    len: usize,
    records: u16,
    full: bool,
    pending: &'a mut [Option<u64>],
}

impl<'a> ProposalBuffer<'a> {
    /// An empty buffer over `bytes`, with one pending slot per port. A
    /// buffer shorter than its 8-byte header holds nothing: every proposal
    /// is dropped.
    pub fn new(bytes: &'a mut [u8], pending: &'a mut [Option<u64>]) -> Self {
        pending.fill(None);
        let mut b = Self {
            bytes,
            len: 0,
            records: 0,
            full: false,
            pending,
        };
        match b.bytes.get_mut(..HEADER_LEN) {
            Some(h) => {
                h.copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]);
                b.len = HEADER_LEN;
            }
            None => b.full = true,
        }
        b
    }

    fn push_parts(&mut self, parts: &[&[u8]]) -> bool {
        let n: usize = parts.iter().map(|p| p.len()).sum();
        let end = self.len + n;
        if self.full || end > self.bytes.len() || self.records == u16::MAX {
            self.full = true;
            return false;
        }
        let mut at = self.len;
        for p in parts {
            if let Some(dst) = self.bytes.get_mut(at..at + p.len()) {
                dst.copy_from_slice(p);
            }
            at += p.len();
        }
        self.len = end;
        self.records += 1;
        true
    }

    fn head(kind: u8, flags: u8, port: u32, value: u64) -> [u8; 14] {
        let mut r = [0u8; 14];
        r[0] = kind;
        r[1] = flags;
        r[2..6].copy_from_slice(&port.to_le_bytes());
        r[6..14].copy_from_slice(&value.to_le_bytes());
        r
    }

    pub(crate) fn write(&mut self, port: u32, flags: u8, value: u64) {
        let r = Self::head(WRITE, flags, port, value);
        if self.push_parts(&[&r])
            && let Some(p) = self.pending.get_mut(port as usize)
        {
            *p = (flags & RELINQUISH == 0).then_some(value);
        }
    }

    pub(crate) fn publish(&mut self, port: u32, value: u64) {
        let r = Self::head(PUBLISH, 0, port, value);
        self.push_parts(&[&r]);
    }

    pub(crate) fn publish_inline(&mut self, port: u32, data: &[u8]) {
        let r = Self::head(PUBLISH, INLINE, port, data.len() as u64);
        self.push_parts(&[&r, data]);
    }

    pub(crate) fn counter_add(&mut self, port: u32, delta: u64) {
        let r = Self::head(COUNTER_ADD, 0, port, delta);
        self.push_parts(&[&r]);
    }

    pub(crate) fn note(&mut self, port: u32) {
        let mut r = [0u8; 6];
        r[0] = NOTE;
        r[2..6].copy_from_slice(&port.to_le_bytes());
        self.push_parts(&[&r]);
    }

    /// An `EMIT` record whose payload `encode` writes in place, without a
    /// copy on the heap.
    pub(crate) fn emit_with(&mut self, port: u32, encode: impl FnOnce(&mut Writer<'_>)) {
        let start = self.len;
        let body = start + 8;
        if self.full || self.records == u16::MAX || body > self.bytes.len() {
            self.full = true;
            return;
        }
        let end = self.bytes.len().min(body + EVENT_MAX);
        let written = match self.bytes.get_mut(body..end) {
            Some(room) => {
                let mut w = Writer::new(room);
                encode(&mut w);
                w.finish()
            }
            None => None,
        };
        let Some(n) = written else {
            self.full = true;
            return;
        };
        if let Some(h) = self.bytes.get_mut(start..body) {
            h[0] = EMIT;
            h[1] = 0;
            h[2..6].copy_from_slice(&port.to_le_bytes());
            h[6..8].copy_from_slice(&(n as u16).to_le_bytes());
        }
        self.len = body + n;
        self.records += 1;
    }

    pub(crate) fn pending(&self, port: u32) -> Option<u64> {
        self.pending.get(port as usize).copied().flatten()
    }

    /// True when a record did not fit (`J-E0203`).
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Completes the header and returns the length of the buffer handed to
    /// the host.
    pub fn finish(&mut self) -> usize {
        if let Some(h) = self.bytes.get_mut(..HEADER_LEN) {
            h[2..4].copy_from_slice(&self.records.to_le_bytes());
            h[4..8].copy_from_slice(&(self.len as u32).to_le_bytes());
        }
        self.len
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
    pub(crate) buffer: RefCell<ProposalBuffer<'a>>,
}

/// One sample entry of the image, or the sample of an async read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Raw {
    pub value: u64,
    pub quality: Quality,
    pub flags: u16,
    pub seq: u64,
    pub ts_acq: u64,
}

impl Raw {
    pub(crate) const UNBOUND: Raw = Raw {
        value: 0,
        quality: Quality::bad(QualityReason::NotConnected),
        flags: UNBOUND,
        seq: 0,
        ts_acq: 0,
    };
}

impl<'a> CycleCx<'a> {
    /// A call over `image`, with the layout from `init` and the call's
    /// buffer (completed with [`CycleCx::finish`]).
    pub fn new(
        image: &'a [u8],
        layout: &'a [Layout],
        task: &'static str,
        buffer: ProposalBuffer<'a>,
    ) -> Self {
        Self {
            image,
            layout,
            n_slots: u32_at(image, 44) as usize,
            task,
            buffer: RefCell::new(buffer),
        }
    }

    /// Completes the proposal buffer: its length, and whether a proposal
    /// was dropped because it did not fit (`J-E0203`).
    pub fn finish(&self) -> (usize, bool) {
        let mut buffer = self.buffer.borrow_mut();
        (buffer.finish(), buffer.is_full())
    }

    pub(crate) fn layout(&self, port: u32) -> Layout {
        self.layout.get(port as usize).copied().unwrap_or_default()
    }

    pub(crate) fn raw(&self, port: u32) -> Raw {
        let l = self.layout(port);
        let Some(slot) = l.slot.filter(|_| l.bound) else {
            return Raw::UNBOUND;
        };
        let at = HEADER + ENTRY * slot as usize;
        Raw {
            value: u64_at(self.image, at),
            quality: Quality::from_codes(
                self.image.get(at + 8).copied().unwrap_or(2),
                self.image.get(at + 9).copied().unwrap_or(0),
            ),
            flags: u16_at(self.image, at + 10),
            seq: u64::from(u32_at(self.image, at + 12)),
            ts_acq: u64_at(self.image, at + 16),
        }
    }

    /// The bytes of a wide value (`string`, `bytes`, `Internal<T>`).
    pub(crate) fn wide(&self, raw: &Raw) -> Option<&'a [u8]> {
        if raw.flags & WIDE == 0 {
            return None;
        }
        let off = (raw.value & 0xffff_ffff) as usize;
        let len = (raw.value >> 32) as usize;
        self.image.get(off..off.checked_add(len)?)
    }

    pub(crate) fn sample<T>(&self, port: u32, value: impl FnOnce(&Raw) -> T) -> Sample<T> {
        let raw = self.raw(port);
        let (ts_src, ts_hw) = if raw.flags & (HAS_TS_SRC | HAS_TS_HW) != 0 {
            let s = crate::runtime::read(port);
            (s.0.map(UtcNanos), s.1.map(MonoNanos))
        } else {
            (None, None)
        };
        Sample {
            value: value(&raw),
            quality: raw.quality,
            ts_src,
            ts_hw,
            ts_acq: MonoNanos(raw.ts_acq),
            seq: raw.seq,
        }
    }

    pub(crate) fn result(&self, port: u32) -> Option<(u8, u8, u8, u8, u64, u64)> {
        let o = self.layout(port).outcome?;
        let at = HEADER + ENTRY * (self.n_slots + o as usize);
        let e = self.image.get(at..at + ENTRY)?;
        Some((e[0], e[1], e[2], e[3], u64_at(e, 8), u64_at(e, 16)))
    }

    fn flags(&self) -> u16 {
        u16_at(self.image, 2)
    }
}

/// One run of an async task: the host and the port layout of the async
/// instance, all of it `'static`, so that the async view needs no lifetime.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct TaskCx {
    pub(crate) host: &'static dyn TaskHost,
    layout: &'static [Layout],
    task: &'static str,
}

impl core::fmt::Debug for TaskCx {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TaskCx")
            .field("task", &self.task)
            .finish_non_exhaustive()
    }
}

impl TaskCx {
    /// A run of `task` over the layout of the async instance.
    pub fn new(layout: &'static [Layout], task: &'static str) -> Self {
        Self {
            host: host::host(),
            layout,
            task,
        }
    }

    pub(crate) fn bound(&self, port: u32) -> bool {
        self.layout.get(port as usize).is_some_and(|l| l.bound)
    }

    /// The async view of `A` for this run.
    pub fn io<A: App>(self) -> TaskIo<A> {
        TaskIo {
            cx: self,
            view: A::task_view(self),
        }
    }
}

/// Where a handle reads and writes: the call of a cycle task or the run of
/// an async task.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum Src<'a> {
    /// A cycle task.
    Cycle(&'a CycleCx<'a>),
    /// An async task.
    Task(TaskCx),
}

impl<'a> Src<'a> {
    pub(crate) fn raw(self, port: u32) -> Raw {
        match self {
            Src::Cycle(cx) => cx.raw(port),
            Src::Task(cx) => {
                if !cx.bound(port) {
                    return Raw::UNBOUND;
                }
                let s = cx.host.read(port);
                Raw {
                    value: host::bits(&s.value),
                    quality: host::quality(&s.quality),
                    flags: 0,
                    seq: s.seq,
                    ts_acq: s.ts_acq,
                }
            }
        }
    }

    pub(crate) fn bound(self, port: u32) -> bool {
        match self {
            Src::Cycle(cx) => cx.layout(port).bound,
            Src::Task(cx) => cx.bound(port),
        }
    }
}

/// The initial state of a stateful cycle task, created at `init`, where
/// allocation is allowed.
#[doc(hidden)]
pub fn new_state<S: Default + Send + 'static>() -> Option<Box<dyn Any + Send>> {
    Some(Box::new(S::default()))
}

/// The call of one cycle task.
#[doc(hidden)]
pub struct CycleRun<'a> {
    cx: &'a CycleCx<'a>,
    state: &'a mut Option<Box<dyn Any + Send>>,
}

impl core::fmt::Debug for CycleRun<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CycleRun")
            .field("task", &self.cx.task)
            .finish_non_exhaustive()
    }
}

impl<'a> CycleRun<'a> {
    /// A call over `cx`, with the task's state slot.
    pub fn new(cx: &'a CycleCx<'a>, state: &'a mut Option<Box<dyn Any + Send>>) -> Self {
        Self { cx, state }
    }

    /// Runs a task with state `S`, created at `init`: a cycle task cannot
    /// allocate it (ref/04 §4.13).
    pub fn run<A: App, S: Send + 'static>(
        &mut self,
        f: fn(&mut CycleIo<'_, A>, &mut S) -> Result<()>,
    ) -> Result<()> {
        let st = self
            .state
            .as_mut()
            .and_then(|s| s.downcast_mut::<S>())
            .ok_or_else(|| {
                Error::new(
                    Code::from_static("J-E0008"),
                    "task state missing or of another type",
                )
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

/// The view of an async task (ref/04 §4.5), shared by the tasks of the app.
/// It has no lifetime parameter, so that `async fn f(io: &TaskIo<App>)`
/// compiles (ADR 0106).
pub struct TaskIo<A: App> {
    cx: TaskCx,
    view: A::Task<'static>,
}

impl<A: App> core::fmt::Debug for TaskIo<A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TaskIo")
            .field("app", &A::NAME)
            .field("task", &self.cx.task)
            .finish_non_exhaustive()
    }
}

impl<A: App> Deref for TaskIo<A> {
    type Target = A::Task<'static>;
    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

impl<A: App> TaskIo<A> {
    /// The current instant.
    pub fn now(&self) -> Instant {
        host::instant(&self.cx.host.now())
    }

    /// Waits for `d`.
    pub async fn sleep(&self, d: Duration) {
        let until = self.now().mono + d;
        self.cx.host.sleep_until(until.0).await;
    }

    /// Waits until the monotonic instant `t` (right away if it passed).
    pub async fn sleep_until(&self, t: MonoNanos) {
        self.cx.host.sleep_until(t.0).await;
    }

    /// A number of the instance's deterministic generator (ref/04 §2.7).
    pub fn random(&self) -> u64 {
        self.cx.host.random()
    }

    /// The task name.
    pub fn task(&self) -> &'static str {
        self.cx.task
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_time_lookups() {
        const PORTS: &[PortDecl] = &[
            PortDecl {
                name: "soc",
                kind: PortKind::Input,
                ty: 3,
            },
            PortDecl {
                name: "battery",
                kind: PortKind::Output,
                ty: 3,
            },
        ];
        assert_eq!(port_index(PORTS, "battery", &[PortKind::Output]), Some(1));
        assert_eq!(port_index(PORTS, "battery", &[PortKind::Input]), None);
        assert_eq!(port_index(PORTS, "x", &[PortKind::Input]), None);
        assert!(is_crate_root("peak_shaver"));
        assert!(!is_crate_root("peak_shaver::inner"));
    }
}
