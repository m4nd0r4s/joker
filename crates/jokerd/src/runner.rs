//! A running generation: the cycle thread and the service threads that
//! drain it (docs/01 §2).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use joker_core::envelope::Applied;

use crate::cycle::{CycleError, CycleExit, GroupTasks, RunOptions, run};
use crate::decisions::{DecisionWriter, SharedLog};
use crate::pacer::Pacer;
use crate::plan::Plan;
use crate::shared::Shared;
use crate::trace_ring::{RingFeeder, SharedRing, TraceRing};

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
}

impl Running {
    /// Starts the cycle thread `rt/cycle` and the services of a plan.
    pub fn start(
        plan: Arc<Plan>,
        tasks: Vec<GroupTasks>,
        mut pacer: Box<dyn Pacer>,
        log: SharedLog,
        start: Start,
    ) -> Result<Self, CycleError> {
        let shared = Arc::new(Shared::new(&plan));
        let ring = Arc::new(Mutex::new(TraceRing::new(&plan, start.retention)));
        let decisions = DecisionWriter::spawn(
            Arc::clone(&plan),
            Arc::clone(&shared),
            log,
            start.generation,
            start.utc_origin_ns,
        );
        let feeder = RingFeeder::spawn(&plan, Arc::clone(&shared), Arc::clone(&ring));
        let cycle = {
            let plan = Arc::clone(&plan);
            let shared = Arc::clone(&shared);
            let options = RunOptions {
                inherit: start.inherit,
                max_ticks: start.max_ticks,
                epoch_mono_ns: None,
            };
            let cpus = start.cpus;
            std::thread::Builder::new()
                .name("rt/cycle".into())
                .spawn(move || {
                    if let Err(e) = crate::resources::pin_current_thread(&cpus) {
                        eprintln!("rt/cycle: {e}");
                    }
                    run(&plan, &shared, tasks, pacer.as_mut(), options)
                })
                .map_err(|e| CycleError(format!("cannot start the cycle thread: {e}")))?
        };
        Ok(Self {
            generation: start.generation,
            plan,
            shared,
            ring,
            cycle: Some(cycle),
            decisions: Some(decisions),
            feeder: Some(feeder),
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
        if let Some(f) = self.feeder.take() {
            f.stop();
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
        if let Some(f) = self.feeder.take() {
            f.stop();
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
        if let Some(f) = self.feeder.take() {
            f.stop();
        }
        if let Some(d) = self.decisions.take() {
            let _ = d.stop();
        }
    }
}
