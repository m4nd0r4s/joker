//! The decision log (docs/01 §19, ADR 0033 §10–§11, AGENTS.md invariant 8).
//!
//! One JSON object per line, appended to `decisions.jsonl`. The core reports
//! a record on each change of an output's triple (applied value, outcome,
//! active level) and of each writer's results, with the repeat count of what
//! held before. Trips, manual writes, rejections and changes of active level
//! are written with `fsync` at once; the rest goes in a batch synced at
//! least once a second. Records the cycle thread could not queue are counted
//! and written as a `gap`.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant as StdInstant};

use joker_core::commit::{Cause, DecisionRecord, Durability, RecordKind, Span};
use joker_core::envelope::Applied;
use joker_core::outcome::Outcome;
use joker_core::{Level, ValueType, WriterId};
use serde_json::{Map, Value, json};

use crate::plan::Plan;
use crate::shared::Shared;

/// How often batch records are synced.
pub const BATCH_SYNC: Duration = Duration::from_secs(1);

/// The append-only decision log file.
#[derive(Debug)]
pub struct DecisionLog {
    path: PathBuf,
    out: BufWriter<File>,
    seq: u64,
    unsynced: bool,
    last_sync: StdInstant,
}

impl DecisionLog {
    /// Opens or creates the log, continuing its sequence numbers.
    pub fn open(path: &Path) -> io::Result<Self> {
        let seq = match std::fs::read_to_string(path) {
            Ok(text) => text.lines().count() as u64,
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e),
        };
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            path: path.to_owned(),
            out: BufWriter::new(file),
            seq,
            unsynced: false,
            last_sync: StdInstant::now(),
        })
    }

    /// The log's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one entry; `immediate` entries are synced before returning.
    pub fn append(&mut self, mut entry: Map<String, Value>, immediate: bool) -> io::Result<u64> {
        let seq = self.seq;
        entry.insert("seq".into(), json!(seq));
        serde_json::to_writer(&mut self.out, &Value::Object(entry))?;
        self.out.write_all(b"\n")?;
        self.seq += 1;
        self.unsynced = true;
        if immediate {
            self.sync()?;
        }
        Ok(seq)
    }

    /// Flushes and syncs what is pending.
    pub fn sync(&mut self) -> io::Result<()> {
        if self.unsynced {
            self.out.flush()?;
            self.out.get_ref().sync_data()?;
            self.unsynced = false;
        }
        self.last_sync = StdInstant::now();
        Ok(())
    }

    /// Syncs the batch if it is older than [`BATCH_SYNC`].
    pub fn sync_if_due(&mut self) -> io::Result<()> {
        if self.unsynced && self.last_sync.elapsed() >= BATCH_SYNC {
            self.sync()?;
        }
        Ok(())
    }

    /// Reads every entry of a log file.
    pub fn read_all(path: &Path) -> io::Result<Vec<Value>> {
        let text = std::fs::read_to_string(path)?;
        text.lines()
            .map(|l| serde_json::from_str(l).map_err(io::Error::other))
            .collect()
    }
}

/// A shared handle on the decision log.
pub type SharedLog = Arc<Mutex<DecisionLog>>;

/// Locks the log, recovering from a poisoned lock.
pub fn lock(log: &SharedLog) -> MutexGuard<'_, DecisionLog> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The JSON form of a value of a signal (ref/00 §4.1).
pub fn value_json(plan: &Plan, path: &str, applied: Applied) -> Value {
    let bits = match applied {
        Applied::Relinquish => return json!("relinquish"),
        Applied::Value(bits) => bits,
    };
    let Some(signal) = plan.signal(path) else {
        return json!(bits);
    };
    match signal.ty {
        ValueType::Bool => json!(bits != 0),
        ValueType::F64 | ValueType::F32 => json!(f64::from_bits(bits)),
        ValueType::I64 => json!(bits as i64),
        ValueType::Enum => signal
            .variants
            .get(bits as usize)
            .map_or(json!(bits), |v| json!(v)),
        _ => json!(bits),
    }
}

fn level_json(level: Option<Level>) -> Value {
    level.map_or(Value::Null, |l| json!(l.to_string()))
}

fn writer_json(plan: &Plan, writer: WriterId) -> Value {
    plan.writers
        .get(writer.index())
        .map_or(json!(writer.0), |w| json!(w.name))
}

fn outcome_fields(entry: &mut Map<String, Value>, outcome: Outcome) {
    entry.insert("outcome".into(), json!(outcome.name()));
    match outcome {
        Outcome::Rejected(reason) => {
            entry.insert("reason".into(), json!(reason.name()));
        }
        Outcome::Overridden(level) => {
            entry.insert("by".into(), json!(level.to_string()));
        }
        _ => {}
    }
}

fn span_json(plan: &Plan, path: &str, span: &Span) -> Value {
    let mut m = Map::new();
    m.insert("applied".into(), value_json(plan, path, span.applied));
    outcome_fields(&mut m, span.outcome);
    m.insert("level".into(), level_json(span.level));
    m.insert("first_tick".into(), json!(span.first_tick));
    m.insert("last_tick".into(), json!(span.last_tick));
    m.insert("repeats".into(), json!(span.repeats));
    Value::Object(m)
}

/// The log entry of a core decision record.
pub fn record_json(plan: &Plan, generation: u64, r: &DecisionRecord) -> Map<String, Value> {
    let path = plan.path(r.output).unwrap_or("").to_owned();
    let mut m = Map::new();
    m.insert("gen".into(), json!(generation));
    match r.kind {
        RecordKind::Output => {
            m.insert("record".into(), json!("output"));
        }
        RecordKind::Write(w) => {
            m.insert("record".into(), json!("write"));
            m.insert("writer".into(), writer_json(plan, w));
        }
    }
    m.insert("output".into(), json!(path));
    m.insert("tick".into(), json!(r.tick));
    m.insert("at_mono_ns".into(), json!(r.at_mono_ns));
    m.insert("applied".into(), value_json(plan, &path, r.applied));
    outcome_fields(&mut m, r.outcome);
    m.insert("level".into(), level_json(r.level));
    let cause = match r.cause {
        Cause::Proposal(w) => json!({"kind": "proposal", "writer": writer_json(plan, w)}),
        Cause::Expiry => json!({"kind": "expiry"}),
        Cause::Reset => json!({"kind": "reset"}),
        Cause::Envelope => json!({"kind": "envelope"}),
    };
    m.insert("cause".into(), cause);
    m.insert(
        "previous".into(),
        r.previous
            .as_ref()
            .map_or(Value::Null, |s| span_json(plan, &path, s)),
    );
    m.insert(
        "durability".into(),
        json!(match r.durability {
            Durability::Immediate => "immediate",
            Durability::Batch => "batch",
        }),
    );
    m
}

/// The thread that drains the decision records of one generation.
#[derive(Debug)]
pub struct DecisionWriter {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<io::Result<()>>>,
}

impl DecisionWriter {
    /// Starts draining `shared.decisions` into `log`.
    pub fn spawn(plan: Arc<Plan>, shared: Arc<Shared>, log: SharedLog, generation: u64) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("svc/decisions".into())
            .spawn(move || {
                let mut reported_drops = 0;
                loop {
                    let stopping = flag.load(Ordering::Acquire);
                    drain(&plan, &shared, &log, generation, &mut reported_drops)?;
                    if stopping {
                        return lock(&log).sync();
                    }
                    lock(&log).sync_if_due()?;
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .ok();
        Self { stop, handle }
    }

    /// Drains what is left, syncs and stops.
    pub fn stop(mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        match self.handle.take().map(JoinHandle::join) {
            Some(Ok(result)) => result,
            Some(Err(_)) => Err(io::Error::other("decision writer panicked")),
            None => Ok(()),
        }
    }
}

/// Drains the queued records into the log once.
pub fn drain(
    plan: &Plan,
    shared: &Shared,
    log: &SharedLog,
    generation: u64,
    reported_drops: &mut u64,
) -> io::Result<()> {
    let mut log = lock(log);
    while let Some(r) = shared.decisions.pop() {
        let immediate = r.durability == Durability::Immediate;
        log.append(record_json(plan, generation, &r), immediate)?;
    }
    let dropped = shared.decisions_dropped.get();
    if dropped > *reported_drops {
        let mut m = Map::new();
        m.insert("gen".into(), json!(generation));
        m.insert("record".into(), json!("gap"));
        m.insert("dropped".into(), json!(dropped - *reported_drops));
        log.append(m, true)?;
        *reported_drops = dropped;
    }
    Ok(())
}
