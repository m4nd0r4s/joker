//! Manual writes through the API (docs/16 A10, A13; ref/09 §5.6).
//!
//! The core arbitrates by level; this module adds what only the API knows:
//! who holds each manual level of each output, until when and why (A13), and
//! the tickets that carry a write's result back to the caller.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use joker_core::commit::WriteResult;
use serde::{Deserialize, Serialize};

use crate::shared::{ResetOutcome, Shared};
use crate::state_file;

/// The name of the persisted manual-levels file under the state
/// directory (docs/ref/02 §2.2.4).
pub const STATE_FILE: &str = "manual";

/// Who holds a manual level of an output.
#[derive(Debug, Clone, PartialEq)]
pub struct Occupant {
    /// The principal (`user:<name>`).
    pub principal: String,
    /// The value, as written (JSON).
    pub value: serde_json::Value,
    /// The TTL in ns.
    pub ttl_ns: u64,
    /// When it was written, UTC ns.
    pub since_utc_ns: i64,
    /// When it expires, UTC ns.
    pub until_utc_ns: i64,
    /// When it expires, on the daemon's monotonic clock.
    pub until_mono_ns: u64,
    /// Why.
    pub reason: String,
}

/// An [`Occupant`] as written to `state/manual` (A14): everything but
/// `until_mono_ns`, which is meaningless across a restart.
/// `remaining_ns_at_write` is the time left on the lease, computed from
/// the monotonic clock at the moment this record was (re)written; it is
/// the fallback used when the clock is not `Synced` at boot, since
/// `until_utc_ns` cannot be trusted then (A14).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Persisted {
    output: String,
    level: u8,
    principal: String,
    value: serde_json::Value,
    ttl_ns: u64,
    since_utc_ns: i64,
    until_utc_ns: i64,
    remaining_ns_at_write: u64,
    reason: String,
}

/// What [`ManualTable::load`] restored, for the caller to log (A14: a
/// level restored without a trustworthy clock is a
/// `manual_restored`-worthy event — logged today, an alarm once M9/
/// AUR-288 exists).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    /// Levels restored normally, using the recorded UTC deadline.
    pub synced: Vec<(String, u8)>,
    /// Levels restored from `remaining_ns_at_write` because the clock
    /// was not `Synced` at boot.
    pub degraded: Vec<(String, u8)>,
}

/// The manual levels held, by (output path, level); optionally backed
/// by a file so a level survives a `jokerd` restart within its TTL
/// (docs/16 A14).
#[derive(Debug, Default)]
pub struct ManualTable {
    levels: Mutex<BTreeMap<(String, u8), Occupant>>,
    state_dir: Option<PathBuf>,
}

impl ManualTable {
    /// Loads `state_dir`'s persisted manual levels, dropping records
    /// whose TTL has already passed, and recomputing `until_mono_ns` for
    /// the rest. When `clock_synced`, the real time left comes from
    /// `until_utc_ns - now_utc_ns`; otherwise, from each record's own
    /// `remaining_ns_at_write`, counted from `now_mono_ns` (A14's
    /// documented fallback). Further mutations persist to `state_dir`. A
    /// missing or unreadable file loads as empty (first boot).
    pub fn load(
        state_dir: PathBuf,
        now_utc_ns: i64,
        now_mono_ns: u64,
        clock_synced: bool,
    ) -> (Self, Restored) {
        let records: Vec<Persisted> = state_file::read(&state_dir, STATE_FILE)
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut levels = BTreeMap::new();
        let mut restored = Restored::default();
        for r in records {
            let remaining_ns = if clock_synced {
                if r.until_utc_ns <= now_utc_ns {
                    continue;
                }
                restored.synced.push((r.output.clone(), r.level));
                u64::try_from(r.until_utc_ns - now_utc_ns).unwrap_or(0)
            } else {
                if r.remaining_ns_at_write == 0 {
                    continue;
                }
                restored.degraded.push((r.output.clone(), r.level));
                r.remaining_ns_at_write
            };
            levels.insert(
                (r.output, r.level),
                Occupant {
                    principal: r.principal,
                    value: r.value,
                    ttl_ns: r.ttl_ns,
                    since_utc_ns: r.since_utc_ns,
                    until_utc_ns: r.until_utc_ns,
                    until_mono_ns: now_mono_ns.saturating_add(remaining_ns),
                    reason: r.reason,
                },
            );
        }
        (
            Self {
                levels: Mutex::new(levels),
                state_dir: Some(state_dir),
            },
            restored,
        )
    }

    /// The holder of a level, dropping it if its TTL has passed.
    pub fn get(&self, output: &str, level: u8, now_mono_ns: u64) -> Option<Occupant> {
        let mut m = self.lock();
        let key = (output.to_owned(), level);
        if m.get(&key).is_some_and(|o| o.until_mono_ns <= now_mono_ns) {
            m.remove(&key);
        }
        m.get(&key).cloned()
    }

    /// Sets the holder of a level.
    pub fn set(&self, output: &str, level: u8, occupant: Occupant, now_mono_ns: u64) {
        self.lock().insert((output.to_owned(), level), occupant);
        self.save(now_mono_ns);
    }

    /// Frees a level.
    pub fn remove(&self, output: &str, level: u8, now_mono_ns: u64) -> Option<Occupant> {
        let r = self.lock().remove(&(output.to_owned(), level));
        self.save(now_mono_ns);
        r
    }

    /// Every level held, in (output, level) order, without expired ones.
    pub fn all(&self, now_mono_ns: u64) -> Vec<(String, u8, Occupant)> {
        let mut m = self.lock();
        m.retain(|_, o| o.until_mono_ns > now_mono_ns);
        m.iter()
            .map(|((o, l), v)| (o.clone(), *l, v.clone()))
            .collect()
    }

    /// Forgets everything (a new generation starts without manual levels).
    pub fn clear(&self, now_mono_ns: u64) {
        self.lock().clear();
        self.save(now_mono_ns);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<(String, u8), Occupant>> {
        self.levels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Rewrites `state_dir` (if any) with the current table, best-effort:
    /// a write failure is not fatal to the request that triggered it.
    fn save(&self, now_mono_ns: u64) {
        let Some(state_dir) = &self.state_dir else {
            return;
        };
        let records: Vec<Persisted> = self
            .lock()
            .iter()
            .map(|((output, level), o)| Persisted {
                output: output.clone(),
                level: *level,
                principal: o.principal.clone(),
                value: o.value.clone(),
                ttl_ns: o.ttl_ns,
                since_utc_ns: o.since_utc_ns,
                until_utc_ns: o.until_utc_ns,
                remaining_ns_at_write: o.until_mono_ns.saturating_sub(now_mono_ns),
                reason: o.reason.clone(),
            })
            .collect();
        if let Ok(text) = serde_json::to_vec_pretty(&records) {
            let _ = state_file::write_atomic(state_dir, STATE_FILE, &text);
        }
    }
}

/// A write ticket (ref/09 §5.6), kept 10 minutes.
#[derive(Debug, Clone, PartialEq)]
pub struct Ticket {
    /// ULID.
    pub id: String,
    /// The JSON `WriteTicket` as last reported.
    pub body: serde_json::Value,
    /// When it was created.
    pub created: Instant,
}

/// How long tickets are kept.
pub const TICKET_RETENTION: Duration = Duration::from_secs(600);

/// The results of proposals with tickets, collected from the cycle thread.
#[derive(Debug, Default)]
pub struct TicketBook {
    next: Mutex<u32>,
    results: Mutex<HashMap<u32, WriteResult>>,
    reset_results: Mutex<HashMap<u32, ResetOutcome>>,
    tickets: Mutex<BTreeMap<String, Ticket>>,
}

impl TicketBook {
    /// A fresh ticket number for a proposal (never 0).
    pub fn number(&self) -> u32 {
        let mut n = self
            .next
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *n = n.wrapping_add(1).max(1);
        *n
    }

    /// Waits for the result of ticket `n`, draining the generation's result
    /// queue meanwhile; `None` after `timeout`.
    pub fn wait(&self, shared: &Shared, n: u32, timeout: Duration) -> Option<WriteResult> {
        let start = Instant::now();
        loop {
            {
                let mut results = self
                    .results
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                while let Some(r) = shared.results.pop() {
                    results.insert(r.proposal.ticket, r);
                }
                if let Some(r) = results.remove(&n) {
                    return Some(r);
                }
            }
            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Waits for the resolution of reset ticket `n` (`jk reset`, docs/16
    /// E7), draining the generation's reset-result queue meanwhile;
    /// `None` after `timeout`.
    pub fn wait_reset(&self, shared: &Shared, n: u32, timeout: Duration) -> Option<ResetOutcome> {
        let start = Instant::now();
        loop {
            {
                let mut results = self
                    .reset_results
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                while let Some(r) = shared.reset_results.pop() {
                    results.insert(r.ticket, r);
                }
                if let Some(r) = results.remove(&n) {
                    return Some(r);
                }
            }
            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Stores a ticket.
    pub fn put(&self, id: &str, body: serde_json::Value) {
        let mut t = self
            .tickets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        t.retain(|_, v| v.created.elapsed() < TICKET_RETENTION);
        t.insert(
            id.to_owned(),
            Ticket {
                id: id.to_owned(),
                body,
                created: Instant::now(),
            },
        );
    }

    /// A stored ticket.
    pub fn get(&self, id: &str) -> Option<Ticket> {
        let t = self
            .tickets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        t.get(id)
            .filter(|v| v.created.elapsed() < TICKET_RETENTION)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occupant(until_mono_ns: u64) -> Occupant {
        Occupant {
            principal: "user:x".into(),
            value: serde_json::json!(true),
            ttl_ns: 10_000_000_000,
            since_utc_ns: 1_000_000_000,
            until_utc_ns: 11_000_000_000,
            until_mono_ns,
            reason: "test".into(),
        }
    }

    #[test]
    fn a_level_survives_a_save_load_round_trip_with_a_synced_clock() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = ManualTable::load(dir.path().to_owned(), 0, 0, true);
        table.set("pi.act_led", 4, occupant(10_000_000_000), 0);

        let (loaded, restored) = ManualTable::load(dir.path().to_owned(), 5_000_000_000, 0, true);
        let occ = loaded.get("pi.act_led", 4, 0).expect("restored");
        assert_eq!(occ.principal, "user:x");
        // 11s (until_utc_ns) - 5s (now_utc_ns) = 6s left, from a mono now of 0.
        assert_eq!(occ.until_mono_ns, 6_000_000_000);
        assert_eq!(restored.synced, vec![("pi.act_led".to_owned(), 4)]);
        assert!(restored.degraded.is_empty());
    }

    #[test]
    fn an_expired_level_is_dropped_with_a_synced_clock() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = ManualTable::load(dir.path().to_owned(), 0, 0, true);
        table.set("pi.act_led", 4, occupant(10_000_000_000), 0);

        // now_utc_ns (20s) is past until_utc_ns (11s): expired.
        let (loaded, restored) = ManualTable::load(dir.path().to_owned(), 20_000_000_000, 0, true);
        assert_eq!(loaded.get("pi.act_led", 4, 0), None);
        assert!(restored.synced.is_empty());
    }

    #[test]
    fn a_level_restores_from_remaining_ttl_without_a_synced_clock() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = ManualTable::load(dir.path().to_owned(), 0, 0, true);
        // 7s left at the moment of this save (now_mono_ns = 3s, until_mono_ns = 10s).
        table.set("pi.act_led", 4, occupant(10_000_000_000), 3_000_000_000);

        // A restart with an unsynced clock: now_utc_ns is meaningless (0,
        // as if the clock reset), but the degraded path must still
        // restore using the recorded remaining TTL, from a fresh
        // now_mono_ns of 100s.
        let (loaded, restored) =
            ManualTable::load(dir.path().to_owned(), 0, 100_000_000_000, false);
        let occ = loaded
            .get("pi.act_led", 4, 100_000_000_000)
            .expect("restored");
        assert_eq!(occ.until_mono_ns, 107_000_000_000);
        assert_eq!(restored.degraded, vec![("pi.act_led".to_owned(), 4)]);
        assert!(restored.synced.is_empty());
    }
}
