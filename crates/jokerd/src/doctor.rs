//! `jk doctor` on the node (ref/09 §5.17, document 07): the `doctor` job
//! and its report. The `base` checks read the node and the running
//! generation; the `rt` checks measure how late a `SCHED_FIFO` thread on the
//! `rt` CPUs wakes up. `ml` and `network` have nothing to check in 1.0 and
//! are reported as `skip`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::plan::Plan;
use crate::shared::Shared;
use crate::wire::{duration, timestamp, utc_now_ns};

/// What the checks read besides the node's files.
#[derive(Debug, Clone)]
pub struct Inputs {
    /// The running generation's plan and shared state, if one runs.
    pub running: Option<(Arc<Plan>, Arc<Shared>)>,
    /// The state directory, for the disk check.
    pub state_dir: PathBuf,
    /// `[resources] rt.cpus` of the running generation.
    pub rt_cpus: Vec<usize>,
    /// The daemon's monotonic clock, in ns since it started.
    pub now_mono_ns: u64,
}

fn check(id: &str, status: &str, detail: impl Into<String>) -> Value {
    json!({"id": id, "status": status, "detail": detail.into()})
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// The CPUs of a kernel CPU list (`2-3,5`).
fn cpu_list(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in text.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    out.extend(a..=b);
                }
            }
            None => {
                if let Ok(c) = part.parse() {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Whether the effective capabilities (`CapEff` of `/proc/self/status`)
/// hold capability number `bit`.
fn has_capability(status: &str, bit: u32) -> bool {
    status
        .lines()
        .find_map(|l| l.strip_prefix("CapEff:"))
        .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
        .is_some_and(|caps| caps & (1 << bit) != 0)
}

/// The `base` checks (ref/09 §5.17).
pub fn base(inputs: &Inputs) -> Vec<Value> {
    let mut out = Vec::new();
    let rt = read("/sys/kernel/realtime").is_some_and(|t| t.trim() == "1")
        || read("/proc/version").is_some_and(|v| v.contains("PREEMPT_RT"));
    out.push(if rt {
        check("kernel.preempt_rt", "ok", "PREEMPT_RT kernel")
    } else {
        check(
            "kernel.preempt_rt",
            "warn",
            "the kernel is not PREEMPT_RT: jitter is not guaranteed (J0352)",
        )
    });
    if inputs.rt_cpus.is_empty() {
        out.push(check("cpu.isolcpus", "skip", "no `[resources] rt.cpus`"));
    } else {
        let isolated = cpu_list(&read("/sys/devices/system/cpu/isolated").unwrap_or_default());
        let missing: Vec<usize> = inputs
            .rt_cpus
            .iter()
            .copied()
            .filter(|c| !isolated.contains(c))
            .collect();
        out.push(if missing.is_empty() {
            check(
                "cpu.isolcpus",
                "ok",
                format!("rt CPUs {:?} are isolated", inputs.rt_cpus),
            )
        } else {
            check(
                "cpu.isolcpus",
                "warn",
                format!("rt CPUs {missing:?} are not in isolcpus; other work shares them"),
            )
        });
    }
    let status = read("/proc/self/status").unwrap_or_default();
    let locked = status
        .lines()
        .find_map(|l| l.strip_prefix("VmLck:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .unwrap_or(0);
    let nice = has_capability(&status, 23);
    out.push(match (nice, locked > 0) {
        (true, true) => check("permissions", "ok", "SCHED_FIFO allowed and memory locked"),
        (false, _) => check(
            "permissions",
            "warn",
            "no CAP_SYS_NICE: the rt threads run at the default policy",
        ),
        (true, false) => check(
            "permissions",
            "warn",
            "memory is not locked (mlockall failed): page faults can reach the cycle",
        ),
    });
    let watchdogs: Vec<String> = std::fs::read_dir("/dev")
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("watchdog"))
                .collect()
        })
        .unwrap_or_default();
    out.push(if watchdogs.is_empty() {
        check(
            "watchdog",
            "warn",
            "no /dev/watchdog: a frozen board is not restarted",
        )
    } else {
        check(
            "watchdog",
            "ok",
            format!("{} present", watchdogs.join(", ")),
        )
    });
    out.push(match crate::pacer::SystemPacer::host_clock_quality() {
        joker_core::time::ClockQuality::Synced => check("clock", "ok", "synced"),
        joker_core::time::ClockQuality::Holdover => {
            check("clock", "warn", "holdover: the clock is free-running")
        }
        joker_core::time::ClockQuality::Unsynced => check(
            "clock",
            "warn",
            "unsynced: writers that require a synced clock are refused",
        ),
    });
    let free = crate::system::sample(&inputs.state_dir).disk_free_bytes;
    out.push(match free {
        Some(f) if f < 10 << 20 => check("disk", "fail", format!("{} MiB free", f >> 20)),
        Some(f) if f < 100 << 20 => check("disk", "warn", format!("{} MiB free", f >> 20)),
        Some(f) => check("disk", "ok", format!("{} MiB free", f >> 20)),
        None => check("disk", "skip", "the state directory's disk cannot be read"),
    });
    match &inputs.running {
        None => out.push(check("groups", "fail", "no generation is running")),
        Some((plan, shared)) => {
            let mut stalled = Vec::new();
            let mut overrun = Vec::new();
            for (g, s) in plan.groups.iter().zip(shared.groups.iter()) {
                let silent = inputs.now_mono_ns.saturating_sub(s.last_tick_mono_ns.get());
                if silent > crate::supervisor::STALE_FACTOR * g.period_ns {
                    stalled.push(g.name.clone());
                } else if s.overruns.get() > 0 {
                    overrun.push(format!("{} ({})", g.name, s.overruns.get()));
                }
            }
            out.push(if !stalled.is_empty() {
                check("groups", "fail", format!("stalled: {}", stalled.join(", ")))
            } else if !overrun.is_empty() {
                check(
                    "groups",
                    "warn",
                    format!("overruns: {}", overrun.join(", ")),
                )
            } else {
                check(
                    "groups",
                    "ok",
                    format!("{} groups ticking, no overrun", plan.groups.len()),
                )
            });
            let uncovered: Vec<String> = plan
                .devices
                .iter()
                .filter(|d| {
                    d.points
                        .iter()
                        .any(|p| p.direction == joker_hal::types::Direction::Output)
                        && d.fallback_outputs.is_none()
                })
                .map(|d| d.name.clone())
                .collect();
            out.push(if uncovered.is_empty() {
                check(
                    "device_fallbacks",
                    "ok",
                    "every device with outputs declares its fallback",
                )
            } else {
                check(
                    "device_fallbacks",
                    "warn",
                    format!("no device fallback (M1): {}", uncovered.join(", ")),
                )
            });
        }
    }
    out
}

/// The `rt` checks: how late a `SCHED_FIFO` thread pinned to the `rt` CPUs
/// wakes from 1 ms sleeps for `length` (`rt.cyclictest`).
pub fn rt(inputs: &Inputs, length: Duration) -> Vec<Value> {
    let cpus = inputs.rt_cpus.clone();
    let measured = std::thread::Builder::new()
        .name("svc/doctor".into())
        .spawn(move || {
            let _ = crate::resources::pin_current_thread(&cpus);
            let fifo = crate::resources::raise_priority(80).is_ok();
            let mut late: Vec<u64> = Vec::new();
            let start = Instant::now();
            let mut next = start;
            while start.elapsed() < length {
                next += Duration::from_millis(1);
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                }
                let woke = Instant::now();
                late.push(
                    u64::try_from(woke.saturating_duration_since(next).as_nanos())
                        .unwrap_or(u64::MAX),
                );
            }
            (late, fifo)
        })
        .ok()
        .and_then(|h| h.join().ok());
    let Some((mut late, fifo)) = measured else {
        return vec![check(
            "rt.cyclictest",
            "fail",
            "the measuring thread did not run",
        )];
    };
    late.sort_unstable();
    let at = |q: f64| {
        late.get(((late.len() as f64 * q) as usize).min(late.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0)
    };
    let (p99, max) = (at(0.99), late.last().copied().unwrap_or(0));
    let shortest = inputs
        .running
        .as_ref()
        .and_then(|(plan, _)| plan.groups.iter().map(|g| g.period_ns).min())
        .unwrap_or(u64::MAX);
    let status = if max > shortest {
        "fail"
    } else if p99 > 100_000 || !fifo {
        "warn"
    } else {
        "ok"
    };
    let detail = format!(
        "{} wake-ups at SCHED_FIFO{}: p99 {}, max {}",
        late.len(),
        if fifo {
            ""
        } else {
            " (refused: default policy)"
        },
        duration(p99),
        duration(max)
    );
    let mut c = check("rt.cyclictest", status, detail);
    c["measured"] = json!({"p99": duration(p99), "max": duration(max), "samples": late.len()});
    vec![c]
}

/// The `summary` of a report: its worst status.
pub fn summary(checks: &[Value]) -> &'static str {
    let worst = |s: &str| checks.iter().any(|c| c["status"] == s);
    if worst("fail") {
        "fail"
    } else if worst("warn") {
        "warn"
    } else {
        "ok"
    }
}

/// The node's jobs (`GET /v1/jobs/{job}`) and the last doctor report
/// (`GET /v1/doctor`). Kept in memory: they end with the daemon.
#[derive(Debug, Default)]
pub struct Jobs {
    jobs: Mutex<BTreeMap<String, Value>>,
    last_report: Mutex<Option<Value>>,
}

impl Jobs {
    /// A job by id.
    pub fn get(&self, id: &str) -> Option<Value> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    /// The last finished doctor report.
    pub fn last_report(&self) -> Option<Value> {
        self.last_report
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn put(&self, job: Value) {
        if let Some(id) = job["id"].as_str() {
            self.jobs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id.to_owned(), job);
        }
    }

    /// Whether a job of `kind` is still running.
    pub fn running(&self, kind: &str) -> bool {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|j| j["kind"] == kind && j["state"] == "running")
    }

    /// Starts a job of `kind` created by `who`: `work` runs on a thread of
    /// its own, and its value is the job's `result` (`succeeded`), or its
    /// problem the job's `error` (`failed`). Returns the job as created.
    pub fn start<F>(self: &Arc<Self>, kind: &str, who: &str, work: F) -> std::io::Result<Value>
    where
        F: FnOnce() -> Result<Value, Value> + Send + 'static,
    {
        let id = joker_api::id::ulid(u128::try_from(utc_now_ns()).unwrap_or(0));
        let created = timestamp(utc_now_ns());
        let job = json!({
            "id": id,
            "kind": kind,
            "state": "running",
            "created_at": created,
            "created_by": who,
            "started_at": created,
        });
        self.put(job.clone());
        let jobs = Arc::clone(self);
        let mut running = job.clone();
        std::thread::Builder::new()
            .name(format!("svc/{kind}"))
            .spawn(move || {
                let outcome = work();
                running["finished_at"] = json!(timestamp(utc_now_ns()));
                match outcome {
                    Ok(result) => {
                        running["state"] = json!("succeeded");
                        running["result"] = result;
                    }
                    Err(problem) => {
                        running["state"] = json!("failed");
                        running["error"] = problem;
                    }
                }
                jobs.put(running);
            })?;
        Ok(job)
    }

    /// Starts a `doctor` job created by `who`: `base` always, plus `rt`
    /// for `rt_length` when asked, plus the `skipped` checks as they are.
    /// Returns the job as created; it runs on a thread of its own and is
    /// updated when it ends.
    pub fn start_doctor(
        self: &Arc<Self>,
        who: &str,
        rt_length: Option<Duration>,
        skipped: Vec<Value>,
        inputs: Inputs,
    ) -> std::io::Result<Value> {
        let id = joker_api::id::ulid(u128::try_from(utc_now_ns()).unwrap_or(0));
        let created = timestamp(utc_now_ns());
        let mut job = Map::new();
        job.insert("id".into(), json!(id));
        job.insert("kind".into(), json!("doctor"));
        job.insert("state".into(), json!("running"));
        job.insert("created_at".into(), json!(created));
        job.insert("created_by".into(), json!(who));
        job.insert("started_at".into(), json!(created));
        let job = Value::Object(job);
        self.put(job.clone());
        let jobs = Arc::clone(self);
        let mut running = job.clone();
        std::thread::Builder::new()
            .name("svc/doctor".into())
            .spawn(move || {
                let mut checks = base(&inputs);
                if let Some(length) = rt_length {
                    checks.extend(rt(&inputs, length));
                }
                checks.extend(skipped);
                let finished = timestamp(utc_now_ns());
                let report = json!({
                    "id": running["id"],
                    "started_at": running["started_at"],
                    "finished_at": finished,
                    "summary": summary(&checks),
                    "checks": checks,
                });
                running["state"] = json!("succeeded");
                running["finished_at"] = json!(finished);
                running["result"] = report.clone();
                jobs.put(running);
                *jobs
                    .last_report
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(report);
            })?;
        Ok(job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists_and_capabilities_parse() {
        assert_eq!(cpu_list("2-3,5\n"), [2, 3, 5]);
        assert_eq!(cpu_list(""), Vec::<usize>::new());
        assert!(has_capability("CapEff:\t0000000000800000\n", 23));
        assert!(!has_capability("CapEff:\t0000000000004000\n", 23));
    }

    #[test]
    fn the_summary_is_the_worst_status() {
        let c = |s: &str| check("x", s, "");
        assert_eq!(summary(&[c("ok"), c("skip")]), "ok");
        assert_eq!(summary(&[c("ok"), c("warn")]), "warn");
        assert_eq!(summary(&[c("warn"), c("fail")]), "fail");
    }
}
