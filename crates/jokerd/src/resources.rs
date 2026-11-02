//! Reads `[resources]` from the manifest (ref/01 §5.4, M8) and applies it
//! to the running process: CPU affinity and the rate-monotonic `SCHED_FIFO`
//! priority of each `rt/<group>` thread (ADR 0089).

use joker_manifest::Manifest;
use joker_manifest::value::Value;

/// The CPU ids of `[resources] rt.cpus`, in the order written; empty when
/// the section is absent or malformed (already validated separately by
/// `joker-manifest`'s own checks, `J0140`/`J0144`/`J0145`/`J0360`).
pub fn rt_cpus(manifest: &Manifest) -> Vec<usize> {
    let Some(resources) = manifest.pending.get("resources") else {
        return Vec::new();
    };
    let Some(rt) = resources.as_table().and_then(|t| t.get("rt")) else {
        return Vec::new();
    };
    let Some(cpus) = rt.node.as_table().and_then(|t| t.get("cpus")) else {
        return Vec::new();
    };
    let Value::Array(items) = &cpus.node.value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|n| match n.value {
            Value::Integer(i) => usize::try_from(i).ok(),
            _ => None,
        })
        .collect()
}

/// Pins the calling thread to `cpus` (`sched_setaffinity`, ref/01 §5.4).
/// A failure (no permission, an invalid cpu id) is reported, not fatal:
/// the thread keeps running, unpinned.
#[cfg(target_os = "linux")]
pub fn pin_current_thread(cpus: &[usize]) -> Result<(), String> {
    use nix::sched::{CpuSet, sched_setaffinity};
    use nix::unistd::Pid;

    if cpus.is_empty() {
        return Ok(());
    }
    let mut set = CpuSet::new();
    for &cpu in cpus {
        set.set(cpu)
            .map_err(|e| format!("invalid cpu {cpu}: {e}"))?;
    }
    sched_setaffinity(Pid::from_raw(0), &set).map_err(|e| format!("sched_setaffinity: {e}"))
}

/// No-op outside Linux (ref/01 §5.4 is a Linux-only section, `J0145` on a
/// Joker Micro target).
#[cfg(not(target_os = "linux"))]
pub fn pin_current_thread(_cpus: &[usize]) -> Result<(), String> {
    Ok(())
}

/// Runs the calling thread `SCHED_FIFO` at `priority` (1–99; a cycle
/// group's rate-monotonic priority, ADR 0089). A failure (no
/// `CAP_SYS_NICE`) is reported, not fatal: the thread keeps the default
/// policy.
#[cfg(target_os = "linux")]
pub fn raise_priority(priority: u8) -> Result<(), String> {
    use thread_priority::{
        RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
        set_thread_priority_and_policy, thread_native_id,
    };
    let value = ThreadPriorityValue::try_from(priority)?;
    set_thread_priority_and_policy(
        thread_native_id(),
        ThreadPriority::Crossplatform(value),
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
    )
    .map_err(|e| format!("SCHED_FIFO {priority}: {e:?}"))
}

/// No-op outside Linux.
#[cfg(not(target_os = "linux"))]
pub fn raise_priority(_priority: u8) -> Result<(), String> {
    Ok(())
}

/// Whether this process may run threads `SCHED_FIFO`: `CAP_SYS_NICE` in
/// its effective capabilities, or an `RLIMIT_RTPRIO` that is not zero
/// (ADR 0108).
#[cfg(target_os = "linux")]
pub fn realtime_allowed() -> bool {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let limits = std::fs::read_to_string("/proc/self/limits").unwrap_or_default();
    realtime_allowed_by(&status, &limits)
}

/// Never outside Linux: the node has no `SCHED_FIFO` there (ADR 0108).
#[cfg(not(target_os = "linux"))]
pub fn realtime_allowed() -> bool {
    false
}

/// [`realtime_allowed`] from the text of `/proc/self/status` and
/// `/proc/self/limits`.
pub fn realtime_allowed_by(status: &str, limits: &str) -> bool {
    let nice = status
        .lines()
        .find_map(|l| l.strip_prefix("CapEff:"))
        .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
        .is_some_and(|caps| caps & (1 << 23) != 0);
    let rtprio = limits
        .lines()
        .find_map(|l| l.strip_prefix("Max realtime priority"))
        .and_then(|rest| rest.split_whitespace().next())
        .is_some_and(|soft| soft == "unlimited" || soft.parse::<u64>().is_ok_and(|p| p > 0));
    nice || rtprio
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use joker_manifest::MemoryFileSystem;

    use super::*;
    use crate::load::load;
    use crate::native::NativeTasks;

    const BASE: &str = r#"
[project]
name  = "bench"
joker = "1.0"
mode  = "dev"

[outputs."pi.act_led"]
safe = false

[cycles.main]
period = "20ms"

[apps.light]
path   = "apps/light"
bind   = { led = "pi.act_led" }
cycles = { blink = "main" }
"#;

    const SIM: &str = r#"
extends = ["joker.toml"]

[devices.pi]
driver = "sim"

[devices.pi.signals]
act_led = { type = "bool", direction = "output" }
"#;

    fn loaded(resources: &str) -> Manifest {
        let mut fs = MemoryFileSystem::default();
        fs.dir("apps/light");
        fs.file("joker.toml", format!("{BASE}{resources}"));
        fs.file("joker.sim.toml", SIM);
        load(&fs, Path::new("joker.sim.toml"), &NativeTasks)
            .unwrap()
            .manifest
    }

    #[test]
    fn no_resources_section_gives_no_cpus() {
        assert_eq!(rt_cpus(&loaded("")), Vec::<usize>::new());
    }

    #[test]
    fn rt_cpus_are_read_in_order() {
        let manifest = loaded("[resources]\nrt = { cpus = [2, 0] }\n");
        assert_eq!(rt_cpus(&manifest), vec![2, 0]);
    }

    #[test]
    fn an_empty_cpu_list_pins_nothing() {
        pin_current_thread(&[]).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pinning_to_cpu_0_succeeds() {
        pin_current_thread(&[0]).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pinning_to_an_out_of_range_cpu_fails() {
        assert!(pin_current_thread(&[usize::MAX]).is_err());
    }

    const LIMITS: &str = "Limit                     Soft Limit           Hard Limit           Units\nMax realtime priority     RTPRIO               RTPRIO\n";

    #[test]
    fn real_time_needs_cap_sys_nice_or_an_rtprio_limit() {
        let none = LIMITS.replace("RTPRIO", "0");
        assert!(!realtime_allowed_by("CapEff:\t0000000000000000\n", &none));
        assert!(realtime_allowed_by("CapEff:\t0000000000800000\n", &none));
        assert!(realtime_allowed_by("CapEff:\t000001ffffffffff\n", &none));
        let some = LIMITS.replace("RTPRIO", "99");
        assert!(realtime_allowed_by("CapEff:\t0000000000000000\n", &some));
        let unlimited = LIMITS.replace("RTPRIO", "unlimited");
        assert!(realtime_allowed_by("", &unlimited));
        assert!(!realtime_allowed_by("", ""));
    }
}
