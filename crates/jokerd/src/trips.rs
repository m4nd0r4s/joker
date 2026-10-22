//! Persistent trips (docs/01 §7, docs/16): an output in trip stays at
//! `safe` across a `jokerd` restart, because it is kept in `state/trips`.
//!
//! Unlike manual levels (A14), a trip has no TTL and no principal: it is
//! either tripped or not, until an explicit reset. There is also no
//! reliable "why" today (the only wired trip source is the internal
//! range evaluation, api.rs's own `trips()` doc comment) — so only the
//! output and when are recorded.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::state_file;

/// The name of the persisted trips file under the state directory
/// (docs/ref/02 §2.2.4).
pub const STATE_FILE: &str = "trips";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Persisted {
    output: String,
    since_utc_ns: i64,
}

/// The tripped outputs, by path, with when each last transitioned into
/// trip (UTC ns); optionally backed by a file so a trip survives a
/// `jokerd` restart.
#[derive(Debug, Default)]
pub struct TripTable {
    tripped: Mutex<BTreeMap<String, i64>>,
    state_dir: Option<PathBuf>,
}

impl TripTable {
    /// Loads `state_dir`'s persisted trips. Further mutations persist to
    /// `state_dir`. A missing or unreadable file loads as empty (first
    /// boot). Returns the restored output paths, for the caller to
    /// re-inject into the arbiter before the first tick.
    pub fn load(state_dir: PathBuf) -> (Self, Vec<String>) {
        let records: Vec<Persisted> = state_file::read(&state_dir, STATE_FILE)
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let tripped: BTreeMap<String, i64> = records
            .into_iter()
            .map(|r| (r.output, r.since_utc_ns))
            .collect();
        let paths = tripped.keys().cloned().collect();
        (
            Self {
                tripped: Mutex::new(tripped),
                state_dir: Some(state_dir),
            },
            paths,
        )
    }

    /// Records `output` as tripped since `since_utc_ns`; a no-op (no
    /// write) if it was already recorded.
    pub fn set_tripped(&self, output: &str, since_utc_ns: i64) {
        let mut t = self.lock();
        if t.contains_key(output) {
            return;
        }
        t.insert(output.to_owned(), since_utc_ns);
        drop(t);
        self.save();
    }

    /// Forgets `output`; a no-op (no write) if it was not recorded.
    pub fn clear_tripped(&self, output: &str) {
        let mut t = self.lock();
        if t.remove(output).is_none() {
            return;
        }
        drop(t);
        self.save();
    }

    /// Whether `output` is recorded as tripped.
    pub fn is_tripped(&self, output: &str) -> bool {
        self.lock().contains_key(output)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, i64>> {
        self.tripped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn save(&self) {
        let Some(state_dir) = &self.state_dir else {
            return;
        };
        let records: Vec<Persisted> = self
            .lock()
            .iter()
            .map(|(output, since_utc_ns)| Persisted {
                output: output.clone(),
                since_utc_ns: *since_utc_ns,
            })
            .collect();
        if let Ok(text) = serde_json::to_vec_pretty(&records) {
            let _ = state_file::write_atomic(state_dir, STATE_FILE, &text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trip_survives_a_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = TripTable::load(dir.path().to_owned());
        table.set_tripped("pi.p_set", 1_000_000_000);

        let (loaded, restored) = TripTable::load(dir.path().to_owned());
        assert!(loaded.is_tripped("pi.p_set"));
        assert_eq!(restored, vec!["pi.p_set".to_owned()]);
    }

    #[test]
    fn clearing_an_untripped_output_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = TripTable::load(dir.path().to_owned());
        table.clear_tripped("pi.p_set");
        assert!(!dir.path().join(STATE_FILE).exists());
    }

    #[test]
    fn a_cleared_trip_does_not_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = TripTable::load(dir.path().to_owned());
        table.set_tripped("pi.p_set", 1_000_000_000);
        table.clear_tripped("pi.p_set");

        let (loaded, restored) = TripTable::load(dir.path().to_owned());
        assert!(!loaded.is_tripped("pi.p_set"));
        assert!(restored.is_empty());
    }
}
