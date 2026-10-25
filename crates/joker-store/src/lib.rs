//! The local historian of a Joker node (docs/01 §12, ADR 0013, ADR 0081).
//!
//! Every record first goes to a shared write-ahead log, synced according to
//! the `fsync` policy of ref/01 §5.17.2. Each series keeps an open block
//! per tier in memory: raw samples and the `1s`, `1m` and `15m` aggregates
//! are Gorilla-encoded into blocks of at most 4 KiB, and events are
//! compressed with zstd. A full block is sealed into a segment file shared
//! by the series with the same tier, class and `keep`. A checkpoint syncs
//! the segment files, writes the open blocks into a fresh WAL file and
//! deletes the older ones, so the WAL stays bounded and recovery replays
//! only what was not sealed. Retention deletes whole segment files: by
//! time, after each tier's `keep`, and by size, oldest raw telemetry first,
//! once the store passes 90% of `max_disk`. Below 1% of free space only
//! alarms and decisions are accepted. Producers never wait on the store:
//! [`Ingest`] is a pair of bounded lock-free queues whose drops are counted
//! (AGENTS.md invariant 7).

mod agg;
mod bits;
mod block;
mod codec;
mod crc;
mod cursor;
mod frame;
mod ingest;
mod segment;
mod store;
mod wal;

pub use agg::{Bucket, aggregate, quality_code, reaggregate};
pub use ingest::{Ingest, IngestStats, Payload, Record, Writer};
pub use store::{Recovery, Store};

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// The nanoseconds of one second.
pub const SECOND_NS: u64 = 1_000_000_000;

/// One sample of a scalar signal (ref/00 §4.2, ref/09 §5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// Acquisition instant, UTC ns.
    pub ts_ns: i64,
    /// Source instant, UTC ns, when the equipment gave one.
    pub ts_src_ns: Option<i64>,
    /// The value's transport bits (ref/00 §4.1).
    pub bits: u64,
    /// The quality word: code in the low byte, reason in the next one.
    pub quality: u32,
    /// The signal's sequence number.
    pub seq: u64,
}

/// One occurrence of an event series: a signal with shape `event`, an
/// alarm transition or a decision (ref/09 §5.5, §5.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Instant, UTC ns.
    pub ts_ns: i64,
    /// Source instant, UTC ns.
    pub ts_src_ns: Option<i64>,
    /// Sequence number of the source.
    pub seq: u64,
    /// The record, encoded by the producer.
    pub payload: Vec<u8>,
}

/// One aggregate of a tier (docs/01 §12): the samples whose acquisition
/// instant falls in `[ts_ns, ts_ns + resolution)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aggregate {
    /// Start of the interval, UTC ns.
    pub ts_ns: i64,
    /// Samples in the interval.
    pub count: u64,
    /// Samples that were not `Bad`; only they enter the values (AGENTS.md
    /// invariant 4).
    pub valid: u64,
    /// Smallest value; NaN when the series is not numeric or `valid` is 0.
    pub min: f64,
    /// Largest value; NaN as `min`.
    pub max: f64,
    /// Mean value; NaN as `min`.
    pub mean: f64,
    /// Bits of the first valid sample; meaningless when `valid` is 0.
    pub first: u64,
    /// Bits of the last valid sample; meaningless when `valid` is 0.
    pub last: u64,
    /// The dominant quality code: the most frequent of `good`, `uncertain`
    /// and `bad`, the worse one on a tie (ADR 0081).
    pub quality: u32,
}

/// How the bits of a scalar series read as a number, for the aggregates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// `f64`.
    F64,
    /// `f32` in the low 32 bits.
    F32,
    /// `i64`.
    I64,
    /// `u64`.
    U64,
    /// Not numeric (`bool`, `enum`, `bits`, wide values): aggregates keep
    /// `count`, `first`, `last` and the quality only.
    Other,
}

impl ValueKind {
    /// The number the bits stand for, or `None` when not numeric.
    pub fn number(self, bits: u64) -> Option<f64> {
        match self {
            Self::F64 => Some(f64::from_bits(bits)),
            Self::F32 => Some(f64::from(f32::from_bits(bits as u32))),
            Self::I64 => Some(bits as i64 as f64),
            Self::U64 => Some(bits as f64),
            Self::Other => None,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::F64 => 0,
            Self::F32 => 1,
            Self::I64 => 2,
            Self::U64 => 3,
            Self::Other => 4,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::F64,
            1 => Self::F32,
            2 => Self::I64,
            3 => Self::U64,
            4 => Self::Other,
            _ => return None,
        })
    }
}

/// What a series holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesKind {
    /// Samples of a scalar signal, Gorilla-encoded.
    Scalar(ValueKind),
    /// Events, compressed with zstd; raw tier only.
    Event,
}

/// The priority of a series under pressure (docs/01 §20, ADR 0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// Signals recorded by `[telemetry.record]`.
    Telemetry,
    /// Alarm transitions and operator actions on alarms.
    Alarm,
    /// Decisions.
    Decision,
}

impl Class {
    /// Alarms and decisions: synced at once, kept when the disk is
    /// critical and deleted last by size retention.
    pub fn is_protected(self) -> bool {
        !matches!(self, Self::Telemetry)
    }

    fn code(self) -> u8 {
        match self {
            Self::Telemetry => 0,
            Self::Alarm => 1,
            Self::Decision => 2,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Telemetry,
            1 => Self::Alarm,
            2 => Self::Decision,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Telemetry => "telemetry",
            Self::Alarm => "alarm",
            Self::Decision => "decision",
        }
    }
}

/// A layer of the historian (ref/01 §5.17.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Every sample.
    Raw,
    /// One aggregate per second.
    S1,
    /// One aggregate per minute.
    M1,
    /// One aggregate per 15 minutes.
    M15,
}

impl Tier {
    /// Every tier, finest first.
    pub const ALL: [Tier; 4] = [Tier::Raw, Tier::S1, Tier::M1, Tier::M15];

    /// The interval of an aggregate, ns; 0 for `raw`.
    pub const fn resolution_ns(self) -> u64 {
        match self {
            Self::Raw => 0,
            Self::S1 => SECOND_NS,
            Self::M1 => 60 * SECOND_NS,
            Self::M15 => 900 * SECOND_NS,
        }
    }

    /// The name used by ref/01 and ref/09: `raw`, `1s`, `1m`, `15m`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::S1 => "1s",
            Self::M1 => "1m",
            Self::M15 => "15m",
        }
    }

    /// The tier with that name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Raw => 0,
            Self::S1 => 1,
            Self::M1 => 2,
            Self::M15 => 3,
        }
    }

    pub(crate) fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(usize::from(i)).copied()
    }

    /// How much time one segment file of this tier spans at most.
    pub(crate) const fn file_span_ns(self) -> u64 {
        match self {
            Self::Raw => 3600 * SECOND_NS,
            Self::S1 => 6 * 3600 * SECOND_NS,
            Self::M1 => 86_400 * SECOND_NS,
            Self::M15 => 7 * 86_400 * SECOND_NS,
        }
    }
}

/// How long each tier of a series is kept (`keep`, ref/01 §5.17.1); a tier
/// without a duration is not built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Keep(pub [Option<u64>; 4]);

impl Keep {
    /// `{ raw = "7d" }`, the default of ref/01 §5.17.1.
    pub const DEFAULT: Keep = Keep([Some(7 * 86_400 * SECOND_NS), None, None, None]);

    /// The duration of a tier, ns.
    pub fn get(&self, tier: Tier) -> Option<u64> {
        self.0[tier.index()]
    }

    /// Sets the duration of a tier.
    pub fn with(mut self, tier: Tier, ns: u64) -> Self {
        self.0[tier.index()] = Some(ns);
        self
    }
}

/// A series: one recorded signal, one alarm journal or one event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesSpec {
    /// The canonical path (or name) of the series; unique in the store.
    pub path: String,
    /// What it holds.
    pub kind: SeriesKind,
    /// Its class.
    pub class: Class,
    /// Its retention per tier; aggregates only for scalar series.
    pub keep: Keep,
}

/// A series of the store, by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeriesId(pub u32);

/// The `fsync` policy of the WAL (ref/01 §5.17.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fsync {
    /// Synced at most every `fsync_interval`, and at once after a batch
    /// with an alarm or a decision.
    Batch,
    /// Synced after every batch.
    Always,
    /// Never synced by the store, except at checkpoints.
    Never,
}

impl Fsync {
    /// The policy with that name (`batch`, `always`, `never`).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "batch" => Self::Batch,
            "always" => Self::Always,
            "never" => Self::Never,
            _ => return None,
        })
    }
}

/// Free and total bytes of the partition that holds the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskSpace {
    /// Bytes available to the store.
    pub free: u64,
    /// Bytes of the partition.
    pub total: u64,
}

/// Measures the partition of a directory; `None` when it cannot.
pub type DiskProbe = Arc<dyn Fn(&Path) -> Option<DiskSpace> + Send + Sync>;

/// The clocks the store reads.
pub trait Clock: Send + Sync {
    /// Monotonic ns, for the sync and checkpoint intervals.
    fn mono_ns(&self) -> u64;
    /// UTC ns, for retention by time.
    fn utc_ns(&self) -> i64;
}

/// The system clocks.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: std::time::Instant,
}

impl SystemClock {
    /// A clock whose monotonic zero is now.
    pub fn new() -> Self {
        Self {
            origin: std::time::Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn mono_ns(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    fn utc_ns(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_nanos()).ok())
            .unwrap_or(0)
    }
}

/// How a store is opened.
#[derive(Clone)]
pub struct StoreConfig {
    /// The WAL policy.
    pub fsync: Fsync,
    /// The batch interval, ns (`fsync_interval`, default 1 s).
    pub fsync_interval_ns: u64,
    /// Retention by size: the segments and the WAL together (`max_disk`).
    pub max_disk: u64,
    /// A WAL file larger than this starts a checkpoint.
    pub wal_file_bytes: u64,
    /// A checkpoint at least this often, ns, so a quiet store still trims
    /// its WAL.
    pub checkpoint_interval_ns: u64,
    /// A segment file larger than this is closed and a new one started.
    pub segment_file_bytes: u64,
    /// How often retention and the disk probe run, ns.
    pub maintenance_interval_ns: u64,
    /// Measures free space; `None` relies on `max_disk` only.
    pub probe: Option<DiskProbe>,
}

impl std::fmt::Debug for StoreConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreConfig")
            .field("fsync", &self.fsync)
            .field("fsync_interval_ns", &self.fsync_interval_ns)
            .field("max_disk", &self.max_disk)
            .field("wal_file_bytes", &self.wal_file_bytes)
            .field("checkpoint_interval_ns", &self.checkpoint_interval_ns)
            .field("segment_file_bytes", &self.segment_file_bytes)
            .field("maintenance_interval_ns", &self.maintenance_interval_ns)
            .field("probe", &self.probe.is_some())
            .finish()
    }
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            fsync: Fsync::Batch,
            fsync_interval_ns: SECOND_NS,
            max_disk: 8 << 30,
            wal_file_bytes: 64 << 20,
            checkpoint_interval_ns: 300 * SECOND_NS,
            segment_file_bytes: 16 << 20,
            maintenance_interval_ns: 10 * SECOND_NS,
            probe: None,
        }
    }
}

/// Counters of a store (docs/01 §19, "Store/link").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreStats {
    /// Series in the catalog.
    pub series: u64,
    /// Records accepted.
    pub records: u64,
    /// Records refused because the disk is critical.
    pub dropped_disk_full: u64,
    /// Records refused for a write error.
    pub dropped_write_error: u64,
    /// Write errors seen.
    pub write_errors: u64,
    /// WAL syncs.
    pub fsyncs: u64,
    /// Checkpoints done.
    pub checkpoints: u64,
    /// Bytes of the segment files.
    pub segment_bytes: u64,
    /// Bytes of the WAL files.
    pub wal_bytes: u64,
    /// Segment files deleted by retention by time.
    pub expired_files: u64,
    /// Segment files deleted by retention by size.
    pub evicted_files: u64,
    /// True while only alarms and decisions are accepted.
    pub disk_full: bool,
}
