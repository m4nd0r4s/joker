//! The cycle instance of an app (docs/01 §4–§5, ref/04 §2.3, §2.6, §3): one
//! `on-cycle` call per task and tick, with the packed image in and the
//! proposal buffer out.
//!
//! Everything the tick path touches is allocated in [`CycleInstance::new`]:
//! the image buffer, the per-slot samples, the dynamic proposal buffer, the
//! notes and the log arena. The one exception is the `cycle-result` of each
//! call, which wasmtime 48 only hands over as owned values (see `wit`).

use core::fmt;
use std::sync::Arc;

use joker_core::image::{NO_TASK, RELINQUISH};
use joker_core::registry::Direction;
use joker_core::value::NO_TS_SRC;
use joker_core::{
    Applied, Outcome, Outputs, ProcessImage, Proposal, Quality, QualityCode, Registry, Sample,
    SignalId, ValueType, WriterId,
};
use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Instance, Linker, ResourceTable, TypedFunc, WasmList, WasmStr};
use wasmtime::{ResourceLimiter, Store, StoreContextMut, Trap};
use wasmtime_wasi::{I32Exit, WasiCtx, WasiCtxView, WasiView};

use crate::abi::{
    self, Header, Record, ResultEntry, SampleEntry, image_flags, record_flags, result_flags,
    sample_flags,
};
use crate::component::{AppComponent, WORLD};
use crate::engine::HostEngine;
use crate::error::{HostError, HostTrap, codes, trap};
use crate::meta::{PortKind, TaskKind};
use crate::rng::{self, Xoshiro};
use crate::wasi::{self, LatchTime};
use crate::wit;

/// The run mode of an instance (ref/04 §2.6.2).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RunMode {
    /// Normal operation.
    Normal,
    /// Shadow mode (M16): writes come out `shadowed`.
    Shadow,
    /// Replay of a trace.
    Replay,
    /// Under `joker-test`.
    Test,
}

/// What a port of the app is bound to in this generation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PortSource {
    /// A signal of the registry: an input to read, or an output to write.
    Signal(SignalId),
    /// A parameter value, as bits in the port's type and unit.
    Param(u64),
    /// An optional port left unbound.
    Unbound,
}

/// The binding of one app port, in `joker:meta` order.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PortBinding<'a> {
    /// What the port is bound to.
    pub source: PortSource,
    /// Resolved name shown to the app (the signal path, or the parameter path).
    pub name: &'a str,
}

/// Limits of a cycle instance.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InstanceLimits {
    /// `memory.max` of the instance, bytes (default 16 MiB for Rust apps,
    /// docs/01 §9).
    pub memory_max: usize,
    /// Epoch ticks allowed for `init` (1 s with a 100 µs ticker).
    pub init_deadline_epochs: u64,
    /// Epoch ticks allowed for one `on-cycle` call (`max(wcet × 2, 200 µs)`,
    /// docs/01 §4).
    pub cycle_deadline_epochs: u64,
    /// Lowest log level kept (`log.enabled`).
    pub log_level: LogLevel,
}

impl Default for InstanceLimits {
    fn default() -> Self {
        Self {
            memory_max: 16 * 1024 * 1024,
            init_deadline_epochs: 10_000,
            cycle_deadline_epochs: 2,
            log_level: LogLevel::Info,
        }
    }
}

/// Everything the host needs to create a cycle instance, besides the
/// component, the registry and the group's image.
#[derive(Copy, Clone, Debug)]
pub struct CycleSetup<'a> {
    /// Node name.
    pub node: &'a str,
    /// Run mode.
    pub mode: RunMode,
    /// Generation number.
    pub generation: u64,
    /// SHA-256 of the generation.
    pub generation_hash: [u8; 32],
    /// The app's writer id in the generation.
    pub writer: WriterId,
    /// Name of the group this instance runs in.
    pub group: &'a str,
    /// One binding per `joker:meta` port.
    pub ports: &'a [PortBinding<'a>],
    /// The cycle tasks that run in this group, as `joker:meta` task indices.
    pub tasks: &'a [u32],
    /// Limits.
    pub limits: InstanceLimits,
}

/// Log levels (`joker:runtime/log`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// `trace`.
    Trace,
    /// `debug`.
    Debug,
    /// `info`.
    Info,
    /// `warn`.
    Warn,
    /// `error`.
    Error,
}

impl From<wit::LogLevel> for LogLevel {
    fn from(l: wit::LogLevel) -> Self {
        match l {
            wit::LogLevel::Trace => Self::Trace,
            wit::LogLevel::Debug => Self::Debug,
            wit::LogLevel::Info => Self::Info,
            wit::LogLevel::Warn => Self::Warn,
            wit::LogLevel::Error => Self::Error,
        }
    }
}

/// How one `on-cycle` call ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CycleStatus {
    /// The task returned `Ok`; its proposals were staged.
    Ok,
    /// The task returned `Err` (`cycle-result.error`): its proposals were
    /// staged all the same. The code and message are in
    /// [`CycleInstance::last_error`].
    AppError,
    /// A trap, a malformed buffer or `J-E0816`: no proposal of the app was
    /// staged, and the trap counts toward the crashloop (docs/01 §9).
    Trap(&'static str),
}

/// The outcome of one `on-cycle` call.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CycleReport {
    /// How the call ended.
    pub status: CycleStatus,
    /// Proposals staged into the image.
    pub staged: u32,
    /// Proposals dropped: staging full, dynamic buffer full or unbound port.
    pub dropped: u32,
    /// Proposals the host refused before the envelope; see
    /// [`CycleInstance::rejections`].
    pub rejected: u32,
}

/// A proposal refused by the host before it reached the core: it never gets
/// an outcome from the envelope, so the caller logs it as a decision
/// (AGENTS.md, invariant 8).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// Port handle.
    pub port: u32,
    /// The registry code (`J-E0104`).
    pub code: &'static str,
}

/// One log entry of the app.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LogRecord<'a> {
    /// Level.
    pub level: LogLevel,
    /// Message, truncated to 1 KiB.
    pub message: &'a str,
    fields: &'a [(u16, u16)],
    text: &'a str,
}

impl<'a> LogRecord<'a> {
    /// The fields, truncated to 16 pairs of 256 bytes.
    pub fn fields(&self) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        let mut at = 0usize;
        let text = self.text;
        self.fields.iter().map(move |&(k, v)| {
            let (k, v) = (k as usize, v as usize);
            let key = text.get(at..at + k).unwrap_or("");
            let value = text.get(at + k..at + k + v).unwrap_or("");
            at += k + v;
            (key, value)
        })
    }
}

const LOG_ENTRIES: usize = 256;
const LOG_ARENA: usize = 64 * 1024;
const LOG_MESSAGE: usize = 1024;
const LOG_FIELDS: usize = 16;
const LOG_PAIR: usize = 256;
const LOG_RATE: u32 = 100;
const ERROR_MESSAGE: usize = 256;
const ERROR_CODE: usize = 16;
const NOTES: usize = 256;

#[derive(Copy, Clone, Debug)]
struct LogEntry {
    level: LogLevel,
    start: u32,
    message: u16,
    n_fields: u8,
    fields: [(u16, u16); LOG_FIELDS],
}

#[derive(Debug)]
struct LogRing {
    entries: Vec<LogEntry>,
    arena: String,
    dropped: u64,
    window_start_ns: u64,
    in_window: u32,
    min: LogLevel,
}

impl LogRing {
    fn new(min: LogLevel) -> Self {
        Self {
            entries: Vec::with_capacity(LOG_ENTRIES),
            arena: String::with_capacity(LOG_ARENA),
            dropped: 0,
            window_start_ns: 0,
            in_window: 0,
            min,
        }
    }

    fn admit(&mut self, now_ns: u64) -> bool {
        if now_ns.saturating_sub(self.window_start_ns) >= 1_000_000_000 {
            self.window_start_ns = now_ns;
            self.in_window = 0;
        }
        if self.in_window >= LOG_RATE || self.entries.len() >= LOG_ENTRIES {
            self.dropped += 1;
            return false;
        }
        self.in_window += 1;
        true
    }
}

fn copy_truncated(buf: &mut [u8], at: usize, s: &str, limit: usize) -> usize {
    let n = floor_char_boundary(s, limit.min(buf.len().saturating_sub(at)));
    buf[at..at + n].copy_from_slice(&s.as_bytes()[..n]);
    n
}

fn floor_char_boundary(s: &str, limit: usize) -> usize {
    if s.len() <= limit {
        return s.len();
    }
    let mut i = limit;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[derive(Copy, Clone, Debug)]
struct PortEntry {
    kind: PortKind,
    ty: ValueType,
    signal: Option<SignalId>,
    snapshot: Option<usize>,
    param: u64,
    slot: Option<u32>,
    outcome: Option<u32>,
    bound: bool,
}

/// What `on-cycle` sees through `cycle-io` during one call.
struct Tick {
    header: Header,
    samples: Vec<Sample>,
    results: Vec<ResultEntry>,
    dynamic: Vec<Proposal>,
    dynamic_dropped: u32,
    pending: Vec<Option<u64>>,
    notes: Vec<u32>,
    rejections: Vec<Rejection>,
}

pub(crate) struct CycleState {
    wasi: WasiCtx,
    table: ResourceTable,
    memory_max: usize,
    rng: Arc<Xoshiro>,
    writer: WriterId,
    ports: Vec<PortEntry>,
    tick: Tick,
    log: LogRing,
    guest_trap: Option<&'static str>,
}

impl WasiView for CycleState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

#[derive(Debug)]
struct MemoryLimit;

impl fmt::Display for MemoryLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("memory limit exceeded")
    }
}

impl std::error::Error for MemoryLimit {}

impl ResourceLimiter for CycleState {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory_max {
            return Err(wasmtime::Error::new(MemoryLimit));
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 1_000_000)
    }
}

/// The cycle instance of one app in one group.
pub struct CycleInstance {
    store: Store<CycleState>,
    on_cycle: TypedFunc<(u32, &'static [u8]), (wit::CycleResult,)>,
    latch: Arc<LatchTime>,
    image: Vec<u8>,
    proposals: Vec<u8>,
    n_slots: usize,
    proposal_capacity: usize,
    deadline: u64,
    mode: RunMode,
    first: bool,
    poisoned: bool,
    tasks: Vec<bool>,
    prev_seq: Vec<u64>,
    group: joker_core::GroupId,
    notes: Vec<u32>,
    rejections: Vec<Rejection>,
    error_code: String,
    error_message: String,
    app: String,
    causal_task: u16,
}

impl fmt::Debug for CycleInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CycleInstance")
            .field("app", &self.app)
            .field("image_len", &self.image.len())
            .finish_non_exhaustive()
    }
}

fn layout(message: impl Into<String>) -> HostError {
    HostError::new(codes::LAYOUT_MISMATCH, message)
}

impl CycleInstance {
    /// Creates the cycle instance and calls `init` (ref/04 §2.6). `image` is
    /// the process image of the group the instance runs in: every bound
    /// signal of the app must be one of its ports.
    pub fn new(
        engine: &HostEngine,
        component: &AppComponent,
        registry: &Registry<'_>,
        image: &ProcessImage<'_>,
        setup: &CycleSetup<'_>,
    ) -> Result<Self, HostError> {
        let meta = component.meta();
        if meta.devices != 0 || meta.implements {
            return Err(HostError::new(
                codes::NOT_AVAILABLE,
                "class devices and implementations are not served by the cycle subset",
            ));
        }
        if setup.ports.len() != meta.ports.len() {
            return Err(layout(format!(
                "{} bindings for {} ports",
                setup.ports.len(),
                meta.ports.len()
            )));
        }
        let mut ports = Vec::with_capacity(meta.ports.len());
        let mut bindings = Vec::with_capacity(meta.ports.len());
        let (mut n_slots, mut n_outcomes) = (0u32, 0u32);
        for (i, (port, binding)) in meta.ports.iter().zip(setup.ports).enumerate() {
            if !matches!(
                port.kind,
                PortKind::Input | PortKind::Output | PortKind::Param
            ) || port.many
            {
                return Err(HostError::new(
                    codes::NOT_AVAILABLE,
                    format!(
                        "port {}: {:?} ports are not served by the cycle subset",
                        port.name, port.kind
                    ),
                ));
            }
            let ty = port.ty.ok_or_else(|| {
                HostError::new(
                    codes::META_INVALID,
                    format!("port {} has no type", port.name),
                )
            })?;
            if matches!(ty, ValueType::String | ValueType::Bytes | ValueType::Record) {
                return Err(HostError::new(
                    codes::NOT_AVAILABLE,
                    format!(
                        "port {}: wide values are not served by the cycle subset",
                        port.name
                    ),
                ));
            }
            let mut entry = PortEntry {
                kind: port.kind,
                ty,
                signal: None,
                snapshot: None,
                param: 0,
                slot: Some(n_slots),
                outcome: None,
                bound: true,
            };
            n_slots += 1;
            if port.kind == PortKind::Output {
                entry.outcome = Some(n_outcomes);
                n_outcomes += 1;
            }
            match (port.kind, binding.source) {
                (PortKind::Param, PortSource::Param(bits)) => entry.param = bits,
                (PortKind::Input | PortKind::Output, PortSource::Signal(signal)) => {
                    let s = registry
                        .signal(signal)
                        .ok_or_else(|| layout(format!("port {}: unknown signal", port.name)))?;
                    if s.ty != ty {
                        return Err(HostError::new(
                            codes::WRONG_TYPE,
                            format!("port {} is {:?}, {} is {:?}", port.name, ty, s.path, s.ty),
                        ));
                    }
                    if ty.is_numeric() && port.unit.as_deref().is_some_and(|u| u != s.unit.0) {
                        return Err(HostError::new(
                            codes::WRONG_TYPE,
                            format!(
                                "port {} is in {:?}, {} in {}; unit conversion is not served yet",
                                port.name, port.unit, s.path, s.unit.0
                            ),
                        ));
                    }
                    if port.kind == PortKind::Output && s.direction != Direction::Output {
                        return Err(HostError::new(
                            codes::WRONG_TYPE,
                            format!("port {} is an output, {} is not", port.name, s.path),
                        ));
                    }
                    entry.signal = Some(signal);
                    entry.snapshot =
                        Some(image.ports().binary_search(&signal).map_err(|_| {
                            layout(format!("{} is not latched by the group", s.path))
                        })?);
                }
                (_, PortSource::Unbound) if port.optional => entry.bound = false,
                (_, PortSource::Unbound) => {
                    return Err(HostError::new(
                        codes::NOT_BOUND,
                        format!("port {} is not bound", port.name),
                    ));
                }
                _ => {
                    return Err(layout(format!(
                        "port {}: binding of the wrong kind",
                        port.name
                    )));
                }
            }
            bindings.push(wit::PortBinding {
                origin: wit::PortOrigin::Declared,
                decl: u32::try_from(i).map_err(|_| layout("too many ports"))?,
                elem: 0,
                member: 0,
                name: binding.name.to_owned(),
                bound: entry.bound,
                slot: entry.slot,
                outcome: entry.outcome,
            });
            ports.push(entry);
        }
        let mut tasks = vec![false; meta.tasks.len()];
        let mut task_bindings = Vec::with_capacity(setup.tasks.len());
        for &t in setup.tasks {
            let task = meta
                .tasks
                .get(t as usize)
                .filter(|task| task.kind == TaskKind::Cycle)
                .ok_or_else(|| layout(format!("task {t} is not a cycle task")))?;
            tasks[t as usize] = true;
            let _ = task;
            task_bindings.push(wit::TaskBinding {
                task: t,
                group: setup.group.to_owned(),
                period_ns: image.period_ns(),
            });
        }
        let proposal_capacity = meta
            .proposal_capacity
            .unwrap_or(u64::from(abi::DEFAULT_PROPOSAL_CAPACITY))
            .clamp(
                u64::from(abi::MIN_PROPOSAL_CAPACITY),
                u64::from(abi::MAX_PROPOSAL_CAPACITY),
            ) as usize;
        let image_len = abi::image_len(n_slots as usize, n_outcomes as usize, 0);
        let seed = rng::instance_seed(&setup.generation_hash, &meta.name, "cycle", None);
        let generator = Arc::new(Xoshiro::new(seed));
        let latch = Arc::new(LatchTime::default());
        let dynamic_capacity = proposal_capacity / 14;
        let state = CycleState {
            wasi: wasi::cycle_ctx(&latch, &generator, seed),
            table: ResourceTable::new(),
            memory_max: setup.limits.memory_max,
            rng: generator,
            writer: setup.writer,
            ports,
            tick: Tick {
                header: Header::default(),
                samples: vec![Sample::INITIAL; n_slots as usize],
                results: vec![ResultEntry::default(); n_outcomes as usize],
                dynamic: Vec::with_capacity(dynamic_capacity),
                dynamic_dropped: 0,
                pending: vec![None; n_outcomes as usize],
                notes: Vec::with_capacity(NOTES),
                rejections: Vec::with_capacity(dynamic_capacity),
            },
            log: LogRing::new(setup.limits.log_level),
            guest_trap: None,
        };
        let mut store = Store::new(engine.wasmtime(), state);
        store.limiter(|s| s as &mut dyn ResourceLimiter);
        let linker = linker(engine, component)?;
        store.set_epoch_deadline(setup.limits.init_deadline_epochs);
        let instance = linker
            .instantiate(&mut store, &component.component)
            .map_err(|e| HostError::new(codes::INIT_FAILED, format!("instantiate: {e}")))?;
        let (init, on_cycle) = exports(&mut store, &instance)?;
        let cx = wit::InitContext {
            kind: wit::InstanceKind::Cycle,
            mode: match setup.mode {
                RunMode::Normal => wit::RunMode::Normal,
                RunMode::Shadow => wit::RunMode::Shadow,
                RunMode::Replay => wit::RunMode::Replay,
                RunMode::Test => wit::RunMode::Test,
            },
            app: meta.name.clone(),
            app_version: meta.version.clone(),
            node: setup.node.to_owned(),
            served_world: WORLD.to_owned(),
            generation: setup.generation,
            generation_hash: setup.generation_hash.to_vec(),
            seed,
            ports: bindings,
            instances: Vec::new(),
            tasks: task_bindings,
            implements: None,
            cycle_abi: abi::ABI_VERSION,
            image_len: u32::try_from(image_len).map_err(|_| layout("image too large"))?,
            proposal_capacity: proposal_capacity as u32,
        };
        match init.call(&mut store, (cx,)) {
            Ok((Ok(()),)) => {}
            Ok((Err(e),)) => {
                return Err(HostError::new(
                    codes::INIT_FAILED,
                    format!("{}: {}", e.code, e.message),
                ));
            }
            Err(e) => {
                return Err(HostError::new(
                    codes::INIT_FAILED,
                    format!("init trapped ({}): {e}", trap_code(&e)),
                ));
            }
        }
        Ok(Self {
            store,
            on_cycle,
            latch,
            image: vec![0u8; image_len],
            proposals: Vec::with_capacity(proposal_capacity),
            n_slots: n_slots as usize,
            proposal_capacity,
            deadline: setup.limits.cycle_deadline_epochs,
            mode: setup.mode,
            first: true,
            poisoned: false,
            tasks,
            prev_seq: vec![0; n_slots as usize],
            group: image.group(),
            notes: Vec::with_capacity(NOTES),
            rejections: Vec::with_capacity(dynamic_capacity),
            error_code: String::with_capacity(ERROR_CODE),
            error_message: String::with_capacity(ERROR_MESSAGE),
            app: meta.name.clone(),
            causal_task: NO_TASK,
        })
    }

    /// Sets the number of this instance's task in its group: its
    /// proposals carry it, and the ports packed for it are its reads, so
    /// the image keeps its causal set (ADR 0043, ADR 0097).
    pub fn set_causal_task(&mut self, task: u16) {
        self.causal_task = task;
    }

    /// Runs cycle task `task` for the tick latched in `image` (step 4 of
    /// docs/01 §4): packs the image, calls `on-cycle` once, checks the
    /// proposal buffer and stages its writes. `outputs` gives the results of
    /// the previous commit. An `Err` is a misuse by the caller; what the app
    /// did is in the report.
    pub fn run(
        &mut self,
        task: u32,
        image: &mut ProcessImage<'_>,
        outputs: &Outputs<'_>,
    ) -> Result<CycleReport, HostError> {
        if self.poisoned {
            return Err(HostError::new(
                codes::GUEST_ABORT,
                "the instance trapped; recreate it (docs/01 §9)",
            ));
        }
        let tick = image.tick().ok_or_else(|| layout("no tick is latched"))?;
        if !self.tasks.get(task as usize).copied().unwrap_or(false) {
            return Err(layout(format!("task {task} does not run in this instance")));
        }
        self.pack(task, image, outputs, tick)?;
        self.latch.set(tick.at.mono_ns, tick.at.utc_ns);
        self.notes.clear();
        self.rejections.clear();
        self.error_code.clear();
        self.error_message.clear();
        self.store.set_epoch_deadline(self.deadline);
        self.store.data_mut().guest_trap = None;
        let image_bytes = core::mem::take(&mut self.image);
        let on_cycle: TypedFunc<(u32, &[u8]), (wit::CycleResult,)> = self.on_cycle;
        let call = on_cycle.call(&mut self.store, (task, image_bytes.as_slice()));
        self.image = image_bytes;
        let state = self.store.data();
        self.notes.extend_from_slice(&state.tick.notes);
        self.rejections.extend_from_slice(&state.tick.rejections);
        let dynamic_dropped = state.tick.dynamic_dropped;
        let result = match call {
            Ok((result,)) => result,
            Err(e) => {
                self.poisoned = true;
                let code = match (trap_code(&e), self.store.data().guest_trap) {
                    (codes::GUEST_ABORT, Some(announced)) => announced,
                    (code, _) => code,
                };
                return Ok(self.trapped(code));
            }
        };
        let Ok(status) = self.lift(&result) else {
            return Ok(self.trapped(codes::MALFORMED));
        };
        if status == CycleStatus::Trap(codes::UNCAUGHT) {
            return Ok(self.trapped(codes::UNCAUGHT));
        }
        let bytes = self.proposals.as_slice();
        let state = self.store.data();
        if validate(bytes, &state.ports).is_err() {
            return Ok(self.trapped(codes::MALFORMED));
        }
        let mut report = CycleReport {
            status,
            staged: 0,
            dropped: dynamic_dropped,
            rejected: 0,
        };
        let writer = state.writer;
        let task = self.causal_task;
        for entry in state.ports.iter().filter(|e| e.bound) {
            if let Some(port) = entry.snapshot {
                image.mark_read(task, port);
            }
        }
        for proposal in &state.tick.dynamic {
            stage(image, Proposal { task, ..*proposal }, &mut report);
        }
        for record in abi::records(bytes).into_iter().flatten().flatten() {
            match record {
                Record::Write {
                    flags, port, value, ..
                } => {
                    let entry = &state.ports[port as usize];
                    let Some(signal) = entry.signal.filter(|_| entry.bound) else {
                        report.dropped += 1;
                        continue;
                    };
                    if flags & record_flags::INLINE != 0 {
                        if self.rejections.len() < self.rejections.capacity() {
                            self.rejections.push(Rejection {
                                port,
                                code: codes::WRONG_TYPE,
                            });
                        }
                        continue;
                    }
                    let relinquish = flags & record_flags::RELINQUISH != 0;
                    stage(
                        image,
                        Proposal {
                            writer,
                            signal,
                            value,
                            flags: if relinquish { RELINQUISH } else { 0 },
                            task,
                            ..Proposal::EMPTY
                        },
                        &mut report,
                    );
                }
                Record::Note { port } if self.notes.len() < self.notes.capacity() => {
                    self.notes.push(port);
                }
                _ => {}
            }
        }
        report.rejected = u32::try_from(self.rejections.len()).unwrap_or(u32::MAX);
        self.first = false;
        Ok(report)
    }

    /// Copies `cycle-result` out of guest memory into the buffers reserved
    /// at creation, without allocating. `Err` when the buffer is larger than
    /// the proposal capacity or the error is not UTF-8 (`J-E0907`).
    fn lift(&mut self, result: &wit::CycleResult) -> Result<CycleStatus, ()> {
        let bytes = result.proposals.as_le_slice(&self.store);
        if bytes.len() > self.proposal_capacity {
            return Err(());
        }
        self.proposals.clear();
        self.proposals.extend_from_slice(bytes);
        let Some(error) = &result.error else {
            return Ok(CycleStatus::Ok);
        };
        let code = error.code.to_str(&self.store).map_err(|_| ())?;
        if code == codes::UNCAUGHT {
            return Ok(CycleStatus::Trap(codes::UNCAUGHT));
        }
        let message = error.message.to_str(&self.store).map_err(|_| ())?;
        self.error_code
            .push_str(&code[..floor_char_boundary(&code, ERROR_CODE)]);
        self.error_message
            .push_str(&message[..floor_char_boundary(&message, ERROR_MESSAGE)]);
        Ok(CycleStatus::AppError)
    }

    fn trapped(&mut self, code: &'static str) -> CycleReport {
        self.first = false;
        self.notes.clear();
        self.rejections.clear();
        CycleReport {
            status: CycleStatus::Trap(code),
            staged: 0,
            dropped: 0,
            rejected: 0,
        }
    }

    fn pack(
        &mut self,
        task: u32,
        image: &ProcessImage<'_>,
        outputs: &Outputs<'_>,
        tick: joker_core::TickInfo,
    ) -> Result<(), HostError> {
        let mut flags = 0u16;
        if self.first {
            flags |= image_flags::FIRST_TICK;
        }
        if tick.after_skip {
            flags |= image_flags::AFTER_SKIP;
        }
        match self.mode {
            RunMode::Shadow => flags |= image_flags::SHADOW,
            RunMode::Replay => flags |= image_flags::REPLAY,
            RunMode::Normal | RunMode::Test => {}
        }
        let n_outcomes = self.store.data().tick.results.len();
        let header = Header {
            flags,
            task,
            tick_seq: tick.index,
            t_mono_ns: tick.at.mono_ns,
            t_utc_ns: tick.at.utc_ns,
            dt_ns: tick.dt_ns,
            clock: tick.at.clock as u8,
            n_slots: self.n_slots as u32,
            n_outcomes: n_outcomes as u32,
            wide_off: (abi::HEADER_LEN + abi::SAMPLE_LEN * (self.n_slots + n_outcomes)) as u32,
            wide_len: 0,
        };
        let abi_error = |e: abi::AbiError| layout(format!("image: {e:?}"));
        header.write(&mut self.image).map_err(abi_error)?;
        let previous = if tick.first {
            None
        } else {
            Some(
                tick.index
                    .saturating_sub(tick.dt_ns / image.period_ns().max(1)),
            )
        };
        let group = self.group;
        let state = self.store.data_mut();
        state.tick.header = header;
        state.tick.dynamic.clear();
        state.tick.dynamic_dropped = 0;
        state.tick.notes.clear();
        state.tick.rejections.clear();
        state.tick.pending.fill(None);
        let snapshot = image.snapshot();
        for (port, entry) in state.ports.iter().enumerate() {
            let Some(slot) = entry.slot else { continue };
            let slot = slot as usize;
            let sample = match (entry.bound, entry.snapshot) {
                (false, _) => Sample::new(0, Quality::NOT_CONNECTED, NO_TS_SRC, 0, 0),
                (true, Some(i)) => snapshot
                    .get(i)
                    .copied()
                    .ok_or_else(|| layout("snapshot too short"))?,
                (true, None) => Sample::new(entry.param, Quality::GOOD, NO_TS_SRC, 0, 1),
            };
            let mut sflags = 0u16;
            if !entry.bound {
                sflags |= sample_flags::UNBOUND;
            }
            if sample.ts_src().is_some() {
                sflags |= sample_flags::HAS_TS_SRC;
            }
            if sample.seq() != self.prev_seq[slot] {
                sflags |= sample_flags::CHANGED;
            }
            self.prev_seq[slot] = sample.seq();
            let q = sample.quality();
            SampleEntry {
                value: sample.transport_bits(),
                quality_code: q.code() as u8,
                quality_reason: q.reason().to_u8(),
                flags: sflags,
                seq: sample.seq() as u32,
                ts_acq: sample.ts_acq(),
            }
            .write(&mut self.image, slot)
            .map_err(abi_error)?;
            state.tick.samples[slot] = sample;
            let (Some(outcome), Some(signal)) = (entry.outcome, entry.signal) else {
                continue;
            };
            let mut result = ResultEntry::default();
            if let Some(view) = outputs.view(signal) {
                let other = outputs
                    .configs()
                    .iter()
                    .find(|c| c.signal == signal)
                    .is_some_and(|c| c.group != group);
                let mine = outputs
                    .last_outcome(state.writer, signal)
                    .unwrap_or(Outcome::None);
                result = ResultEntry {
                    outcome: mine.code(),
                    detail: mine.detail(),
                    winner: view.level.map_or(0, |l| l.get()),
                    flags: (if view.tripped {
                        result_flags::TRIPPED
                    } else {
                        0
                    }) | (if other { result_flags::OTHER_GROUP } else { 0 }),
                    commit_seq: previous.unwrap_or(0) as u32,
                    applied: match view.applied {
                        Applied::Value(bits) => bits,
                        Applied::Relinquish => 0,
                    },
                    mine: 0,
                };
            }
            let _ = port;
            result
                .write(&mut self.image, self.n_slots, outcome as usize)
                .map_err(abi_error)?;
            state.tick.results[outcome as usize] = result;
        }
        Ok(())
    }

    /// True after a trap: Wasm forbids entering the instance again, so the
    /// caller drops it and creates a new one, which counts toward the
    /// crashloop (docs/01 §9). A malformed buffer and `J-E0816` do not
    /// poison the instance.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// The notes of the last call: ports whose read was substituted by
    /// `get_or`, for the trace (docs/16 §3.1).
    pub fn notes(&self) -> &[u32] {
        &self.notes
    }

    /// The proposals of the last call that the host refused.
    pub fn rejections(&self) -> &[Rejection] {
        &self.rejections
    }

    /// The `cycle-result.error` of the last call, if it returned one.
    pub fn last_error(&self) -> Option<(&str, &str)> {
        (!self.error_code.is_empty())
            .then_some((self.error_code.as_str(), self.error_message.as_str()))
    }

    /// Hands every buffered log entry to `f`, oldest first, and empties the
    /// buffer.
    pub fn drain_log(&mut self, mut f: impl FnMut(LogRecord<'_>)) {
        let log = &mut self.store.data_mut().log;
        for e in &log.entries {
            let start = e.start as usize;
            let msg_end = start + e.message as usize;
            let fields = &e.fields[..e.n_fields as usize];
            let text_len: usize = fields.iter().map(|&(k, v)| k as usize + v as usize).sum();
            f(LogRecord {
                level: e.level,
                message: log.arena.get(start..msg_end).unwrap_or(""),
                fields,
                text: log.arena.get(msg_end..msg_end + text_len).unwrap_or(""),
            });
        }
        log.entries.clear();
        log.arena.clear();
    }

    /// Log entries dropped so far (full buffer or rate limit), for
    /// `joker.app.<app>.log_dropped`.
    pub fn log_dropped(&self) -> u64 {
        self.store.data().log.dropped
    }
}

fn stage(image: &mut ProcessImage<'_>, proposal: Proposal, report: &mut CycleReport) {
    match image.stage_cycle(proposal) {
        Ok(()) => report.staged += 1,
        Err(_) => report.dropped += 1,
    }
}

fn validate(bytes: &[u8], ports: &[PortEntry]) -> Result<(), abi::AbiError> {
    for record in abi::records(bytes)? {
        let ok = match record? {
            Record::Write { port, at, .. } => {
                at.is_none()
                    && ports
                        .get(port as usize)
                        .is_some_and(|p| p.kind == PortKind::Output)
            }
            Record::Note { port } => ports.get(port as usize).is_some_and(|p| p.slot.is_some()),
            Record::Method { .. }
            | Record::Publish { .. }
            | Record::CounterAdd { .. }
            | Record::Emit { .. } => false,
        };
        if !ok {
            return Err(abi::AbiError::Flags(0));
        }
    }
    Ok(())
}

/// The trap a guest announces with an `error` log record that starts with
/// its code and a colon, before it traps (ADR 0056): only the guest-raised
/// codes of ref/04 §7.
fn guest_trap_code(message: &str) -> Option<&'static str> {
    [codes::ALLOC_IN_CYCLE].into_iter().find(|code| {
        message
            .strip_prefix(code)
            .is_some_and(|rest| rest.starts_with(':'))
    })
}

fn trap_code(e: &wasmtime::Error) -> &'static str {
    if let Some(t) = e.downcast_ref::<HostTrap>() {
        return t.code;
    }
    if e.downcast_ref::<MemoryLimit>().is_some() {
        return codes::MEMORY_LIMIT;
    }
    if e.downcast_ref::<I32Exit>().is_some() {
        return codes::GUEST_ABORT;
    }
    match e.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => codes::DEADLINE,
        Some(Trap::StackOverflow) => codes::STACK_OVERFLOW,
        _ => codes::GUEST_ABORT,
    }
}

type Exports = (
    TypedFunc<(wit::InitContext,), (Result<(), wit::Error>,)>,
    TypedFunc<(u32, &'static [u8]), (wit::CycleResult,)>,
);

fn exports(store: &mut Store<CycleState>, instance: &Instance) -> Result<Exports, HostError> {
    let missing = |what: &str| HostError::new(codes::INIT_FAILED, format!("missing export {what}"));
    let guest = instance
        .get_export_index(&mut *store, None, "joker:runtime/guest@0.1.0")
        .ok_or_else(|| missing("joker:runtime/guest@0.1.0"))?;
    let init = instance
        .get_export_index(&mut *store, Some(&guest), "init")
        .ok_or_else(|| missing("init"))?;
    let on_cycle = instance
        .get_export_index(&mut *store, Some(&guest), "on-cycle")
        .ok_or_else(|| missing("on-cycle"))?;
    let wrong =
        |e: wasmtime::Error| HostError::new(codes::INIT_FAILED, format!("export type: {e}"));
    Ok((
        instance.get_typed_func(&mut *store, init).map_err(wrong)?,
        instance
            .get_typed_func(&mut *store, on_cycle)
            .map_err(wrong)?,
    ))
}

fn value_of(ty: ValueType, bits: u64) -> wit::Value {
    match ty {
        ValueType::Bool => wit::Value::Boolean(bits != 0),
        ValueType::I64 => wit::Value::Int(bits as i64),
        ValueType::F64 | ValueType::F32 => wit::Value::Float(f64::from_bits(bits)),
        _ => wit::Value::Uint(bits),
    }
}

fn bits_of(ty: ValueType, v: &wit::Value) -> Option<u64> {
    match (ty, v) {
        (ValueType::Bool, wit::Value::Boolean(b)) => Some(u64::from(*b)),
        (ValueType::I64, wit::Value::Int(i)) => Some(*i as u64),
        (ValueType::U64 | ValueType::Enum | ValueType::Bits, wit::Value::Uint(u)) => Some(*u),
        (ValueType::F64, wit::Value::Float(f)) => Some(f.to_bits()),
        (ValueType::F32, wit::Value::Float(f)) => Some(f64::from(*f as f32).to_bits()),
        _ => None,
    }
}

fn quality_of(q: Quality) -> wit::Quality {
    wit::Quality {
        code: match q.code() {
            QualityCode::Good => wit::QualityCode::Good,
            QualityCode::Uncertain => wit::QualityCode::Uncertain,
            QualityCode::Bad => wit::QualityCode::Bad,
        },
        reason: wit::QualityReason::from_code(q.reason().to_u8()),
    }
}

fn outcome_of(ty: ValueType, r: &ResultEntry) -> Option<wit::WriteOutcome> {
    let reject = |d: u8| match d {
        1 => wit::RejectReason::WrongType,
        2 => wit::RejectReason::Interlock,
        3 => wit::RejectReason::Quality,
        4 => wit::RejectReason::Range,
        5 => wit::RejectReason::AtomicGroup,
        6 => wit::RejectReason::NotOwner,
        7 => wit::RejectReason::ClockUnsynced,
        _ => wit::RejectReason::Budget,
    };
    Some(match r.outcome {
        1 => wit::WriteOutcome::Accepted,
        2 | 3 => wit::WriteOutcome::Clamped(wit::Clamp {
            by: if r.outcome == 2 {
                wit::ClampKind::Range
            } else {
                wit::ClampKind::Rate
            },
            applied: value_of(ty, r.applied),
        }),
        4 => wit::WriteOutcome::Suppressed,
        5 => wit::WriteOutcome::Overridden(r.detail),
        6 => wit::WriteOutcome::Deferred,
        7 => wit::WriteOutcome::Shadowed,
        8 => wit::WriteOutcome::Rejected(reject(r.detail)),
        9 => wit::WriteOutcome::Tripped,
        _ => return None,
    })
}

fn port(state: &CycleState, p: u32) -> wasmtime::Result<&PortEntry> {
    state
        .ports
        .get(p as usize)
        .ok_or_else(|| trap(codes::MALFORMED, "port out of range"))
}

fn output(state: &CycleState, p: u32) -> wasmtime::Result<PortEntry> {
    let entry = *port(state, p)?;
    if entry.kind != PortKind::Output {
        return Err(trap(codes::MALFORMED, "not an output port"));
    }
    Ok(entry)
}

fn push_dynamic(state: &mut CycleState, p: u32, entry: PortEntry, value: u64, flags: u16) {
    let Some(signal) = entry.signal.filter(|_| entry.bound) else {
        state.tick.dynamic_dropped += 1;
        return;
    };
    if state.tick.dynamic.len() >= state.tick.dynamic.capacity() {
        state.tick.dynamic_dropped += 1;
        return;
    }
    let _ = p;
    state.tick.dynamic.push(Proposal {
        writer: state.writer,
        signal,
        value,
        flags,
        ..Proposal::EMPTY
    });
}

fn linker(engine: &HostEngine, component: &AppComponent) -> Result<Linker<CycleState>, HostError> {
    let fail = |e: wasmtime::Error| HostError::new(codes::INIT_FAILED, format!("linker: {e}"));
    let mut linker = Linker::<CycleState>::new(engine.wasmtime());
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(fail)?;
    let version = component
        .meta()
        .world
        .strip_prefix("joker:runtime")
        .unwrap_or("@0.1.0");
    {
        let mut io = linker
            .instance(&format!("joker:runtime/cycle-io{version}"))
            .map_err(fail)?;
        io.func_wrap(
            "read",
            |cx: StoreContextMut<'_, CycleState>, (p,): (u32,)| {
                let state = cx.data();
                let entry = port(state, p)?;
                let slot = entry
                    .slot
                    .ok_or_else(|| trap(codes::MALFORMED, "port has no sample"))?;
                let s = state.tick.samples[slot as usize];
                Ok((wit::Sample {
                    value: value_of(entry.ty, s.transport_bits()),
                    quality: quality_of(s.quality()),
                    ts_src: s.ts_src(),
                    ts_hw: None,
                    ts_acq: s.ts_acq(),
                    seq: s.seq(),
                },))
            },
        )
        .map_err(fail)?;
        io.func_wrap(
            "write",
            |mut cx: StoreContextMut<'_, CycleState>, (p, v): (u32, wit::Value)| {
                let state = cx.data_mut();
                let entry = output(state, p)?;
                match bits_of(entry.ty, &v) {
                    Some(bits) => {
                        push_dynamic(state, p, entry, bits, 0);
                        if let Some(o) = entry.outcome {
                            state.tick.pending[o as usize] = Some(bits);
                        }
                    }
                    None if state.tick.rejections.len() < state.tick.rejections.capacity() => {
                        state.tick.rejections.push(Rejection {
                            port: p,
                            code: codes::WRONG_TYPE,
                        });
                    }
                    None => state.tick.dynamic_dropped += 1,
                }
                Ok(())
            },
        )
        .map_err(fail)?;
        io.func_wrap(
            "relinquish",
            |mut cx: StoreContextMut<'_, CycleState>, (p,): (u32,)| {
                let state = cx.data_mut();
                let entry = output(state, p)?;
                push_dynamic(state, p, entry, 0, RELINQUISH);
                Ok(())
            },
        )
        .map_err(fail)?;
        io.func_wrap(
            "pending",
            |cx: StoreContextMut<'_, CycleState>, (p,): (u32,)| {
                let state = cx.data();
                let entry = *port(state, p)?;
                let pending = entry
                    .outcome
                    .and_then(|o| state.tick.pending[o as usize])
                    .map(|bits| value_of(entry.ty, bits));
                Ok((pending,))
            },
        )
        .map_err(fail)?;
        io.func_wrap(
            "last-outcome",
            |cx: StoreContextMut<'_, CycleState>, (p,): (u32,)| {
                let state = cx.data();
                let entry = *port(state, p)?;
                let outcome = entry
                    .outcome
                    .and_then(|o| outcome_of(entry.ty, &state.tick.results[o as usize]));
                Ok((outcome,))
            },
        )
        .map_err(fail)?;
        io.func_wrap(
            "param",
            |cx: StoreContextMut<'_, CycleState>, (p,): (u32,)| {
                let entry = *port(cx.data(), p)?;
                if entry.kind != PortKind::Param {
                    return Err(trap(codes::MALFORMED, "not a parameter port"));
                }
                Ok((value_of(entry.ty, entry.param),))
            },
        )
        .map_err(fail)?;
        io.func_wrap(
            "note-substituted",
            |mut cx: StoreContextMut<'_, CycleState>, (p,): (u32,)| {
                let state = cx.data_mut();
                if port(state, p)?.slot.is_none() {
                    return Err(trap(codes::MALFORMED, "port has no sample"));
                }
                if state.tick.notes.len() < state.tick.notes.capacity() {
                    state.tick.notes.push(p);
                }
                Ok(())
            },
        )
        .map_err(fail)?;
        io.func_wrap("now", |cx: StoreContextMut<'_, CycleState>, (): ()| {
            let h = cx.data().tick.header;
            Ok((wit::Instant {
                mono_ns: h.t_mono_ns,
                utc_ns: h.t_utc_ns,
                clock: match h.clock {
                    0 => wit::ClockQuality::Synced,
                    1 => wit::ClockQuality::Holdover,
                    _ => wit::ClockQuality::Unsynced,
                },
            },))
        })
        .map_err(fail)?;
        io.func_wrap("dt-ns", |cx: StoreContextMut<'_, CycleState>, (): ()| {
            Ok((cx.data().tick.header.dt_ns,))
        })
        .map_err(fail)?;
        io.func_wrap(
            "random-u64",
            |cx: StoreContextMut<'_, CycleState>, (): ()| Ok((cx.data().rng.next_u64(),)),
        )
        .map_err(fail)?;
        for name in [
            "call",
            "publish",
            "counter-add",
            "emit",
            "mailbox-send",
            "mailbox-recv",
        ] {
            io.func_new(name, |_, _, _, _| {
                Err(trap(codes::MALFORMED, "no port or instance of this kind"))
            })
            .map_err(fail)?;
        }
        io.func_new("write-at", |_, _, _, _| {
            Err(trap(
                codes::NOT_AVAILABLE,
                "scheduled writes are reserved for 1.1",
            ))
        })
        .map_err(fail)?;
    }
    {
        let mut log = linker
            .instance(&format!("joker:runtime/log{version}"))
            .map_err(fail)?;
        log.func_wrap(
            "log",
            |mut cx: StoreContextMut<'_, CycleState>,
             (level, msg, fields): (wit::LogLevel, WasmStr, WasmList<(WasmStr, WasmStr)>)| {
                let level = LogLevel::from(level);
                if level == LogLevel::Error
                    && let Some(code) = guest_trap_code(&msg.to_str(&cx)?)
                {
                    cx.data_mut().guest_trap = Some(code);
                }
                let now = cx.data().tick.header.t_mono_ns;
                {
                    let log = &mut cx.data_mut().log;
                    if level < log.min || !log.admit(now) {
                        return Ok(());
                    }
                }
                let mut text = [0u8; LOG_MESSAGE + LOG_FIELDS * LOG_PAIR];
                let mut lens = [(0u16, 0u16); LOG_FIELDS];
                let message = {
                    let m = msg.to_str(&cx)?;
                    copy_truncated(&mut text, 0, &m, LOG_MESSAGE)
                };
                let mut used = message;
                let n_fields = fields.len().min(LOG_FIELDS);
                for (i, len) in lens.iter_mut().enumerate().take(n_fields) {
                    let Some(pair) = fields.get(&mut cx, i) else { break };
                    let (k, v) = pair?;
                    let key = copy_truncated(&mut text, used, &k.to_str(&cx)?, LOG_PAIR);
                    let value = copy_truncated(&mut text, used + key, &v.to_str(&cx)?, LOG_PAIR - key);
                    *len = (key as u16, value as u16);
                    used += key + value;
                }
                let log = &mut cx.data_mut().log;
                let Ok(all) = core::str::from_utf8(&text[..used]) else {
                    log.dropped += 1;
                    return Ok(());
                };
                if log.arena.len() + all.len() > LOG_ARENA {
                    log.dropped += 1;
                    return Ok(());
                }
                let start = log.arena.len() as u32;
                log.arena.push_str(all);
                log.entries.push(LogEntry {
                    level,
                    start,
                    message: message as u16,
                    n_fields: n_fields as u8,
                    fields: lens,
                });
                Ok(())
            },
        )
        .map_err(fail)?;
        log.func_wrap(
            "enabled",
            |cx: StoreContextMut<'_, CycleState>, (level,): (wit::LogLevel,)| {
                Ok((LogLevel::from(level) >= cx.data().log.min,))
            },
        )
        .map_err(fail)?;
    }
    trap_async_imports(engine, component, &mut linker, version)?;
    Ok(linker)
}

fn trap_async_imports(
    engine: &HostEngine,
    component: &AppComponent,
    linker: &mut Linker<CycleState>,
    version: &str,
) -> Result<(), HostError> {
    let fail = |e: wasmtime::Error| HostError::new(codes::INIT_FAILED, format!("linker: {e}"));
    let ty = component.component.component_type();
    for (name, item) in ty.imports(engine.wasmtime()) {
        let Some(interface) = name
            .strip_prefix("joker:runtime/")
            .and_then(|n| n.strip_suffix(version))
        else {
            continue;
        };
        if matches!(interface, "types" | "cycle-io" | "log") {
            continue;
        }
        let ComponentItem::ComponentInstance(instance) = item.ty else {
            continue;
        };
        let mut li = linker.instance(name).map_err(fail)?;
        for (export, export_item) in instance.exports(engine.wasmtime()) {
            match export_item.ty {
                ComponentItem::ComponentFunc(f) if f.async_() => {
                    return Err(HostError::new(
                        codes::NOT_AVAILABLE,
                        format!(
                            "{name}: `{export}` is an async import; the cycle subset does not serve the async instance"
                        ),
                    ));
                }
                ComponentItem::ComponentFunc(_) => {
                    li.func_new(export, |_, _, _, _| {
                        Err(trap(
                            codes::ASYNC_FROM_CYCLE,
                            "async API called from the cycle instance",
                        ))
                    })
                    .map_err(fail)?;
                }
                ComponentItem::Resource(_) => {
                    return Err(HostError::new(
                        codes::NOT_AVAILABLE,
                        format!("{name}: resource `{export}` is not served by the cycle subset"),
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}
