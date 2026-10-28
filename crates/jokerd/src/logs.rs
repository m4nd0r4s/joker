//! The node's structured log ring behind `GET /v1/logs` (ref/09 §5.2):
//! entries from `jokerd` itself and from the drivers, oldest dropped
//! first once the ring holds its byte budget (16 MiB by default), every
//! drop counted (AGENTS.md invariant 7). It is written and read off the
//! tick: the cycle domain never touches it.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

/// The ring's default byte budget (ref/09 §5.2).
pub const DEFAULT_CAPACITY_BYTES: usize = 16 << 20;

/// What one entry costs beyond its text, in the ring's byte budget.
const ENTRY_OVERHEAD_BYTES: usize = 64;

/// The level of an entry (`LogLevel` of ref/09), in increasing severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// `trace`.
    Trace,
    /// `debug`.
    Debug,
    /// `info`.
    Info,
    /// `warn`.
    Warn,
    /// `error`.
    Error,
}

impl LogLevel {
    /// The name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    /// The level named `text`, if any.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "trace" => Self::Trace,
            "debug" => Self::Debug,
            "info" => Self::Info,
            "warn" => Self::Warn,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

impl From<joker_host::LogLevel> for LogLevel {
    fn from(level: joker_host::LogLevel) -> Self {
        match level {
            joker_host::LogLevel::Trace => Self::Trace,
            joker_host::LogLevel::Debug => Self::Debug,
            joker_host::LogLevel::Info => Self::Info,
            joker_host::LogLevel::Warn => Self::Warn,
            joker_host::LogLevel::Error => Self::Error,
        }
    }
}

impl From<joker_hal::LogLevel> for LogLevel {
    fn from(level: joker_hal::LogLevel) -> Self {
        match level {
            joker_hal::LogLevel::Debug => Self::Debug,
            joker_hal::LogLevel::Info => Self::Info,
            joker_hal::LogLevel::Warn => Self::Warn,
            joker_hal::LogLevel::Error => Self::Error,
        }
    }
}

/// One entry (`LogEntry` of ref/09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Its number in the ring, from 1, never reused.
    pub seq: u64,
    /// When it was written, UTC nanoseconds.
    pub ts_utc_ns: i64,
    /// Its level.
    pub level: LogLevel,
    /// Who wrote it: `jokerd`, `driver:<d>`, `app:<a>`, `micro:<d>`, `api`.
    pub source: String,
    /// The message.
    pub msg: String,
    /// Structured fields, in the order written.
    pub fields: Vec<(String, String)>,
}

impl LogEntry {
    fn cost(&self) -> usize {
        ENTRY_OVERHEAD_BYTES
            + self.source.len()
            + self.msg.len()
            + self
                .fields
                .iter()
                .map(|(k, v)| k.len() + v.len())
                .sum::<usize>()
    }
}

/// What a query of the ring selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFilter {
    /// Only this source.
    pub source: Option<String>,
    /// Only entries at this level or above.
    pub min_level: LogLevel,
    /// Only entries written at or after this UTC instant.
    pub from_ns: i64,
    /// Only entries written at or before this UTC instant.
    pub to_ns: i64,
    /// Oldest first instead of newest first.
    pub ascending: bool,
    /// At most this many entries.
    pub limit: usize,
    /// Continue after the entry with this `seq`, the previous page's
    /// cursor.
    pub cursor: Option<u64>,
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            source: None,
            min_level: LogLevel::Trace,
            from_ns: i64::MIN,
            to_ns: i64::MAX,
            ascending: false,
            limit: 100,
            cursor: None,
        }
    }
}

#[derive(Debug, Default)]
struct Inner {
    entries: VecDeque<LogEntry>,
    bytes: usize,
    next_seq: u64,
    dropped: u64,
}

/// The ring.
#[derive(Debug)]
pub struct LogRing {
    capacity_bytes: usize,
    inner: Mutex<Inner>,
}

impl Default for LogRing {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY_BYTES)
    }
}

impl LogRing {
    /// An empty ring holding up to `capacity_bytes` of entries.
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity_bytes,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Writes an entry, dropping the oldest ones it does not fit beside.
    pub fn push(
        &self,
        level: LogLevel,
        source: &str,
        msg: &str,
        fields: Vec<(String, String)>,
        ts_utc_ns: i64,
    ) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.next_seq += 1;
        let entry = LogEntry {
            seq: inner.next_seq,
            ts_utc_ns,
            level,
            source: source.to_owned(),
            msg: msg.to_owned(),
            fields,
        };
        inner.bytes += entry.cost();
        inner.entries.push_back(entry);
        while inner.bytes > self.capacity_bytes {
            let Some(old) = inner.entries.pop_front() else {
                break;
            };
            inner.bytes -= old.cost();
            inner.dropped += 1;
        }
    }

    /// Entries dropped to make room, since the ring was created.
    pub fn dropped(&self) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .dropped
    }

    /// One page of the entries `filter` selects, and the cursor of the
    /// next page when there is one.
    pub fn query(&self, filter: &LogFilter) -> (Vec<LogEntry>, Option<u64>) {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let selected = |e: &&LogEntry| {
            e.level >= filter.min_level
                && (filter.from_ns..=filter.to_ns).contains(&e.ts_utc_ns)
                && filter.source.as_deref().is_none_or(|s| s == e.source)
                && filter.cursor.is_none_or(|c| {
                    if filter.ascending {
                        e.seq > c
                    } else {
                        e.seq < c
                    }
                })
        };
        let limit = filter.limit.max(1);
        let mut page: Vec<LogEntry> = if filter.ascending {
            inner
                .entries
                .iter()
                .filter(selected)
                .take(limit + 1)
                .cloned()
                .collect()
        } else {
            inner
                .entries
                .iter()
                .rev()
                .filter(selected)
                .take(limit + 1)
                .cloned()
                .collect()
        };
        let next = (page.len() > limit).then(|| page[limit - 1].seq);
        page.truncate(limit);
        (page, next)
    }
}
