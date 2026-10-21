//! The continuous trace ring: the last minutes of every tick of every group,
//! in memory, from which `jk record` and `jk trace-dump` cut `.jktrace`
//! files (docs/16 §7, ref/09 §5.16).
//!
//! The cycle thread encodes each tick into a pooled buffer and queues it;
//! this ring takes the buffers, keeps them for the retention window, and
//! hands old ones back to the pool, so the cycle thread never allocates.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use joker_core::trace::{
    FLAG_STATE, FLAG_UNCOMPRESSED, Header, SEGMENT_HEADER_LEN, compress_segment,
};

use crate::plan::Plan;
use crate::shared::{Shared, TraceChunk, trace_buffer_len};

/// The default retention of the continuous trace (`trace.continuous`).
pub const DEFAULT_RETENTION: Duration = Duration::from_secs(600);
/// The most ticks in one stored segment.
pub const SEGMENT_TICKS: usize = 1024;

/// The ring of one generation.
#[derive(Debug)]
pub struct TraceRing {
    retention_ns: u64,
    groups: Vec<VecDeque<TraceChunk>>,
}

/// What to cut from the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    /// First monotonic instant, inclusive.
    pub from_mono_ns: u64,
    /// Last monotonic instant, inclusive.
    pub to_mono_ns: u64,
}

impl TraceRing {
    /// An empty ring for the groups of `plan`.
    pub fn new(plan: &Plan, retention: Duration) -> Self {
        Self {
            retention_ns: u64::try_from(retention.as_nanos()).unwrap_or(u64::MAX),
            groups: plan.groups.iter().map(|_| VecDeque::new()).collect(),
        }
    }

    /// Takes every queued chunk, evicts what is older than the retention,
    /// and keeps the cycle thread's pool full.
    pub fn absorb(&mut self, shared: &Shared, buffer_len: usize) {
        while let Some(chunk) = shared.trace_filled.pop() {
            let newest = chunk.at_mono_ns;
            let Some(ring) = self.groups.get_mut(chunk.group as usize) else {
                let _ = shared.trace_free.push(chunk.segment);
                continue;
            };
            ring.push_back(chunk);
            while ring
                .front()
                .is_some_and(|c| newest.saturating_sub(c.at_mono_ns) > self.retention_ns)
            {
                if let Some(old) = ring.pop_front() {
                    let _ = shared.trace_free.push(old.segment);
                }
            }
        }
        while !shared.trace_free.is_full() {
            if shared
                .trace_free
                .push(Vec::with_capacity(buffer_len))
                .is_err()
            {
                break;
            }
        }
    }

    /// Ticks held, over all groups.
    pub fn len(&self) -> usize {
        self.groups.iter().map(VecDeque::len).sum()
    }

    /// True when no tick is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The oldest and newest instants held.
    pub fn bounds(&self) -> Option<(u64, u64)> {
        let first = self
            .groups
            .iter()
            .filter_map(|g| g.front())
            .map(|c| c.at_mono_ns)
            .min()?;
        let last = self
            .groups
            .iter()
            .filter_map(|g| g.back())
            .map(|c| c.at_mono_ns)
            .max()?;
        Some((first, last))
    }

    /// Cuts a `.jktrace` file (docs/16 §7) with the ticks in `cut`, zstd
    /// compressed, with an optional state segment already encoded; a group
    /// with an entry in `first` starts at that tick index.
    pub fn export(
        &self,
        header: Header,
        cut: Cut,
        state: Option<&[u8]>,
        first: &std::collections::BTreeMap<u32, u64>,
    ) -> Result<Vec<u8>, String> {
        let mut header = header;
        header.flags &= !FLAG_UNCOMPRESSED;
        if state.is_some() {
            header.flags |= FLAG_STATE;
        } else {
            header.flags &= !FLAG_STATE;
        }
        let mut out = header.encode().to_vec();
        if let Some(state) = state {
            out.extend_from_slice(state);
        }
        for (g, ring) in self.groups.iter().enumerate() {
            let ticks: Vec<&TraceChunk> = ring
                .iter()
                .filter(|c| c.at_mono_ns >= cut.from_mono_ns && c.at_mono_ns <= cut.to_mono_ns)
                .filter(|c| first.get(&(g as u32)).is_none_or(|f| c.index >= *f))
                .collect();
            for batch in ticks.chunks(SEGMENT_TICKS) {
                let mut payload = Vec::new();
                for c in batch {
                    payload.extend_from_slice(c.segment.get(SEGMENT_HEADER_LEN..).unwrap_or(&[]));
                }
                let segment = compress_segment(g as u32, batch.len() as u32, &payload)
                    .map_err(|e| format!("{e:?}"))?;
                out.extend_from_slice(&segment);
            }
        }
        Ok(out)
    }
}

/// A shared handle on a ring.
pub type SharedRing = Arc<Mutex<TraceRing>>;

/// Locks the ring, recovering from a poisoned lock.
pub fn lock(ring: &SharedRing) -> MutexGuard<'_, TraceRing> {
    ring.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The thread that feeds a ring.
#[derive(Debug)]
pub struct RingFeeder {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl RingFeeder {
    /// Starts moving chunks from `shared` into `ring`.
    pub fn spawn(plan: &Plan, shared: Arc<Shared>, ring: SharedRing) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let len = trace_buffer_len(plan);
        let handle = std::thread::Builder::new()
            .name("svc/trace".into())
            .spawn(move || {
                loop {
                    let stopping = flag.load(Ordering::Acquire);
                    lock(&ring).absorb(&shared, len);
                    if stopping {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .ok();
        Self { stop, handle }
    }

    /// Absorbs what is left and stops.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// The 32 bytes of a `sha256:<hex>` digest, or zeros.
pub fn digest_bytes(digest: &str) -> [u8; 32] {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = hex
            .get(2 * i..2 * i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(0);
    }
    out
}
