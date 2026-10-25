//! Manual writes through the API (docs/16 A10, A13; ref/09 §5.6).
//!
//! The core arbitrates by level; this module adds what only the API knows:
//! who holds each manual level of each output, until when and why (A13), and
//! the tickets that carry a write's result back to the caller.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use joker_core::class::MethodResult;
use joker_core::commit::WriteResult;
use serde::{Deserialize, Serialize};

use crate::shared::{ResetOutcome, Shared};
use crate::state_file;

/// The name of the persisted manual-levels file, `state/manual` under
/// the state directory (docs/ref/02 §2.2.4, ADR 0061).
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

/// One manual level found by [`ManualTable::load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredLevel {
    /// The output.
    pub output: String,
    /// The manual level.
    pub level: u8,
    /// Who held it.
    pub principal: String,
    /// The time left on it at startup, ns (0 for an expired one).
    pub remaining_ns: u64,
}

/// What [`ManualTable::load`] restored, for the caller to log in the
/// decision log (A14: a level restored without a trustworthy clock is
/// `manual_restored`, a stand-in for the alarm until M9/AUR-288; one
/// whose deadline had passed is `manual_expired`, ADR 0061).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    /// Levels restored normally, using the recorded UTC deadline.
    pub synced: Vec<RestoredLevel>,
    /// Levels restored from `remaining_ns_at_write` because the clock
    /// was not `Synced` at boot.
    pub degraded: Vec<RestoredLevel>,
    /// Levels discarded because their deadline had passed.
    pub expired: Vec<RestoredLevel>,
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
    /// missing file loads as empty (first boot); a file that cannot be
    /// read or parsed is an error (ADR 0061).
    pub fn load(
        state_dir: PathBuf,
        now_utc_ns: i64,
        now_mono_ns: u64,
        clock_synced: bool,
    ) -> std::io::Result<(Self, Restored)> {
        let records: Vec<Persisted> = match state_file::read(&state_dir, STATE_FILE)? {
            None => Vec::new(),
            Some(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| state_file::corrupt(&state_dir, STATE_FILE, e))?,
        };
        let mut levels = BTreeMap::new();
        let mut restored = Restored::default();
        for r in records {
            let remaining_ns = if clock_synced {
                u64::try_from(r.until_utc_ns.saturating_sub(now_utc_ns)).unwrap_or(0)
            } else {
                r.remaining_ns_at_write
            };
            let found = RestoredLevel {
                output: r.output.clone(),
                level: r.level,
                principal: r.principal.clone(),
                remaining_ns,
            };
            if remaining_ns == 0 {
                restored.expired.push(found);
                continue;
            }
            if clock_synced {
                restored.synced.push(found);
            } else {
                restored.degraded.push(found);
            }
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
        Ok((
            Self {
                levels: Mutex::new(levels),
                state_dir: Some(state_dir),
            },
            restored,
        ))
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

    /// Sets the holder of a level, and persists the table.
    pub fn set(
        &self,
        output: &str,
        level: u8,
        occupant: Occupant,
        now_mono_ns: u64,
    ) -> std::io::Result<()> {
        self.lock().insert((output.to_owned(), level), occupant);
        self.save(now_mono_ns)
    }

    /// Frees a level, and persists the table.
    pub fn remove(
        &self,
        output: &str,
        level: u8,
        now_mono_ns: u64,
    ) -> std::io::Result<Option<Occupant>> {
        let r = self.lock().remove(&(output.to_owned(), level));
        self.save(now_mono_ns)?;
        Ok(r)
    }

    /// Every level held, in (output, level) order, without expired ones.
    pub fn all(&self, now_mono_ns: u64) -> Vec<(String, u8, Occupant)> {
        let mut m = self.lock();
        m.retain(|_, o| o.until_mono_ns > now_mono_ns);
        m.iter()
            .map(|((o, l), v)| (o.clone(), *l, v.clone()))
            .collect()
    }

    /// Forgets everything (a new generation starts without manual
    /// levels), and persists the table.
    pub fn clear(&self, now_mono_ns: u64) -> std::io::Result<()> {
        self.lock().clear();
        self.save(now_mono_ns)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<(String, u8), Occupant>> {
        self.levels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Rewrites `state_dir` (if any) with the current table.
    fn save(&self, now_mono_ns: u64) -> std::io::Result<()> {
        let Some(state_dir) = &self.state_dir else {
            return Ok(());
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
        let text = serde_json::to_vec_pretty(&records).map_err(std::io::Error::other)?;
        state_file::write_atomic(state_dir, STATE_FILE, &text)
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
    results: Mutex<HashMap<u32, Vec<WriteResult>>>,
    method_results: Mutex<HashMap<u32, MethodResult>>,
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
                    results.entry(r.proposal.ticket).or_default().push(r);
                }
                if let Some(list) = results.get_mut(&n)
                    && !list.is_empty()
                {
                    let r = list.remove(0);
                    if list.is_empty() {
                        results.remove(&n);
                    }
                    return Some(r);
                }
            }
            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Waits for what the method record with ticket `n` expanded to,
    /// draining the generation's method-result queue meanwhile; `None`
    /// after `timeout` (ADR 0084).
    pub fn wait_method(&self, shared: &Shared, n: u32, timeout: Duration) -> Option<MethodResult> {
        let start = Instant::now();
        loop {
            {
                let mut results = self
                    .method_results
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                while let Some(r) = shared.method_results.pop() {
                    results.insert(r.call.ticket, r);
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

    fn level(remaining_ns: u64) -> RestoredLevel {
        RestoredLevel {
            output: "pi.act_led".into(),
            level: 4,
            principal: "user:x".into(),
            remaining_ns,
        }
    }

    #[test]
    fn a_level_survives_a_save_load_round_trip_with_a_synced_clock() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = ManualTable::load(dir.path().to_owned(), 0, 0, true).unwrap();
        table
            .set("pi.act_led", 4, occupant(10_000_000_000), 0)
            .unwrap();
        assert!(dir.path().join("state").join(STATE_FILE).exists());

        let (loaded, restored) =
            ManualTable::load(dir.path().to_owned(), 5_000_000_000, 0, true).unwrap();
        let occ = loaded.get("pi.act_led", 4, 0).expect("restored");
        assert_eq!(occ.principal, "user:x");
        assert_eq!(occ.until_mono_ns, 6_000_000_000);
        assert_eq!(restored.synced, vec![level(6_000_000_000)]);
        assert!(restored.degraded.is_empty());
    }

    #[test]
    fn an_expired_level_is_dropped_with_a_synced_clock() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = ManualTable::load(dir.path().to_owned(), 0, 0, true).unwrap();
        table
            .set("pi.act_led", 4, occupant(10_000_000_000), 0)
            .unwrap();

        let (loaded, restored) =
            ManualTable::load(dir.path().to_owned(), 20_000_000_000, 0, true).unwrap();
        assert_eq!(loaded.get("pi.act_led", 4, 0), None);
        assert!(restored.synced.is_empty());
        assert_eq!(restored.expired, vec![level(0)]);
    }

    #[test]
    fn a_level_restores_from_remaining_ttl_without_a_synced_clock() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = ManualTable::load(dir.path().to_owned(), 0, 0, true).unwrap();
        table
            .set("pi.act_led", 4, occupant(10_000_000_000), 3_000_000_000)
            .unwrap();

        let (loaded, restored) =
            ManualTable::load(dir.path().to_owned(), 0, 100_000_000_000, false).unwrap();
        let occ = loaded
            .get("pi.act_led", 4, 100_000_000_000)
            .expect("restored");
        assert_eq!(occ.until_mono_ns, 107_000_000_000);
        assert_eq!(restored.degraded, vec![level(7_000_000_000)]);
        assert!(restored.synced.is_empty());
    }

    #[test]
    fn a_corrupt_manual_file_is_an_error_not_an_empty_table() {
        let dir = tempfile::tempdir().unwrap();
        state_file::write_atomic(dir.path(), STATE_FILE, b"not json").unwrap();
        let e = ManualTable::load(dir.path().to_owned(), 0, 0, true).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("state/manual"), "{e}");
    }
}
