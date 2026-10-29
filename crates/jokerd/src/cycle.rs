//! The cycle domain: the cycle groups of a generation, tick after tick
//! (docs/01 §2, §4; docs/26 §6; ADR 0089).
//!
//! On the real clock each group ticks on its own thread `rt/<group>`; on a
//! virtual clock every group ticks on the calling thread, so that replay
//! and the tests stay deterministic. Both tick the same per-group state.
//!
//! Each tick of a group latches its ports, drains its async inbox, runs its
//! tasks in order, commits through the arbiter and the envelope, publishes
//! the output status, and hands one trace chunk and the decision records to
//! the services. Every buffer is allocated before the first tick.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use joker_core::alarm::{AlarmCause, AlarmConfig, AlarmEvent, AlarmRuntime, AlarmSnapshot, Alarms};
use joker_core::class::{Classes, MethodResult};
use joker_core::commit::{CommitSink, DecisionRecord, OutputResult, OutputState, WriteResult};
use joker_core::envelope::Applied;
use joker_core::image::{ImageStorage, MethodCall, ProcessImage, Proposal};
use joker_core::output::OutputConfig;
use joker_core::slot::SeqExtender;
use joker_core::trace::{SegmentWriter, TickRecord, TraceResult};
use joker_core::{GroupId, IntakeSlot, Outputs, OutputsStorage, Sample, SignalId, WriterId};

use crate::pacer::Pacer;
use crate::plan::Plan;
use crate::shared::{
    OutputMeta, RESET_CAPACITY, ResetKind, ResetOutcome, STAGING_CAPACITY, Shared, TraceChunk,
};
use crate::task::{CycleTask, Prepare, TaskContext};

/// Why the cycle thread could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleError(pub String);

impl fmt::Display for CycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CycleError {}

/// The tasks of one group, in tick order, each with its writer.
pub type GroupTasks = Vec<(WriterId, Box<dyn CycleTask>)>;

/// How a run ended: the last applied value of every output, for the next
/// generation to inherit (docs/16 A8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleExit {
    /// Canonical output path to its last applied value.
    pub applied: BTreeMap<String, Applied>,
    /// Ticks executed, over all groups.
    pub ticks: u64,
    /// The groups whose thread panicked (ADR 0089 decision 6): their
    /// outputs are not in `applied`, so the next generation starts them at
    /// `safe`.
    pub panicked: Vec<String>,
}

/// Options of a run.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Last applied values of the previous generation (docs/16 A8).
    pub inherit: BTreeMap<String, Applied>,
    /// Stop after this many ticks over all groups (tests and replay).
    pub max_ticks: Option<u64>,
    /// The monotonic instant of tick 0 of every group; `None` is the
    /// pacer's current time.
    pub epoch_mono_ns: Option<u64>,
    /// Outputs put in trip before the first tick: the persisted trips
    /// (docs/01 §7), which hold across restarts and generations until a
    /// reset (ADR 0061).
    pub initial_trips: Vec<SignalId>,
    /// Runs every tick of every group inside this wrapper (tests: an
    /// allocation guard that proves the tick does not allocate, AGENTS.md
    /// invariant 6); `None` runs it directly.
    pub tick_guard: Option<fn(&mut dyn FnMut())>,
    /// Alarm states restored before the first tick (ADR 0082), by alarm
    /// number.
    pub alarms: Vec<(u32, AlarmSnapshot)>,
    /// CPU ids every group thread is pinned to (`[resources] rt.cpus`);
    /// empty does nothing.
    pub cpus: Vec<usize>,
}

struct Sink<'s> {
    shared: &'s Shared,
    results: &'s mut Vec<TraceResult>,
    tick: u64,
    at_mono_ns: u64,
    /// Reset requests admitted this tick (`Outputs::reset` returned
    /// true), as (signal, ticket), awaiting this same tick's own
    /// `output()` to learn whether the output cleared or retripped
    /// (docs/16 E7). Preallocated with [`RESET_CAPACITY`] and never
    /// grown, so the tick does not allocate (AGENTS.md invariant 6).
    pending_resets: &'s mut Vec<(SignalId, u32)>,
    /// The trips of this group's alarms on outputs of other groups.
    edge: &'s [ForeignTrip],
}

impl CommitSink for Sink<'_> {
    fn write(&mut self, result: &WriteResult) {
        if result.proposal.ticket != 0 && self.shared.results.push(*result).is_err() {
            self.shared.results_dropped.bump();
        }
    }

    fn output(&mut self, result: &OutputResult) {
        if self.results.len() < self.results.capacity() {
            self.results.push(TraceResult::from(result));
        }
        let was_tripped = self
            .shared
            .output(result.signal)
            .and_then(|s| s.read())
            .is_some_and(|(_, meta, _)| meta.tripped);
        if result.tripped && !was_tripped {
            self.shared.set_trip_since(result.signal, self.at_mono_ns);
        } else if !result.tripped {
            self.shared.set_trip_since(result.signal, 0);
        }
        if let Some(status) = self.shared.output(result.signal) {
            status.publish(
                result.applied.bits(),
                OutputMeta {
                    level: result.level.map_or(0, |l| l.get()),
                    outcome: result.outcome.code(),
                    detail: result.outcome.detail(),
                    tripped: result.tripped,
                    relinquish: result.applied == Applied::Relinquish,
                },
                self.tick,
                self.at_mono_ns,
            );
        }
        let kind = if result.tripped {
            ResetKind::Retripped
        } else {
            ResetKind::Cleared
        };
        let mut i = 0;
        while i < self.pending_resets.len() {
            let (signal, ticket) = self.pending_resets[i];
            if signal != result.signal {
                i += 1;
                continue;
            }
            self.pending_resets.remove(i);
            let outcome = ResetOutcome {
                ticket,
                signal,
                kind,
            };
            if self.shared.reset_results.push(outcome).is_err() {
                self.shared.reset_results_dropped.bump();
            }
        }
    }

    fn decision(&mut self, record: &DecisionRecord) {
        if self.shared.decisions.push(*record).is_err() {
            self.shared.decisions_dropped.bump();
        }
    }

    fn alarm(&mut self, event: &AlarmEvent) {
        if matches!(event.cause, AlarmCause::Condition)
            && event.to.is_active()
            && !event.from.is_active()
        {
            for t in self.edge.iter().filter(|t| t.alarm == event.alarm) {
                let request = crate::shared::TripRequest { signal: t.output };
                let pushed = self
                    .shared
                    .trips
                    .get(t.owner)
                    .is_some_and(|q| q.push(request).is_ok());
                if !pushed {
                    self.shared.trip_requests_dropped.bump();
                }
            }
        }
        if self.shared.alarm_events.push(*event).is_err() {
            self.shared.alarm_events_dropped.bump();
        }
    }

    fn method(&mut self, result: &MethodResult) {
        if result.call.ticket != 0 && self.shared.method_results.push(*result).is_err() {
            self.shared.method_results_dropped.bump();
        }
    }
}

/// What an alarm of one group trips in another group (ADR 0089 decision
/// 3): the alarm, its `alarm.<a>.active` signal, the output and the index
/// of the output's owner group.
#[derive(Clone, Copy, Debug)]
struct ForeignTrip {
    alarm: u32,
    active: Option<SignalId>,
    output: SignalId,
    owner: usize,
}

/// The trips of `alarms` whose output belongs to a group other than the
/// alarm's own.
fn foreign_trips(alarms: &[AlarmConfig<'_>], outputs: &[OutputConfig<'_>]) -> Vec<ForeignTrip> {
    let mut trips = Vec::new();
    for alarm in alarms {
        for &output in alarm.trip {
            let Some(owner) = outputs.iter().find(|c| c.signal == output).map(|c| c.group) else {
                continue;
            };
            if owner != alarm.group {
                trips.push(ForeignTrip {
                    alarm: alarm.id,
                    active: alarm.active_signal,
                    output,
                    owner: owner.0 as usize,
                });
            }
        }
    }
    trips
}

/// The buffers one group ticks over, allocated before the first tick.
struct GroupStore {
    states: Vec<OutputState>,
    intakes: Vec<IntakeSlot>,
    output_arena: Vec<u64>,
    method_scratch: Vec<Proposal>,
    derived_arena: Vec<u64>,
    alarm_states: Vec<AlarmRuntime>,
    alarm_arena: Vec<u64>,
    commit_last: Vec<Vec<Option<Applied>>>,
    snapshot: Vec<Sample>,
    extenders: Vec<SeqExtender>,
    proposals: Vec<Proposal>,
    method_buffer: Vec<MethodCall>,
}

struct GroupRun<'a> {
    index: usize,
    exported: u64,
    period_ns: u64,
    next_index: u64,
    ticks: u64,
    image: ProcessImage<'a>,
    tasks: GroupTasks,
    results: Vec<TraceResult>,
    pending_resets: Vec<(SignalId, u32)>,
    outputs: Outputs<'a>,
    classes: Classes<'a>,
    alarms: Alarms<'a>,
    commit_tx: Vec<joker_hal::CommitTx<'a>>,
    verifier: crate::verify::Verifier,
    edge: &'a [ForeignTrip],
    level: &'a [ForeignTrip],
    name: &'a str,
    apps: Vec<usize>,
    generation_hash: [u8; 32],
}

/// The ticks a run may still take, over all groups (`RunOptions::max_ticks`).
struct Budget {
    max: Option<u64>,
    claimed: AtomicU64,
}

impl Budget {
    /// Takes one tick; false once `max` ticks were taken.
    fn claim(&self) -> bool {
        let n = self.claimed.fetch_add(1, Ordering::Relaxed);
        self.max.is_none_or(|max| n < max)
    }
}

/// What every group of a run reads and never changes.
struct RunContext<'r> {
    plan: &'r Plan,
    shared: &'r Shared,
    registry: &'r joker_core::Registry<'r>,
    epoch: u64,
    budget: Budget,
    tick_guard: Option<fn(&mut dyn FnMut())>,
}

/// The `SCHED_FIFO` priority of each group (ADR 0089 decision 5): the
/// shortest period gets 90, the next distinct period 89, never below 2.
pub fn rt_priorities(periods_ns: &[u64]) -> Vec<u8> {
    let mut distinct: Vec<u64> = periods_ns.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    periods_ns
        .iter()
        .map(|p| {
            let rank = distinct.iter().position(|d| d == p).unwrap_or(0);
            u8::try_from(90usize.saturating_sub(rank)).map_or(2, |v| v.max(2))
        })
        .collect()
}

/// Runs a plan until `shared.stop` is set or `options.max_ticks` is
/// reached. `tasks` has one entry per group of the plan, in plan order.
///
/// When the pacer forks (the real clock), each group ticks on its own
/// thread `rt/<group>`; otherwise every group ticks on the calling
/// thread, earliest deadline first and
/// ties by group index, which keeps replay deterministic (ADR 0089).
///
/// Each group has one `CommitTx` per real-driver device, bound only to
/// the outputs it owns (ref/06 §2.9, step 7): after each commit it
/// publishes the outputs of that device that changed, for the device's io
/// task to drain through a matching `CommitRx` on the same boxes.
pub fn run(
    plan: &Plan,
    shared: &Shared,
    tasks: Vec<GroupTasks>,
    pacer: &mut dyn Pacer,
    options: RunOptions,
) -> Result<CycleExit, CycleError> {
    let tables = plan.tables().map_err(|e| CycleError(e.to_string()))?;
    let registry = tables.registry.as_registry();
    if shared.slots.len() != registry.len() {
        return Err(CycleError("slot table does not match the registry".into()));
    }
    if tasks.len() != plan.groups.len() {
        return Err(CycleError("one task list per group is required".into()));
    }
    let derived = crate::expr_plan::derived_defs(plan, &registry).map_err(CycleError)?;
    let call_args = crate::classes::call_args(plan);
    let statements = crate::classes::statements(plan, &call_args).map_err(CycleError)?;
    let methods = crate::classes::methods(plan, &statements);
    let instances = crate::classes::instances(&methods);
    let alarm_configs = crate::expr_plan::alarm_configs(plan, &registry).map_err(CycleError)?;
    let foreign = foreign_trips(&alarm_configs, &tables.outputs);
    let edges: Vec<Vec<ForeignTrip>> = (0..plan.groups.len())
        .map(|g| {
            foreign
                .iter()
                .filter(|t| {
                    alarm_configs
                        .iter()
                        .any(|a| a.id == t.alarm && a.group.0 as usize == g)
                })
                .copied()
                .collect()
        })
        .collect();
    let levels: Vec<Vec<ForeignTrip>> = (0..plan.groups.len())
        .map(|g| foreign.iter().filter(|t| t.owner == g).copied().collect())
        .collect();
    let configs: Vec<Vec<OutputConfig<'_>>> = (0..plan.groups.len())
        .map(|g| {
            tables
                .outputs
                .iter()
                .filter(|c| c.group.0 as usize == g)
                .copied()
                .collect()
        })
        .collect();
    let ports: Vec<Vec<SignalId>> = plan
        .groups
        .iter()
        .map(|g| g.ports.iter().filter_map(|p| plan.id(p)).collect())
        .collect();
    let derived_words = Classes::new(&instances, &derived, &mut []).state_words();
    let mut sizing = vec![AlarmRuntime::EMPTY; alarm_configs.len()];
    let alarm_words = Alarms::new(&alarm_configs, &mut sizing)
        .map_err(|e| CycleError(format!("alarms: {e:?}")))?
        .state_words();
    let mut stores: Vec<GroupStore> = configs
        .iter()
        .zip(&ports)
        .map(|(configs, ports)| GroupStore {
            states: vec![OutputState::EMPTY; configs.len()],
            intakes: vec![IntakeSlot::EMPTY; STAGING_CAPACITY],
            output_arena: vec![0u64; configs.iter().map(|c| c.state_words()).sum()],
            method_scratch: vec![Proposal::EMPTY; STAGING_CAPACITY],
            derived_arena: vec![0u64; derived_words],
            alarm_states: vec![AlarmRuntime::EMPTY; alarm_configs.len()],
            alarm_arena: vec![0u64; alarm_words],
            commit_last: shared
                .commit_bindings
                .iter()
                .map(|b| vec![None; b.len()])
                .collect(),
            snapshot: vec![Sample::INITIAL; ports.len()],
            extenders: vec![SeqExtender::for_slot(); ports.len()],
            proposals: vec![Proposal::EMPTY; STAGING_CAPACITY],
            method_buffer: vec![MethodCall::EMPTY; STAGING_CAPACITY],
        })
        .collect();

    let generation_hash = crate::trace_ring::digest_bytes(&plan.manifest_hash);
    let app_names = plan.apps();
    let mut groups = Vec::with_capacity(plan.groups.len());
    for (index, (((store, mut tasks), (configs, ports)), (edge, level))) in stores
        .iter_mut()
        .zip(tasks)
        .zip(configs.iter().zip(&ports))
        .zip(edges.iter().zip(&levels))
        .enumerate()
    {
        let group = &plan.groups[index];
        let GroupStore {
            states,
            intakes,
            output_arena,
            method_scratch,
            derived_arena,
            alarm_states,
            alarm_arena,
            commit_last,
            snapshot,
            extenders,
            proposals,
            method_buffer,
        } = store;
        let mut outputs = Outputs::new(
            &registry,
            &tables.writers,
            OutputsStorage {
                configs,
                states,
                intakes,
            },
        )
        .map_err(|e| CycleError(format!("outputs: {e:?}")))?;
        outputs
            .set_state_arena(output_arena)
            .map_err(|e| CycleError(format!("outputs: {e:?}")))?;
        for (path, applied) in &options.inherit {
            if let Some(id) = plan.id(path) {
                outputs.inherit(id, *applied);
            }
        }
        for signal in &options.initial_trips {
            outputs.trip(*signal);
        }
        let mut classes = Classes::new(&instances, &derived, method_scratch);
        classes
            .set_state_arena(derived_arena)
            .map_err(|e| CycleError(format!("derived signals: {e:?}")))?;
        let mut alarms = Alarms::new(&alarm_configs, alarm_states)
            .map_err(|e| CycleError(format!("alarms: {e:?}")))?;
        alarms
            .set_state_arena(alarm_arena)
            .map_err(|e| CycleError(format!("alarms: {e:?}")))?;
        for (alarm, snapshot) in &options.alarms {
            let _ = alarms.restore(*alarm, *snapshot);
        }
        let commit_tx = shared
            .commit_bindings
            .iter()
            .zip(&shared.commit_boxes)
            .zip(commit_last.iter_mut())
            .map(|((bindings, boxes), last)| {
                joker_hal::CommitTx::owned(&outputs, bindings, boxes, last)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CycleError(format!("commit channel: {e:?}")))?;
        let mut image = ProcessImage::new(
            GroupId(index as u32),
            group.period_ns,
            ImageStorage {
                ports,
                snapshot,
                extenders,
                proposals,
            },
        )
        .map_err(|e| CycleError(format!("group `{}`: {e:?}", group.name)))?;
        image.set_method_buffer(method_buffer);
        for (writer, task) in &mut tasks {
            task.prepare(&Prepare {
                registry: &registry,
                image: &image,
                group: &group.name,
                writer: *writer,
                generation_hash,
            })
            .map_err(|e| CycleError(format!("group `{}`: {e}", group.name)))?;
        }
        groups.push(GroupRun {
            index,
            exported: 0,
            period_ns: group.period_ns,
            next_index: 0,
            ticks: 0,
            image,
            tasks,
            results: Vec::with_capacity(plan.outputs.len()),
            pending_resets: Vec::with_capacity(RESET_CAPACITY),
            outputs,
            classes,
            alarms,
            commit_tx,
            verifier: crate::verify::Verifier::new(plan),
            edge,
            level,
            name: &group.name,
            apps: group
                .tasks
                .iter()
                .map(|t| {
                    app_names
                        .iter()
                        .position(|a| *a == t.app)
                        .unwrap_or(usize::MAX)
                })
                .collect(),
            generation_hash,
        });
    }

    let context = RunContext {
        plan,
        shared,
        registry: &registry,
        epoch: options.epoch_mono_ns.unwrap_or_else(|| pacer.now().mono_ns),
        budget: Budget {
            max: options.max_ticks,
            claimed: AtomicU64::new(0),
        },
        tick_guard: options.tick_guard,
    };
    let forks: Option<Vec<Box<dyn Pacer>>> = groups.iter().map(|_| pacer.fork()).collect();
    shared.running.store(true, Ordering::Release);
    let panicked = match forks {
        Some(pacers) => run_threads(&mut groups, pacers, &context, &options.cpus),
        None => {
            run_interleaved(&mut groups, pacer, &context);
            Ok(Vec::new())
        }
    };
    shared.running.store(false, Ordering::Release);
    let panicked = panicked?;

    let mut exit = CycleExit {
        applied: BTreeMap::new(),
        ticks: groups.iter().map(|g| g.ticks).sum(),
        panicked: panicked
            .iter()
            .filter_map(|&g| plan.groups.get(g).map(|p| p.name.clone()))
            .collect(),
    };
    for g in groups.iter().filter(|g| !panicked.contains(&g.index)) {
        for o in &plan.outputs {
            if let Some(view) = plan.id(&o.path).and_then(|id| g.outputs.view(id)) {
                exit.applied.insert(o.path.clone(), view.applied);
            }
        }
    }
    Ok(exit)
}

/// Every group on the calling thread, earliest deadline first, ties by
/// group index.
fn run_interleaved(groups: &mut [GroupRun<'_>], pacer: &mut dyn Pacer, cx: &RunContext<'_>) {
    while !cx.shared.stop_requested() && cx.budget.claim() {
        let Some(g) = groups
            .iter_mut()
            .min_by_key(|g| (cx.epoch + g.next_index * g.period_ns, g.index))
        else {
            break;
        };
        step(g, pacer, cx);
    }
}

/// One thread `rt/<group>` per group, pinned to `cpus` at its
/// rate-monotonic `SCHED_FIFO` priority. Returns the groups whose thread
/// panicked; an error only when a thread could not be started, after the
/// started ones were stopped and joined.
fn run_threads(
    groups: &mut [GroupRun<'_>],
    pacers: Vec<Box<dyn Pacer>>,
    cx: &RunContext<'_>,
    cpus: &[usize],
) -> Result<Vec<usize>, CycleError> {
    let periods: Vec<u64> = groups.iter().map(|g| g.period_ns).collect();
    let priorities = rt_priorities(&periods);
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(groups.len());
        let mut failed = None;
        for ((g, mut pacer), priority) in groups.iter_mut().zip(pacers).zip(priorities) {
            let index = g.index;
            let name = cx
                .plan
                .groups
                .get(index)
                .map_or_else(|| format!("rt/{index}"), |p| format!("rt/{}", p.name));
            let spawned =
                std::thread::Builder::new()
                    .name(name.clone())
                    .spawn_scoped(scope, move || {
                        if let Err(e) = crate::resources::pin_current_thread(cpus) {
                            eprintln!("{name}: {e}");
                        }
                        if let Err(e) = crate::resources::raise_priority(priority) {
                            eprintln!("{name}: {e}; running at default priority");
                        }
                        while !cx.shared.stop_requested() && cx.budget.claim() {
                            step(g, pacer.as_mut(), cx);
                        }
                    });
            match spawned {
                Ok(h) => handles.push((index, h)),
                Err(e) => {
                    failed = Some(CycleError(format!("cannot start the group thread: {e}")));
                    cx.shared.request_stop();
                    break;
                }
            }
        }
        let mut panicked = Vec::new();
        for (index, handle) in handles {
            if handle.join().is_err() {
                panicked.push(index);
            }
        }
        match failed {
            Some(e) => Err(e),
            None => Ok(panicked),
        }
    })
}

/// How long a group waits for its deadline before it looks at the stop
/// request again: a slow group stops within this, not within its period.
const STOP_POLL_NS: u64 = 50_000_000;

/// Waits for the next deadline of `g` and runs one tick, with its
/// statistics. The last wait is always one `wait_until(deadline)`; a wait
/// longer than [`STOP_POLL_NS`] is cut in pieces, and a stop requested
/// between them ends it without a tick.
fn step(g: &mut GroupRun<'_>, pacer: &mut dyn Pacer, cx: &RunContext<'_>) {
    let deadline = cx.epoch + g.next_index * g.period_ns;
    loop {
        let now = pacer.now().mono_ns;
        if deadline.saturating_sub(now) <= STOP_POLL_NS {
            pacer.wait_until(deadline);
            break;
        }
        if cx.shared.stop_requested() {
            return;
        }
        pacer.wait_until(now + STOP_POLL_NS);
    }
    let started = pacer.now();
    let stats = &cx.shared.groups[g.index];
    stats.last_tick_mono_ns.set(started.mono_ns);
    let late = started.mono_ns.saturating_sub(deadline);
    stats.max_late_ns.max(late);
    stats.jitter.record(late, started.mono_ns);
    let mut index = g.next_index;
    if late >= g.period_ns {
        let current = (started.mono_ns - cx.epoch) / g.period_ns;
        stats.overruns.add(current - index);
        stats.consecutive_overruns.bump();
        index = current;
    } else {
        stats.consecutive_overruns.set(0);
    }
    g.next_index = index + 1;
    let clock: &dyn Pacer = pacer;
    let mut one = || tick(g, index, started, cx.shared, cx.registry, clock);
    match cx.tick_guard {
        Some(guard) => guard(&mut one),
        None => one(),
    }
    export_state(g, cx.plan, cx.shared);
    let ended = pacer.now().mono_ns;
    stats
        .duration
        .record(ended.saturating_sub(started.mono_ns), ended);
    stats
        .max_duration_ns
        .max(ended.saturating_sub(started.mono_ns));
    stats.ticks.bump();
    stats.last_tick.set(index);
    g.ticks += 1;
}

fn tick(
    g: &mut GroupRun<'_>,
    index: u64,
    at: joker_core::Instant,
    shared: &Shared,
    registry: &joker_core::Registry<'_>,
    clock: &dyn Pacer,
) {
    let stats = &shared.groups[g.index];
    if g.image.latch(index, at, &shared.slots, registry).is_err() {
        stats.task_errors.bump();
        return;
    }
    if let Some(inbox) = shared.inboxes.get(g.index) {
        while let Some(p) = inbox.pop() {
            if g.image.stage_async(p).is_err() {
                stats.dropped_proposals.bump();
            }
        }
    }
    if let Some(inbox) = shared.method_inboxes.get(g.index) {
        while let Some(call) = inbox.pop() {
            if g.image.stage_method(call).is_err() {
                stats.dropped_proposals.bump();
            }
        }
    }
    g.pending_resets.clear();
    if let Some(resets) = shared.resets.get(g.index) {
        while g.pending_resets.len() < g.pending_resets.capacity() {
            let Some(req) = resets.pop() else {
                break;
            };
            if g.outputs.reset(req.signal) {
                g.pending_resets.push((req.signal, req.ticket));
            } else {
                let outcome = ResetOutcome {
                    ticket: req.ticket,
                    signal: req.signal,
                    kind: crate::shared::ResetKind::NotTripped,
                };
                if shared.reset_results.push(outcome).is_err() {
                    shared.reset_results_dropped.bump();
                }
            }
        }
    }
    if let Some(swaps) = shared.task_swaps.get(g.index) {
        while let Some(swap) = swaps.pop() {
            swap_task(g, swap, shared, registry);
        }
    }
    if let Some(trips) = shared.trips.get(g.index) {
        while let Some(req) = trips.pop() {
            g.outputs.trip(req.signal);
        }
    }
    for t in g.level {
        let active = t
            .active
            .and_then(|s| shared.slots.get(s.index()))
            .and_then(|s| s.read(8).ok())
            .is_some_and(|s| s.transport_bits() != 0);
        if active {
            g.outputs.trip(t.output);
        }
    }
    if g.classes.evaluate_derived(&g.image, &shared.slots).is_err() {
        stats.task_errors.bump();
    }
    g.results.clear();
    let mut sink = Sink {
        shared,
        results: &mut g.results,
        tick: index,
        at_mono_ns: at.mono_ns,
        pending_resets: &mut g.pending_resets,
        edge: g.edge,
    };
    if let Some(commands) = shared.alarm_commands.get(g.index) {
        while let Some(c) = commands.pop() {
            let alarms = &mut g.alarms;
            let result = match c.op {
                crate::shared::AlarmOp::Ack => alarms.ack(c.alarm, at.mono_ns, &mut sink),
                crate::shared::AlarmOp::Shelve(ttl) => alarms
                    .shelve(c.alarm, ttl, at.mono_ns, &mut sink)
                    .map(|()| true),
                crate::shared::AlarmOp::Unshelve => alarms
                    .unshelve(c.alarm, at.mono_ns, &mut sink)
                    .map(|()| true),
            };
            let reply = crate::shared::AlarmReply {
                ticket: c.ticket,
                result,
            };
            if shared.alarm_replies.push(reply).is_err() {
                shared.alarm_events_dropped.bump();
            }
        }
    }
    if g.alarms
        .evaluate(&g.image, &shared.slots, &mut g.outputs, &mut sink)
        .is_err()
    {
        stats.task_errors.bump();
    }
    for (i, (writer, task)) in g.tasks.iter_mut().enumerate() {
        let stopped = g
            .apps
            .get(i)
            .and_then(|a| shared.app_stopped.get(*a))
            .is_some_and(|s| s.load(Ordering::Acquire));
        if stopped {
            continue;
        }
        let begun = clock.mono_ns();
        let mut cx = TaskContext::new(&mut g.image, &g.outputs, *writer);
        if task.run(&mut cx).is_err() {
            stats.task_errors.bump();
        }
        stats.dropped_proposals.add(cx.dropped());
        let ended = clock.mono_ns();
        if let Some(h) = stats.tasks.get(i) {
            h.record(ended.saturating_sub(begun), ended);
        }
    }
    g.results.clear();
    let mut sink = Sink {
        shared,
        results: &mut g.results,
        tick: index,
        at_mono_ns: at.mono_ns,
        pending_resets: &mut g.pending_resets,
        edge: g.edge,
    };
    if g.classes
        .expand(&mut g.image, &shared.slots, &g.outputs, &mut sink)
        .is_err()
    {
        stats.task_errors.bump();
    }
    if g.outputs
        .commit(&g.image, &shared.slots, &mut sink)
        .is_err()
    {
        stats.task_errors.bump();
    }
    shared.apply_confirmations(g.index, at.mono_ns);
    g.verifier.check(g.index, &g.outputs, shared, at.mono_ns);
    for tx in g.commit_tx.iter_mut() {
        tx.publish(&g.outputs);
    }
    trace(g, index, at, shared);
    let _ = g.image.end_tick();
}

/// Swaps a fresh instance into the task order of `g` (an app restart,
/// ref/09 §5.14): it is prepared like the instances of the first tick, and
/// the old one goes to `retired_tasks` for the daemon to drop outside the
/// cycle; only a full queue drops it here. An instance that does not
/// prepare is dropped, and the old one keeps running.
fn swap_task(
    g: &mut GroupRun<'_>,
    swap: crate::shared::TaskSwap,
    shared: &Shared,
    registry: &joker_core::Registry<'_>,
) {
    let Some((writer, slot)) = g.tasks.get_mut(swap.index) else {
        return;
    };
    let mut task = swap.task;
    let prepared = task.prepare(&Prepare {
        registry,
        image: &g.image,
        group: g.name,
        writer: *writer,
        generation_hash: g.generation_hash,
    });
    let retired = match prepared {
        Ok(()) => std::mem::replace(slot, task),
        Err(_) => {
            shared.groups[g.index].task_errors.bump();
            task
        }
    };
    if let Err(retired) = shared.retired_tasks.push(retired) {
        drop(retired);
    }
}

/// Between two ticks of a group, answers a pending state export request
/// with the state of each app's cycle instance (docs/16 §7). This is the
/// one place the cycle thread allocates, as the export of a hot swap does
/// (ref/04 §4.12), and only when a recording starts.
fn export_state(g: &mut GroupRun<'_>, plan: &Plan, shared: &Shared) {
    let request = shared.export_request.load(Ordering::Acquire);
    if request == 0 || request == g.exported {
        return;
    }
    g.exported = request;
    let first_seq = g.next_index;
    let mut done: Vec<&str> = Vec::new();
    let names = plan.groups.get(g.index).map(|p| &p.tasks);
    for (i, (_, task)) in g.tasks.iter_mut().enumerate() {
        let Some(app) = names.and_then(|t| t.get(i)).map(|t| t.app.as_str()) else {
            continue;
        };
        if done.contains(&app) {
            continue;
        }
        done.push(app);
        let (state, blob) = match task.export_state() {
            Ok(Some(blob)) => (0, blob),
            Ok(None) => (1, Vec::new()),
            Err(_) => (2, Vec::new()),
        };
        let export = crate::shared::StateExport {
            request,
            app: app.to_owned(),
            group: g.index as u32,
            first_seq,
            state,
            blob,
        };
        if shared.exports.push(export).is_err() {
            shared.trace_dropped.bump();
        }
    }
}

fn trace(g: &mut GroupRun<'_>, index: u64, at: joker_core::Instant, shared: &Shared) {
    let Some(mut buf) = shared.trace_free.pop() else {
        shared.trace_dropped.bump();
        return;
    };
    let capacity = buf.capacity();
    buf.clear();
    buf.resize(capacity, 0);
    let record = TickRecord {
        index,
        at,
        inputs: g.image.snapshot(),
        proposals: g.image.direct_proposals(),
        results: &g.results,
        methods: g.image.method_calls(),
    };
    let len = match SegmentWriter::new(g.index as u32, &mut buf) {
        Ok(mut w) => match w.push(&record) {
            Ok(()) => Some(w.finish().len()),
            Err(_) => None,
        },
        Err(_) => None,
    };
    match len {
        Some(len) => {
            buf.truncate(len);
            let chunk = TraceChunk {
                group: g.index as u32,
                index,
                at_mono_ns: at.mono_ns,
                segment: buf,
            };
            if let Err(chunk) = shared.trace_filled.push(chunk) {
                shared.trace_dropped.bump();
                let _ = shared.trace_free.push(chunk.segment);
            }
        }
        None => {
            shared.trace_dropped.bump();
            let _ = shared.trace_free.push(buf);
        }
    }
}
