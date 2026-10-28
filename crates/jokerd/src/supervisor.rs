//! `rt/supervisor` (AUR-274, docs/01 §2, §8, §20; docs/09 attack 4(c)):
//! watches each group's heartbeat (`GroupStats::last_tick_mono_ns`) and,
//! when a group goes stale past `STALE_FACTOR` times its period, forces
//! the `force_safe` bit of that group's outputs — bypassing the group
//! entirely, through the already-built
//! `joker_hal::commit::ForceSafe` (docs/09 attack 4(c)'s own
//! refinement: the single-writer principle forbids writing into a
//! group's own boxes any other way) — and records it in the decision
//! log (`supervisor_force_safe`, ADR 0061).
//!
//! The heartbeat is stamped by the cycle thread with its pacer's clock,
//! so the supervisor reads that same clock ([`crate::pacer::Pacer::monotonic`]):
//! one origin for every generation of the daemon. A group is judged from
//! the moment the cycle loop starts, so a group that never ticks at all
//! is stale once its deadline passes, like one that stopped.
//!
//! Each group ticks on its own `rt/<group>` thread (ADR 0089), so a hang
//! or a panic stales only that group, and only its outputs are forced;
//! the other groups keep running until a hardware watchdog, which stops
//! being fed once any group was forced (ADR 0061), restarts the board.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use joker_hal::commit::ForceSafe;
use serde_json::{Map, json};

use crate::decisions::{SharedLog, lock};
use crate::pacer::Monotonic;
use crate::plan::Plan;
use crate::shared::Shared;

/// A group is stale once it has gone this many periods without a tick.
pub const STALE_FACTOR: u64 = 3;

/// How often the supervisor checks, independent of any group's period.
pub const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The (device_index, box_index) pairs of every output of a plan,
/// grouped by owning group index — precomputed once per generation
/// (`Running::start`) so a hang's response does not need to search the
/// plan.
pub fn group_boxes(plan: &Plan, shared: &Shared) -> Vec<Vec<(usize, usize)>> {
    let mut boxes = vec![Vec::new(); plan.groups.len()];
    for (device_index, bindings) in shared.commit_bindings.iter().enumerate() {
        for (box_index, binding) in bindings.iter().enumerate() {
            let Some(o) = plan
                .outputs
                .iter()
                .find(|o| plan.id(&o.path) == Some(binding.signal))
            else {
                continue;
            };
            let Some(g) = plan.groups.iter().position(|g| g.name == o.group) else {
                continue;
            };
            boxes[g].push((device_index, box_index));
        }
    }
    boxes
}

/// The groups that have not ticked for more than `STALE_FACTOR` periods
/// at `now_mono_ns`, each with how long it has been silent. A group's
/// silence counts from its last tick, or from `armed_at_mono_ns` (when
/// the cycle loop started) if it has not ticked since then.
pub fn stale_groups(
    plan: &Plan,
    shared: &Shared,
    armed_at_mono_ns: u64,
    now_mono_ns: u64,
) -> Vec<(usize, u64)> {
    plan.groups
        .iter()
        .enumerate()
        .filter_map(|(g, group)| {
            let last = shared
                .groups
                .get(g)
                .map_or(0, |s| s.last_tick_mono_ns.get())
                .max(armed_at_mono_ns);
            let silent = now_mono_ns.saturating_sub(last);
            (silent > STALE_FACTOR * group.period_ns).then_some((g, silent))
        })
        .collect()
}

/// Sets the `force_safe` bit of every output in `group_boxes`.
pub fn force_safe(shared: &Shared, group_boxes: &[Vec<(usize, usize)>]) {
    for boxes in group_boxes {
        for &(device, index) in boxes {
            if let Some(device_boxes) = shared.commit_boxes.get(device) {
                ForceSafe::new(device_boxes).set(index, true);
            }
        }
    }
}

/// How many of `boxes` the supervisor holds at `safe` through their
/// `force_safe` bit.
pub fn held_at_safe(shared: &Shared, boxes: &[(usize, usize)]) -> usize {
    boxes
        .iter()
        .filter(|&&(device, index)| {
            shared
                .commit_boxes
                .get(device)
                .is_some_and(|b| ForceSafe::new(b).is_set(index))
        })
        .count()
}

/// Checks every group's heartbeat against `now_mono_ns` and sets the
/// `force_safe` bit of the outputs of each stale group in `group_boxes`
/// (ADR 0089 decision 7); the other groups keep their own threads and
/// their outputs. Returns the stale groups, empty when none is.
pub fn check_and_respond(
    plan: &Plan,
    shared: &Shared,
    group_boxes: &[Vec<(usize, usize)>],
    armed_at_mono_ns: u64,
    now_mono_ns: u64,
) -> Vec<(usize, u64)> {
    let stale = stale_groups(plan, shared, armed_at_mono_ns, now_mono_ns);
    for (g, _) in &stale {
        if let Some(boxes) = group_boxes.get(*g) {
            force_safe(shared, std::slice::from_ref(boxes));
        }
    }
    stale
}

/// How long the watchdog feed tolerates a silent supervisor (ADR 0061):
/// many polls, so that scheduling jitter does not stop the feed, and
/// well under any hardware watchdog timeout.
pub const HEARTBEAT_MAX_AGE: Duration = Duration::from_millis(500);

/// What the watchdog feed should do now (docs/01 §8, ADR 0061).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    /// Feed: the supervisor is alive, or there is nothing to supervise.
    Feed,
    /// Do not feed this time: a supervisor runs but has not beaten
    /// within [`HEARTBEAT_MAX_AGE`].
    Skip,
    /// Stop feeding for good: a supervisor forced `safe`, and the
    /// watchdog must restart the daemon.
    Stop,
}

/// The liveness of the supervisors of a daemon, across its generations:
/// each poll of a running supervisor beats it, and the watchdog feed
/// reads it without taking any lock. It keeps its own clock, so a beat
/// and its age are always on one origin.
#[derive(Debug)]
pub struct Heartbeat {
    clock: Monotonic,
    last_beat_ns: AtomicU64,
    armed: AtomicU32,
    forced: AtomicBool,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self::new()
    }
}

impl Heartbeat {
    /// A heartbeat with no supervisor running.
    pub fn new() -> Self {
        Self {
            clock: Monotonic::start(),
            last_beat_ns: AtomicU64::new(0),
            armed: AtomicU32::new(0),
            forced: AtomicBool::new(false),
        }
    }

    /// Records that a supervisor is alive now.
    pub fn beat(&self) {
        self.last_beat_ns
            .store(self.clock.now_ns(), Ordering::Release);
    }

    pub(crate) fn arm(&self) {
        self.beat();
        self.armed.fetch_add(1, Ordering::AcqRel);
    }

    fn disarm(&self) {
        let mut current = self.armed.load(Ordering::Acquire);
        while current > 0 {
            match self.armed.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(seen) => current = seen,
            }
        }
    }

    pub(crate) fn force(&self) {
        self.forced.store(true, Ordering::Release);
    }

    /// Whether a supervisor of this daemon has ever forced `safe`
    /// (sticky until the process restarts, docs/01 §8).
    pub fn forced(&self) -> bool {
        self.forced.load(Ordering::Acquire)
    }

    /// What the watchdog feed should do, with a supervisor that must
    /// have beaten within `max_age`.
    pub fn feed(&self, max_age: Duration) -> Feed {
        if self.forced() {
            return Feed::Stop;
        }
        if self.armed.load(Ordering::Acquire) == 0 {
            return Feed::Feed;
        }
        let age = self
            .clock
            .now_ns()
            .saturating_sub(self.last_beat_ns.load(Ordering::Acquire));
        if u128::from(age) > max_age.as_nanos() {
            Feed::Skip
        } else {
            Feed::Feed
        }
    }
}

/// What the supervisor of one generation records in the decision log.
#[derive(Debug, Clone)]
pub struct Recorder {
    /// The decision log.
    pub log: SharedLog,
    /// Generation number.
    pub generation: u64,
    /// UTC time at monotonic zero, to stamp the entry.
    pub utc_origin_ns: i64,
}

impl Recorder {
    fn force_safe(&self, plan: &Plan, stale: &[(usize, u64)], now_mono_ns: u64) {
        let groups: Vec<&str> = stale
            .iter()
            .filter_map(|(g, _)| plan.groups.get(*g).map(|g| g.name.as_str()))
            .collect();
        let silent_ns = stale.iter().map(|(_, s)| *s).max().unwrap_or(0);
        let mut m = Map::new();
        m.insert("gen".into(), json!(self.generation));
        m.insert("record".into(), json!("supervisor_force_safe"));
        m.insert("groups".into(), json!(groups));
        m.insert("silent_ns".into(), json!(silent_ns));
        m.insert("at_mono_ns".into(), json!(now_mono_ns));
        m.insert(
            "ts".into(),
            json!(crate::wire::timestamp(self.utc_origin_ns.saturating_add(
                i64::try_from(now_mono_ns).unwrap_or(i64::MAX)
            ))),
        );
        if let Err(e) = lock(&self.log).append(m, true) {
            eprintln!("jokerd: rt/supervisor: decision log: {e}");
        }
    }
}

/// The `rt/supervisor` thread of one running generation.
#[derive(Debug)]
pub struct Supervisor {
    stop: Arc<AtomicBool>,
    heartbeat: Arc<Heartbeat>,
    handle: Option<JoinHandle<()>>,
}

impl Supervisor {
    /// Starts watching `shared`'s groups against `plan`'s periods, on
    /// `clock`, the clock the cycle thread stamps its ticks with, beating
    /// `heartbeat` on every poll. A thread that cannot be started is an
    /// error: a generation does not run unsupervised.
    pub fn spawn(
        plan: Arc<Plan>,
        shared: Arc<Shared>,
        clock: Monotonic,
        recorder: Recorder,
        heartbeat: Arc<Heartbeat>,
    ) -> std::io::Result<Self> {
        let group_boxes = group_boxes(&plan, &shared);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let beats = Arc::clone(&heartbeat);
        heartbeat.arm();
        let spawned = std::thread::Builder::new()
            .name("rt/supervisor".into())
            .stack_size(crate::shared::SERVICE_STACK)
            .spawn(move || {
                if let Err(e) = raise_priority() {
                    eprintln!("rt/supervisor: {e}; running at default priority");
                }
                let mut armed_at = None;
                let mut was_stale = vec![false; plan.groups.len()];
                while !flag.load(Ordering::Acquire) {
                    beats.beat();
                    let now_mono_ns = clock.now_ns();
                    if !shared.running.load(Ordering::Acquire) || shared.stop_requested() {
                        armed_at = None;
                        std::thread::sleep(POLL_INTERVAL);
                        continue;
                    }
                    let since = *armed_at.get_or_insert(now_mono_ns);
                    let found = check_and_respond(&plan, &shared, &group_boxes, since, now_mono_ns);
                    let new: Vec<(usize, u64)> = found
                        .iter()
                        .filter(|(g, _)| !was_stale.get(*g).copied().unwrap_or(true))
                        .copied()
                        .collect();
                    if !new.is_empty() {
                        eprintln!(
                            "jokerd: rt/supervisor: a group has not ticked in over {}x its \
                             period; forcing its outputs to safe (ADR 0089)",
                            STALE_FACTOR
                        );
                        recorder.force_safe(&plan, &new, now_mono_ns);
                    }
                    for (g, stale) in was_stale.iter_mut().enumerate() {
                        *stale = found.iter().any(|(s, _)| *s == g);
                    }
                    if !found.is_empty() {
                        beats.force();
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            });
        match spawned {
            Ok(handle) => Ok(Self {
                stop,
                heartbeat,
                handle: Some(handle),
            }),
            Err(e) => {
                heartbeat.disarm();
                Err(e)
            }
        }
    }

    /// Stops the supervisor and waits for its thread to end.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
            self.heartbeat.disarm();
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.halt();
    }
}

/// `SCHED_FIFO` at the highest priority (docs/01 §2): a failure (no
/// `CAP_SYS_NICE`, e.g. under `cargo test`) is a warning, not fatal —
/// same convention as `daemon::lock_memory`. `unsafe_code = "forbid"`
/// applies to `jokerd` (AGENTS.md invariant 10), so this goes through
/// `thread-priority`'s safe wrapper rather than a raw `libc` call.
#[cfg(target_os = "linux")]
fn raise_priority() -> Result<(), String> {
    use thread_priority::{
        RealtimeThreadSchedulePolicy, ThreadPriority, ThreadSchedulePolicy,
        set_thread_priority_and_policy, thread_native_id,
    };
    set_thread_priority_and_policy(
        thread_native_id(),
        ThreadPriority::Max,
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
    )
    .map_err(|e| format!("{e:?}"))
}

#[cfg(not(target_os = "linux"))]
fn raise_priority() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use joker_hal::commit::CommitRx;
    use joker_hal::types::WriteKind;

    /// A plan with one output bound to a real (non-`sim`) driver's point:
    /// `driver = "sim"` never goes through `joker_hal::CommitRx`
    /// (`bridge::device_plans` skips it on purpose), so `force_safe` has
    /// nothing to act on with a simulated device — the device plan is
    /// built by hand here, the same shape `bridge::device_plan` would
    /// produce from a real profile (same technique as
    /// `control::action_tests::plan_with_one_action`).
    pub(crate) fn plan_with_one_bound_output() -> (crate::plan::Plan, crate::shared::Shared) {
        let mut fs = joker_manifest::MemoryFileSystem::default();
        fs.file(
            "joker.toml",
            "[project]\nname = \"t\"\njoker = \"1.0\"\nmode = \"dev\"\n\n\
             [outputs.\"pcs.a\"]\nsafe = false\n\n\
             [cycles.main]\nperiod = \"20ms\"\n\n\
             [devices.pcs]\ndriver = \"sim\"\n\n\
             [devices.pcs.signals]\na = { type = \"bool\", direction = \"output\" }\n",
        );
        let loaded = crate::load::load(
            &fs,
            std::path::Path::new("joker.toml"),
            &crate::native::NativeTasks,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let mut plan = loaded.plan;
        plan.devices.push(crate::bridge::DevicePlan {
            name: "pcs".into(),
            driver: "modbus-rtu".into(),
            bus: None,
            bus_kind: None,
            bus_params: Vec::new(),
            config: Vec::new(),
            quirks: Vec::new(),
            timeout_ns: 1_000_000_000,
            fallback_keepalive_ns: None,
            fallback_outputs: None,
            points: vec![crate::bridge::DevicePointPlan {
                path: "pcs.a".into(),
                addr: Vec::new(),
                type_name: "bool".into(),
                direction: joker_hal::types::Direction::Output,
                poll_ns: None,
                expect: joker_hal::types::RawKind::Bool,
                width: 0,
                acquisition: joker_manifest::context::Acquisition::NONE,
            }],
            actions: Vec::new(),
        });
        let shared = crate::shared::Shared::new(&plan);
        (plan, shared)
    }

    #[test]
    fn a_heartbeat_with_no_supervisor_feeds() {
        assert_eq!(Heartbeat::new().feed(Duration::ZERO), Feed::Feed);
    }

    #[test]
    fn a_supervisor_that_stopped_beating_stops_the_feed() {
        let heartbeat = Heartbeat::new();
        heartbeat.arm();
        assert_eq!(heartbeat.feed(Duration::from_secs(60)), Feed::Feed);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(heartbeat.feed(Duration::from_millis(1)), Feed::Skip);
        heartbeat.beat();
        assert_eq!(heartbeat.feed(Duration::from_secs(60)), Feed::Feed);
    }

    #[test]
    fn a_disarmed_heartbeat_feeds_again_and_a_forced_one_never_does() {
        let heartbeat = Heartbeat::new();
        heartbeat.arm();
        heartbeat.disarm();
        heartbeat.disarm();
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(heartbeat.feed(Duration::from_millis(1)), Feed::Feed);
        heartbeat.force();
        assert_eq!(heartbeat.feed(Duration::from_secs(60)), Feed::Stop);
    }

    #[test]
    fn a_group_that_never_ticked_is_not_stale_before_its_deadline() {
        let (plan, shared) = plan_with_one_bound_output();
        let boxes = group_boxes(&plan, &shared);
        let armed_at = 5_000_000_000;
        let shortest = plan.groups.iter().map(|g| g.period_ns).min().unwrap();
        let deadline = armed_at + STALE_FACTOR * shortest;
        assert!(check_and_respond(&plan, &shared, &boxes, armed_at, deadline).is_empty());
    }

    #[test]
    fn a_group_that_never_ticked_is_stale_after_its_deadline() {
        let (plan, shared) = plan_with_one_bound_output();
        let boxes = group_boxes(&plan, &shared);
        let armed_at = 5_000_000_000;
        let longest = plan.groups.iter().map(|g| g.period_ns).max().unwrap();
        let past = armed_at + STALE_FACTOR * longest + 1;
        let stale = check_and_respond(&plan, &shared, &boxes, armed_at, past);
        let every: Vec<(usize, u64)> = (0..plan.groups.len())
            .map(|g| (g, past - armed_at))
            .collect();
        assert_eq!(stale, every);
    }

    #[test]
    fn a_tick_before_the_loop_started_does_not_count_as_silence() {
        let (plan, shared) = plan_with_one_bound_output();
        shared.groups[0].last_tick_mono_ns.set(1);
        let armed_at = 9_000_000_000;
        assert!(stale_groups(&plan, &shared, armed_at, armed_at + 1).is_empty());
    }

    #[test]
    fn a_group_stale_past_the_factor_forces_every_output_safe() {
        let (plan, shared) = plan_with_one_bound_output();
        let boxes = group_boxes(&plan, &shared);
        shared.groups[0].last_tick_mono_ns.set(1);
        let period_ns = plan.groups[0].period_ns;
        let now = 1 + STALE_FACTOR * period_ns + 1;
        assert!(!check_and_respond(&plan, &shared, &boxes, 0, now).is_empty());

        let mut any_checked = false;
        for (bindings, boxes) in shared
            .commit_bindings
            .iter()
            .zip(shared.commit_boxes.iter())
        {
            if bindings.is_empty() {
                continue;
            }
            let mut state = vec![joker_hal::commit::RxState::default(); bindings.len()];
            let mut rx = CommitRx::new(bindings, boxes, &mut state).unwrap();
            for _ in 0..bindings.len() {
                let write = rx.take().expect("a forced box yields a write");
                assert_eq!(write.kind, WriteKind::ForceSafe);
                any_checked = true;
            }
        }
        assert!(any_checked, "the test plan has no bound outputs to check");
    }

    #[test]
    fn only_the_outputs_of_the_stale_group_are_forced() {
        let mut fs = joker_manifest::MemoryFileSystem::default();
        fs.file(
            "joker.toml",
            "[project]\nname = \"t\"\njoker = \"1.0\"\nmode = \"dev\"\n\n\
             [outputs.\"pcs.a\"]\nsafe = false\ngroup = \"fast\"\n\n\
             [outputs.\"pcs.b\"]\nsafe = false\ngroup = \"slow\"\n\n\
             [cycles.fast]\nperiod = \"10ms\"\n\n\
             [cycles.slow]\nperiod = \"100ms\"\n\n\
             [devices.pcs]\ndriver = \"sim\"\n\n\
             [devices.pcs.signals]\n\
             a = { type = \"bool\", direction = \"output\" }\n\
             b = { type = \"bool\", direction = \"output\" }\n",
        );
        let loaded = crate::load::load(
            &fs,
            std::path::Path::new("joker.toml"),
            &crate::native::NativeTasks,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let mut plan = loaded.plan;
        let point = |path: &str| crate::bridge::DevicePointPlan {
            path: path.into(),
            addr: Vec::new(),
            type_name: "bool".into(),
            direction: joker_hal::types::Direction::Output,
            poll_ns: None,
            expect: joker_hal::types::RawKind::Bool,
            width: 0,
            acquisition: joker_manifest::context::Acquisition::NONE,
        };
        plan.devices.push(crate::bridge::DevicePlan {
            name: "pcs".into(),
            driver: "modbus-rtu".into(),
            bus: None,
            bus_kind: None,
            bus_params: Vec::new(),
            config: Vec::new(),
            quirks: Vec::new(),
            timeout_ns: 1_000_000_000,
            fallback_keepalive_ns: None,
            fallback_outputs: None,
            points: vec![point("pcs.a"), point("pcs.b")],
            actions: Vec::new(),
        });
        let shared = crate::shared::Shared::new(&plan);
        let boxes = group_boxes(&plan, &shared);
        let fast = plan.groups.iter().position(|g| g.name == "fast").unwrap();
        let slow = plan.groups.iter().position(|g| g.name == "slow").unwrap();
        let now = 1_000_000_000;
        shared.groups[fast].last_tick_mono_ns.set(now);
        shared.groups[slow].last_tick_mono_ns.set(1);
        let stale = check_and_respond(&plan, &shared, &boxes, 0, now);
        assert_eq!(stale.iter().map(|(g, _)| *g).collect::<Vec<_>>(), [slow]);
        assert_eq!(held_at_safe(&shared, &boxes[slow]), 1);
        assert_eq!(held_at_safe(&shared, &boxes[fast]), 0);
    }
}
