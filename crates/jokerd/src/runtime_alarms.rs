//! The runtime alarms (`joker.*`, ref/01 §5.15, ADR 0083): raised and
//! cleared by `jokerd` itself from what the cycle thread, the io tasks,
//! the manual table and the historian publish, with the lifecycle of
//! docs/02 §11 and without delays.
//!
//! One thread per generation (`svc/watch`) evaluates every condition every
//! [`WATCH_PERIOD`] and hands each transition to the alarm book and the
//! journal, like the alarm pump does for the manifest's alarms.

use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use joker_core::alarm::Severity;

use crate::alarms::{Journal, RuntimeCondition};
use crate::historian::Historian;
use crate::manual::ManualTable;
use crate::pacer::Monotonic;
use crate::plan::Plan;
use crate::shared::Shared;
use crate::wire::{timestamp, utc_now_ns};

/// How often the conditions are evaluated.
pub const WATCH_PERIOD: Duration = Duration::from_millis(50);
/// How long before the end of a manual TTL `manual_expiring` rises
/// (ref/01 §5.10).
pub const MANUAL_EXPIRING_NS: u64 = 15 * 60 * 1_000_000_000;

/// What the watch reads besides the generation's shared state.
#[derive(Debug, Clone)]
pub struct WatchSources {
    /// The manual levels held.
    pub manual: Option<Arc<ManualTable>>,
    /// The manual levels restored at startup without a synced clock
    /// (docs/16 A14), as (output, level).
    pub restored: Vec<(String, u8)>,
    /// The historian, for its critical disk.
    pub historian: Option<Arc<Historian>>,
}

/// Every runtime condition of `plan` now.
pub fn conditions(
    plan: &Plan,
    shared: &Shared,
    sources: &WatchSources,
    restored_since: &mut BTreeMap<(String, u8), Option<i64>>,
    now_mono_ns: u64,
) -> Vec<RuntimeCondition> {
    let mut out = Vec::new();
    let held = sources
        .manual
        .as_ref()
        .map(|m| m.all(now_mono_ns))
        .unwrap_or_default();
    for o in &plan.outputs {
        let Some(id) = plan.id(&o.path) else { continue };
        let budget = shared.output(id).is_some_and(|s| s.budget_refused());
        if o.max_actuations.is_some() || budget {
            out.push(RuntimeCondition {
                id: format!("joker.output.{}.budget", o.path),
                severity: Severity::High,
                message: format!("{}: actuation budget exhausted", o.path),
                active: budget,
            });
        }
        if o.verify.is_some() {
            out.push(RuntimeCondition {
                id: format!("joker.output.{}.unconfirmed", o.path),
                severity: Severity::High,
                message: format!(
                    "{}: the readback does not confirm the applied value",
                    o.path
                ),
                active: shared.unconfirmed(id),
            });
        }
        let ending = held
            .iter()
            .filter(|(p, _, _)| *p == o.path)
            .map(|(_, level, occ)| (*level, occ))
            .find(|(_, occ)| occ.until_mono_ns.saturating_sub(now_mono_ns) <= MANUAL_EXPIRING_NS);
        out.push(RuntimeCondition {
            id: format!("joker.output.{}.manual_expiring", o.path),
            severity: Severity::Medium,
            message: match ending {
                Some((level, occ)) => format!(
                    "{}: manual:{level} held by {} ends at {}",
                    o.path,
                    occ.principal,
                    timestamp(occ.until_utc_ns)
                ),
                None => format!("{}: no manual level is about to end", o.path),
            },
            active: ending.is_some(),
        });
    }
    for (output, level) in &sources.restored {
        let current = held
            .iter()
            .find(|(p, l, _)| p == output && l == level)
            .map(|(_, _, occ)| occ.since_utc_ns);
        let key = (output.clone(), *level);
        if current.is_some() && !restored_since.contains_key(&key) {
            restored_since.insert(key.clone(), current);
        }
        let first = restored_since.get(&key).copied().flatten();
        out.push(RuntimeCondition {
            id: format!("joker.output.{output}.manual_restored"),
            severity: Severity::Medium,
            message: format!(
                "{output}: manual:{level} restored at startup without a synced clock; confirm or release it"
            ),
            active: current.is_some() && current == first,
        });
    }
    for (g, group) in plan.groups.iter().enumerate() {
        let Some(stats) = shared.groups.get(g) else {
            continue;
        };
        let consecutive = stats.consecutive_overruns.get();
        out.push(RuntimeCondition {
            id: format!("joker.group.{}.overrun", group.name),
            severity: Severity::High,
            message: format!(
                "group {}: {consecutive} consecutive overruns (alarm_after {})",
                group.name, group.alarm_after
            ),
            active: consecutive >= u64::from(group.alarm_after),
        });
    }
    for (d, device) in plan.devices.iter().enumerate() {
        let Some(stats) = shared.devices.get(d) else {
            continue;
        };
        let link = stats.link.get();
        for (suffix, code, severity, text) in [
            ("degraded", 2, Severity::Medium, "is degraded"),
            ("offline", 3, Severity::High, "is offline"),
            ("drift", 4, Severity::High, "does not match its lock"),
        ] {
            out.push(RuntimeCondition {
                id: format!("joker.device.{}.{suffix}", device.name),
                severity,
                message: format!("device {} {text}", device.name),
                active: link == code,
            });
        }
    }
    if let Some(h) = &sources.historian {
        out.push(RuntimeCondition {
            id: "joker.store.disk_full".into(),
            severity: Severity::High,
            message: "the historian's disk is critical: only alarms and decisions are recorded"
                .into(),
            active: h.store().stats().disk_full,
        });
    }
    out
}

/// The thread that raises and clears the runtime alarms of a generation.
#[derive(Debug)]
pub struct Watch {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Watch {
    /// Starts evaluating.
    pub fn spawn(
        plan: Arc<Plan>,
        shared: Arc<Shared>,
        sources: WatchSources,
        journal: Journal,
        clock: Monotonic,
    ) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("svc/watch".into())
            .spawn(move || {
                let mut restored_since = BTreeMap::new();
                let mut reported = false;
                while !flag.load(Ordering::Acquire) {
                    let now = utc_now_ns();
                    let mut events = Vec::new();
                    for c in conditions(
                        &plan,
                        &shared,
                        &sources,
                        &mut restored_since,
                        clock.now_ns(),
                    ) {
                        if let Some(e) = journal.book.runtime(&c, now) {
                            events.push((now, e));
                        }
                    }
                    for e in journal.book.expire_runtime_shelves(now) {
                        events.push((now, e));
                    }
                    let events: Vec<(i64, serde_json::Value)> = events;
                    journal.write(&events, &mut reported);
                    std::thread::sleep(WATCH_PERIOD);
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

impl Drop for Watch {
    fn drop(&mut self) {
        self.halt();
    }
}
