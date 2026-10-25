//! The harness: construction (ref/04 §6.2), execution (§6.3) and results
//! (§6.4) of one app over a virtual clock and the real core.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::{Mutex, MutexGuard, Once, PoisonError};

use core::time::Duration;

use joker::__private::{AppEntry, CycleTask, Runtime, inventory};
use joker::App;
use joker_core::commit::OutputState;
use joker_core::output::ClockRequirement;
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::Instant;
use joker_core::value::NO_TS_SRC;
use joker_core::{
    Applied, ClockQuality, CommitSink, ImageStorage, Level, OutputConfig, Outputs, OutputsStorage,
    ProcessImage, Proposal, Quality, QualityCode, QualityReason, Registry, RegistryBuilder,
    SafeValue, Sample, SignalId, Slot, ValueType, WriteResult, WriterConfig, WriterId, WriterKind,
};
use joker_host::abi::{
    self, Header, Record, ResultEntry, SampleEntry, image_flags, record_flags, sample_flags,
};
use joker_host::meta::{self, AppMeta, PortKind, TaskKind};
use joker_sdk_sys::cycle::{CycleGuest, CycleOut};
use joker_sdk_sys::exports::joker::runtime::guest::{
    Guest, InitContext, InstanceKind, PortBinding, PortOrigin, RunMode, TaskBinding,
};

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

/// An error a cycle task returned (ref/04 §6.4).
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
    slot: u32,
    outcome: Option<u32>,
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

fn core_quality(q: joker::Quality) -> Quality {
    let code = match q.code {
        joker::QualityCode::Good => QualityCode::Good,
        joker::QualityCode::Uncertain => QualityCode::Uncertain,
        joker::QualityCode::Bad => QualityCode::Bad,
    };
    Quality::new(code, QualityReason::from_u8(q.reason as u8))
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

    /// The lease of the app's writer (default: 3 × the period, docs/16 A9).
    pub fn lease(mut self, d: Duration) -> Self {
        self.before_run("lease");
        self.lease_ns = Some(ns(d));
        self
    }

    /// The level of the app's writer (default: `"control:12"`).
    pub fn level(mut self, level: &str) -> Self {
        self.before_run("level");
        self.level = Level::parse(level).unwrap_or_else(|| panic!("invalid level `{level}`"));
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
        let mut n_outcomes = 0u32;
        for (i, p) in self.meta.ports.iter().enumerate() {
            let ty =
                p.ty.unwrap_or_else(|| panic!("port {} has no type", p.name));
            assert!(
                matches!(p.kind, PortKind::Input | PortKind::Output | PortKind::Param) && !p.many,
                "port {}: {:?} ports are not served by the native harness yet",
                p.name,
                p.kind
            );
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
            let outcome = (p.kind == PortKind::Output).then(|| {
                n_outcomes += 1;
                n_outcomes - 1
            });
            ports.push(Port {
                name: p.name.clone(),
                kind: p.kind,
                ty,
                signal: None,
                snapshot: None,
                param,
                slot: i as u32,
                outcome,
            });
        }
        let registry = builder.build().expect("a valid registry");
        let group = registry.resolve_group("main").expect("the group");
        for port in &mut ports {
            if port.kind != PortKind::Param {
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
        let outputs = Outputs::new(
            &registry,
            leak(vec![WriterConfig {
                kind: WriterKind::Cycle,
                level: self.level,
                lease_ns: Some(lease),
                requires_clock: ClockRequirement::Any,
            }]),
            OutputsStorage {
                configs,
                states: leak(vec![OutputState::EMPTY; n_outs]),
                intakes: leak(vec![None; 256]),
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
        let n_slots = ports.len();
        let image_len = abi::image_len(n_slots, n_outcomes as usize, 0);
        let capacity = abi::DEFAULT_PROPOSAL_CAPACITY;
        let cx = InitContext {
            kind: InstanceKind::Cycle,
            mode: RunMode::Test,
            app: self.meta.name.clone(),
            app_version: self.meta.version.clone(),
            node: "test".to_owned(),
            served_world: joker_host::WORLD.to_owned(),
            generation: 1,
            generation_hash: vec![0; 32],
            seed: 0,
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
                    slot: Some(p.slot),
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
        if let Err(e) = <Runtime as Guest>::init(cx) {
            panic!("init of {}: {}: {}", self.meta.name, e.code, e.message);
        }
        let traces = ports
            .iter()
            .filter(|p| p.kind == PortKind::Output)
            .map(|p| (p.name.clone(), Vec::new()))
            .collect();
        Run {
            slots: leak((0..registry.len()).map(|_| Slot::new()).collect()),
            registry,
            image,
            outputs,
            period_ns,
            tick: 0,
            prev_seq: vec![0; n_slots],
            ports,
            tasks: tasks.into_iter().map(|(i, name, _)| (i, name)).collect(),
            image_buf: vec![0; image_len],
            proposals: vec![0; capacity as usize],
            first: true,
            stopped: false,
            traces,
            errors: Vec::new(),
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

    /// One tick: inputs, latch, the cycle tasks, commit (docs/01 §4).
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
        let mut proposed: BTreeMap<SignalId, u64> = BTreeMap::new();
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

    /// Changes a parameter from the next tick on.
    pub fn set_param(&mut self, name: &str, v: impl Into<Value>) {
        let v = v.into();
        self.port(name, &[PortKind::Param]);
        self.params.insert(name.to_owned(), v);
        if let Some(run) = self.run.as_mut()
            && let Some(port) = run.ports.iter_mut().find(|p| p.name == name)
        {
            port.param = v.to_bits(port.ty);
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

    /// The errors the cycle tasks returned.
    pub fn errors(&self) -> &[TaskError] {
        self.run.as_ref().map_or(&[], |r| &r.errors)
    }
}

impl Run {
    fn pack(&mut self, task: u32, t: u64) {
        let tick = self.image.tick().expect("a latched tick");
        let n_slots = self.ports.len();
        let n_outcomes = self.ports.iter().filter(|p| p.outcome.is_some()).count();
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
            n_slots: n_slots as u32,
            n_outcomes: n_outcomes as u32,
            wide_off: (abi::HEADER_LEN + abi::SAMPLE_LEN * (n_slots + n_outcomes)) as u32,
            wide_len: 0,
        };
        header
            .write(&mut self.image_buf)
            .unwrap_or_else(|e| panic!("image header: {e:?}"));
        let snapshot = self.image.snapshot();
        for port in &self.ports {
            let sample = match port.snapshot {
                Some(i) => snapshot[i],
                None => Sample::new(port.param, Quality::GOOD, NO_TS_SRC, 0, 1),
            };
            let slot = port.slot as usize;
            let mut flags = 0u16;
            if sample.ts_src().is_some() {
                flags |= sample_flags::HAS_TS_SRC;
            }
            if sample.seq() != self.prev_seq[slot] {
                flags |= sample_flags::CHANGED;
            }
            self.prev_seq[slot] = sample.seq();
            let q = sample.quality();
            SampleEntry {
                value: sample.transport_bits(),
                quality_code: q.code() as u8,
                quality_reason: q.reason().to_u8(),
                flags,
                seq: sample.seq() as u32,
                ts_acq: sample.ts_acq(),
            }
            .write(&mut self.image_buf, slot)
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
                .write(&mut self.image_buf, n_slots, outcome as usize)
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
        let records: Vec<(u32, u8, u64)> = abi::records(out.buffer())
            .unwrap_or_else(|e| panic!("proposal buffer of {name}: {e:?}"))
            .filter_map(|r| match r {
                Ok(Record::Write {
                    flags, port, value, ..
                }) => Some((port, flags, value)),
                Ok(_) => None,
                Err(e) => panic!("proposal buffer of {name}: {e:?}"),
            })
            .collect();
        for (port, flags, value) in records {
            let entry = self
                .ports
                .get(port as usize)
                .unwrap_or_else(|| panic!("{name} wrote to port {port}, which does not exist"));
            let signal = entry
                .signal
                .filter(|_| entry.kind == PortKind::Output)
                .unwrap_or_else(|| {
                    panic!("{name} wrote to {}, which is not an output", entry.name)
                });
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
    }
}
