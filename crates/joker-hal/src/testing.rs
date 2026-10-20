//! Test support for drivers (feature `testing`): a virtual-time host and
//! executor, an in-memory serial line, a minimal bus scheduler, and a real
//! envelope to obtain committed writes from.
//!
//! Nothing here bypasses the envelope: [`OutputHarness`] stages manual
//! proposals in a [`joker_core::ProcessImage`] and commits them through
//! [`joker_core::Outputs`], exactly as a cycle group does.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use std::boxed::Box;
use std::collections::VecDeque;
use std::string::String;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::vec;
use std::vec::Vec;

use joker_core::commit::OutputState;
use joker_core::image::ImageStorage;
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::{
    ClockQuality, Instant, Level, OutputConfig, Outputs, OutputsStorage, ProcessImage, Proposal,
    Registry, RegistryBuilder, SafeValue, Sample, SignalId, Slot, ValueType, WriterConfig,
    WriterId, WriterKind,
};

use crate::acquire::{SampleBuffer, Settled, settle};
use crate::commit::{OutputBinding, RawEncoding};
use crate::cx::{Cx, Host, LogLevel};
use crate::driver::{
    AckSlot, CommittedWrite, ConnectOut, Driver, OpenArgs, RawSink, ReadBatch, WriteAck, WriteAcks,
};
use crate::link::{IoError, Link, SerialInfo, SerialIo};
use crate::plan::PlanView;
use crate::spec::PlanRequest;
use crate::types::{BatchId, BatchKind, DriverError, MonoNs};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A host whose clock only moves when told to, or when the executor finds
/// every task waiting for a sleep.
#[derive(Debug, Default)]
pub struct VirtualHost {
    now: AtomicU64,
    wake: Mutex<Option<MonoNs>>,
    cancelled: AtomicBool,
    logs: Mutex<Vec<(LogLevel, String)>>,
}

/// The future waits for something that is not a sleep: it would never wake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stalled;

impl VirtualHost {
    /// A host at monotonic time `start_ns`.
    pub fn new(start_ns: MonoNs) -> Self {
        Self {
            now: AtomicU64::new(start_ns),
            ..Self::default()
        }
    }

    /// Monotonic now.
    pub fn mono(&self) -> MonoNs {
        self.now.load(Ordering::Acquire)
    }

    /// Moves time forward by `ns`.
    pub fn advance(&self, ns: u64) {
        self.now.fetch_add(ns, Ordering::AcqRel);
    }

    /// Moves time forward to `at`; never backwards.
    pub fn advance_to(&self, at: MonoNs) {
        self.now.fetch_max(at, Ordering::AcqRel);
    }

    /// Sets the cancellation flag seen by `Cx::cancelled`.
    pub fn set_cancelled(&self, on: bool) {
        self.cancelled.store(on, Ordering::Release);
    }

    /// The log lines received.
    pub fn logs(&self) -> Vec<(LogLevel, String)> {
        lock(&self.logs).clone()
    }

    /// Polls `fut` to completion, jumping the clock to the earliest pending
    /// sleep whenever the future waits. Fails with [`Stalled`] when it waits
    /// for something else.
    pub fn run<F: Future>(&self, fut: F) -> Result<F::Output, Stalled> {
        let mut fut = pin!(fut);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            *lock(&self.wake) = None;
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return Ok(v);
            }
            match lock(&self.wake).take() {
                Some(at) => self.advance_to(at),
                None => return Err(Stalled),
            }
        }
    }
}

impl joker_core::Clock for VirtualHost {
    fn now(&self) -> Instant {
        let mono_ns = self.mono();
        Instant {
            mono_ns,
            utc_ns: 1_700_000_000_000_000_000 + mono_ns as i64,
            clock: ClockQuality::Synced,
        }
    }
}

impl Host for VirtualHost {
    fn poll_sleep(&self, at: MonoNs, _: &mut Context<'_>) -> Poll<()> {
        if self.mono() >= at {
            return Poll::Ready(());
        }
        let mut wake = lock(&self.wake);
        *wake = Some(wake.map_or(at, |w| w.min(at)));
        Poll::Pending
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn log(&self, level: LogLevel, message: &str) {
        lock(&self.logs).push((level, String::from(message)));
    }
}

/// How an in-memory serial line misbehaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineFault {
    /// Works.
    None,
    /// Bytes go nowhere and nothing comes back: a cut cable on a bus.
    Silent,
    /// The port is gone: a USB bridge unplugged.
    Gone,
}

/// The equipment on the far end of a [`MemorySerial`]: given the bytes of
/// one write and the time, it returns what it sends back.
pub type Responder = Box<dyn FnMut(&[u8], MonoNs) -> Vec<u8> + Send>;

struct LineState {
    rx: VecDeque<u8>,
    written: Vec<u8>,
    fault: LineFault,
    responder: Responder,
    waker: Option<Waker>,
}

/// Control of a [`MemorySerial`] from the test.
#[derive(Clone)]
pub struct LineHandle {
    state: Arc<Mutex<LineState>>,
    host: Arc<VirtualHost>,
}

impl core::fmt::Debug for LineHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LineHandle").finish_non_exhaustive()
    }
}

impl LineHandle {
    /// Sets the fault.
    pub fn set_fault(&self, fault: LineFault) {
        let mut s = lock(&self.state);
        s.fault = fault;
        if let Some(w) = s.waker.take() {
            w.wake();
        }
    }

    /// Queues bytes for the driver to read, as if the equipment sent them.
    pub fn inject(&self, bytes: &[u8]) {
        lock(&self.state).rx.extend(bytes);
    }

    /// Every byte the driver wrote.
    pub fn written(&self) -> Vec<u8> {
        lock(&self.state).written.clone()
    }

    /// The host whose clock the line passes to the responder.
    pub fn host(&self) -> &VirtualHost {
        &self.host
    }
}

/// An in-memory serial line whose far end is a [`Responder`].
pub struct MemorySerial {
    state: Arc<Mutex<LineState>>,
    host: Arc<VirtualHost>,
    info: SerialInfo,
}

impl core::fmt::Debug for MemorySerial {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemorySerial")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl MemorySerial {
    /// A line with `info` settings whose far end is `responder`, stamped
    /// with `host`'s clock.
    pub fn new(
        host: Arc<VirtualHost>,
        info: SerialInfo,
        responder: Responder,
    ) -> (Self, LineHandle) {
        let state = Arc::new(Mutex::new(LineState {
            rx: VecDeque::new(),
            written: Vec::new(),
            fault: LineFault::None,
            responder,
            waker: None,
        }));
        let handle = LineHandle {
            state: state.clone(),
            host: host.clone(),
        };
        (Self { state, host, info }, handle)
    }
}

impl SerialIo for MemorySerial {
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        let mut s = lock(&self.state);
        if s.fault == LineFault::Gone {
            return Poll::Ready(Err(IoError::Disconnected));
        }
        if s.rx.is_empty() || buf.is_empty() {
            s.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let n = buf.len().min(s.rx.len());
        for b in buf.iter_mut().take(n) {
            *b = s.rx.pop_front().unwrap_or(0);
        }
        Poll::Ready(Ok(n))
    }

    fn poll_write(&mut self, _: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        let now = self.host.mono();
        let mut s = lock(&self.state);
        match s.fault {
            LineFault::Gone => return Poll::Ready(Err(IoError::Disconnected)),
            LineFault::Silent => {}
            LineFault::None => {
                let reply = (s.responder)(buf, now);
                s.rx.extend(reply);
            }
        }
        s.written.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        if lock(&self.state).fault == LineFault::Gone {
            return Poll::Ready(Err(IoError::Disconnected));
        }
        Poll::Ready(Ok(()))
    }

    fn discard_input(&mut self) -> Result<(), IoError> {
        lock(&self.state).rx.clear();
        Ok(())
    }

    fn info(&self) -> SerialInfo {
        self.info
    }
}

/// A real registry, process image and envelope with one manual writer, from
/// which committed writes are obtained in driver tests.
pub struct OutputHarness {
    registry: Registry<'static>,
    slots: &'static [Slot],
    image: ProcessImage<'static>,
    outputs: Outputs<'static>,
    points: Vec<u32>,
    tick: u64,
}

impl core::fmt::Debug for OutputHarness {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OutputHarness")
            .field("tick", &self.tick)
            .finish_non_exhaustive()
    }
}

/// Period of the harness's cycle group.
pub const HARNESS_PERIOD_NS: u64 = 10_000_000;

fn leak<T>(v: Vec<T>) -> &'static mut [T] {
    Box::leak(v.into_boxed_slice())
}

/// The harness could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HarnessError(pub &'static str);

impl OutputHarness {
    /// Outputs `(path, type, safe bits, driver point)`, all in group `test`
    /// and written by manual writer 0.
    pub fn new(outputs: &[(&'static str, ValueType, u64, u32)]) -> Result<Self, HarnessError> {
        let n = outputs.len();
        let signals = leak(vec![SignalMeta::EMPTY; n]);
        let aliases = leak(Vec::<Alias<'static>>::new());
        let groups = leak(vec![GroupMeta::EMPTY; 1]);
        let mut b = RegistryBuilder::new(signals, aliases, groups);
        b.group(GroupMeta {
            name: "test",
            period_ns: HARNESS_PERIOD_NS,
        })
        .map_err(|_| HarnessError("group"))?;
        for (path, ty, _, _) in outputs {
            b.signal(SignalMeta::output(path, *ty, UnitRef::ONE, "test"))
                .map_err(|_| HarnessError("signal"))?;
        }
        let registry = b.build().map_err(|_| HarnessError("registry"))?;
        let group = registry
            .resolve_group("test")
            .ok_or(HarnessError("group"))?;
        let mut configs = Vec::new();
        let mut points = Vec::new();
        for (path, ty, safe, point) in outputs {
            let signal = registry.resolve(path).ok_or(HarnessError("path"))?;
            configs.push(OutputConfig::new(
                signal,
                *ty,
                group,
                SafeValue::Value(*safe),
            ));
            points.push((signal, *point));
        }
        configs.sort_by_key(|c| c.signal);
        points.sort_by_key(|p| p.0);
        let configs: &'static [OutputConfig<'static>] = leak(configs);
        let writers: &'static [WriterConfig] = leak(vec![WriterConfig {
            kind: WriterKind::Manual,
            level: Level::MANUAL_DEFAULT,
            lease_ns: Some(3_600_000_000_000),
            ..WriterConfig::EMPTY
        }]);
        let slots: &'static [Slot] = leak((0..registry.len()).map(|_| Slot::new()).collect());
        let outputs = Outputs::new(
            &registry,
            writers,
            OutputsStorage {
                configs,
                states: leak(vec![OutputState::EMPTY; configs.len()]),
                intakes: leak(vec![None; 16]),
            },
        )
        .map_err(|_| HarnessError("outputs"))?;
        let image = ProcessImage::new(
            group,
            HARNESS_PERIOD_NS,
            ImageStorage {
                ports: leak(Vec::new()),
                snapshot: leak(Vec::<Sample>::new()),
                extenders: leak(Vec::<SeqExtender>::new()),
                proposals: leak(vec![Proposal::EMPTY; 16]),
            },
        )
        .map_err(|_| HarnessError("image"))?;
        let mut h = Self {
            registry,
            slots,
            image,
            outputs,
            points: points.iter().map(|p| p.1).collect(),
            tick: 0,
        };
        h.commit(None);
        Ok(h)
    }

    /// The outputs, after the last commit.
    pub fn outputs(&self) -> &Outputs<'static> {
        &self.outputs
    }

    /// The signal of `path`.
    pub fn signal(&self, path: &str) -> Option<SignalId> {
        self.registry.resolve(path)
    }

    /// Bindings of the outputs, in signal order, with `encodings`.
    pub fn bindings(&self, encodings: &[RawEncoding]) -> Vec<OutputBinding> {
        self.outputs
            .configs()
            .iter()
            .zip(&self.points)
            .zip(encodings)
            .map(|((c, point), encoding)| OutputBinding {
                signal: c.signal,
                point: *point,
                encoding: *encoding,
            })
            .collect()
    }

    fn commit(&mut self, proposal: Option<Proposal>) {
        let at = Instant::mono(self.tick * HARNESS_PERIOD_NS);
        if self
            .image
            .latch(self.tick, at, self.slots, &self.registry)
            .is_ok()
        {
            if let Some(p) = proposal {
                let _ = self.image.stage_async(p);
            }
            let _ = self.outputs.commit(&self.image, self.slots, &mut ());
            let _ = self.image.end_tick();
        }
        self.tick += 1;
    }

    /// One tick in which manual writer 0 proposes `bits` on `path`; the
    /// envelope decides what is applied.
    pub fn manual(&mut self, path: &str, bits: u64) {
        let proposal = self.signal(path).map(|signal| Proposal {
            writer: WriterId(0),
            signal,
            value: bits,
            ..Proposal::EMPTY
        });
        self.commit(proposal);
    }

    /// One tick without proposals.
    pub fn idle(&mut self) {
        self.commit(None);
    }
}

/// One read or write recorded by a [`Bench`].
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// An execution of a batch, settled.
    Read {
        /// When it started.
        at: MonoNs,
        /// The batch.
        batch: BatchId,
        /// Settled points.
        points: Vec<Settled>,
    },
    /// A write call.
    Write {
        /// When it started.
        at: MonoNs,
        /// The acks.
        acks: Vec<WriteAck>,
    },
}

/// A minimal bus scheduler for driver tests: one instance, its poll
/// batches on their periods, writes before reads, on a [`VirtualHost`].
pub struct Bench<'r, D: Driver> {
    /// The driver under test.
    pub driver: D,
    host: Arc<VirtualHost>,
    request: PlanRequest<'r>,
    plan: PlanView<'r>,
    timeout_ns: u64,
    buf: SampleBuffer,
    due: Vec<MonoNs>,
}

impl<D: Driver> core::fmt::Debug for Bench<'_, D> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Bench")
            .field("due", &self.due)
            .finish_non_exhaustive()
    }
}

/// Why a bench step failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchError {
    /// The driver returned an error.
    Driver(DriverError),
    /// The driver waited for something that never comes.
    Stalled,
    /// The batch does not exist.
    NoBatch,
}

impl<'r, D: Driver> Bench<'r, D> {
    /// Opens the driver with `request`, `plan` and `link`. Each call gets the
    /// deadline `now + timeout` of device 0.
    pub fn open(
        host: Arc<VirtualHost>,
        request: PlanRequest<'r>,
        plan: PlanView<'r>,
        link: Link,
    ) -> Result<Self, BenchError> {
        let driver = D::open(&OpenArgs::new(request, plan), link).map_err(BenchError::Driver)?;
        let timeout_ns = request
            .devices
            .first()
            .map_or(1_000_000_000, |d| d.timeout_ns);
        let now = host.mono();
        Ok(Self {
            driver,
            host,
            request,
            plan,
            timeout_ns,
            buf: SampleBuffer::with_capacity(4096),
            due: vec![now; plan.len()],
        })
    }

    /// The host.
    pub fn host(&self) -> &VirtualHost {
        &self.host
    }

    /// Runs `connect`.
    pub fn connect(&mut self) -> Result<(), BenchError> {
        let host = self.host.clone();
        let mut cx = Cx::new(&*host, host.mono() + self.timeout_ns);
        let mut sink = ();
        let mut out = ConnectOut::new(&mut sink);
        host.run(self.driver.connect(&mut cx, &mut out))
            .map_err(|_| BenchError::Stalled)?
            .map_err(BenchError::Driver)
    }

    /// Runs `disconnect`.
    pub fn disconnect(&mut self) -> Result<(), BenchError> {
        let host = self.host.clone();
        let mut cx = Cx::new(&*host, host.mono() + 2_000_000_000);
        host.run(self.driver.disconnect(&mut cx))
            .map_err(|_| BenchError::Stalled)
    }

    /// One execution of batch `id` now, settled.
    pub fn read(&mut self, id: BatchId) -> Result<Vec<Settled>, BenchError> {
        let spec = self.plan.batch(id).ok_or(BenchError::NoBatch)?;
        let host = self.host.clone();
        let deadline = host.mono() + self.timeout_ns;
        let batch = ReadBatch::new(&spec, deadline);
        let mut cx = Cx::new(&*host, deadline);
        self.buf.clear();
        let mut sink = RawSink::new(&mut self.buf);
        let result = host
            .run(self.driver.read(&mut cx, &batch, &mut sink))
            .map_err(|_| BenchError::Stalled)?;
        Ok(settle(&self.request, &batch, self.buf.samples(), result))
    }

    /// One `write` call now.
    pub fn write(&mut self, writes: &[CommittedWrite<'_>]) -> Result<Vec<WriteAck>, BenchError> {
        let host = self.host.clone();
        let deadline = host.mono() + self.timeout_ns * (1 + writes.len() as u64 / 8);
        let mut cx = Cx::new(&*host, deadline);
        let mut slots = vec![AckSlot::default(); writes.len()];
        let Some(mut acks) = WriteAcks::new(writes, &mut slots) else {
            return Ok(Vec::new());
        };
        host.run(self.driver.write(&mut cx, writes, &mut acks))
            .map_err(|_| BenchError::Stalled)?;
        Ok(acks.iter().collect())
    }

    /// Runs the poll batches on their periods until `end`, taking writes
    /// from `writes` before each round. Returns what happened, in order.
    pub fn run_until(
        &mut self,
        end: MonoNs,
        mut writes: impl FnMut(MonoNs) -> Vec<CommittedWrite<'static>>,
    ) -> Result<Vec<Event>, BenchError> {
        let mut events = Vec::new();
        loop {
            let next = self
                .plan
                .iter()
                .filter(|b| b.head.kind == BatchKind::Poll)
                .map(|b| self.due[b.head.id as usize])
                .min();
            let Some(next) = next else {
                break;
            };
            if next > end {
                break;
            }
            self.host.advance_to(next);
            let pending = writes(self.host.mono());
            if !pending.is_empty() {
                let at = self.host.mono();
                let acks = self.write(&pending)?;
                events.push(Event::Write { at, acks });
            }
            let ids: Vec<BatchId> = self
                .plan
                .iter()
                .filter(|b| {
                    b.head.kind == BatchKind::Poll
                        && self.due[b.head.id as usize] <= self.host.mono()
                })
                .map(|b| b.head.id)
                .collect();
            for id in ids {
                let at = self.host.mono();
                let points = self.read(id)?;
                events.push(Event::Read {
                    at,
                    batch: id,
                    points,
                });
                if let Some(b) = self.plan.batch(id) {
                    let period = b.head.period_ns.max(1);
                    let due = &mut self.due[id as usize];
                    while *due <= at {
                        *due += period;
                    }
                }
            }
        }
        self.host.advance_to(end);
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_time_jumps_to_the_earliest_sleep() {
        let host = VirtualHost::new(100);
        let cx = Cx::new(&host, 1_000);
        assert_eq!(host.run(cx.sleep_until(500)), Ok(()));
        assert_eq!(host.mono(), 500);
        let never = core::future::pending::<()>();
        assert_eq!(host.run(cx.within(never)), Ok(Err(crate::cx::Elapsed)));
        assert_eq!(host.mono(), 1_000);
        assert_eq!(host.run(core::future::pending::<()>()), Err(Stalled));
        assert_eq!(host.run(cx.within(async { 7 })), Ok(Ok(7)));
        cx.log(LogLevel::Warn, "hello");
        assert_eq!(host.logs(), [(LogLevel::Warn, String::from("hello"))]);
        assert!(!cx.cancelled());
        host.set_cancelled(true);
        assert!(cx.cancelled());
    }

    #[test]
    fn memory_serial_answers_and_fails_on_demand() {
        use crate::link::SerialPort;
        let host = Arc::new(VirtualHost::new(0));
        let (line, handle) = MemorySerial::new(
            host.clone(),
            SerialInfo::new_8n1(115_200),
            Box::new(|req: &[u8], _| req.iter().rev().copied().collect()),
        );
        let mut port = SerialPort::new(line);
        let cx = Cx::new(&*host, 1_000);
        host.run(port.send(&[1, 2, 3])).unwrap().unwrap();
        let mut buf = [0u8; 3];
        host.run(port.receive_exact(&mut buf)).unwrap().unwrap();
        assert_eq!(buf, [3, 2, 1]);
        assert_eq!(handle.written(), [1, 2, 3]);
        handle.set_fault(LineFault::Silent);
        host.run(port.send(&[9])).unwrap().unwrap();
        let r = host.run(cx.within(port.receive_exact(&mut buf))).unwrap();
        assert_eq!(r, Err(crate::cx::Elapsed));
        handle.set_fault(LineFault::Gone);
        assert_eq!(
            host.run(port.send(&[9])).unwrap(),
            Err(IoError::Disconnected)
        );
        handle.set_fault(LineFault::None);
        handle.inject(&[5]);
        port.discard_input().unwrap();
        assert_eq!(port.info().baud, 115_200);
    }

    #[test]
    fn harness_applies_through_the_envelope() {
        let mut h = OutputHarness::new(&[("a.led", ValueType::Bool, 0, 4)]).unwrap();
        let s = h.signal("a.led").unwrap();
        assert_eq!(
            h.outputs().view(s).map(|v| v.applied),
            Some(joker_core::Applied::Value(0))
        );
        h.manual("a.led", 1);
        assert_eq!(
            h.outputs().view(s).map(|v| v.applied),
            Some(joker_core::Applied::Value(1))
        );
        h.manual("a.led", 7);
        assert_eq!(
            h.outputs().view(s).map(|v| v.applied),
            Some(joker_core::Applied::Value(1))
        );
        h.idle();
        assert_eq!(h.bindings(&[RawEncoding::Bool])[0].point, 4);
    }
}
