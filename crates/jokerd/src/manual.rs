//! Manual writes through the API (docs/16 A10, A13; ref/09 §5.6).
//!
//! The core arbitrates by level; this module adds what only the API knows:
//! who holds each manual level of each output, until when and why (A13), and
//! the tickets that carry a write's result back to the caller.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use joker_core::commit::WriteResult;

use crate::shared::Shared;

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

/// The manual levels held, by (output path, level).
#[derive(Debug, Default)]
pub struct ManualTable {
    levels: Mutex<BTreeMap<(String, u8), Occupant>>,
}

impl ManualTable {
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
    pub fn set(&self, output: &str, level: u8, occupant: Occupant) {
        self.lock().insert((output.to_owned(), level), occupant);
    }

    /// Frees a level.
    pub fn remove(&self, output: &str, level: u8) -> Option<Occupant> {
        self.lock().remove(&(output.to_owned(), level))
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
    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<(String, u8), Occupant>> {
        self.levels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
