//! A running generation: the cycle thread and the service threads that
//! drain it (docs/01 §2).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use joker_core::SignalId;
use joker_core::envelope::Applied;
use joker_core::image::Proposal;
use serde_json::{Map, json};

use crate::cycle::{CycleError, CycleExit, GroupTasks, RunOptions, run};
use crate::decisions::{DecisionWriter, SharedLog, lock};
use crate::pacer::Pacer;
use crate::plan::Plan;
use crate::shared::{INBOX_CAPACITY, Shared};
use crate::supervisor::{Heartbeat, Recorder, Supervisor};
use crate::trace_ring::{RingFeeder, SharedRing, TraceRing};
use crate::trips::TripTable;

/// What a generation needs to start.
#[derive(Debug)]
pub struct Start {
    /// Generation number, for the decision log.
    pub generation: u64,
    /// Last applied values of the previous generation (docs/16 A8).
    pub inherit: BTreeMap<String, Applied>,
    /// Retention of the continuous trace.
    pub retention: std::time::Duration,
    /// Stop after this many ticks (tests).
    pub max_ticks: Option<u64>,
    /// UTC time at monotonic zero, to stamp decision records.
    pub utc_origin_ns: i64,
    /// CPU ids to pin the cycle thread to (`[resources] rt.cpus`,
    /// `resources::rt_cpus`); empty does nothing.
    pub cpus: Vec<usize>,
}

/// What a generation receives from the daemon that runs it, beyond
/// [`Start`].
#[derive(Debug, Default)]
pub struct StartContext {
    /// The daemon's supervisor heartbeat, beaten by this generation's
    /// `rt/supervisor` (the watchdog feed reads it, ADR 0061).
    pub heartbeat: Option<Arc<Heartbeat>>,
    /// The persisted trips, kept up to date with this generation's trip
    /// transitions (docs/01 §7).
    pub trips: Option<Arc<TripTable>>,
    /// What to restore before the first tick.
    pub restore: Restore,
    /// The node's alarm book: restores the alarms' state and receives
    /// their events (ADR 0082).
    pub alarms: Option<Arc<crate::alarms::AlarmBook>>,
    /// The node's historian: records `[telemetry.record]` and the alarm
    /// journal (ADR 0081, ADR 0082).
    pub historian: Option<Arc<crate::historian::Historian>>,
    /// The manual levels, for `manual_expiring` and `manual_restored`
    /// (ADR 0085).
    pub manual: Option<Arc<crate::manual::ManualTable>>,
    /// The manual levels restored at startup without a synced clock, as
    /// (output, level).
    pub restored_manual: Vec<(String, u8)>,
}

/// The persisted state a generation restores before its first tick
/// (docs/01 §7, docs/16 A14, ADR 0061).
#[derive(Debug, Default)]
pub struct Restore {
    /// Outputs to put in trip.
    pub trips: Vec<SignalId>,
    /// Manual levels to stage, as proposals, by owning group index.
    pub manual: Vec<(usize, Proposal)>,
}

/// Stages `manual` in the inboxes of `shared` before the cycle thread
/// starts, so that the first tick of each group drains them. A group that
/// would receive more than its inbox holds is an error, recorded in the
/// decision log (`restore_failed`), and nothing is staged.
fn stage_restored(
    plan: &Plan,
    shared: &Shared,
    manual: &[(usize, Proposal)],
    log: &SharedLog,
    start: &Start,
) -> Result<(), CycleError> {
    for (g, group) in plan.groups.iter().enumerate() {
        let count = manual.iter().filter(|(owner, _)| *owner == g).count();
        if count > INBOX_CAPACITY {
            let mut m = Map::new();
            m.insert("gen".into(), json!(start.generation));
            m.insert("record".into(), json!("restore_failed"));
            m.insert("what".into(), json!("manual"));
            m.insert("group".into(), json!(group.name));
            m.insert("count".into(), json!(count));
            m.insert("capacity".into(), json!(INBOX_CAPACITY));
            m.insert(
                "ts".into(),
                json!(crate::wire::timestamp(crate::wire::utc_now_ns())),
            );
            if let Err(e) = lock(log).append(m, true) {
                eprintln!("jokerd: decision log: {e}");
            }
            return Err(CycleError(format!(
                "group `{}`: {count} manual levels to restore, more than its inbox holds \
                 ({INBOX_CAPACITY})",
                group.name
            )));
        }
    }
    for (g, proposal) in manual {
        let inbox = shared
            .inboxes
            .get(*g)
            .ok_or_else(|| CycleError(format!("no inbox for group {g}")))?;
        inbox
            .push(*proposal)
            .map_err(|_| CycleError(format!("the inbox of group {g} is full")))?;
    }
    Ok(())
}

/// A generation that is ticking.
#[derive(Debug)]
pub struct Running {
    /// Generation number.
    pub generation: u64,
    /// Its plan.
    pub plan: Arc<Plan>,
    /// The state shared with the cycle thread.
    pub shared: Arc<Shared>,
    /// The continuous trace ring.
    pub ring: SharedRing,
    cycle: Option<JoinHandle<Result<CycleExit, CycleError>>>,
    decisions: Option<DecisionWriter>,
    feeder: Option<RingFeeder>,
    supervisor: Option<Supervisor>,
    pump: Option<crate::alarms::AlarmPump>,
    recorder: Option<crate::historian::Recorder>,
    watch: Option<crate::runtime_alarms::Watch>,
}

impl Running {
    /// Starts the cycle thread `rt/cycle` and the services of a plan.
    /// With a real clock (`Pacer::monotonic`), `rt/supervisor` watches
    /// the groups on that same clock; a virtual clock runs unsupervised.
    pub fn start(
        plan: Arc<Plan>,
        tasks: Vec<GroupTasks>,
        pacer: Box<dyn Pacer>,
        log: SharedLog,
        start: Start,
    ) -> Result<Self, CycleError> {
        Self::start_with(plan, tasks, pacer, log, start, StartContext::default())
    }

    /// [`Running::start`], with what the daemon hands every generation.
    pub fn start_with(
        plan: Arc<Plan>,
        tasks: Vec<GroupTasks>,
        mut pacer: Box<dyn Pacer>,
        log: SharedLog,
        start: Start,
        context: StartContext,
    ) -> Result<Self, CycleError> {
        let shared = Arc::new(Shared::new(&plan));
        stage_restored(&plan, &shared, &context.restore.manual, &log, &start)?;
        let supervisor = match pacer.monotonic() {
            Some(clock) => Some(
                Supervisor::spawn(
                    Arc::clone(&plan),
                    Arc::clone(&shared),
                    clock,
                    Recorder {
                        log: Arc::clone(&log),
                        generation: start.generation,
                        utc_origin_ns: start.utc_origin_ns,
                    },
                    context.heartbeat.unwrap_or_default(),
                )
                .map_err(|e| CycleError(format!("cannot start the supervisor thread: {e}")))?,
            ),
            None => None,
        };
        let ring = Arc::new(Mutex::new(TraceRing::new(&plan, start.retention)));
        let alarm_restore = context
            .alarms
            .as_ref()
            .map(|book| book.begin(&plan, pacer.now().mono_ns))
            .unwrap_or_default();
        let journal = context.alarms.as_ref().map(|book| crate::alarms::Journal {
            book: Arc::clone(book),
            journal: context
                .historian
                .as_ref()
                .map(|h| (Arc::clone(h.store()), h.alarm_series())),
            log: Arc::clone(&log),
            generation: start.generation,
            utc_origin_ns: start.utc_origin_ns,
        });
        let pump = match &journal {
            Some(j) => Some(
                crate::alarms::AlarmPump::spawn(Arc::clone(&plan), Arc::clone(&shared), j.clone())
                    .map_err(|e| CycleError(format!("cannot start the alarm pump: {e}")))?,
            ),
            None => None,
        };
        let watch = match journal {
            Some(j) => Some(
                crate::runtime_alarms::Watch::spawn(
                    Arc::clone(&plan),
                    Arc::clone(&shared),
                    crate::runtime_alarms::WatchSources {
                        manual: context.manual.clone(),
                        restored: context.restored_manual.clone(),
                        historian: context.historian.clone(),
                    },
                    j,
                    pacer
                        .monotonic()
                        .unwrap_or_else(crate::pacer::Monotonic::start),
                )
                .map_err(|e| CycleError(format!("cannot start the runtime alarms: {e}")))?,
            ),
            None => None,
        };
        let recorder = match &context.historian {
            Some(h) => {
                h.configure(&plan.telemetry.store);
                let watches = h
                    .series_for(&plan)
                    .map_err(|e| CycleError(format!("historian: {e}")))?;
                let clock = pacer
                    .monotonic()
                    .unwrap_or_else(crate::pacer::Monotonic::start);
                Some(
                    crate::historian::Recorder::spawn(
                        Arc::clone(&shared),
                        Arc::clone(h.ingest()),
                        watches,
                        start.utc_origin_ns,
                        clock,
                    )
                    .map_err(|e| CycleError(format!("cannot start the recorder: {e}")))?,
                )
            }
            None => None,
        };
        let decisions = DecisionWriter::spawn(
            Arc::clone(&plan),
            Arc::clone(&shared),
            log,
            start.generation,
            start.utc_origin_ns,
            context.trips,
        );
        let feeder = RingFeeder::spawn(&plan, Arc::clone(&shared), Arc::clone(&ring));
        let spawned = {
            let plan = Arc::clone(&plan);
            let shared = Arc::clone(&shared);
            let options = RunOptions {
                inherit: start.inherit,
                max_ticks: start.max_ticks,
                epoch_mono_ns: None,
                initial_trips: context.restore.trips,
                tick_guard: None,
                alarms: alarm_restore,
                cpus: start.cpus,
            };
            std::thread::Builder::new()
                .name("rt/cycle".into())
                .spawn(move || run(&plan, &shared, tasks, pacer.as_mut(), options))
        };
        let cycle = match spawned {
            Ok(cycle) => cycle,
            Err(e) => {
                if let Some(s) = supervisor {
                    s.stop();
                }
                feeder.stop();
                let _ = decisions.stop();
                return Err(CycleError(format!("cannot start the cycle thread: {e}")));
            }
        };
        Ok(Self {
            generation: start.generation,
            plan,
            shared,
            ring,
            cycle: Some(cycle),
            decisions: Some(decisions),
            feeder: Some(feeder),
            supervisor,
            pump,
            recorder,
            watch,
        })
    }

    /// True while the cycle thread runs.
    pub fn is_running(&self) -> bool {
        self.cycle.as_ref().is_some_and(|h| !h.is_finished())
    }

    /// Stops after the current tick, drains the services and returns the
    /// last applied values.
    pub fn stop(mut self) -> Result<CycleExit, CycleError> {
        self.shared.request_stop();
        let exit = match self.cycle.take().map(JoinHandle::join) {
            Some(Ok(result)) => result,
            Some(Err(_)) => Err(CycleError("the cycle thread panicked".into())),
            None => Err(CycleError("not started".into())),
        };
        if let Some(s) = self.supervisor.take() {
            s.stop();
        }
        if let Some(f) = self.feeder.take() {
            f.stop();
        }
        if let Some(w) = self.watch.take() {
            w.stop();
        }
        if let Some(p) = self.pump.take() {
            p.stop();
        }
        if let Some(r) = self.recorder.take() {
            r.stop();
        }
        if let Some(d) = self.decisions.take()
            && let Err(e) = d.stop()
        {
            return Err(CycleError(format!("decision log: {e}")));
        }
        exit
    }

    /// Waits for a run with `max_ticks` to end on its own.
    pub fn join(mut self) -> Result<CycleExit, CycleError> {
        let exit = match self.cycle.take().map(JoinHandle::join) {
            Some(Ok(result)) => result,
            Some(Err(_)) => Err(CycleError("the cycle thread panicked".into())),
            None => Err(CycleError("not started".into())),
        };
        if let Some(s) = self.supervisor.take() {
            s.stop();
        }
        if let Some(f) = self.feeder.take() {
            f.stop();
        }
        if let Some(w) = self.watch.take() {
            w.stop();
        }
        if let Some(p) = self.pump.take() {
            p.stop();
        }
        if let Some(r) = self.recorder.take() {
            r.stop();
        }
        if let Some(d) = self.decisions.take()
            && let Err(e) = d.stop()
        {
            return Err(CycleError(format!("decision log: {e}")));
        }
        exit
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.shared.request_stop();
        if let Some(h) = self.cycle.take() {
            let _ = h.join();
        }
        if let Some(s) = self.supervisor.take() {
            s.stop();
        }
        if let Some(f) = self.feeder.take() {
            f.stop();
        }
        if let Some(w) = self.watch.take() {
            w.stop();
        }
        if let Some(p) = self.pump.take() {
            p.stop();
        }
        if let Some(r) = self.recorder.take() {
            r.stop();
        }
        if let Some(d) = self.decisions.take() {
            let _ = d.stop();
        }
    }
}
