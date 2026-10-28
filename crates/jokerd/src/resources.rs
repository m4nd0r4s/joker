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
}
