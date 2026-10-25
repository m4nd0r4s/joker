//! The cycle domain: one thread that runs every cycle group of a
//! generation, tick after tick (docs/01 §2, §4; docs/26 §6).
//!
//! Each tick of a group latches its ports, drains its async inbox, runs its
//! tasks in order, commits through the arbiter and the envelope, publishes
//! the output status, and hands one trace chunk and the decision records to
//! the services. Every buffer is allocated before the first tick.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::Ordering;

use joker_core::alarm::{AlarmEvent, AlarmRuntime, AlarmSnapshot, Alarms};
use joker_core::class::{Classes, MethodResult};
use joker_core::commit::{CommitSink, DecisionRecord, OutputResult, OutputState, WriteResult};
use joker_core::envelope::Applied;
use joker_core::image::{ImageStorage, MethodCall, ProcessImage, Proposal};
use joker_core::slot::SeqExtender;
use joker_core::trace::{SegmentWriter, TickRecord, TraceResult};
use joker_core::{GroupId, Outputs, OutputsStorage, Sample, SignalId, WriterId};

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

struct GroupRun<'a> {
    index: usize,
    exported: u64,
    period_ns: u64,
    next_index: u64,
    image: ProcessImage<'a>,
    tasks: GroupTasks,
    results: Vec<TraceResult>,
    pending_resets: Vec<(SignalId, u32)>,
}

/// Runs a plan on the current thread until `shared.stop` is set or
/// `options.max_ticks` is reached. `tasks` has one entry per group of the
/// plan, in plan order.
///
/// There is one `CommitTx` per real-driver device (ref/06 §2.9, step 7):
/// after each group's commit, it publishes every output that device owns
/// and that changed, for that device's io task to drain through a
/// matching `CommitRx` on the same boxes. A device with no output
/// bindings (no real driver, or no output points) gets an empty, harmless
/// `CommitTx`.
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
    let mut states = vec![OutputState::EMPTY; tables.outputs.len()];
    let mut intakes = vec![None; STAGING_CAPACITY];
    let mut output_arena = vec![0u64; tables.outputs.iter().map(|c| c.state_words()).sum()];
    let derived = crate::expr_plan::derived_defs(plan, &registry).map_err(CycleError)?;
    let call_args = crate::classes::call_args(plan);
    let statements = crate::classes::statements(plan, &call_args).map_err(CycleError)?;
    let methods = crate::classes::methods(plan, &statements);
    let instances = crate::classes::instances(&methods);
    let mut method_scratch = vec![Proposal::EMPTY; STAGING_CAPACITY];
    let mut classes = Classes::new(&instances, &derived, &mut method_scratch);
    let mut derived_arena = vec![0u64; classes.state_words()];
    classes
        .set_state_arena(&mut derived_arena)
        .map_err(|e| CycleError(format!("derived signals: {e:?}")))?;
    let alarm_configs = crate::expr_plan::alarm_configs(plan, &registry).map_err(CycleError)?;
    let mut alarm_states = vec![AlarmRuntime::EMPTY; alarm_configs.len()];
    let mut alarms = Alarms::new(&alarm_configs, &mut alarm_states)
        .map_err(|e| CycleError(format!("alarms: {e:?}")))?;
    let mut alarm_arena = vec![0u64; alarms.state_words()];
    alarms
        .set_state_arena(&mut alarm_arena)
        .map_err(|e| CycleError(format!("alarms: {e:?}")))?;
    for (alarm, snapshot) in &options.alarms {
        let _ = alarms.restore(*alarm, *snapshot);
    }
    let mut outputs = Outputs::new(
        &registry,
        &tables.writers,
        OutputsStorage {
            configs: &tables.outputs,
            states: &mut states,
            intakes: &mut intakes,
        },
    )
    .map_err(|e| CycleError(format!("outputs: {e:?}")))?;
    outputs
        .set_state_arena(&mut output_arena)
        .map_err(|e| CycleError(format!("outputs: {e:?}")))?;
    for (path, applied) in &options.inherit {
        if let Some(id) = plan.id(path) {
            outputs.inherit(id, *applied);
        }
    }
    for signal in &options.initial_trips {
        outputs.trip(*signal);
    }
    let mut verifier = crate::verify::Verifier::new(plan);

    let mut commit_last: Vec<Vec<Option<Applied>>> = shared
        .commit_bindings
        .iter()
        .map(|b| vec![None; b.len()])
        .collect();
    let mut commit_tx: Vec<joker_hal::CommitTx<'_>> = shared
        .commit_bindings
        .iter()
        .zip(&shared.commit_boxes)
        .zip(&mut commit_last)
        .map(|((bindings, boxes), last)| joker_hal::CommitTx::new(&outputs, bindings, boxes, last))
        .collect::<Result<_, _>>()
        .map_err(|e| CycleError(format!("commit channel: {e:?}")))?;

    if tasks.len() != plan.groups.len() {
        return Err(CycleError("one task list per group is required".into()));
    }
    let ports: Vec<Vec<SignalId>> = plan
        .groups
        .iter()
        .map(|g| g.ports.iter().filter_map(|p| plan.id(p)).collect())
        .collect();
    let mut snapshots: Vec<Vec<Sample>> = ports
        .iter()
        .map(|p| vec![Sample::INITIAL; p.len()])
        .collect();
    let mut extenders: Vec<Vec<SeqExtender>> = ports
        .iter()
        .map(|p| vec![SeqExtender::for_slot(); p.len()])
        .collect();
    let mut proposals: Vec<Vec<Proposal>> = plan
        .groups
        .iter()
        .map(|_| vec![Proposal::EMPTY; STAGING_CAPACITY])
        .collect();
    let mut method_buffers: Vec<Vec<MethodCall>> = plan
        .groups
        .iter()
        .map(|_| vec![MethodCall::EMPTY; STAGING_CAPACITY])
        .collect();

    let mut groups = Vec::with_capacity(plan.groups.len());
    let storage = ports
        .iter()
        .zip(snapshots.iter_mut())
        .zip(extenders.iter_mut())
        .zip(proposals.iter_mut())
        .zip(method_buffers.iter_mut());
    let generation_hash = crate::trace_ring::digest_bytes(&plan.manifest_hash);
    for (
        index,
        ((group, mut tasks), ((((ports, snapshot), extenders), proposals), method_buffer)),
    ) in plan.groups.iter().zip(tasks).zip(storage).enumerate()
    {
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
            image,
            tasks,
            results: Vec::with_capacity(plan.outputs.len()),
            pending_resets: Vec::with_capacity(RESET_CAPACITY),
        });
    }

    let epoch = options.epoch_mono_ns.unwrap_or_else(|| pacer.now().mono_ns);
    let mut total = 0u64;
    shared.running.store(true, Ordering::Release);
    while !shared.stop_requested() && options.max_ticks.is_none_or(|max| total < max) {
        let Some(g) = groups
            .iter_mut()
            .min_by_key(|g| (epoch + g.next_index * g.period_ns, g.index))
        else {
            break;
        };
        let deadline = epoch + g.next_index * g.period_ns;
        pacer.wait_until(deadline);
        let started = pacer.now();
        let stats = &shared.groups[g.index];
        stats.last_tick_mono_ns.set(started.mono_ns);
        let late = started.mono_ns.saturating_sub(deadline);
        stats.max_late_ns.max(late);
        let mut index = g.next_index;
        if late >= g.period_ns {
            let current = (started.mono_ns - epoch) / g.period_ns;
            stats.overruns.add(current - index);
            stats.consecutive_overruns.bump();
            index = current;
        } else {
            stats.consecutive_overruns.set(0);
        }
        g.next_index = index + 1;
        let mut one = || {
            tick(
                g,
                index,
                started,
                shared,
                &registry,
                &mut Expressions {
                    classes: &mut classes,
                    alarms: &mut alarms,
                },
                &mut outputs,
                &mut commit_tx,
                &mut verifier,
            )
        };
        match options.tick_guard {
            Some(guard) => guard(&mut one),
            None => one(),
        }
        export_state(g, plan, shared);
        let ended = pacer.now().mono_ns;
        stats
            .max_duration_ns
            .max(ended.saturating_sub(started.mono_ns));
        stats.ticks.bump();
        stats.last_tick.set(index);
        total += 1;
    }
    shared.running.store(false, Ordering::Release);

    let mut exit = CycleExit {
        applied: BTreeMap::new(),
        ticks: total,
    };
    for o in &plan.outputs {
        if let Some(view) = plan.id(&o.path).and_then(|id| outputs.view(id)) {
            exit.applied.insert(o.path.clone(), view.applied);
        }
    }
    Ok(exit)
}

/// The derived signals and alarms of a generation (docs/01 §4 step 3).
struct Expressions<'e, 'a> {
    classes: &'e mut Classes<'a>,
    alarms: &'e mut Alarms<'a>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "every argument is a distinct piece of the generation's tick state, borrowed for one tick"
)]
fn tick(
    g: &mut GroupRun<'_>,
    index: u64,
    at: joker_core::Instant,
    shared: &Shared,
    registry: &joker_core::Registry<'_>,
    expressions: &mut Expressions<'_, '_>,
    outputs: &mut Outputs<'_>,
    commit_tx: &mut [joker_hal::CommitTx<'_>],
    verifier: &mut crate::verify::Verifier,
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
            if outputs.reset(req.signal) {
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
    if let Some(trips) = shared.trips.get(g.index) {
        while let Some(req) = trips.pop() {
            outputs.trip(req.signal);
        }
    }
    if expressions
        .classes
        .evaluate_derived(&g.image, &shared.slots)
        .is_err()
    {
        stats.task_errors.bump();
    }
    g.results.clear();
    let mut sink = Sink {
        shared,
        results: &mut g.results,
        tick: index,
        at_mono_ns: at.mono_ns,
        pending_resets: &mut g.pending_resets,
    };
    while let Some(c) = shared.alarm_commands.pop() {
        let alarms = &mut *expressions.alarms;
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
    if expressions
        .alarms
        .evaluate(&g.image, &shared.slots, outputs, &mut sink)
        .is_err()
    {
        stats.task_errors.bump();
    }
    for (writer, task) in &mut g.tasks {
        let mut cx = TaskContext::new(&mut g.image, outputs, *writer);
        if task.run(&mut cx).is_err() {
            stats.task_errors.bump();
        }
        stats.dropped_proposals.add(cx.dropped());
    }
    g.results.clear();
    let mut sink = Sink {
        shared,
        results: &mut g.results,
        tick: index,
        at_mono_ns: at.mono_ns,
        pending_resets: &mut g.pending_resets,
    };
    if expressions
        .classes
        .expand(&mut g.image, &shared.slots, outputs, &mut sink)
        .is_err()
    {
        stats.task_errors.bump();
    }
    if outputs.commit(&g.image, &shared.slots, &mut sink).is_err() {
        stats.task_errors.bump();
    }
    shared.apply_confirmations(at.mono_ns);
    verifier.check(g.index, outputs, shared, at.mono_ns);
    for tx in commit_tx.iter_mut() {
        tx.publish(outputs);
    }
    trace(g, index, at, shared);
    let _ = g.image.end_tick();
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
