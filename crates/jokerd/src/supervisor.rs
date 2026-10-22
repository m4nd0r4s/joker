//! `rt/supervisor` (AUR-274, docs/01 §2, §8, §20; docs/09 attack 4(c)):
//! watches the cycle thread's own per-group heartbeat
//! (`GroupStats::last_tick_mono_ns`) and, if any group goes stale past
//! `STALE_FACTOR` times its period, forces every output's `force_safe`
//! bit — bypassing the group entirely, through the already-built
//! `joker_hal::commit::ForceSafe` (docs/09 attack 4(c)'s own
//! refinement: the single-writer principle forbids writing into a
//! group's own boxes any other way).
//!
//! Today, every group shares one `rt/cycle` thread (AUR-275, one thread
//! per group, is not built yet), so a real hang makes every group go
//! stale together: the response below forces every output of every
//! group, not just the stale one. The detection math (`stale`) does not
//! change when AUR-275 lands — only how many groups the response
//! reaches narrows to the group that is actually stuck.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use joker_hal::commit::ForceSafe;

use crate::plan::Plan;
use crate::shared::Shared;

/// A group is stale once it has gone this many periods without a tick.
pub const STALE_FACTOR: u64 = 3;

/// How often the supervisor checks (independent of any group's period,
/// since today it watches the one shared cycle thread as a whole).
const POLL_INTERVAL: Duration = Duration::from_millis(20);

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

/// Checks every group's heartbeat against `now_mono_ns` and, if any is
/// stale, sets the `force_safe` bit of every output of every group in
/// `group_boxes` (today's whole-process response, see the module doc).
/// Returns whether any group was found stale (for logging and for the
/// hardware-watchdog feed to stop on, AUR-274 phase 4).
pub fn check_and_respond(
    plan: &Plan,
    shared: &Shared,
    group_boxes: &[Vec<(usize, usize)>],
    now_mono_ns: u64,
) -> bool {
    let any_stale = plan.groups.iter().enumerate().any(|(g, group)| {
        let last = shared
            .groups
            .get(g)
            .map_or(0, |s| s.last_tick_mono_ns.get());
        last != 0 && now_mono_ns.saturating_sub(last) > STALE_FACTOR * group.period_ns
    });
    if any_stale {
        for boxes in group_boxes {
            for &(device, index) in boxes {
                if let Some(device_boxes) = shared.commit_boxes.get(device) {
                    ForceSafe::new(device_boxes).set(index, true);
                }
            }
        }
    }
    any_stale
}

/// The `rt/supervisor` thread of one running generation.
#[derive(Debug)]
pub struct Supervisor {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Supervisor {
    /// Starts watching `shared`'s groups against `plan`'s periods. Uses
    /// its own `Monotonic` clock rather than sharing the cycle thread's
    /// pacer (which would need a lock neither thread may block on): the
    /// two clocks' origins are microseconds apart at most, set within
    /// the same `Running::start` call, which is negligible next to the
    /// `STALE_FACTOR`-periods tolerance (tens to hundreds of ms).
    pub fn spawn(plan: Arc<Plan>, shared: Arc<Shared>) -> Self {
        let clock = crate::pacer::Monotonic::start();
        let group_boxes = group_boxes(&plan, &shared);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("rt/supervisor".into())
            .spawn(move || {
                if let Err(e) = raise_priority() {
                    eprintln!("rt/supervisor: {e}; running at default priority");
                }
                let mut was_stale = false;
                while !flag.load(Ordering::Acquire) {
                    let now_mono_ns = clock.now_ns();
                    let stale = check_and_respond(&plan, &shared, &group_boxes, now_mono_ns);
                    if stale && !was_stale {
                        eprintln!(
                            "jokerd: rt/supervisor: a group has not ticked in over {}x its \
                             period; forcing every output to safe (AUR-274)",
                            STALE_FACTOR
                        );
                    }
                    was_stale = stale;
                    std::thread::sleep(POLL_INTERVAL);
                }
            })
            .ok();
        Self { stop, handle }
    }

    /// Stops the supervisor and waits for its thread to end.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
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
mod tests {
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
    fn plan_with_one_bound_output() -> (crate::plan::Plan, crate::shared::Shared) {
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
            points: vec![crate::bridge::DevicePointPlan {
                path: "pcs.a".into(),
                addr: Vec::new(),
                type_name: "bool".into(),
                direction: joker_hal::types::Direction::Output,
                poll_ns: None,
                expect: joker_hal::types::RawKind::Bool,
                width: 0,
            }],
            actions: Vec::new(),
        });
        let shared = crate::shared::Shared::new(&plan);
        (plan, shared)
    }

    #[test]
    fn a_group_that_never_ticked_is_not_stale() {
        let (plan, shared) = plan_with_one_bound_output();
        let boxes = group_boxes(&plan, &shared);
        assert!(!check_and_respond(
            &plan,
            &shared,
            &boxes,
            1_000_000_000_000
        ));
    }

    #[test]
    fn a_group_stale_past_the_factor_forces_every_output_safe() {
        let (plan, shared) = plan_with_one_bound_output();
        let boxes = group_boxes(&plan, &shared);
        // The default group's period is small (no explicit [cycles] ->
        // whatever load()'s own default is); tick it once at t=0, then
        // ask "is it stale" from far enough in the future.
        shared.groups[0].last_tick_mono_ns.set(0);
        shared.groups[0].last_tick_mono_ns.set(1); // any non-zero "has ticked"
        let period_ns = plan.groups[0].period_ns;
        let now = 1 + STALE_FACTOR * period_ns + 1;
        assert!(check_and_respond(&plan, &shared, &boxes, now));

        // Every output's box must now report `forced`: the next write
        // any device's `CommitRx` pulls is a `ForceSafe` one.
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
}
