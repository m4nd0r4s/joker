//! The decision log (docs/01 §19, ADR 0033 §10–§11, AGENTS.md invariant 8).
//!
//! One JSON object per line, appended to `decisions.jsonl`. The core reports
//! a record on each change of an output's triple (applied value, outcome,
//! active level) and of each writer's results, with the repeat count of what
//! held before. Trips, manual writes, rejections and changes of active level
//! are written with `fsync` at once; the rest goes in a batch synced at
//! least once a second. The writes a real driver did not confirm follow as
//! `write_ack` entries (ref/06 §2.9). Records that could not be queued are
//! counted and written as a `gap`.

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

use joker_hal::{WriteKind, WriteStatus};

use crate::plan::Plan;
use crate::shared::{Shared, WriteAckRecord};
use crate::trips::TripTable;

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

    /// Visits the entries of a log file one line at a time, newest first
    /// (read backwards in blocks from the end) or oldest first, until
    /// `visit` returns `false`. Memory stays at one block and one line, so
    /// a read of the last few entries costs the same on a log of any size.
    /// A line that is not JSON is skipped.
    pub fn scan(
        path: &Path,
        newest_first: bool,
        visit: impl FnMut(Value) -> bool,
    ) -> io::Result<()> {
        Self::scan_filtered(path, newest_first, &[], visit)
    }

    /// [`DecisionLog::scan`] over the lines that hold at least one of
    /// `needles` (all of them when it is empty); the others are never
    /// parsed. Parsing is what a scan of a large log spends its time on, so
    /// a needle that names a rare entry (an exact output path) makes the
    /// scan for it fast: newest first, a block that holds no needle is
    /// skipped whole, with one search instead of one per line.
    pub fn scan_filtered(
        path: &Path,
        newest_first: bool,
        needles: &[&[u8]],
        mut visit: impl FnMut(Value) -> bool,
    ) -> io::Result<()> {
        use std::io::{BufRead, Read, Seek, SeekFrom};
        let finders: Vec<memchr::memmem::Finder<'_>> =
            needles.iter().map(memchr::memmem::Finder::new).collect();
        let holds =
            |bytes: &[u8]| finders.is_empty() || finders.iter().any(|f| f.find(bytes).is_some());
        let mut file = File::open(path)?;
        let mut line_of = |bytes: &[u8]| -> bool {
            let trimmed = bytes.trim_ascii();
            if trimmed.is_empty() || !holds(trimmed) {
                return true;
            }
            match serde_json::from_slice(trimmed) {
                Ok(v) => visit(v),
                Err(_) => true,
            }
        };
        if !newest_first {
            for line in io::BufReader::new(file).split(b'\n') {
                if !line_of(&line?) {
                    break;
                }
            }
            return Ok(());
        }
        const BLOCK: u64 = 64 * 1024;
        let mut end = file.seek(SeekFrom::End(0))?;
        let mut carry: Vec<u8> = Vec::new();
        while end > 0 {
            let start = end.saturating_sub(BLOCK);
            let mut block = vec![0u8; usize::try_from(end - start).unwrap_or(0)];
            file.seek(SeekFrom::Start(start))?;
            file.read_exact(&mut block)?;
            block.extend_from_slice(&carry);
            if !holds(&block) {
                let first = memchr::memchr(b'\n', &block).unwrap_or(block.len());
                block.truncate(first);
                carry = block;
                end = start;
                continue;
            }
            let mut tail = block.len();
            while let Some(nl) = memchr::memrchr(b'\n', &block[..tail]) {
                if !line_of(&block[nl + 1..tail]) {
                    return Ok(());
                }
                tail = nl;
            }
            block.truncate(tail);
            carry = block;
            end = start;
        }
        line_of(&carry);
        Ok(())
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
        Cause::Takeover(w) => json!({"kind": "takeover", "writer": writer_json(plan, w)}),
        Cause::ForcedRelease(w) => json!({"kind": "release", "writer": writer_json(plan, w)}),
        Cause::Trip => json!({"kind": "trip"}),
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

fn write_status_name(status: WriteStatus) -> &'static str {
    match status {
        WriteStatus::Ok => "ok",
        WriteStatus::Timeout => "timeout",
        WriteStatus::CommFailure => "comm_failure",
        WriteStatus::DeviceRejected => "device_rejected",
        WriteStatus::NotConnected => "not_connected",
        WriteStatus::Unsupported => "unsupported",
        WriteStatus::Late => "late",
        WriteStatus::Skipped => "skipped",
    }
}

fn write_kind_name(kind: WriteKind) -> &'static str {
    match kind {
        WriteKind::Set => "set",
        WriteKind::Keepalive => "keepalive",
        WriteKind::ForceSafe => "force_safe",
        WriteKind::Relinquish => "relinquish",
        WriteKind::Resync => "resync",
    }
}

/// The registry code a write status records (ref/06 §2.9): `J0877` for a
/// write the equipment rejected or does not support, `J0878` for a timed
/// write applied late.
pub fn ack_code(status: WriteStatus) -> Option<&'static str> {
    match status {
        WriteStatus::DeviceRejected | WriteStatus::Unsupported => Some("J0877"),
        WriteStatus::Late => Some("J0878"),
        _ => None,
    }
}

/// The log entry of a write a real driver did not confirm (ref/06 §2.9):
/// `record = "write_ack"`, with the output, the device, the write's kind
/// and sequence number, the driver's status and its code, if any. Those
/// with a code are synced at once, like rejections.
pub fn write_ack_json(plan: &Plan, generation: u64, ack: &WriteAckRecord) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("gen".into(), json!(generation));
    m.insert("record".into(), json!("write_ack"));
    m.insert("output".into(), json!(plan.path(ack.output).unwrap_or("")));
    m.insert(
        "device".into(),
        plan.devices
            .get(ack.device as usize)
            .map_or(Value::Null, |d| json!(d.name)),
    );
    m.insert("write_seq".into(), json!(ack.seq));
    m.insert("kind".into(), json!(write_kind_name(ack.kind)));
    m.insert("status".into(), json!(write_status_name(ack.status)));
    m.insert(
        "code".into(),
        ack_code(ack.status).map_or(Value::Null, |c| json!(c)),
    );
    m.insert("at_mono_ns".into(), json!(ack.at_mono_ns));
    m
}

/// A failure to write a runtime state file while running (ADR 0070).
#[derive(Debug, Clone, Copy)]
pub struct StateWriteFailure<'a> {
    /// The generation running, if any.
    pub generation: Option<u64>,
    /// `state/manual` or `state/trips`.
    pub file: &'static str,
    /// The output whose change was being written; `None` when the whole
    /// table was being rewritten.
    pub output: Option<&'a str>,
    /// `set`, `remove` or `clear` for manual levels; `trip` or `reset`
    /// for trips.
    pub op: &'static str,
}

impl StateWriteFailure<'_> {
    /// Reports `error` on standard error and appends the
    /// `state_write_failed` record to `log` with `fsync`.
    pub fn report(&self, log: &SharedLog, error: &io::Error) {
        let output = self.output.unwrap_or("");
        eprintln!(
            "jokerd: cannot persist {} ({} {output}): {error}",
            self.file, self.op
        );
        let mut m = Map::new();
        m.insert(
            "gen".into(),
            self.generation.map_or(Value::Null, |g| json!(g)),
        );
        m.insert("record".into(), json!("state_write_failed"));
        m.insert("file".into(), json!(self.file));
        m.insert(
            "output".into(),
            self.output.map_or(Value::Null, |o| json!(o)),
        );
        m.insert("op".into(), json!(self.op));
        m.insert("error".into(), json!(error.to_string()));
        m.insert(
            "ts".into(),
            json!(crate::wire::timestamp(crate::wire::utc_now_ns())),
        );
        if let Err(e) = lock(log).append(m, true) {
            eprintln!("jokerd: decision log: {e}");
        }
    }
}

/// Persists each transition of an output into or out of trip to
/// `state/trips` as the services observe it (docs/01 §7, ADR 0061): the
/// cycle thread publishes when each output became tripped, and this
/// compares it with what it saw last, so a trip is persisted whether or
/// not anyone reads the API.
#[derive(Debug)]
pub struct TripWatch {
    table: Arc<TripTable>,
    paths: Vec<String>,
    tripped: Vec<bool>,
    reported: Vec<bool>,
}

impl TripWatch {
    /// Watches the outputs of `shared` (in [`Shared::output_ids`] order).
    pub fn new(plan: &Plan, shared: &Shared, table: Arc<TripTable>) -> Self {
        let paths = shared
            .output_ids
            .iter()
            .map(|id| plan.path(*id).unwrap_or("").to_owned())
            .collect();
        Self {
            table,
            paths,
            tripped: vec![false; shared.output_ids.len()],
            reported: vec![false; shared.output_ids.len()],
        }
    }

    /// Persists every transition since the last call; one that fails to
    /// persist is recorded once in `log` (`state_write_failed`, ADR 0070)
    /// and retried on the next call.
    pub fn observe(
        &mut self,
        shared: &Shared,
        utc_origin_ns: i64,
        log: &SharedLog,
        generation: u64,
    ) {
        let cells = shared.trip_since.iter();
        let state = cells
            .zip(&self.paths)
            .zip(self.tripped.iter_mut().zip(self.reported.iter_mut()));
        for ((cell, path), (seen, reported)) in state {
            let since = cell.load(Ordering::Relaxed);
            let now = since != 0;
            if now == *seen || path.is_empty() {
                *reported = false;
                continue;
            }
            let result = if now {
                let since_utc_ns =
                    utc_origin_ns.saturating_add(i64::try_from(since).unwrap_or(i64::MAX));
                self.table.set_tripped(path, since_utc_ns)
            } else {
                self.table.clear_tripped(path)
            };
            match result {
                Ok(()) => {
                    *seen = now;
                    *reported = false;
                }
                Err(e) if !*reported => {
                    *reported = true;
                    StateWriteFailure {
                        generation: Some(generation),
                        file: "state/trips",
                        output: Some(path),
                        op: if now { "trip" } else { "reset" },
                    }
                    .report(log, &e);
                }
                Err(_) => {}
            }
        }
    }
}

/// The thread that drains the decision records of one generation.
#[derive(Debug)]
pub struct DecisionWriter {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<io::Result<()>>>,
}

impl DecisionWriter {
    /// Starts draining `shared.decisions` into `log` and, with `trips`,
    /// persisting the trip transitions of `shared` ([`TripWatch`]).
    pub fn spawn(
        plan: Arc<Plan>,
        shared: Arc<Shared>,
        log: SharedLog,
        generation: u64,
        utc_origin_ns: i64,
        trips: Option<Arc<TripTable>>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let mut watch = trips.map(|t| TripWatch::new(&plan, &shared, t));
        let handle = std::thread::Builder::new()
            .name("svc/decisions".into())
            .stack_size(crate::shared::SERVICE_STACK)
            .spawn(move || {
                let mut reported_drops = 0;
                loop {
                    let stopping = flag.load(Ordering::Acquire);
                    if let Some(w) = watch.as_mut() {
                        w.observe(&shared, utc_origin_ns, &log, generation);
                    }
                    drain(
                        &plan,
                        &shared,
                        &log,
                        generation,
                        utc_origin_ns,
                        &mut reported_drops,
                    )?;
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
    utc_origin_ns: i64,
    reported_drops: &mut u64,
) -> io::Result<()> {
    let mut log = lock(log);
    while let Some(r) = shared.decisions.pop() {
        let immediate = r.durability == Durability::Immediate;
        let mut entry = record_json(plan, generation, &r);
        entry.insert(
            "ts".into(),
            json!(crate::wire::timestamp(
                utc_origin_ns.saturating_add(r.at_mono_ns as i64)
            )),
        );
        log.append(entry, immediate)?;
    }
    while let Some(ack) = shared.write_acks.pop() {
        let immediate = ack_code(ack.status).is_some();
        let mut entry = write_ack_json(plan, generation, &ack);
        entry.insert(
            "ts".into(),
            json!(crate::wire::timestamp(
                utc_origin_ns.saturating_add(ack.at_mono_ns as i64)
            )),
        );
        log.append(entry, immediate)?;
    }
    let dropped = shared.decisions_dropped.get() + shared.write_acks_dropped.get();
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
