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
use std::collections::{BTreeMap, VecDeque};
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
    ClockQuality, Instant, IntakeSlot, Level, OutputConfig, Outputs, OutputsStorage, ProcessImage,
    Proposal, Registry, RegistryBuilder, SafeValue, Sample, SignalId, Slot, ValueType,
    WriterConfig, WriterId, WriterKind,
};

use crate::acquire::{SampleBuffer, Settled, settle};
use crate::cbor::Cbor;
use crate::commit::{OutputBinding, RawEncoding};
use crate::cx::{Cx, Host, LogLevel};
use crate::driver::{
    AckSlot, CommittedWrite, ConnectOut, ConnectSink, DeviceInfo, Driver, HealthOut, OpenArgs,
    RawSink, ReadBatch, WriteAck, WriteAcks,
};
use crate::link::{GpioLineConfig, GpioLines, IoError, Link, SerialInfo, SerialIo};
use crate::plan::PlanView;
use crate::spec::PlanRequest;
use crate::types::{BatchId, BatchKind, DeviceIx, DriverError, MonoNs};

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

/// An in-memory set of digital lines, with a shared [`GpioHandle`] a test
/// uses to inspect or drive the far end (an external signal, a denied
/// line).
pub struct MemoryGpio {
    state: Arc<Mutex<GpioState>>,
}

#[derive(Debug, Default)]
struct GpioState {
    levels: BTreeMap<u32, bool>,
    denied: Vec<u32>,
}

impl core::fmt::Debug for MemoryGpio {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemoryGpio").finish_non_exhaustive()
    }
}

impl MemoryGpio {
    /// An empty set of lines (every line reads `false` until set).
    pub fn new() -> (Self, GpioHandle) {
        let state = Arc::new(Mutex::new(GpioState::default()));
        (
            Self {
                state: state.clone(),
            },
            GpioHandle { state },
        )
    }

    /// A set of lines requested the way a host requests real ones
    /// (`LinuxGpio::open`): each output already sits at the physical level
    /// its config carries, before the driver opens or writes.
    pub fn with_lines(lines: &[GpioLineConfig]) -> (Self, GpioHandle) {
        let (gpio, handle) = Self::new();
        for config in lines {
            if let Some(level) = config.output {
                handle.drive(config.line, level);
            }
        }
        (gpio, handle)
    }
}

impl GpioLines for MemoryGpio {
    fn get(&mut self, line: u32) -> Result<bool, IoError> {
        let s = lock(&self.state);
        if s.denied.contains(&line) {
            return Err(IoError::Denied);
        }
        Ok(*s.levels.get(&line).unwrap_or(&false))
    }

    fn set(&mut self, line: u32, value: bool) -> Result<(), IoError> {
        let mut s = lock(&self.state);
        if s.denied.contains(&line) {
            return Err(IoError::Denied);
        }
        s.levels.insert(line, value);
        Ok(())
    }
}

/// The far end of a [`MemoryGpio`]: what a test uses to drive an input
/// line or deny access to one, and to observe what the driver wrote.
#[derive(Clone, Debug)]
pub struct GpioHandle {
    state: Arc<Mutex<GpioState>>,
}

impl GpioHandle {
    /// Sets `line`'s level, as an external signal would.
    pub fn drive(&self, line: u32, value: bool) {
        lock(&self.state).levels.insert(line, value);
    }

    /// The last level of `line` (`false` if never touched).
    pub fn level(&self, line: u32) -> bool {
        *lock(&self.state).levels.get(&line).unwrap_or(&false)
    }

    /// Makes `line` fail every `get`/`set` with [`IoError::Denied`].
    pub fn deny(&self, line: u32) {
        lock(&self.state).denied.push(line);
    }
}

/// An in-memory I²C bus, one register file per 7-bit address, with a
/// shared [`I2cHandle`] a test uses to preload registers, inspect what was
/// written, or deny an address.
pub struct MemoryI2c {
    state: Arc<Mutex<I2cState>>,
}

#[derive(Debug, Default)]
struct I2cState {
    chips: BTreeMap<u8, Vec<u8>>,
    denied: Vec<u8>,
}

impl core::fmt::Debug for MemoryI2c {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemoryI2c").finish_non_exhaustive()
    }
}

impl MemoryI2c {
    /// An empty bus (every register reads `0` until set).
    pub fn new() -> (Self, I2cHandle) {
        let state = Arc::new(Mutex::new(I2cState::default()));
        (
            Self {
                state: state.clone(),
            },
            I2cHandle { state },
        )
    }
}

impl crate::link::I2cTransfers for MemoryI2c {
    fn transfer(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), IoError> {
        let mut s = lock(&self.state);
        if s.denied.contains(&addr) {
            return Err(IoError::Denied);
        }
        let regs = s.chips.entry(addr).or_default();
        let Some((&at, bytes)) = write.split_first() else {
            return Ok(());
        };
        let at = at as usize;
        if read.is_empty() {
            if regs.len() < at + bytes.len() {
                regs.resize(at + bytes.len(), 0);
            }
            regs[at..at + bytes.len()].copy_from_slice(bytes);
            return Ok(());
        }
        for (i, out) in read.iter_mut().enumerate() {
            *out = regs.get(at + i).copied().unwrap_or(0);
        }
        Ok(())
    }
}

/// The far end of a [`MemoryI2c`]: what a test uses to preload a chip's
/// registers, read back what the driver wrote, or deny an address.
#[derive(Clone, Debug)]
pub struct I2cHandle {
    state: Arc<Mutex<I2cState>>,
}

impl I2cHandle {
    /// Sets `addr`'s registers starting at 0, as a datasheet-default chip
    /// would already hold.
    pub fn preload(&self, addr: u8, registers: &[u8]) {
        lock(&self.state).chips.insert(addr, registers.to_vec());
    }

    /// The byte at `addr`'s register `reg` (`0` if never touched).
    pub fn register(&self, addr: u8, reg: u8) -> u8 {
        lock(&self.state)
            .chips
            .get(&addr)
            .and_then(|r| r.get(reg as usize))
            .copied()
            .unwrap_or(0)
    }

    /// Makes `addr` fail every `transfer` with [`IoError::Denied`].
    pub fn deny(&self, addr: u8) {
        lock(&self.state).denied.push(addr);
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
                intakes: leak(vec![IntakeSlot::EMPTY; 16]),
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

/// One device a driver reported in `connect`: its identity and its
/// discovered map (ref/06 §2.11), owned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reported {
    /// The device.
    pub index: DeviceIx,
    /// Vendor.
    pub vendor: Option<String>,
    /// Model.
    pub model: Option<String>,
    /// Serial number.
    pub serial: Option<String>,
    /// Firmware version.
    pub firmware: Option<String>,
    /// Hardware revision.
    pub hw_revision: Option<String>,
    /// The driver fields, CBOR.
    pub extra: Vec<u8>,
    /// The discovered map, CBOR.
    pub discovered: Option<Vec<u8>>,
}

struct Reports(Vec<Reported>);

impl ConnectSink for Reports {
    fn device(&mut self, index: DeviceIx, info: &DeviceInfo<'_>, discovered: Option<Cbor<'_>>) {
        let own = |s: Option<&str>| s.map(String::from);
        self.0.push(Reported {
            index,
            vendor: own(info.vendor),
            model: own(info.model),
            serial: own(info.serial),
            firmware: own(info.firmware),
            hw_revision: own(info.hw_revision),
            extra: info.extra.as_bytes().to_vec(),
            discovered: discovered.map(|d| d.as_bytes().to_vec()),
        });
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

/// What a step program run by [`Bench::steps`] ended with: the values
/// its `read` steps stored, or why it stopped.
pub type StepsOutcome<'p> =
    Result<Vec<(&'p str, crate::interpreter::Reading)>, crate::interpreter::Failure<'p>>;

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
        Ok(Self::with_driver(host, request, plan, driver))
    }

    /// A bench over a driver the test already opened, for a driver whose
    /// transport a test replaces behind its own seam (a fake process
    /// image, for `revpi`).
    pub fn with_driver(
        host: Arc<VirtualHost>,
        request: PlanRequest<'r>,
        plan: PlanView<'r>,
        driver: D,
    ) -> Self {
        let timeout_ns = request
            .devices
            .first()
            .map_or(1_000_000_000, |d| d.timeout_ns);
        let now = host.mono();
        Self {
            driver,
            host,
            request,
            plan,
            timeout_ns,
            buf: SampleBuffer::with_capacity(4096),
            due: vec![now; plan.len()],
        }
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

    /// Runs `connect` and returns what the driver reported through
    /// `ConnectOut::device` (ref/06 §2.11).
    pub fn connect_reports(&mut self) -> Result<Vec<Reported>, BenchError> {
        let host = self.host.clone();
        let mut cx = Cx::new(&*host, host.mono() + self.timeout_ns);
        let mut sink = Reports(Vec::new());
        let mut out = ConnectOut::new(&mut sink);
        host.run(self.driver.connect(&mut cx, &mut out))
            .map_err(|_| BenchError::Stalled)?
            .map_err(BenchError::Driver)?;
        Ok(sink.0)
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

    /// One execution of batch `id` now, into a sample ring that is already
    /// full: every sample the driver pushes is dropped. Returns how many
    /// the sink refused.
    pub fn read_into_full_ring(&mut self, id: BatchId) -> Result<u32, BenchError> {
        let spec = self.plan.batch(id).ok_or(BenchError::NoBatch)?;
        let host = self.host.clone();
        let deadline = host.mono() + self.timeout_ns;
        let batch = ReadBatch::new(&spec, deadline);
        let mut cx = Cx::new(&*host, deadline);
        let mut full = SampleBuffer::with_capacity(0);
        let mut sink = RawSink::new(&mut full);
        let _ = host
            .run(self.driver.read(&mut cx, &batch, &mut sink))
            .map_err(|_| BenchError::Stalled)?;
        Ok(sink.dropped())
    }

    /// Runs the step `program` against device 0 through the interpreter
    /// (ADR 0058), with `args`, every driver call bounded by the device's
    /// `timeout` and the whole program by `budget_ns`. Returns what its
    /// `read` steps stored, or why it stopped.
    pub fn steps<'p>(
        &mut self,
        program: &'p [crate::steps::Step<'p>],
        args: &dyn crate::interpreter::Args,
        budget_ns: u64,
    ) -> Result<StepsOutcome<'p>, BenchError> {
        let host = self.host.clone();
        let mut cx = Cx::new(&*host, host.mono() + budget_ns);
        let call = crate::interpreter::Call {
            device: 0,
            args,
            step_timeout_ns: self.timeout_ns,
        };
        let mut results = Vec::new();
        let outcome = host
            .run(crate::interpreter::run_into(
                &mut self.driver,
                &mut cx,
                &call,
                program,
                &mut results,
            ))
            .map_err(|_| BenchError::Stalled)?;
        Ok(outcome.map(|()| results))
    }

    /// The counters the driver reports in `health` now.
    pub fn counters(&self) -> Vec<(&'static str, u64)> {
        let mut counters = [("", 0); crate::driver::MAX_COUNTERS];
        let mut out = HealthOut::new(&mut counters);
        self.driver.health(&mut out);
        out.counters().to_vec()
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
