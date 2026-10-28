//! Bounded ingestion: producers never wait on the store (ADR 0013,
//! AGENTS.md invariant 7). Alarms and decisions have their own queue, so a
//! telemetry flood never drops them; a full queue drops the new record and
//! counts it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_queue::ArrayQueue;

use crate::{Event, Sample, SeriesId, Store};

/// What a record carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// A sample of a scalar series.
    Sample(Sample),
    /// An occurrence of an event series.
    Event(Event),
}

/// One record for a series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The series.
    pub series: SeriesId,
    /// The sample or event.
    pub payload: Payload,
}

/// Counters of an [`Ingest`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IngestStats {
    /// Records queued.
    pub queued: u64,
    /// Telemetry records dropped by a full queue.
    pub dropped_telemetry: u64,
    /// Alarm and decision records dropped by a full queue.
    pub dropped_priority: u64,
    /// Records waiting.
    pub pending: u64,
}

/// The queues between the producers and the writer thread.
#[derive(Debug)]
pub struct Ingest {
    priority: ArrayQueue<Record>,
    telemetry: ArrayQueue<Record>,
    queued: AtomicU64,
    dropped_telemetry: AtomicU64,
    dropped_priority: AtomicU64,
}

impl Ingest {
    /// Queues with room for `priority` alarm and decision records and
    /// `telemetry` other records.
    pub fn new(priority: usize, telemetry: usize) -> Self {
        Self {
            priority: ArrayQueue::new(priority.max(1)),
            telemetry: ArrayQueue::new(telemetry.max(1)),
            queued: AtomicU64::new(0),
            dropped_telemetry: AtomicU64::new(0),
            dropped_priority: AtomicU64::new(0),
        }
    }

    /// Queues a telemetry record; false when it was dropped.
    pub fn push(&self, record: Record) -> bool {
        match self.telemetry.push(record) {
            Ok(()) => {
                self.queued.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(_) => {
                self.dropped_telemetry.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Queues an alarm or decision record; false when it was dropped.
    pub fn push_priority(&self, record: Record) -> bool {
        match self.priority.push(record) {
            Ok(()) => {
                self.queued.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(_) => {
                self.dropped_priority.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Moves up to `max` waiting records into `out`, priority first.
    pub fn drain(&self, out: &mut Vec<Record>, max: usize) {
        while out.len() < max {
            match self.priority.pop() {
                Some(r) => out.push(r),
                None => break,
            }
        }
        while out.len() < max {
            match self.telemetry.pop() {
                Some(r) => out.push(r),
                None => break,
            }
        }
    }

    /// The counters.
    pub fn stats(&self) -> IngestStats {
        IngestStats {
            queued: self.queued.load(Ordering::Relaxed),
            dropped_telemetry: self.dropped_telemetry.load(Ordering::Relaxed),
            dropped_priority: self.dropped_priority.load(Ordering::Relaxed),
            pending: (self.priority.len() + self.telemetry.len()) as u64,
        }
    }
}

/// The largest batch the writer appends at once.
pub const BATCH: usize = 4096;

/// Stack of the writer thread. A daemon that locks its memory pays for
/// every page of it; the writer drains queues and appends blocks.
pub const WRITER_STACK: usize = 512 * 1024;

/// The writer thread (`svc/store`): drains an [`Ingest`] into a [`Store`]
/// and runs its housekeeping.
#[derive(Debug)]
pub struct Writer {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Writer {
    /// Starts the thread.
    pub fn spawn(store: Arc<Store>, ingest: Arc<Ingest>) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("svc/store".into())
            .stack_size(WRITER_STACK)
            .spawn(move || {
                let mut batch = Vec::with_capacity(BATCH);
                loop {
                    let stopping = flag.load(Ordering::Acquire);
                    batch.clear();
                    ingest.drain(&mut batch, BATCH);
                    let idle = batch.is_empty();
                    if !idle {
                        store.append(&batch);
                    }
                    store.maintain();
                    if stopping && idle {
                        let _ = store.sync();
                        return;
                    }
                    if idle {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
            })?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Drains what is queued, syncs and stops.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
