//! First-class alarms (M9, docs/02 §11, ADR 0040, ADR 0082): the alarm
//! book the API reads, the journal of every transition and operator
//! action, and what survives a restart.
//!
//! The cycle thread owns the state machines of the manifest's alarms
//! (`joker_core::alarm`) and reports each event into a bounded queue. The
//! alarm pump of a generation (`svc/alarms`) applies them to the book,
//! writes them to the historian's alarm journal with priority, and
//! persists the book to `state/alarms` with `fsync`. Operator commands
//! reach the cycle thread through their own queue and carry who and why
//! as an annotation the pump attaches to the event they cause.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant as StdInstant};

use joker_core::alarm::{AlarmCause, AlarmEvent, AlarmSnapshot, AlarmState, Severity};
use joker_store::{Event, Payload, Record, SeriesId, Store};
use serde_json::{Map, Value, json};

use crate::decisions::{SharedLog, StateWriteFailure};
use crate::plan::Plan;
use crate::shared::{AlarmReply, Shared};
use crate::wire::{duration, timestamp, utc_now_ns};

/// The state file of the alarm book.
pub const STATE_FILE: &str = "alarms";

/// The name of a published state (ref/00 §6).
pub fn state_name(s: AlarmState) -> &'static str {
    match s {
        AlarmState::Normal => "normal",
        AlarmState::ActiveUnacked => "active_unacked",
        AlarmState::ActiveAcked => "active_acked",
        AlarmState::RtnUnacked => "rtn_unacked",
        AlarmState::Unknown => "unknown",
    }
}

fn state_of(name: &str) -> Option<AlarmState> {
    Some(match name {
        "normal" => AlarmState::Normal,
        "active_unacked" => AlarmState::ActiveUnacked,
        "active_acked" => AlarmState::ActiveAcked,
        "rtn_unacked" => AlarmState::RtnUnacked,
        "unknown" => AlarmState::Unknown,
        _ => return None,
    })
}

/// The name of a severity (ref/00 §6).
pub fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

/// The severity with that name.
pub fn severity_of(name: &str) -> Option<Severity> {
    Some(match name {
        "low" => Severity::Low,
        "medium" => Severity::Medium,
        "high" => Severity::High,
        "critical" => Severity::Critical,
        _ => return None,
    })
}

/// A shelve in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelved {
    /// When it ends, UTC ns.
    pub until_utc_ns: i64,
    /// Who shelved it.
    pub by: String,
    /// Why.
    pub reason: String,
}

/// Who asked for an operator action, and why (ref/09 §5.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// The principal.
    pub by: String,
    /// The reason, if one was given.
    pub reason: Option<String>,
    /// For a shelve, when it ends, UTC ns.
    pub until_utc_ns: Option<i64>,
}

/// One alarm as the API shows it (ref/09 §5.8 `Alarm`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The id.
    pub id: String,
    /// `manifest`, `app`, `runtime` or `instance`.
    pub source: &'static str,
    /// The severity.
    pub severity: Severity,
    /// The published state.
    pub state: AlarmState,
    /// The state under an `Unknown` overlay.
    pub underlying: AlarmState,
    /// The condition was `Uncertain` at the last event.
    pub uncertain: bool,
    /// The shelve in force.
    pub shelved: Option<Shelved>,
    /// Who acknowledged the current activation.
    pub acked_by: Option<String>,
    /// When, UTC ns.
    pub acked_at: Option<i64>,
    /// When the published state last changed, UTC ns.
    pub since: Option<i64>,
    /// `message`, with its placeholders.
    pub message: Option<String>,
    /// The source of `when`.
    pub condition: String,
    /// `alarm.<a>.state` and `.active`.
    pub signals: Option<(String, String)>,
    /// The outputs it trips.
    pub trips: Vec<String>,
    /// `max_shelve`, ns.
    pub max_shelve_ns: u64,
    /// Its number in the running generation, for manifest alarms.
    pub number: Option<u32>,
}

impl Entry {
    /// True in both active states, whatever the overlay.
    pub fn active(&self) -> bool {
        self.underlying.is_active()
    }

    /// True while an acknowledgement is pending.
    pub fn unacked(&self) -> bool {
        matches!(
            self.underlying,
            AlarmState::ActiveUnacked | AlarmState::RtnUnacked
        )
    }
}

#[derive(Debug, Default)]
struct Book {
    entries: BTreeMap<String, Entry>,
    annotations: BTreeMap<String, VecDeque<Annotation>>,
    versions: BTreeMap<String, u64>,
    pending: BTreeMap<String, u64>,
    replies: BTreeMap<u32, AlarmReply>,
    dirty: bool,
}

/// The alarms of a node, across generations and restarts.
#[derive(Debug)]
pub struct AlarmBook {
    state_dir: PathBuf,
    inner: Mutex<Book>,
    changed: Condvar,
}

fn persisted_json(e: &Entry) -> Value {
    json!({
        "underlying": state_name(e.underlying),
        "shelved": e.shelved.as_ref().map(|s| json!({
            "until_ns": s.until_utc_ns,
            "by": s.by,
            "reason": s.reason,
        })),
        "acked_by": e.acked_by,
        "acked_at_ns": e.acked_at,
        "since_ns": e.since,
    })
}

impl AlarmBook {
    /// Loads `state/alarms` under `state_dir`; a file that does not parse
    /// is an error, never an empty book.
    pub fn open(state_dir: &Path) -> io::Result<Arc<Self>> {
        let mut book = Book::default();
        if let Some(bytes) = crate::state_file::read(state_dir, STATE_FILE)? {
            let v: Value = serde_json::from_slice(&bytes)
                .map_err(|e| crate::state_file::corrupt(state_dir, STATE_FILE, e))?;
            let Value::Object(items) = v else {
                return Err(crate::state_file::corrupt(
                    state_dir,
                    STATE_FILE,
                    "not an object",
                ));
            };
            for (id, p) in items {
                let underlying = p["underlying"]
                    .as_str()
                    .and_then(state_of)
                    .unwrap_or(AlarmState::Normal);
                let shelved = p["shelved"].as_object().map(|s| Shelved {
                    until_utc_ns: s.get("until_ns").and_then(Value::as_i64).unwrap_or(0),
                    by: s.get("by").and_then(Value::as_str).unwrap_or("").to_owned(),
                    reason: s
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                });
                book.entries.insert(
                    id.clone(),
                    Entry {
                        id,
                        source: "manifest",
                        severity: Severity::Low,
                        state: underlying,
                        underlying,
                        uncertain: false,
                        shelved,
                        acked_by: p["acked_by"].as_str().map(str::to_owned),
                        acked_at: p["acked_at_ns"].as_i64(),
                        since: p["since_ns"].as_i64(),
                        message: None,
                        condition: String::new(),
                        signals: None,
                        trips: Vec::new(),
                        max_shelve_ns: joker_core::alarm::DEFAULT_MAX_SHELVE_NS,
                        number: None,
                    },
                );
            }
        }
        Ok(Arc::new(Self {
            state_dir: state_dir.to_owned(),
            inner: Mutex::new(book),
            changed: Condvar::new(),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, Book> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Takes in the alarms of a generation that starts at `now_mono_ns`
    /// and returns what to restore in its cycle thread: each alarm keeps
    /// the state it had, in the previous generation or before a restart;
    /// a shelve whose end has passed is dropped. Manifest alarms the
    /// generation no longer declares leave the book.
    pub fn begin(&self, plan: &Plan, now_mono_ns: u64) -> Vec<(u32, AlarmSnapshot)> {
        let now_utc = utc_now_ns();
        let mut book = self.lock();
        let declared: Vec<&str> = plan.alarms.iter().map(|a| a.name.as_str()).collect();
        book.entries
            .retain(|id, e| e.source != "manifest" || declared.contains(&id.as_str()));
        let mut restore = Vec::new();
        for (i, a) in plan.alarms.iter().enumerate() {
            let Ok(number) = u32::try_from(i) else {
                continue;
            };
            let old = book.entries.remove(&a.name);
            let shelved = old
                .as_ref()
                .and_then(|o| o.shelved.clone())
                .filter(|s| s.until_utc_ns > now_utc);
            let underlying = old.as_ref().map_or(AlarmState::Normal, |o| o.underlying);
            let entry = Entry {
                id: a.name.clone(),
                source: "manifest",
                severity: a.severity,
                state: underlying,
                underlying,
                uncertain: false,
                shelved: shelved.clone(),
                acked_by: old.as_ref().and_then(|o| o.acked_by.clone()),
                acked_at: old.as_ref().and_then(|o| o.acked_at),
                since: old.as_ref().and_then(|o| o.since),
                message: a.message.clone(),
                condition: a.condition.clone(),
                signals: Some((a.state_path(), a.active_path())),
                trips: a
                    .trip
                    .iter()
                    .filter_map(|id| plan.path(*id).map(str::to_owned))
                    .collect(),
                max_shelve_ns: a.max_shelve_ns,
                number: Some(number),
            };
            if underlying != AlarmState::Normal || shelved.is_some() {
                let left = shelved
                    .map(|s| u64::try_from(s.until_utc_ns.saturating_sub(now_utc)).unwrap_or(0));
                restore.push((
                    number,
                    AlarmSnapshot {
                        underlying,
                        shelved_until: left.map(|l| now_mono_ns.saturating_add(l)),
                    },
                ));
            }
            book.entries.insert(a.name.clone(), entry);
        }
        book.dirty = true;
        restore
    }

    /// Records who asks for the next operator action on `id`.
    pub fn annotate(&self, id: &str, annotation: Annotation) {
        self.lock()
            .annotations
            .entry(id.to_owned())
            .or_default()
            .push_back(annotation);
    }

    /// Drops the oldest annotation of `id` (its command was refused).
    pub fn forget(&self, id: &str) {
        if let Some(q) = self.lock().annotations.get_mut(id) {
            q.pop_front();
        }
    }

    /// Applies one event of the cycle thread at `utc_ns` and returns its
    /// journal entry (`AlarmEvent` of ref/09 §5.8, without `seq`).
    pub fn apply(
        &self,
        plan: &Plan,
        shared: &Shared,
        event: &AlarmEvent,
        utc_ns: i64,
    ) -> Option<Value> {
        let config = plan.alarms.get(event.alarm as usize)?;
        let mut book = self.lock();
        let annotation = match event.cause {
            AlarmCause::Ack | AlarmCause::Shelve | AlarmCause::Unshelve => book
                .annotations
                .get_mut(&config.name)
                .and_then(VecDeque::pop_front),
            _ => None,
        };
        let entry = book.entries.get_mut(&config.name)?;
        let before = entry.underlying;
        entry.state = event.to;
        entry.uncertain = event.uncertain;
        entry.underlying = match (event.to, event.cause, before) {
            (AlarmState::Unknown, AlarmCause::Ack, AlarmState::ActiveUnacked) => {
                AlarmState::ActiveAcked
            }
            (AlarmState::Unknown, AlarmCause::Ack, AlarmState::RtnUnacked) => AlarmState::Normal,
            (AlarmState::Unknown, _, b) => b,
            (to, _, _) => to,
        };
        let kind = match event.cause {
            AlarmCause::Condition => "transition",
            AlarmCause::Ack => "ack",
            AlarmCause::Shelve => "shelve",
            AlarmCause::Unshelve => "unshelve",
            AlarmCause::ShelveExpired => "shelve_expired",
        };
        match event.cause {
            AlarmCause::Condition => {
                entry.since = Some(utc_ns);
                if event.to == AlarmState::ActiveUnacked {
                    entry.acked_by = None;
                    entry.acked_at = None;
                }
            }
            AlarmCause::Ack => {
                entry.acked_by = Some(
                    annotation
                        .as_ref()
                        .map_or_else(String::new, |a| a.by.clone()),
                );
                entry.acked_at = Some(utc_ns);
            }
            AlarmCause::Shelve => {
                entry.shelved = Some(Shelved {
                    until_utc_ns: annotation
                        .as_ref()
                        .and_then(|a| a.until_utc_ns)
                        .unwrap_or(utc_ns),
                    by: annotation
                        .as_ref()
                        .map_or_else(String::new, |a| a.by.clone()),
                    reason: annotation
                        .as_ref()
                        .and_then(|a| a.reason.clone())
                        .unwrap_or_default(),
                });
            }
            AlarmCause::Unshelve | AlarmCause::ShelveExpired => entry.shelved = None,
        }
        let message = interpolate(entry.message.as_deref().unwrap_or(&entry.id), plan, shared);
        let mut m = Map::new();
        m.insert("id".into(), json!(entry.id));
        m.insert("ts".into(), json!(timestamp(utc_ns)));
        m.insert("kind".into(), json!(kind));
        m.insert("from".into(), json!(state_name(event.from)));
        m.insert("to".into(), json!(state_name(event.to)));
        m.insert("severity".into(), json!(severity_name(event.severity)));
        if let Some(a) = &annotation {
            m.insert("by".into(), json!(a.by));
            if let Some(r) = &a.reason {
                m.insert("reason".into(), json!(r));
            }
        }
        m.insert("message".into(), json!(message));
        m.insert("uncertain".into(), json!(event.uncertain));
        m.insert("shelved".into(), json!(event.shelved));
        book.dirty = true;
        *book.pending.entry(config.name.clone()).or_default() += 1;
        Some(Value::Object(m))
    }

    /// Makes events visible to [`AlarmBook::wait_change`] once they are
    /// in the journal (ADR 0082): an API answer never precedes them.
    pub fn published(&self, ids: &[String]) {
        let mut book = self.lock();
        for id in ids {
            *book.versions.entry(id.clone()).or_default() += 1;
            if let Some(p) = book.pending.get_mut(id) {
                *p = p.saturating_sub(1);
            }
        }
        drop(book);
        self.changed.notify_all();
    }

    /// Writes the book to `state/alarms` if it changed.
    pub fn persist(&self) -> io::Result<()> {
        let bytes = {
            let mut book = self.lock();
            if !book.dirty {
                return Ok(());
            }
            book.dirty = false;
            let items: Map<String, Value> = book
                .entries
                .values()
                .map(|e| (e.id.clone(), persisted_json(e)))
                .collect();
            serde_json::to_vec(&Value::Object(items)).map_err(io::Error::other)?
        };
        crate::state_file::write_atomic(&self.state_dir, STATE_FILE, &bytes).inspect_err(|_| {
            self.lock().dirty = true;
        })
    }

    /// One alarm.
    pub fn get(&self, id: &str) -> Option<Entry> {
        self.lock().entries.get(id).cloned()
    }

    /// Every alarm, by id.
    pub fn list(&self) -> Vec<Entry> {
        self.lock().entries.values().cloned().collect()
    }

    /// How many events of `id` the book applied, published or not: a
    /// command whose event is published after this point waits for
    /// [`AlarmBook::wait_change`] past it.
    pub fn version(&self, id: &str) -> u64 {
        let book = self.lock();
        book.versions.get(id).copied().unwrap_or(0) + book.pending.get(id).copied().unwrap_or(0)
    }

    /// Waits until `id` has applied an event after `version`, at most
    /// `timeout`.
    pub fn wait_change(&self, id: &str, version: u64, timeout: Duration) -> bool {
        let start = StdInstant::now();
        let mut book = self.lock();
        loop {
            if book.versions.get(id).copied().unwrap_or(0) > version {
                return true;
            }
            let Some(left) = timeout.checked_sub(start.elapsed()) else {
                return false;
            };
            book = self
                .changed
                .wait_timeout(book, left)
                .map(|(g, _)| g)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// Waits for the cycle thread's reply to command `ticket`.
    pub fn wait_reply(
        &self,
        shared: &Shared,
        ticket: u32,
        timeout: Duration,
    ) -> Option<AlarmReply> {
        let start = StdInstant::now();
        loop {
            {
                let mut book = self.lock();
                while let Some(r) = shared.alarm_replies.pop() {
                    book.replies.insert(r.ticket, r);
                }
                if let Some(r) = book.replies.remove(&ticket) {
                    return Some(r);
                }
            }
            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// `message` with each `{<path>}` replaced by the current value of the
/// signal (ref/01 §5.15); `{{` and `}}` are braces, and a path without a
/// usable value reads `?`.
pub fn interpolate(template: &str, plan: &Plan, shared: &Shared) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(after) = tail.strip_prefix("{{") {
            out.push('{');
            rest = after;
            continue;
        }
        if let Some(after) = tail.strip_prefix("}}").or_else(|| tail.strip_prefix('}')) {
            out.push('}');
            rest = after;
            continue;
        }
        let Some(end) = tail.find('}') else {
            out.push_str(tail);
            rest = "";
            break;
        };
        let path = tail[1..end].trim();
        out.push_str(&render(path, plan, shared));
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

fn render(path: &str, plan: &Plan, shared: &Shared) -> String {
    let Some((signal, id)) = plan.signal(path).zip(plan.id(path)) else {
        return "?".into();
    };
    let Some(sample) = shared.slots.get(id.index()).and_then(|s| s.read(8).ok()) else {
        return "?".into();
    };
    if sample.seq() == 0 || sample.quality().is_bad() {
        return "?".into();
    }
    let v = crate::wire::value(signal, sample.transport_bits());
    let text = match &v {
        Value::String(s) => s.clone(),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.to_string(),
            (None, Some(f)) => format!("{f}"),
            _ => n.to_string(),
        },
        other => other.to_string(),
    };
    if signal.unit != "1" && !signal.unit.is_empty() {
        format!("{text} {}", signal.unit)
    } else {
        text
    }
}

/// The `Alarm` of ref/09 §5.8, with the message interpolated now.
pub fn alarm_json(e: &Entry, live: Option<(&Plan, &Shared)>) -> Value {
    let message = match live {
        Some((plan, shared)) => interpolate(e.message.as_deref().unwrap_or(&e.id), plan, shared),
        None => e.message.clone().unwrap_or_else(|| e.id.clone()),
    };
    let mut m = Map::new();
    m.insert("id".into(), json!(e.id));
    m.insert("source".into(), json!(e.source));
    m.insert("severity".into(), json!(severity_name(e.severity)));
    m.insert("state".into(), json!(state_name(e.state)));
    m.insert("active".into(), json!(e.active()));
    m.insert("uncertain".into(), json!(e.uncertain));
    m.insert(
        "shelved".into(),
        e.shelved.as_ref().map_or(
            Value::Null,
            |s| json!({"until": timestamp(s.until_utc_ns), "by": s.by, "reason": s.reason}),
        ),
    );
    if let Some(by) = &e.acked_by {
        m.insert("acked_by".into(), json!(by));
    }
    if let Some(at) = e.acked_at {
        m.insert("acked_at".into(), json!(timestamp(at)));
    }
    if let Some(since) = e.since {
        m.insert("since".into(), json!(timestamp(since)));
    }
    m.insert("message".into(), json!(message));
    if !e.condition.is_empty() {
        m.insert("condition".into(), json!(e.condition));
    }
    if let Some((state, active)) = &e.signals {
        m.insert("signals".into(), json!({"state": state, "active": active}));
    }
    m.insert("trips".into(), json!(e.trips));
    m.insert("max_shelve".into(), json!(duration(e.max_shelve_ns)));
    Value::Object(m)
}

/// The thread of a generation that drains its alarm events into the book,
/// the journal and `state/alarms` (`svc/alarms`).
#[derive(Debug)]
pub struct AlarmPump {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// Where the pump writes.
#[derive(Debug, Clone)]
pub struct PumpTargets {
    /// The book.
    pub book: Arc<AlarmBook>,
    /// The historian's store and the journal's series, if there is one.
    pub journal: Option<(Arc<Store>, SeriesId)>,
    /// The decision log, for a failed write of `state/alarms`.
    pub log: SharedLog,
    /// The generation.
    pub generation: u64,
    /// UTC at the cycle's monotonic zero.
    pub utc_origin_ns: i64,
}

fn drain(plan: &Plan, shared: &Shared, t: &PumpTargets, reported: &mut bool) {
    let mut events = Vec::new();
    while let Some(event) = shared.alarm_events.pop() {
        let utc = t
            .utc_origin_ns
            .saturating_add(i64::try_from(event.at_mono_ns).unwrap_or(i64::MAX));
        if let Some(entry) = t.book.apply(plan, shared, &event, utc) {
            events.push((utc, entry));
        }
    }
    if let Some((store, series)) = &t.journal
        && !events.is_empty()
    {
        let records: Vec<Record> = events
            .iter()
            .map(|(utc, entry)| Record {
                series: *series,
                payload: Payload::Event(Event {
                    ts_ns: *utc,
                    ts_src_ns: None,
                    seq: 0,
                    payload: serde_json::to_vec(entry).unwrap_or_default(),
                }),
            })
            .collect();
        store.append(&records);
    }
    if !events.is_empty() || *reported {
        match t.book.persist() {
            Ok(()) => *reported = false,
            Err(e) if !*reported => {
                *reported = true;
                StateWriteFailure {
                    generation: Some(t.generation),
                    file: "state/alarms",
                    output: None,
                    op: "alarm",
                }
                .report(&t.log, &e);
            }
            Err(_) => {}
        }
    }
    let ids: Vec<String> = events
        .iter()
        .filter_map(|(_, e)| e["id"].as_str().map(str::to_owned))
        .collect();
    t.book.published(&ids);
}

impl AlarmPump {
    /// Starts draining `shared`'s alarm events.
    pub fn spawn(plan: Arc<Plan>, shared: Arc<Shared>, targets: PumpTargets) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("svc/alarms".into())
            .spawn(move || {
                let mut reported = false;
                loop {
                    let stopping = flag.load(Ordering::Acquire);
                    drain(&plan, &shared, &targets, &mut reported);
                    if stopping {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Drains what is left and stops.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for AlarmPump {
    fn drop(&mut self) {
        self.halt();
    }
}
