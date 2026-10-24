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

/// The name of the persisted trips file, `state/trips` under the state
/// directory (docs/ref/02 §2.2.4).
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
    /// `state_dir`. A missing file loads as empty (first boot); a file
    /// that cannot be read or parsed is an error (ADR 0061). Returns the
    /// restored output paths, for the caller to re-inject into the
    /// arbiter before the first tick.
    pub fn load(state_dir: PathBuf) -> std::io::Result<(Self, Vec<String>)> {
        let records: Vec<Persisted> = match state_file::read(&state_dir, STATE_FILE)? {
            None => Vec::new(),
            Some(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| state_file::corrupt(&state_dir, STATE_FILE, e))?,
        };
        let tripped: BTreeMap<String, i64> = records
            .into_iter()
            .map(|r| (r.output, r.since_utc_ns))
            .collect();
        let paths = tripped.keys().cloned().collect();
        Ok((
            Self {
                tripped: Mutex::new(tripped),
                state_dir: Some(state_dir),
            },
            paths,
        ))
    }

    /// Records `output` as tripped since `since_utc_ns`; a no-op (no
    /// write) if it was already recorded.
    pub fn set_tripped(&self, output: &str, since_utc_ns: i64) -> std::io::Result<()> {
        let mut t = self.lock();
        if t.contains_key(output) {
            return Ok(());
        }
        t.insert(output.to_owned(), since_utc_ns);
        drop(t);
        self.save()
    }

    /// Forgets `output`; a no-op (no write) if it was not recorded.
    pub fn clear_tripped(&self, output: &str) -> std::io::Result<()> {
        let mut t = self.lock();
        if t.remove(output).is_none() {
            return Ok(());
        }
        drop(t);
        self.save()
    }

    /// When `output` was recorded as tripped, UTC ns.
    pub fn since(&self, output: &str) -> Option<i64> {
        self.lock().get(output).copied()
    }

    /// Every output recorded as tripped, in path order.
    pub fn paths(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
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

    fn save(&self) -> std::io::Result<()> {
        let Some(state_dir) = &self.state_dir else {
            return Ok(());
        };
        let records: Vec<Persisted> = self
            .lock()
            .iter()
            .map(|(output, since_utc_ns)| Persisted {
                output: output.clone(),
                since_utc_ns: *since_utc_ns,
            })
            .collect();
        let text = serde_json::to_vec_pretty(&records).map_err(std::io::Error::other)?;
        state_file::write_atomic(state_dir, STATE_FILE, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trip_survives_a_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = TripTable::load(dir.path().to_owned()).unwrap();
        table.set_tripped("pi.p_set", 1_000_000_000).unwrap();
        assert!(dir.path().join("state").join(STATE_FILE).exists());

        let (loaded, restored) = TripTable::load(dir.path().to_owned()).unwrap();
        assert!(loaded.is_tripped("pi.p_set"));
        assert_eq!(loaded.since("pi.p_set"), Some(1_000_000_000));
        assert_eq!(restored, vec!["pi.p_set".to_owned()]);
    }

    #[test]
    fn clearing_an_untripped_output_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = TripTable::load(dir.path().to_owned()).unwrap();
        table.clear_tripped("pi.p_set").unwrap();
        assert!(!dir.path().join("state").join(STATE_FILE).exists());
    }

    #[test]
    fn a_cleared_trip_does_not_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let (table, _) = TripTable::load(dir.path().to_owned()).unwrap();
        table.set_tripped("pi.p_set", 1_000_000_000).unwrap();
        table.clear_tripped("pi.p_set").unwrap();

        let (loaded, restored) = TripTable::load(dir.path().to_owned()).unwrap();
        assert!(!loaded.is_tripped("pi.p_set"));
        assert!(restored.is_empty());
    }

    #[test]
    fn a_corrupt_trips_file_is_an_error_not_an_empty_table() {
        let dir = tempfile::tempdir().unwrap();
        state_file::write_atomic(dir.path(), STATE_FILE, b"[{\"output\": ").unwrap();
        let e = TripTable::load(dir.path().to_owned()).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("state/trips"), "{e}");
    }
}
