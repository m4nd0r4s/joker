//! The harness: construction (ref/04 §6.2), execution (§6.3) and results
//! (§6.4) of one app over a virtual clock and the real core.

use std::collections::BTreeMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, Once, PoisonError};
use std::task::{Context, Poll, Waker};

use core::time::Duration;

use joker::__private::{AppEntry, AsyncTask, CycleTask, Runtime, TriggerDecl, inventory};
use joker::{App, Record};
use joker_core::commit::OutputState;
use joker_core::output::ClockRequirement;
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::Instant;
use joker_core::value::NO_TS_SRC;
use joker_core::{
    Applied, ClockQuality, CommitSink, ImageStorage, IntakeSlot, Level, OutputConfig, Outputs,
    OutputsStorage, ProcessImage, Proposal, Quality, QualityCode, QualityReason, Registry,
    RegistryBuilder, SafeValue, Sample, SignalId, Slot, ValueType, WriteResult, WriterConfig,
    WriterId, WriterKind,
};
use joker_host::abi::{
    self, Header, Record as AbiRecord, ResultEntry, SampleEntry, image_flags, record_flags,
    sample_flags,
};
use joker_host::meta::{self, AppMeta, PortKind, TaskKind};
use joker_sdk_sys::cycle::{CycleGuest, CycleOut};
use joker_sdk_sys::exports::joker::runtime::guest::{
    Guest, InitContext, InstanceKind, PortBinding, PortOrigin, RunMode, TaskBinding, Trigger,
};
use joker_sdk_sys::joker::runtime::types;

use crate::events::{AlarmTrace, EventTrace};
use crate::host::{self, Host, HostState, Op};
use crate::series::Series;
use crate::trace::{OutputTrace, Point};
use crate::value::Value;

/// The SDK keeps the state of the app in one static, as a component does,
/// so one harness runs at a time in a process: tests that build one wait
/// for each other.
static RUNTIME: Mutex<()> = Mutex::new(());

/// The start of the virtual UTC clock: 2026-01-01T00:00:00Z (ref/04 §6.2).
const START_UTC_NS: i64 = 1_767_225_600_000_000_000;

const WRITER: WriterId = WriterId(0);

static WARNING: Once = Once::new();

/// An error a task returned (ref/04 §6.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskError {
    /// Virtual time since the start.
    pub at: Duration,
    /// The task.
    pub task: String,
    /// The error code (`J-E…`).
    pub code: String,
    /// The message.
    pub message: String,
}

/// The envelope of an output in a test (ref/04 §6.2): only `safe` by
/// default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Envelope {
    safe: Option<Value>,
    range: Option<(f64, f64)>,
    rate: Option<f64>,
}

impl Envelope {
    /// An envelope with only the default safe value.
    pub fn new() -> Self {
        Self::default()
    }

    /// `range = [lo, hi]`, clamping.
    pub fn range(mut self, lo: f64, hi: f64) -> Self {
        self.range = Some((lo, hi));
        self
    }

    /// `rate`, in units per second.
    pub fn rate(mut self, per_s: f64) -> Self {
        self.rate = Some(per_s);
        self
    }

    /// `safe`.
    pub fn safe(mut self, v: impl Into<Value>) -> Self {
        self.safe = Some(v.into());
        self
    }
}

#[derive(Clone, Debug)]
struct Port {
    name: String,
    kind: PortKind,
    ty: ValueType,
    signal: Option<SignalId>,
    snapshot: Option<usize>,
    param: u64,
    slot: Option<u32>,
    outcome: Option<u32>,
    wide: Option<u32>,
}

/// The app-owned value of an `Internal<T>`, or the condition of an alarm.
#[derive(Clone, Debug, Default)]
struct Owned {
    bytes: Option<Vec<u8>>,
    active: bool,
    was_active: bool,
    seq: u64,
}

type Running = Pin<Box<dyn Future<Output = Result<(), types::Error>>>>;

/// One async task in the executor (ref/04 §6.1).
struct AsyncSlot {
    id: u32,
    name: &'static str,
    trigger: TriggerDecl,
    next_every: u64,
    running: Option<Running>,
    pending: Option<Trigger>,
}

struct Run {
    registry: Registry<'static>,
    slots: &'static [Slot],
    image: ProcessImage<'static>,
    outputs: Outputs<'static>,
    period_ns: u64,
    tick: u64,
    ports: Vec<Port>,
    tasks: Vec<(u32, String)>,
    image_buf: Vec<u8>,
    proposals: Vec<u8>,
    prev_seq: Vec<u64>,
    first: bool,
    stopped: bool,
    traces: BTreeMap<String, Vec<Point>>,
    errors: Vec<TaskError>,
    n_slots: usize,
    n_outcomes: usize,
    owned: Vec<Owned>,
    next_owned: Vec<(usize, Op)>,
    alarms: BTreeMap<String, Vec<(Duration, bool)>>,
    events: BTreeMap<String, Vec<(Duration, Vec<u8>)>>,
    host: Option<Host>,
    exec: Vec<AsyncSlot>,
    trigger_seq: Vec<u64>,
    jitter_rng: u64,
    async_ops: Vec<Op>,
    cycle_cx: InitContext,
}

struct Sink;

impl CommitSink for Sink {
    fn write(&mut self, _: &WriteResult) {}
}

fn leak<T>(v: Vec<T>) -> &'static mut [T] {
    Box::leak(v.into_boxed_slice())
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn core_quality(q: joker::Quality) -> Quality {
    let code = match q.code {
        joker::QualityCode::Good => QualityCode::Good,
        joker::QualityCode::Uncertain => QualityCode::Uncertain,
        joker::QualityCode::Bad => QualityCode::Bad,
    };
    Quality::new(code, QualityReason::from_u8(q.reason as u8))
}

fn sdk_quality(q: Quality) -> joker::Quality {
    joker::Quality::from_codes(q.code() as u8, q.reason().to_u8())
}

fn sdk_outcome(
    o: joker_core::Outcome,
    applied: Option<Value>,
) -> Option<joker::WriteOutcome<Value>> {
    use joker::{ClampKind, RejectReason as R, WriteOutcome as W};
    use joker_core::Outcome;
    Some(match o {
        Outcome::None => return None,
        Outcome::Accepted => W::Accepted,
        Outcome::ClampedRange | Outcome::ClampedRate => W::Clamped {
            by: if o == Outcome::ClampedRange {
                ClampKind::Range
            } else {
                ClampKind::Rate
            },
            applied: applied?,
        },
        Outcome::Suppressed => W::Suppressed,
        Outcome::Overridden(level) => W::Overridden {
            level: joker::Level::new(level.get())?,
        },
        Outcome::Deferred => W::Deferred,
        Outcome::Shadowed => W::Shadowed,
        Outcome::Rejected(reason) => W::Rejected(match reason as u8 {
            1 => R::WrongType,
            2 => R::Interlock,
            3 => R::Quality,
            4 => R::Range,
            5 => R::AtomicGroup,
            6 => R::NotOwner,
            7 => R::ClockUnsynced,
            _ => R::Budget,
        }),
        Outcome::Tripped => W::Tripped,
    })
}

fn wire_outcome(o: joker_core::Outcome, applied: types::Value) -> Option<types::WriteOutcome> {
    use joker_core::Outcome;
    use types::WriteOutcome as W;
    Some(match o {
        Outcome::None => return None,
        Outcome::Accepted => W::Accepted,
        Outcome::ClampedRange | Outcome::ClampedRate => W::Clamped(types::Clamp {
            by: if o == Outcome::ClampedRange {
                types::ClampKind::Range
            } else {
                types::ClampKind::Rate
            },
            applied,
        }),
        Outcome::Suppressed => W::Suppressed,
        Outcome::Overridden(level) => W::Overridden(level.get()),
        Outcome::Deferred => W::Deferred,
        Outcome::Shadowed => W::Shadowed,
        Outcome::Rejected(reason) => W::Rejected(match reason as u8 {
            1 => types::RejectReason::WrongType,
            2 => types::RejectReason::Interlock,
            3 => types::RejectReason::Quality,
            4 => types::RejectReason::Range,
            5 => types::RejectReason::AtomicGroup,
            6 => types::RejectReason::NotOwner,
            7 => types::RejectReason::ClockUnsynced,
            _ => types::RejectReason::Budget,
        }),
        Outcome::Tripped => W::Tripped,
    })
}

/// The test harness of the app `A` (ref/04 §6).
pub struct Harness<A: App> {
    meta: AppMeta,
    periods: BTreeMap<String, u64>,
    params: BTreeMap<String, Value>,
    inputs: BTreeMap<String, Series>,
    envelopes: BTreeMap<String, Envelope>,
    lease_ns: Option<u64>,
    level: Level,
    start_utc_ns: i64,
    clock: ClockQuality,
    history: BTreeMap<String, Series>,
    internals: BTreeMap<String, Vec<u8>>,
    seed: u64,
    run: Option<Run>,
    _lock: MutexGuard<'static, ()>,
    _app: PhantomData<fn() -> A>,
}

impl<A: App> core::fmt::Debug for Harness<A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Harness")
            .field("app", &self.meta.name)
            .field("tick", &self.run.as_ref().map(|r| r.tick))
            .finish_non_exhaustive()
    }
}

impl<A: App> Default for Harness<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: App> Harness<A> {
    /// A harness for `A`, with the periods and parameter defaults the app
    /// declares in its `joker:meta`.
    pub fn new() -> Self {
        let lock = RUNTIME.lock().unwrap_or_else(PoisonError::into_inner);
        let mut fragments: Vec<&[u8]> = inventory::iter::<AppEntry>
            .into_iter()
            .filter(|e| e.name == A::NAME)
            .map(|e| e.meta)
            .collect();
        assert_eq!(
            fragments.len(),
            1,
            "the test binary must link exactly one #[joker::app] named {}",
            A::NAME
        );
        fragments.extend(inventory::iter::<CycleTask>.into_iter().map(|t| t.meta));
        fragments.extend(inventory::iter::<AsyncTask>.into_iter().map(|t| t.meta));
        let meta = meta::from_fragments(&fragments)
            .unwrap_or_else(|e| panic!("joker:meta of {}: {e}", A::NAME));
        WARNING.call_once(|| {
            eprintln!(
                "warning: joker-test runs {} natively, not as the production .wasm component (ref/04 §6.1)",
                A::NAME
            );
        });
        Self {
            meta,
            periods: BTreeMap::new(),
            params: BTreeMap::new(),
            inputs: BTreeMap::new(),
            envelopes: BTreeMap::new(),
            lease_ns: None,
            level: Level::parse("control:12").expect("a valid level"),
            start_utc_ns: START_UTC_NS,
            clock: ClockQuality::Synced,
            history: BTreeMap::new(),
            internals: BTreeMap::new(),
            seed: 0,
            run: None,
            _lock: lock,
            _app: PhantomData,
        }
    }

    fn port_names(&self) -> String {
        self.meta
            .ports
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn port(&self, name: &str, kinds: &[PortKind]) -> &meta::MetaPort {
        self.meta
            .ports
            .iter()
            .find(|p| p.name == name && kinds.contains(&p.kind))
            .unwrap_or_else(|| {
                panic!(
                    "{}: no {kinds:?} port `{name}`; the ports are: {}",
                    self.meta.name,
                    self.port_names()
                )
            })
    }

    fn before_run(&self, what: &str) {
        assert!(
            self.run.is_none(),
            "{what} configures the harness: call it before the first tick"
        );
    }

    /// The period of the group of `task` (default: the task's `period`).
    pub fn group(mut self, task: &str, period: Duration) -> Self {
        self.before_run("group");
        assert!(
            self.meta
                .tasks
                .iter()
                .any(|t| t.name == task && t.kind == TaskKind::Cycle),
            "{}: no cycle task `{task}`",
            self.meta.name
        );
        self.periods.insert(task.to_owned(), ns(period));
        self
    }

    /// The value of a parameter (default: its declared default).
    pub fn param(mut self, name: &str, v: impl Into<Value>) -> Self {
        self.before_run("param");
        self.port(name, &[PortKind::Param]);
        self.params.insert(name.to_owned(), v.into());
        self
    }

    /// The series of an input; without one the input has no sample
    /// (`Uncertain(Initial)`).
    pub fn input(mut self, port: &str, s: impl Into<Series>) -> Self {
        self.before_run("input");
        self.port(port, &[PortKind::Input]);
        self.inputs.insert(port.to_owned(), s.into());
        self
    }

    /// The safe value of an output (default: the type's default).
    pub fn safe(mut self, output: &str, v: impl Into<Value>) -> Self {
        self.before_run("safe");
        self.port(output, &[PortKind::Output]);
        let e = self.envelopes.entry(output.to_owned()).or_default();
        e.safe = Some(v.into());
        self
    }

    /// The envelope of an output (default: only `safe`).
    pub fn envelope(mut self, output: &str, e: Envelope) -> Self {
        self.before_run("envelope");
        self.port(output, &[PortKind::Output]);
        let safe = self.envelopes.get(output).and_then(|e| e.safe);
        self.envelopes.insert(
            output.to_owned(),
            Envelope {
                safe: e.safe.or(safe),
                ..e
            },
        );
        self
    }

    /// The lease of the app's writers (default: 3 × the period, docs/16 A9).
    pub fn lease(mut self, d: Duration) -> Self {
        self.before_run("lease");
        self.lease_ns = Some(ns(d));
        self
    }

    /// The level of the app's writers (default: `"control:12"`).
    pub fn level(mut self, level: &str) -> Self {
        self.before_run("level");
        self.level = Level::parse(level).unwrap_or_else(|| panic!("invalid level `{level}`"));
        self
    }

    /// The data `history()` reads for an input or an output (default: the
    /// series of the input). The series runs on the virtual time since the
    /// start; the instants before the start take its value at the start.
    pub fn history(mut self, port: &str, s: impl Into<Series>) -> Self {
        self.before_run("history");
        self.port(port, &[PortKind::Input, PortKind::Output]);
        self.history.insert(port.to_owned(), s.into());
        self
    }

    /// The value of an `Internal<T>` at the start (default: none,
    /// `Uncertain(Initial)`).
    pub fn internal<T: Record>(mut self, port: &str, v: T) -> Self {
        self.before_run("internal");
        self.port(port, &[PortKind::Internal]);
        let mut buf = vec![0u8; T::MAX_LEN];
        let n = v
            .encode_into(&mut buf)
            .unwrap_or_else(|| panic!("{port}: the record does not encode"));
        buf.truncate(n);
        self.internals.insert(port.to_owned(), buf);
        self
    }

    /// The seed of the deterministic generator: the jitter of `every` and
    /// `random()` (default: 0).
    pub fn seed(mut self, seed: u64) -> Self {
        self.before_run("seed");
        self.seed = seed;
        self
    }

    /// The UTC instant of the start, RFC 3339 in UTC (default:
    /// `"2026-01-01T00:00:00Z"`).
    pub fn start_utc(mut self, rfc3339: &str) -> Self {
        self.before_run("start_utc");
        self.start_utc_ns = crate::time::parse_utc(rfc3339)
            .unwrap_or_else(|| panic!("`{rfc3339}` is not an RFC 3339 UTC instant"));
        self
    }

    /// The quality of the clock (default: `Synced`).
    pub fn clock(mut self, q: ClockQuality) -> Self {
        self.clock = q;
        self
    }

    fn build(&mut self) -> &mut Run {
        if self.run.is_none() {
            let run = self.start();
            self.run = Some(run);
        }
        self.run.as_mut().expect("the run was just built")
    }

    fn start(&self) -> Run {
        let tasks: Vec<(u32, String, u64)> = self
            .meta
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == TaskKind::Cycle)
            .map(|(i, t)| {
                let period = self
                    .periods
                    .get(&t.name)
                    .copied()
                    .or(t.period_ns)
                    .unwrap_or_else(|| panic!("cycle task `{}` has no period", t.name));
                (i as u32, t.name.clone(), period)
            })
            .collect();
        assert!(!tasks.is_empty(), "{} has no cycle task", self.meta.name);
        let period_ns = tasks[0].2;
        assert!(
            tasks.iter().all(|t| t.2 == period_ns),
            "the native harness serves the cycle tasks of one group; set one period with .group()"
        );
        let mut builder = RegistryBuilder::new(
            leak(vec![SignalMeta::EMPTY; self.meta.ports.len()]),
            leak(vec![Alias::EMPTY; 0]),
            leak(vec![GroupMeta::EMPTY; 1]),
        );
        builder
            .group(GroupMeta {
                name: "main",
                period_ns,
            })
            .expect("one group");
        let mut ports = Vec::with_capacity(self.meta.ports.len());
        let (mut n_slots, mut n_outcomes, mut n_wide) = (0u32, 0u32, 0u32);
        for p in &self.meta.ports {
            assert!(
                matches!(
                    p.kind,
                    PortKind::Input
                        | PortKind::Output
                        | PortKind::Param
                        | PortKind::Internal
                        | PortKind::Alarm
                        | PortKind::EventOut
                ) && !p.many,
                "port {}: {:?} ports are not served by the native harness yet",
                p.name,
                p.kind
            );
            let ty = match p.kind {
                PortKind::Internal | PortKind::EventOut => ValueType::Record,
                PortKind::Alarm => ValueType::Enum,
                _ => {
                    p.ty.unwrap_or_else(|| panic!("port {} has no type", p.name))
                }
            };
            let unit = UnitRef(leak_str(p.unit.clone().unwrap_or_else(|| "1".to_owned())));
            let path = leak_str(format!("app.{}", p.name));
            match p.kind {
                PortKind::Input => {
                    builder
                        .signal(SignalMeta::input(path, ty, unit))
                        .expect("a valid input");
                }
                PortKind::Output => {
                    builder
                        .signal(SignalMeta::output(path, ty, unit, "main"))
                        .expect("a valid output");
                }
                _ => {}
            }
            let param = match p.kind {
                PortKind::Param => self
                    .params
                    .get(&p.name)
                    .copied()
                    .or_else(|| p.default.map(Value::F64))
                    .map_or(0, |v| v.to_bits(ty)),
                _ => 0,
            };
            let has_sample = matches!(
                p.kind,
                PortKind::Input
                    | PortKind::Output
                    | PortKind::Param
                    | PortKind::Internal
                    | PortKind::Alarm
            );
            let slot = has_sample.then(|| {
                n_slots += 1;
                n_slots - 1
            });
            let outcome = (p.kind == PortKind::Output).then(|| {
                n_outcomes += 1;
                n_outcomes - 1
            });
            let wide = (p.kind == PortKind::Internal).then(|| {
                n_wide += 1;
                n_wide - 1
            });
            ports.push(Port {
                name: p.name.clone(),
                kind: p.kind,
                ty,
                signal: None,
                snapshot: None,
                param,
                slot,
                outcome,
                wide,
            });
        }
        let registry = builder.build().expect("a valid registry");
        let group = registry.resolve_group("main").expect("the group");
        for port in &mut ports {
            if matches!(port.kind, PortKind::Input | PortKind::Output) {
                port.signal = registry.resolve(&format!("app.{}", port.name));
            }
        }
        let mut latched: Vec<SignalId> = ports.iter().filter_map(|p| p.signal).collect();
        latched.sort();
        for port in &mut ports {
            port.snapshot = port.signal.and_then(|s| latched.binary_search(&s).ok());
        }
        let mut outs: Vec<&Port> = ports
            .iter()
            .filter(|p| p.kind == PortKind::Output)
            .collect();
        outs.sort_by_key(|p| p.signal);
        let configs: &'static [OutputConfig<'static>] = leak(
            outs.iter()
                .map(|p| {
                    let e = self.envelopes.get(&p.name).copied().unwrap_or_default();
                    let safe = e.safe.map_or(0, |v| v.to_bits(p.ty));
                    let mut c = OutputConfig::new(
                        p.signal.expect("an output has a signal"),
                        p.ty,
                        group,
                        SafeValue::Value(safe),
                    );
                    c.range = e.range;
                    c.rate = e.rate;
                    c
                })
                .collect(),
        );
        let n_outs = configs.len();
        let lease = self.lease_ns.unwrap_or(3 * period_ns);
        let writer = |kind| WriterConfig {
            kind,
            level: self.level,
            lease_ns: Some(lease),
            requires_clock: ClockRequirement::Any,
        };
        let outputs = Outputs::new(
            &registry,
            leak(vec![writer(WriterKind::Cycle)]),
            OutputsStorage {
                configs,
                states: leak(vec![OutputState::EMPTY; n_outs]),
                intakes: leak(vec![IntakeSlot::EMPTY; 256]),
            },
        )
        .unwrap_or_else(|e| panic!("outputs: {e:?}"));
        let n = latched.len();
        let image = ProcessImage::new(
            group,
            period_ns,
            ImageStorage {
                ports: leak(latched),
                snapshot: leak(vec![Sample::INITIAL; n]),
                extenders: leak(vec![SeqExtender::for_slot(); n]),
                proposals: leak(vec![Proposal::EMPTY; 256]),
            },
        )
        .unwrap_or_else(|e| panic!("image: {e:?}"));
        let image_len = abi::image_len(n_slots as usize, n_outcomes as usize, n_wide as usize);
        let capacity = abi::DEFAULT_PROPOSAL_CAPACITY;
        let context = |kind| InitContext {
            kind,
            mode: RunMode::Test,
            app: self.meta.name.clone(),
            app_version: self.meta.version.clone(),
            node: "test".to_owned(),
            served_world: joker_host::WORLD.to_owned(),
            generation: 1,
            generation_hash: vec![0; 32],
            seed: self.seed,
            ports: ports
                .iter()
                .enumerate()
                .map(|(i, p)| PortBinding {
                    origin: PortOrigin::Declared,
                    decl: i as u32,
                    elem: 0,
                    member: 0,
                    name: p.signal.map_or_else(
                        || format!("app.{}.{}", self.meta.name, p.name),
                        |_| format!("app.{}", p.name),
                    ),
                    bound: true,
                    slot: p.slot,
                    outcome: p.outcome,
                })
                .collect(),
            instances: Vec::new(),
            tasks: tasks
                .iter()
                .map(|&(task, _, period)| TaskBinding {
                    task,
                    group: "main".to_owned(),
                    period_ns: period,
                })
                .collect(),
            implements: None,
            cycle_abi: abi::ABI_VERSION,
            image_len: image_len as u32,
            proposal_capacity: capacity,
        };
        let cycle_cx = context(InstanceKind::Cycle);
        if let Err(e) = <Runtime as Guest>::init(cycle_cx.clone()) {
            panic!("init of {}: {}: {}", self.meta.name, e.code, e.message);
        }
        let mut exec = Vec::new();
        let mut host = None;
        if self.meta.tasks.iter().any(|t| t.kind == TaskKind::Async) {
            let state = HostState {
                start_utc_ns: self.start_utc_ns,
                types: ports.iter().map(|p| Some(p.ty)).collect(),
                params: ports.iter().map(|p| p.param).collect(),
                samples: vec![None; ports.len()],
                history: ports
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| {
                        let s = self
                            .history
                            .get(&p.name)
                            .or_else(|| self.inputs.get(&p.name))?;
                        Some((i as u32, s.clone()))
                    })
                    .collect(),
                rng: self.seed,
                ..HostState::default()
            };
            let h = Host(Arc::new(Mutex::new(state)));
            joker::__private::install_host(Box::leak(Box::new(h.clone())));
            if let Err(e) = <Runtime as Guest>::init(context(InstanceKind::Task)) {
                panic!(
                    "init of the async instance of {}: {}: {}",
                    self.meta.name, e.code, e.message
                );
            }
            let mut jitter_rng = self.seed ^ 0x6a6f_6b65_725f_6a69;
            for (id, t) in self.meta.tasks.iter().enumerate() {
                if t.kind != TaskKind::Async {
                    continue;
                }
                let decl = inventory::iter::<AsyncTask>
                    .into_iter()
                    .find(|a| a.name == t.name)
                    .unwrap_or_else(|| panic!("async task {} is not linked", t.name));
                let next_every = match decl.trigger {
                    TriggerDecl::Every {
                        period_ns,
                        jitter_ns,
                    } => period_ns + jitter(&mut jitter_rng, jitter_ns),
                    _ => 0,
                };
                exec.push(AsyncSlot {
                    id: id as u32,
                    name: decl.name,
                    trigger: decl.trigger,
                    next_every,
                    running: None,
                    pending: None,
                });
            }
            host = Some(h);
        }
        let traces = ports
            .iter()
            .filter(|p| p.kind == PortKind::Output)
            .map(|p| (p.name.clone(), Vec::new()))
            .collect();
        let owned = ports
            .iter()
            .map(|p| Owned {
                bytes: self.internals.get(&p.name).cloned(),
                seq: u64::from(self.internals.contains_key(&p.name)),
                ..Owned::default()
            })
            .collect();
        Run {
            slots: leak((0..registry.len()).map(|_| Slot::new()).collect()),
            registry,
            image,
            outputs,
            period_ns,
            tick: 0,
            prev_seq: vec![0; ports.len()],
            trigger_seq: vec![0; ports.len()],
            alarms: ports
                .iter()
                .filter(|p| p.kind == PortKind::Alarm)
                .map(|p| (p.name.clone(), Vec::new()))
                .collect(),
            events: ports
                .iter()
                .filter(|p| p.kind == PortKind::EventOut)
                .map(|p| (p.name.clone(), Vec::new()))
                .collect(),
            owned,
            ports,
            tasks: tasks.into_iter().map(|(i, name, _)| (i, name)).collect(),
            image_buf: vec![0; image_len],
            proposals: vec![0; capacity as usize],
            first: true,
            stopped: false,
            traces,
            errors: Vec::new(),
            n_slots: n_slots as usize,
            n_outcomes: n_outcomes as usize,
            next_owned: Vec::new(),
            host,
            exec,
            jitter_rng: self.seed ^ 0x6a6f_6b65_725f_6a69,
            async_ops: Vec::new(),
            cycle_cx,
        }
    }

    /// Runs whole ticks for `d` of virtual time.
    pub fn run_for(&mut self, d: Duration) {
        let period = self.build().period_ns;
        let ticks = ns(d).div_ceil(period);
        for _ in 0..ticks {
            self.step();
        }
    }

    /// Runs until the virtual time since the start reaches `t`.
    pub fn run_until(&mut self, t: Duration) {
        let target = ns(t);
        loop {
            let run = self.build();
            if run.tick * run.period_ns >= target {
                break;
            }
            self.step();
        }
    }

    /// One tick: the async proposals, inputs, latch, the cycle tasks,
    /// commit (docs/01 §4), then the async tasks until they all wait for a
    /// timer or a trigger (ref/04 §6.1).
    pub fn step(&mut self) {
        let clock = self.clock;
        let start_utc_ns = self.start_utc_ns;
        self.build();
        let inputs: Vec<(usize, Series)> = {
            let run = self.run.as_ref().expect("built above");
            run.ports
                .iter()
                .enumerate()
                .filter(|(_, p)| p.kind == PortKind::Input)
                .filter_map(|(i, p)| self.inputs.get(&p.name).map(|s| (i, s.clone())))
                .collect()
        };
        let run = self.run.as_mut().expect("built above");
        let t = run.tick * run.period_ns;
        for (i, series) in &inputs {
            let port = &run.ports[*i];
            let Some(signal) = port.signal else { continue };
            let value = series.value_at(t).to_bits(port.ty);
            let quality = core_quality(series.quality_at(t));
            run.slots[signal.index()].write(value, quality, NO_TS_SRC, t);
        }
        let at = Instant {
            mono_ns: t,
            utc_ns: start_utc_ns.saturating_add(i64::try_from(t).unwrap_or(i64::MAX)),
            clock,
        };
        run.image
            .latch(run.tick, at, run.slots, &run.registry)
            .unwrap_or_else(|e| panic!("latch: {e:?}"));
        let staged_async = run.apply_async(t);
        let mut proposed: BTreeMap<SignalId, u64> = BTreeMap::new();
        for &(port, value) in &staged_async {
            if let (Some(signal), Some(v)) = (run.output_signal(port), value) {
                proposed.insert(signal, v);
            }
        }
        if !run.stopped {
            for (task, name) in run.tasks.clone() {
                run.cycle(task, &name, t, &mut proposed);
            }
        }
        run.outputs
            .commit(&run.image, run.slots, &mut Sink)
            .unwrap_or_else(|e| panic!("commit: {e:?}"));
        for port in run.ports.iter().filter(|p| p.kind == PortKind::Output) {
            let signal = port.signal.expect("an output has a signal");
            let Some(view) = run.outputs.view(signal) else {
                continue;
            };
            let applied = match view.applied {
                Applied::Value(bits) => Some(Value::from_bits(bits, port.ty)),
                Applied::Relinquish => None,
            };
            let point = Point {
                at: Duration::from_nanos(t),
                applied,
                proposal: proposed.get(&signal).map(|&b| Value::from_bits(b, port.ty)),
                outcome: proposed
                    .contains_key(&signal)
                    .then(|| run.outputs.last_outcome(WRITER, signal))
                    .flatten()
                    .and_then(|o| sdk_outcome(o, applied)),
                level: view.level.and_then(|l| joker::Level::new(l.get())),
            };
            if let Some(trace) = run.traces.get_mut(&port.name) {
                trace.push(point);
            }
        }
        if !run.stopped {
            run.tasks_after_commit(t, at, &staged_async);
        }
        run.image
            .end_tick()
            .unwrap_or_else(|e| panic!("end tick: {e:?}"));
        run.tick += 1;
        run.first = false;
    }

    /// The virtual time since the start of the next tick.
    pub fn now(&self) -> Duration {
        self.run.as_ref().map_or(Duration::ZERO, |r| {
            Duration::from_nanos(r.tick * r.period_ns)
        })
    }

    /// Sets an input to a constant from the next tick on.
    pub fn set_input(&mut self, port: &str, v: impl Into<Value>) {
        self.port(port, &[PortKind::Input]);
        self.inputs.insert(port.to_owned(), Series::constant(v));
    }

    /// Changes the quality of an input for `for_`, from the next tick on.
    pub fn inject(&mut self, port: &str, q: joker::Quality, for_: Duration) {
        self.port(port, &[PortKind::Input]);
        let from = self.now();
        let series = self
            .inputs
            .remove(port)
            .unwrap_or_else(|| panic!("input `{port}` has no series to inject into"));
        self.inputs
            .insert(port.to_owned(), series.with_quality(from, from + for_, q));
    }

    /// Changes a parameter from the next tick on, through `on-params`
    /// (ref/04 §4.3.4): when the app refuses it, the parameter keeps its
    /// value and the refusal is in [`Harness::errors`], from the task
    /// `on_params`.
    pub fn set_param(&mut self, name: &str, v: impl Into<Value>) {
        let v = v.into();
        self.port(name, &[PortKind::Param]);
        let at = self.now();
        let Some(run) = self.run.as_mut() else {
            self.params.insert(name.to_owned(), v);
            return;
        };
        let Some(index) = run.ports.iter().position(|p| p.name == name) else {
            return;
        };
        let ty = run.ports[index].ty;
        let change = joker_sdk_sys::exports::joker::runtime::guest::ParamChange {
            port: index as u32,
            value: host::wire(v, ty),
        };
        if let Err(e) = <Runtime as Guest>::on_params(vec![change]) {
            run.errors.push(TaskError {
                at,
                task: "on_params".to_owned(),
                code: e.code,
                message: e.message,
            });
            return;
        }
        self.params.insert(name.to_owned(), v);
        let bits = v.to_bits(ty);
        run.ports[index].param = bits;
        if let Some(h) = &run.host
            && let Some(p) = h.lock().params.get_mut(index)
        {
            *p = bits;
        }
    }

    /// Stops the app: it proposes nothing more, and its lease runs out.
    pub fn stop_app(&mut self) {
        self.build().stopped = true;
    }

    /// Starts the app again after [`Harness::stop_app`].
    pub fn restart_app(&mut self) {
        let run = self.build();
        run.stopped = false;
    }

    /// Replaces the app by itself through `export-state` and `migrate`
    /// (ref/04 §4.12): the cycle task states that persist survive, the
    /// others start again.
    pub fn hot_swap(&mut self) -> Result<(), TaskError> {
        let at = self.now();
        let fail = |e: types::Error| TaskError {
            at,
            task: "hot_swap".to_owned(),
            code: e.code,
            message: e.message,
        };
        let blob = <Runtime as Guest>::export_state().map_err(fail)?;
        let Some(run) = self.run.as_ref() else {
            return Ok(());
        };
        <Runtime as Guest>::init(run.cycle_cx.clone()).map_err(fail)?;
        <Runtime as Guest>::migrate(blob, self.meta.version.clone()).map_err(fail)
    }

    /// The trace of an output.
    pub fn output(&self, port: &str) -> OutputTrace {
        self.port(port, &[PortKind::Output]);
        OutputTrace::new(
            self.run
                .as_ref()
                .and_then(|r| r.traces.get(port).cloned())
                .unwrap_or_default(),
        )
    }

    /// The condition of an alarm of the app, tick by tick.
    pub fn alarm(&self, name: &str) -> AlarmTrace {
        self.port(name, &[PortKind::Alarm]);
        AlarmTrace::new(
            self.run
                .as_ref()
                .and_then(|r| r.alarms.get(name).cloned())
                .unwrap_or_default(),
        )
    }

    /// The events of an `EventOutput<E>`.
    pub fn events(&self, port: &str) -> EventTrace {
        self.port(port, &[PortKind::EventOut]);
        EventTrace::new(
            self.run
                .as_ref()
                .and_then(|r| r.events.get(port).cloned())
                .unwrap_or_default(),
        )
    }

    /// The current value of an `Internal<T>`, if it was written.
    pub fn internal_value<T: Record>(&self, port: &str) -> Option<T> {
        self.port(port, &[PortKind::Internal]);
        let run = self.run.as_ref()?;
        let i = run.ports.iter().position(|p| p.name == port)?;
        let bytes = run.owned.get(i)?.bytes.as_ref()?;
        Some(
            T::decode_from(bytes)
                .unwrap_or_else(|| panic!("internal `{port}` does not decode as {}", T::NAME)),
        )
    }

    /// The errors the tasks returned.
    pub fn errors(&self) -> &[TaskError] {
        self.run.as_ref().map_or(&[], |r| &r.errors)
    }
}

fn jitter(rng: &mut u64, jitter_ns: u64) -> u64 {
    if jitter_ns == 0 {
        0
    } else {
        splitmix(rng) % (jitter_ns + 1)
    }
}

impl Run {
    fn alarm_state(o: &Owned) -> u64 {
        match (o.active, o.was_active) {
            (true, _) => 1,
            (false, true) => 3,
            (false, false) => 0,
        }
    }

    /// The owned values and the async proposals of the previous tick
    /// (docs/01 §4, step 2); returns the outputs the async instance proposed, with the value (none for a
    /// relinquish).
    fn apply_async(&mut self, t: u64) -> Vec<(u32, Option<u64>)> {
        for (i, op) in core::mem::take(&mut self.next_owned) {
            self.apply_owned(i, op, t);
        }
        let mut staged = Vec::new();
        for op in core::mem::take(&mut self.async_ops) {
            match op {
                Op::Set(port, value) => self.stage_async(port, value, false, &mut staged),
                Op::Relinquish(port) => self.stage_async(port, 0, true, &mut staged),
                Op::Publish(port, v) => self.apply_owned(port as usize, Op::Publish(port, v), t),
                Op::Emit(..) => {}
            }
        }
        staged
    }

    fn stage_async(
        &mut self,
        port: u32,
        value: u64,
        relinquish: bool,
        staged: &mut Vec<(u32, Option<u64>)>,
    ) {
        let signal = self.output_signal(port).unwrap_or_else(|| {
            panic!("an async task wrote to port {port}, which is not an output")
        });
        self.image
            .stage_async(Proposal {
                writer: WRITER,
                signal,
                value,
                flags: if relinquish {
                    joker_core::image::RELINQUISH
                } else {
                    0
                },
                ..Proposal::EMPTY
            })
            .unwrap_or_else(|e| panic!("staging an async proposal: {e:?}"));
        staged.push((port, (!relinquish).then_some(value)));
    }

    fn output_signal(&self, port: u32) -> Option<SignalId> {
        self.ports
            .get(port as usize)
            .filter(|p| p.kind == PortKind::Output)
            .and_then(|p| p.signal)
    }

    fn apply_owned(&mut self, i: usize, op: Op, t: u64) {
        let Some(port) = self.ports.get(i) else {
            panic!("a publish to port {i}, which does not exist");
        };
        let Op::Publish(_, value) = op else { return };
        match (port.kind, value) {
            (PortKind::Internal, types::Value::Blob(bytes)) => {
                let o = &mut self.owned[i];
                o.bytes = Some(bytes);
                o.seq += 1;
            }
            (PortKind::Alarm, v) => {
                let active = joker::__private::bits(&v) != 0;
                let o = &mut self.owned[i];
                if o.active != active {
                    o.seq += 1;
                }
                o.was_active |= o.active;
                o.active = active;
                if let Some(trace) = self.alarms.get_mut(&port.name) {
                    trace.push((Duration::from_nanos(t), active));
                }
            }
            (kind, _) => panic!("a publish to {} ({kind:?}) is not served", port.name),
        }
    }

    fn pack(&mut self, task: u32, t: u64) {
        let tick = self.image.tick().expect("a latched tick");
        let wide_off = abi::HEADER_LEN + abi::SAMPLE_LEN * (self.n_slots + self.n_outcomes);
        let header = Header {
            flags: if self.first {
                image_flags::FIRST_TICK
            } else {
                0
            },
            task,
            tick_seq: tick.index,
            t_mono_ns: t,
            t_utc_ns: tick.at.utc_ns,
            dt_ns: tick.dt_ns,
            clock: tick.at.clock as u8,
            n_slots: self.n_slots as u32,
            n_outcomes: self.n_outcomes as u32,
            wide_off: wide_off as u32,
            wide_len: (self.image_buf.len() - wide_off) as u32,
        };
        header
            .write(&mut self.image_buf)
            .unwrap_or_else(|e| panic!("image header: {e:?}"));
        let snapshot = self.image.snapshot();
        for (i, port) in self.ports.iter().enumerate() {
            let Some(slot) = port.slot else { continue };
            let owned = &self.owned[i];
            let mut flags = 0u16;
            let (value, quality, seq, ts_acq) = match port.kind {
                PortKind::Internal => match (&owned.bytes, port.wide) {
                    (Some(bytes), Some(w)) => {
                        let off = wide_off + abi::WIDE_LEN * w as usize;
                        self.image_buf[off..off + bytes.len()].copy_from_slice(bytes);
                        flags |= sample_flags::WIDE;
                        (
                            off as u64 | ((bytes.len() as u64) << 32),
                            Quality::GOOD,
                            owned.seq,
                            t,
                        )
                    }
                    _ => (0, Sample::INITIAL.quality(), 0, 0),
                },
                PortKind::Alarm => (Self::alarm_state(owned), Quality::GOOD, owned.seq, t),
                _ => {
                    let sample = match port.snapshot {
                        Some(s) => snapshot[s],
                        None => Sample::new(port.param, Quality::GOOD, NO_TS_SRC, 0, 1),
                    };
                    if sample.ts_src().is_some() {
                        flags |= sample_flags::HAS_TS_SRC;
                    }
                    (
                        sample.transport_bits(),
                        sample.quality(),
                        sample.seq(),
                        sample.ts_acq(),
                    )
                }
            };
            if seq != self.prev_seq[i] {
                flags |= sample_flags::CHANGED;
            }
            self.prev_seq[i] = seq;
            SampleEntry {
                value,
                quality_code: quality.code() as u8,
                quality_reason: quality.reason().to_u8(),
                flags,
                seq: seq as u32,
                ts_acq,
            }
            .write(&mut self.image_buf, slot as usize)
            .unwrap_or_else(|e| panic!("sample entry: {e:?}"));
            let (Some(outcome), Some(signal)) = (port.outcome, port.signal) else {
                continue;
            };
            let mut result = ResultEntry::default();
            if let Some(view) = self.outputs.view(signal) {
                let mine = self
                    .outputs
                    .last_outcome(WRITER, signal)
                    .unwrap_or(joker_core::Outcome::None);
                result = ResultEntry {
                    outcome: mine.code(),
                    detail: mine.detail(),
                    winner: view.level.map_or(0, |l| l.get()),
                    flags: if view.tripped {
                        abi::result_flags::TRIPPED
                    } else {
                        0
                    },
                    commit_seq: tick.index.saturating_sub(1) as u32,
                    applied: match view.applied {
                        Applied::Value(bits) => bits,
                        Applied::Relinquish => 0,
                    },
                    mine: 0,
                };
            }
            result
                .write(&mut self.image_buf, self.n_slots, outcome as usize)
                .unwrap_or_else(|e| panic!("result entry: {e:?}"));
        }
    }

    fn cycle(&mut self, task: u32, name: &str, t: u64, proposed: &mut BTreeMap<SignalId, u64>) {
        self.pack(task, t);
        let mut out = CycleOut::new(&mut self.proposals);
        <Runtime as CycleGuest>::on_cycle(task, &self.image_buf, &mut out);
        if let Some((code, message)) = out.error() {
            self.errors.push(TaskError {
                at: Duration::from_nanos(t),
                task: name.to_owned(),
                code: code.to_owned(),
                message: message.to_owned(),
            });
        }
        enum Rec {
            Write(u32, u8, u64),
            Publish(u32, u8, u64, Vec<u8>),
            Emit(u32, Vec<u8>),
        }
        let records: Vec<Rec> = abi::records(out.buffer())
            .unwrap_or_else(|e| panic!("proposal buffer of {name}: {e:?}"))
            .filter_map(|r| match r {
                Ok(AbiRecord::Write {
                    flags, port, value, ..
                }) => Some(Rec::Write(port, flags, value)),
                Ok(AbiRecord::Publish {
                    flags,
                    port,
                    value,
                    inline,
                }) => Some(Rec::Publish(port, flags, value, inline.to_vec())),
                Ok(AbiRecord::Emit { port, payload }) => Some(Rec::Emit(port, payload.to_vec())),
                Ok(AbiRecord::Note { .. }) => None,
                Ok(other) => panic!("{name}: {other:?} is not served by the native harness yet"),
                Err(e) => panic!("proposal buffer of {name}: {e:?}"),
            })
            .collect();
        for record in records {
            match record {
                Rec::Write(port, flags, value) => self.write(name, port, flags, value, proposed),
                Rec::Publish(port, flags, value, inline) => {
                    let v = if flags & record_flags::INLINE != 0 {
                        types::Value::Blob(inline)
                    } else {
                        types::Value::Uint(value)
                    };
                    match self.ports.get(port as usize).map(|p| p.kind) {
                        Some(PortKind::Alarm) => {
                            self.apply_owned(port as usize, Op::Publish(port, v), t)
                        }
                        Some(PortKind::Internal) => {
                            self.next_owned.push((port as usize, Op::Publish(port, v)))
                        }
                        _ => panic!("{name} published to port {port}, which it cannot"),
                    }
                }
                Rec::Emit(port, payload) => self.emit(name, port, payload, t),
            }
        }
    }

    fn emit(&mut self, name: &str, port: u32, payload: Vec<u8>, t: u64) {
        let entry = self
            .ports
            .get(port as usize)
            .filter(|p| p.kind == PortKind::EventOut)
            .unwrap_or_else(|| panic!("{name} emitted on port {port}, which is not an event"));
        if let Some(trace) = self.events.get_mut(&entry.name) {
            trace.push((Duration::from_nanos(t), payload));
        }
    }

    fn write(
        &mut self,
        name: &str,
        port: u32,
        flags: u8,
        value: u64,
        proposed: &mut BTreeMap<SignalId, u64>,
    ) {
        let entry = self
            .ports
            .get(port as usize)
            .unwrap_or_else(|| panic!("{name} wrote to port {port}, which does not exist"));
        let signal = entry
            .signal
            .filter(|_| entry.kind == PortKind::Output)
            .unwrap_or_else(|| panic!("{name} wrote to {}, which is not an output", entry.name));
        assert!(
            flags & record_flags::INLINE == 0,
            "{name}: wide values are not served by the native harness yet"
        );
        let relinquish = flags & record_flags::RELINQUISH != 0;
        if !relinquish {
            proposed.insert(signal, value);
        }
        self.image
            .stage_cycle(Proposal {
                writer: WRITER,
                signal,
                value,
                flags: if relinquish {
                    joker_core::image::RELINQUISH
                } else {
                    0
                },
                ..Proposal::EMPTY
            })
            .unwrap_or_else(|e| panic!("staging a proposal of {name}: {e:?}"));
    }

    /// The current samples for the async view, the outcomes of the async
    /// proposals of this tick, then the triggers and the executor.
    fn tasks_after_commit(&mut self, t: u64, at: Instant, staged: &[(u32, Option<u64>)]) {
        let Some(host) = self.host.clone() else {
            return;
        };
        let snapshot = self.image.snapshot();
        let samples: Vec<Option<types::Sample>> = self
            .ports
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let o = &self.owned[i];
                match p.kind {
                    PortKind::Input | PortKind::Output => p.snapshot.map(|s| {
                        let sample = snapshot[s];
                        host::sample(
                            host::wire(Value::from_bits(sample.transport_bits(), p.ty), p.ty),
                            sdk_quality(sample.quality()),
                            sample.seq(),
                            sample.ts_acq(),
                        )
                    }),
                    PortKind::Param => Some(host::sample(
                        host::wire(Value::from_bits(p.param, p.ty), p.ty),
                        joker::Quality::GOOD,
                        1,
                        0,
                    )),
                    PortKind::Internal => Some(match &o.bytes {
                        Some(b) => host::sample(
                            types::Value::Blob(b.clone()),
                            joker::Quality::GOOD,
                            o.seq,
                            t,
                        ),
                        None => host::sample(
                            types::Value::Blob(Vec::new()),
                            joker::Quality::uncertain(joker::QualityReason::Initial),
                            0,
                            0,
                        ),
                    }),
                    PortKind::Alarm => Some(host::sample(
                        types::Value::Uint(Self::alarm_state(o)),
                        joker::Quality::GOOD,
                        o.seq,
                        t,
                    )),
                    _ => None,
                }
            })
            .collect();
        {
            let mut state = host.lock();
            state.tick = self.tick;
            state.now_mono_ns = t;
            state.now_utc_ns = at.utc_ns;
            state.clock = Some(match at.clock {
                ClockQuality::Synced => types::ClockQuality::Synced,
                ClockQuality::Holdover => types::ClockQuality::Holdover,
                ClockQuality::Unsynced => types::ClockQuality::Unsynced,
            });
            for &(port, _) in staged {
                let Some(signal) = self.output_signal(port) else {
                    continue;
                };
                let applied = match self.outputs.view(signal).map(|v| v.applied) {
                    Some(Applied::Value(bits)) => bits,
                    _ => 0,
                };
                let ty = self.ports[port as usize].ty;
                if let Some(o) = self
                    .outputs
                    .last_outcome(WRITER, signal)
                    .and_then(|o| wire_outcome(o, host::wire(Value::from_bits(applied, ty), ty)))
                {
                    state.outcomes.insert(port, (self.tick + 1, o));
                }
            }
            state.samples = samples.clone();
        }
        let startup_running = self
            .exec
            .iter()
            .any(|s| s.trigger == TriggerDecl::Startup && (s.running.is_some() || self.first));
        let now = types::Instant {
            mono_ns: t,
            utc_ns: at.utc_ns,
            clock: types::ClockQuality::Synced,
        };
        let mut fired: Vec<(usize, Trigger)> = Vec::new();
        for (k, slot) in self.exec.iter_mut().enumerate() {
            match slot.trigger {
                TriggerDecl::Startup if self.first => fired.push((k, Trigger::Startup)),
                _ if startup_running && slot.trigger != TriggerDecl::Startup => {}
                TriggerDecl::Every {
                    period_ns,
                    jitter_ns,
                } if t >= slot.next_every => {
                    let k_period = (t / period_ns + 1) * period_ns;
                    slot.next_every = k_period + jitter(&mut self.jitter_rng, jitter_ns);
                    fired.push((k, Trigger::Timer(now)));
                }
                TriggerDecl::On(names) => {
                    for name in names {
                        let Some(i) = self.ports.iter().position(|p| p.name == *name) else {
                            continue;
                        };
                        let Some(sample) = samples[i].clone() else {
                            continue;
                        };
                        if sample.seq != self.trigger_seq[i] {
                            fired.push((
                                k,
                                Trigger::Change(types::Change {
                                    port: i as u32,
                                    sample,
                                }),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        for (i, s) in samples.iter().enumerate() {
            if let Some(s) = s {
                self.trigger_seq[i] = s.seq;
            }
        }
        for (k, why) in fired {
            let slot = &mut self.exec[k];
            if slot.running.is_some() {
                slot.pending = Some(why);
            } else {
                slot.running = Some(Box::pin(<Runtime as Guest>::run_task(slot.id, why)));
            }
        }
        self.poll(t);
        let ops = core::mem::take(&mut host.lock().ops);
        for op in ops {
            match op {
                Op::Emit(port, payload) => self.emit("an async task", port, payload, t),
                other => self.async_ops.push(other),
            }
        }
    }

    fn poll(&mut self, t: u64) {
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            let mut progressed = false;
            for slot in &mut self.exec {
                let Some(running) = slot.running.as_mut() else {
                    continue;
                };
                let Poll::Ready(result) = running.as_mut().poll(&mut cx) else {
                    continue;
                };
                progressed = true;
                slot.running = slot
                    .pending
                    .take()
                    .map(|why| -> Running { Box::pin(<Runtime as Guest>::run_task(slot.id, why)) });
                if let Err(e) = result {
                    self.errors.push(TaskError {
                        at: Duration::from_nanos(t),
                        task: slot.name.to_owned(),
                        code: e.code,
                        message: e.message,
                    });
                }
            }
            if !progressed {
                break;
            }
        }
    }
}
