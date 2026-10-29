//! `svc/metrics`: the runtime's own `joker.*` signals (docs/01 §19,
//! ADR 0090). Once per second it reads the counters the daemon already
//! keeps and writes one sample per runtime signal; it is the single writer
//! of those slots. A value the node cannot read is `bad(not_connected)`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use joker_core::output::WriterKind;
use joker_core::{Quality, QualityCode, QualityReason, SignalId};

use crate::pacer::Monotonic;
use crate::plan::Plan;
use crate::shared::Shared;

/// How often `svc/metrics` writes the runtime signals (ADR 0090).
pub const PERIOD: Duration = Duration::from_secs(1);

/// What `svc/metrics` reads besides the generation's shared state.
#[derive(Debug, Clone)]
pub struct Sources {
    /// The manual levels, for `joker.arbiter.manual_active`.
    pub manual: Option<Arc<crate::manual::ManualTable>>,
    /// The daemon's log ring, whose drops count in `joker.runtime.dropped`.
    pub logs: Option<Arc<crate::logs::LogRing>>,
    /// The state directory: the disk of `joker.system.disk_free_bytes` and
    /// `state/boot_count`.
    pub state_dir: Option<PathBuf>,
    /// The daemon's clock; its zero is the daemon's start.
    pub clock: Monotonic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stat {
    P99,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Result {
    Accepted,
    Clamped,
    Rejected,
    Suppressed,
    Tripped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Missing,
    Ticks(usize),
    Overruns(usize),
    Jitter(usize, Stat),
    Duration(usize, Stat),
    Link(usize),
    Reconnects(usize),
    DroppedSamples(usize),
    InvalidSamples(usize),
    Envelope(SignalId, Result),
    ManualActive,
    LeasesExpiring,
    Temp,
    Throttled,
    MemoryUsed,
    DiskFree,
    BootCount,
    Uptime,
    Dropped,
}

fn source(plan: &Plan, path: &str) -> Option<Source> {
    let rest = path.strip_prefix("joker.")?;
    let group = |g: &str| plan.groups.iter().position(|p| p.name == g);
    let device = |d: &str| plan.devices.iter().position(|p| p.name == d);
    let found = match rest {
        "arbiter.manual_active" => Some(Source::ManualActive),
        "arbiter.leases_expiring" => Some(Source::LeasesExpiring),
        "system.temp" => Some(Source::Temp),
        "system.throttled" => Some(Source::Throttled),
        "system.memory_used_bytes" => Some(Source::MemoryUsed),
        "system.disk_free_bytes" => Some(Source::DiskFree),
        "node.boot_count" => Some(Source::BootCount),
        "node.uptime" => Some(Source::Uptime),
        "runtime.dropped" => Some(Source::Dropped),
        _ => {
            let (head, name) = rest.rsplit_once('.')?;
            if let Some(g) = head.strip_prefix("group.") {
                group(g).and_then(|g| match name {
                    "ticks" => Some(Source::Ticks(g)),
                    "overruns" => Some(Source::Overruns(g)),
                    "jitter_p99" => Some(Source::Jitter(g, Stat::P99)),
                    "jitter_max" => Some(Source::Jitter(g, Stat::Max)),
                    "duration_p99" => Some(Source::Duration(g, Stat::P99)),
                    "duration_max" => Some(Source::Duration(g, Stat::Max)),
                    _ => None,
                })
            } else if let Some(d) = head.strip_prefix("device.") {
                device(d).and_then(|d| match name {
                    "link" => Some(Source::Link(d)),
                    "reconnects" => Some(Source::Reconnects(d)),
                    "dropped_samples" => Some(Source::DroppedSamples(d)),
                    "invalid_samples" => Some(Source::InvalidSamples(d)),
                    _ => None,
                })
            } else if let Some(o) = head.strip_prefix("output.") {
                let result = match name {
                    "accepted" => Some(Result::Accepted),
                    "clamped" => Some(Result::Clamped),
                    "rejected" => Some(Result::Rejected),
                    "suppressed" => Some(Result::Suppressed),
                    "tripped" => Some(Result::Tripped),
                    _ => None,
                };
                plan.id(o)
                    .zip(result)
                    .map(|(id, r)| Source::Envelope(id, r))
            } else {
                None
            }
        }
    };
    Some(found.unwrap_or(Source::Missing))
}

/// The drops of each internal queue of the daemon, by the name
/// `Metrics.runtime.dropped` gives it (ADR 0090 point 5).
pub fn dropped(shared: &Shared, logs: Option<&crate::logs::LogRing>) -> Vec<(&'static str, u64)> {
    let mut out = vec![
        ("decisions", shared.decisions_dropped.get()),
        ("write_results", shared.results_dropped.get()),
        ("trace", shared.trace_dropped.get()),
        ("alarm_events", shared.alarm_events_dropped.get()),
        ("method_results", shared.method_results_dropped.get()),
        ("reset_results", shared.reset_results_dropped.get()),
        ("trip_requests", shared.trip_requests_dropped.get()),
        ("confirmations", shared.confirmations_dropped.get()),
        ("write_acks", shared.write_acks_dropped.get()),
    ];
    if let Some(logs) = logs {
        out.push(("logs", logs.dropped()));
    }
    out
}

/// The outputs whose active level is not manual and whose last commit got
/// no proposal: the level holds on its lease alone (ADR 0090 point 6).
pub fn leases_expiring(plan: &Plan, shared: &Shared) -> u64 {
    let manual: BTreeSet<u8> = plan
        .writers
        .iter()
        .filter(|w| w.config.kind == WriterKind::Manual)
        .map(|w| w.config.level.get())
        .collect();
    let none = joker_core::Outcome::None.code();
    shared
        .output_ids
        .iter()
        .filter_map(|id| shared.output(*id)?.read())
        .filter(|(_, meta, _)| {
            meta.level != 0 && !manual.contains(&meta.level) && meta.outcome == none
        })
        .count() as u64
}

/// Writes every runtime signal of `plan` once, at `now_mono_ns`.
pub fn write_all(plan: &Plan, shared: &Shared, sources: &Sources, now_mono_ns: u64) {
    let system = sources
        .state_dir
        .as_deref()
        .map(crate::system::sample)
        .unwrap_or_default();
    let int = |v: u64| i64::try_from(v).unwrap_or(i64::MAX) as u64;
    let secs = |ns: u64| (ns as f64 / 1e9).to_bits();
    let percentile = |h: &crate::shared::Histogram, max: u64, stat: Stat| match stat {
        Stat::Max => Some(secs(max)),
        Stat::P99 => h.quantile(0.99).map(|v| secs(v.min(max))),
    };
    for s in &plan.signals {
        let Some(source) = source(plan, &s.path) else {
            continue;
        };
        let Some(id) = plan.id(&s.path) else {
            continue;
        };
        let value = match source {
            Source::Missing => None,
            Source::Ticks(g) => shared.groups.get(g).map(|s| int(s.ticks.get())),
            Source::Overruns(g) => shared.groups.get(g).map(|s| int(s.overruns.get())),
            Source::Jitter(g, stat) => shared
                .groups
                .get(g)
                .and_then(|s| percentile(&s.jitter, s.max_late_ns.get(), stat)),
            Source::Duration(g, stat) => shared
                .groups
                .get(g)
                .and_then(|s| percentile(&s.duration, s.max_duration_ns.get(), stat)),
            Source::Link(d) => shared.devices.get(d).map(|s| s.link.get()),
            Source::Reconnects(d) => shared.devices.get(d).map(|s| int(s.reconnects.get())),
            Source::DroppedSamples(d) => {
                shared.devices.get(d).map(|s| int(s.dropped_samples.get()))
            }
            Source::InvalidSamples(d) => {
                shared.devices.get(d).map(|s| int(s.invalid_samples.get()))
            }
            Source::Envelope(o, r) => shared.output(o).map(|status| {
                let results = status.results();
                int(match r {
                    Result::Accepted => results.accepted,
                    Result::Clamped => results.clamped,
                    Result::Rejected => results.rejected,
                    Result::Suppressed => results.suppressed,
                    Result::Tripped => results.tripped,
                })
            }),
            Source::ManualActive => sources
                .manual
                .as_ref()
                .map(|m| int(m.all(now_mono_ns).len() as u64)),
            Source::LeasesExpiring => Some(int(leases_expiring(plan, shared))),
            Source::Temp => system.temp_celsius.map(f64::to_bits),
            Source::Throttled => system.throttled.map(u64::from),
            Source::MemoryUsed => system.memory_used_bytes.map(int),
            Source::DiskFree => system.disk_free_bytes.map(int),
            Source::BootCount => sources.state_dir.as_deref().and_then(|dir| {
                let bytes = crate::state_file::read(dir, "boot_count").ok()??;
                String::from_utf8(bytes)
                    .ok()?
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .map(int)
            }),
            Source::Uptime => Some(secs(sources.clock.now_ns())),
            Source::Dropped => Some(int(dropped(shared, sources.logs.as_deref())
                .iter()
                .map(|(_, n)| *n)
                .sum())),
        };
        let (bits, quality) = match value {
            Some(bits) => (bits, Quality::GOOD),
            None => (
                0,
                Quality::new(QualityCode::Bad, QualityReason::NotConnected),
            ),
        };
        if let Some(slot) = shared.slots.get(id.index()) {
            slot.write(bits, quality, joker_core::value::NO_TS_SRC, now_mono_ns);
        }
    }
}

/// The `svc/metrics` thread of one running generation.
#[derive(Debug)]
pub struct MetricsWriter {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MetricsWriter {
    /// Starts writing `plan`'s runtime signals into `shared` every
    /// [`PERIOD`].
    pub fn spawn(plan: Arc<Plan>, shared: Arc<Shared>, sources: Sources) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("svc/metrics".into())
            .stack_size(crate::shared::SERVICE_STACK)
            .spawn(move || {
                while !flag.load(Ordering::Acquire) {
                    write_all(&plan, &shared, &sources, sources.clock.now_ns());
                    let mut waited = Duration::ZERO;
                    while waited < PERIOD && !flag.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(50));
                        waited += Duration::from_millis(50);
                    }
                }
            })?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Stops the thread.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for MetricsWriter {
    fn drop(&mut self) {
        self.halt();
    }
}
