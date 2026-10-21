//! Trace files cut from the continuous ring (ref/09 §5.16, docs/16 §7):
//! `source = "ring"` cuts what the ring holds now; `source = "record"`
//! records from now on for a duration, starting with the state segment.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use joker_core::trace::{AppState, Header, SEGMENT_HEADER_LEN, STATE_MARKER, write_state_segment};

use crate::compress::compress_segment;

use crate::plan::Plan;
use crate::shared::Shared;
use crate::trace_ring::{Cut, SharedRing, digest_bytes, lock};

/// The state of a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceState {
    /// Recording.
    Recording,
    /// Ready to download.
    Ready,
    /// Stopped before its end; the file is valid.
    Stopped,
    /// Failed.
    Failed,
}

impl TraceState {
    /// The canonical name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Ready => "ready",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

/// One trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntry {
    /// ULID.
    pub id: String,
    /// `record` or `ring`.
    pub source: &'static str,
    /// State.
    pub state: TraceState,
    /// Who asked.
    pub created_by: String,
    /// When, UTC ns.
    pub created_utc_ns: i64,
    /// First instant covered, UTC ns.
    pub from_utc_ns: Option<i64>,
    /// Last instant covered, UTC ns.
    pub to_utc_ns: Option<i64>,
    /// When a recording ends, UTC ns.
    pub ends_utc_ns: Option<i64>,
    /// Size of the file.
    pub size: Option<u64>,
    /// Generation number and hash.
    pub generation: (u64, String),
    /// Registry hash.
    pub registry: String,
    /// Label.
    pub label: Option<String>,
    /// Why it failed.
    pub failure: Option<String>,
}

/// An encoded state segment and the first recorded tick of each group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSegment {
    /// The segment, as stored in the file.
    pub bytes: Vec<u8>,
    /// Group index to the index of its first recorded tick.
    pub first: BTreeMap<u32, u64>,
}

/// What a cut needs from the running generation.
#[derive(Clone)]
pub struct Source {
    /// The plan.
    pub plan: Arc<Plan>,
    /// The shared state of the cycle thread.
    pub shared: Arc<Shared>,
    /// The ring.
    pub ring: SharedRing,
    /// Generation number and hash.
    pub generation: (u64, String),
    /// UTC at monotonic zero.
    pub utc_origin_ns: i64,
    /// Clock quality code.
    pub clock: u8,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("generation", &self.generation)
            .finish()
    }
}

impl Source {
    /// The file header of a cut starting at `from_mono_ns`.
    pub fn header(&self, from_mono_ns: u64) -> Header {
        Header {
            flags: 0,
            generation_hash: digest_bytes(&self.generation.1),
            registry_hash: self.plan.registry_hash,
            epoch_mono_ns: from_mono_ns,
            epoch_utc_ns: self.utc_origin_ns.saturating_add(from_mono_ns as i64),
            clock: self.clock,
        }
    }

    /// Cuts `[from, to]` (monotonic) with an optional state segment, each
    /// group starting at its tick in `first`.
    pub fn cut(&self, from: u64, to: u64, state: Option<&StateSegment>) -> Result<Vec<u8>, String> {
        lock(&self.ring).absorb(&self.shared, crate::shared::trace_buffer_len(&self.plan));
        let empty = BTreeMap::new();
        lock(&self.ring).export(
            self.header(from),
            Cut {
                from_mono_ns: from,
                to_mono_ns: to,
            },
            state.map(|s| s.bytes.as_slice()),
            state.map_or(&empty, |s| &s.first),
        )
    }

    /// Asks the cycle thread for the state of every app with cycle tasks,
    /// waits up to `deadline` for the answers (100 ms per group, docs/16
    /// §7), and encodes the state segment. Apps that did not answer in time
    /// are `failed` (state 2) and replay from their `init`.
    pub fn state_segment(&self, deadline: Duration) -> Result<StateSegment, String> {
        let request = self.shared.request_export();
        let mut wanted: Vec<(String, u32)> = Vec::new();
        for (g, group) in self.plan.groups.iter().enumerate() {
            for t in &group.tasks {
                if !wanted.iter().any(|(a, gg)| *a == t.app && *gg == g as u32) {
                    wanted.push((t.app.clone(), g as u32));
                }
            }
        }
        let start = std::time::Instant::now();
        let mut got: Vec<crate::shared::StateExport> = Vec::new();
        while got.len() < wanted.len() && start.elapsed() < deadline {
            while let Some(e) = self.shared.exports.pop() {
                if e.request == request {
                    got.push(e);
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut first = BTreeMap::new();
        let mut apps = Vec::new();
        for (app, group) in &wanted {
            match got.iter().find(|e| e.app == *app && e.group == *group) {
                Some(e) => {
                    first.insert(e.group, e.first_seq);
                    apps.push((app.clone(), *group, e.first_seq, e.state, e.blob.clone()));
                }
                None => apps.push((app.clone(), *group, 0, 2, Vec::new())),
            }
        }
        let views: Vec<AppState<'_>> = apps
            .iter()
            .map(|(name, group, first_seq, state, blob)| AppState {
                name,
                group: *group,
                first_seq: *first_seq,
                state: *state,
                blob,
            })
            .collect();
        let size: usize =
            SEGMENT_HEADER_LEN + views.iter().map(AppState::encoded_len).sum::<usize>();
        let mut buf = vec![0u8; size];
        let raw = write_state_segment(&views, &mut buf).map_err(|e| format!("{e:?}"))?;
        let payload = raw.get(SEGMENT_HEADER_LEN..).unwrap_or(&[]);
        let bytes = compress_segment(STATE_MARKER, views.len() as u32, payload)
            .map_err(|e| format!("{e:?}"))?;
        Ok(StateSegment { bytes, first })
    }
}

/// The traces of a node, stored under `<state>/traces`.
#[derive(Debug)]
pub struct Traces {
    dir: PathBuf,
    entries: Mutex<BTreeMap<String, TraceEntry>>,
}

impl Traces {
    /// Opens the store.
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
            entries: Mutex::new(BTreeMap::new()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, TraceEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The file of a trace.
    pub fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.jktrace"))
    }

    /// Every trace, oldest first.
    pub fn list(&self) -> Vec<TraceEntry> {
        self.lock().values().cloned().collect()
    }

    /// One trace.
    pub fn get(&self, id: &str) -> Option<TraceEntry> {
        self.lock().get(id).cloned()
    }

    /// Stores or replaces an entry.
    pub fn put(&self, entry: TraceEntry) {
        self.lock().insert(entry.id.clone(), entry);
    }

    /// Writes the bytes of a trace and marks it ready.
    pub fn finish(&self, id: &str, bytes: &[u8], state: TraceState) -> std::io::Result<()> {
        std::fs::write(self.file(id), bytes)?;
        let mut m = self.lock();
        if let Some(e) = m.get_mut(id) {
            e.state = state;
            e.size = Some(bytes.len() as u64);
        }
        Ok(())
    }

    /// Marks a trace failed.
    pub fn fail(&self, id: &str, why: String) {
        if let Some(e) = self.lock().get_mut(id) {
            e.state = TraceState::Failed;
            e.failure = Some(why);
        }
    }
}

/// Records from `from_mono_ns` for `duration`, then cuts the file.
pub fn record(
    traces: Arc<Traces>,
    source: Source,
    id: String,
    from_mono_ns: u64,
    duration: Duration,
    state: Option<StateSegment>,
) {
    let _ = std::thread::Builder::new()
        .name("svc/record".into())
        .spawn(move || {
            std::thread::sleep(duration);
            let to =
                from_mono_ns.saturating_add(u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX));
            match source.cut(from_mono_ns, to, state.as_ref()) {
                Ok(bytes) => {
                    if let Err(e) = traces.finish(&id, &bytes, TraceState::Ready) {
                        traces.fail(&id, e.to_string());
                    }
                }
                Err(e) => traces.fail(&id, e),
            }
        });
}
