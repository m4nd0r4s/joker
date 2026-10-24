//! WebSocket and Server-Sent Events subscriptions (ref/09 §6): live push
//! over the same [`Backend`](crate::Backend) reads the REST endpoints
//! serve. Nothing here reaches an `rt` thread.
//!
//! State topics (`signals`, `outputs`, `status`, `metrics`) are sampled at
//! each subscription's `min_interval` and, in `mode = change`, send only the
//! items that changed; a state subscription never queues a second message
//! while its previous one is still in the output queue, so a slow client
//! only receives less often (§6.6 point 2). `decisions` is an event topic:
//! new decisions go to a ring of `buffer` items, and what the ring evicts is
//! a `gap` and counts in `api.dropped` (§6.6 point 3, invariant 7).
//!
//! The output queue of a connection counts bytes (1 MiB). A queue that stays
//! full for 30 s closes the connection with `closing` and a close frame 4008
//! (ADR 0063); the writer task gives up on a socket that does not drain, so
//! the close never waits behind it.
//!
//! Every subscription reserves its sampling rate against the node budget,
//! and an `outputs` subscription samples no faster than its fastest group
//! (§6.5, §6.6 point 5). A `signals` or `outputs` subscription whose
//! selectors resolve to another list sends `resolved` before the new paths,
//! and the role of a connection is recomputed when the generation changes
//! (§6.9). When a connection ends its session is parked with a replay
//! buffer per subscription, and `resume` (or `Last-Event-ID` on SSE) takes
//! it over within the resume window (§6.7, ADR 0074).
//!
//! **Not built:** the topics without a data source in this build (refused
//! with `J-E1902`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::Extension;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::backend::{Principal, Query};
use crate::id::ulid;
use crate::problem::Problem;
use crate::server::{API_VERSION, Peer, Shared, parse_query, principal, role_rank, role_with};

/// The limits of the subscription engine (ref/09 §3.11, §6.6, §6.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes the output queue of a connection holds (1 MiB).
    pub outbox_bytes: usize,
    /// How long the output queue may stay full before the 4008 close (30 s).
    pub slow_client_grace: Duration,
    /// How long the server waits for `closing` and the close frame to leave (5 s).
    pub close_timeout: Duration,
    /// How often the server pings (15 s).
    pub heartbeat: Duration,
    /// How long a client may stay silent before the 4002 close (45 s).
    pub heartbeat_miss: Duration,
    /// Subscriptions per connection (64).
    pub max_subscriptions: usize,
    /// The sampling budget of the node, in samples per second (1,000,000).
    pub sampling_budget: u64,
    /// How long a dropped session stays resumable (60 s).
    pub resume_window: Duration,
    /// Sessions the node keeps for resumption (16, ADR 0074).
    pub parked_sessions: usize,
    /// Messages kept per subscription for a replay (256).
    pub replay_messages: usize,
    /// Bytes of replay kept per session (1 MiB).
    pub replay_bytes: usize,
    /// How often a connection looks for a generation swap (1 s).
    pub generation_check: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            outbox_bytes: 1 << 20,
            slow_client_grace: Duration::from_secs(30),
            close_timeout: Duration::from_secs(5),
            heartbeat: Duration::from_secs(15),
            heartbeat_miss: Duration::from_secs(45),
            max_subscriptions: 64,
            sampling_budget: 1_000_000,
            resume_window: Duration::from_secs(60),
            parked_sessions: 16,
            replay_messages: 256,
            replay_bytes: 1 << 20,
            generation_check: Duration::from_secs(1),
        }
    }
}

/// The counters of the subscription engine (`Metrics.api`, ref/09 §5.2).
#[derive(Debug, Default)]
pub struct ApiStats {
    connections: AtomicU64,
    subscriptions: AtomicU64,
    sampled: AtomicU64,
    dropped: AtomicU64,
}

impl ApiStats {
    /// The `api` object of `Metrics`: open connections, live
    /// subscriptions, the samples per second they reserve and the items
    /// dropped from event rings (`joker.api.dropped`).
    pub fn json(&self) -> Value {
        json!({
            "connections": self.connections.load(Ordering::Relaxed),
            "subscriptions": self.subscriptions.load(Ordering::Relaxed),
            "sampled_per_s": self.sampled.load(Ordering::Relaxed),
            "dropped": self.dropped.load(Ordering::Relaxed),
        })
    }

    /// Reserves `rate` samples/s if it fits in `budget`; otherwise the
    /// rate that still fits.
    fn reserve(&self, rate: u64, budget: u64) -> Result<(), u64> {
        self.sampled
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(rate).filter(|n| *n <= budget)
            })
            .map(|_| ())
            .map_err(|used| budget.saturating_sub(used))
    }

    fn take(&self, rate: u64) {
        self.sampled.fetch_add(rate, Ordering::AcqRel);
    }

    fn release(&self, rate: u64) {
        let _ = self
            .sampled
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                Some(used.saturating_sub(rate))
            });
    }
}

/// A session kept after its connection ended, for resumption (§6.7).
struct Parked {
    id: String,
    who: Principal,
    subs: BTreeMap<String, Subscription>,
    at: Instant,
}

/// What the subscription engine needs besides the backend.
#[derive(Clone, Default)]
pub struct Engine {
    /// Its limits.
    pub limits: Limits,
    /// Its counters.
    pub stats: Arc<ApiStats>,
    parked: Arc<Mutex<VecDeque<Parked>>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("limits", &self.limits)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// An engine with `limits` and fresh counters.
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            ..Self::default()
        }
    }

    fn store(&self) -> MutexGuard<'_, VecDeque<Parked>> {
        let mut store = match self.parked.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let window = self.limits.resume_window;
        store.retain(|p| p.at.elapsed() < window);
        store
    }

    /// Keeps `parked` for the resume window, dropping the oldest session
    /// beyond the limit (ADR 0074).
    fn park(&self, parked: Parked) {
        if self.limits.parked_sessions == 0 || parked.subs.is_empty() {
            return;
        }
        let mut store = self.store();
        while store.len() >= self.limits.parked_sessions {
            store.pop_front();
        }
        store.push_back(parked);
    }

    /// Takes the parked session `id` of `who`, if it is still there.
    fn unpark(&self, id: &str, who: &Principal) -> Option<Parked> {
        let mut store = self.store();
        let i = store.iter().position(|p| p.id == id && p.who == *who)?;
        store.remove(i)
    }
}

struct ConnectionGuard(Arc<ApiStats>);

impl ConnectionGuard {
    fn new(stats: &Arc<ApiStats>) -> Self {
        stats.connections.fetch_add(1, Ordering::Relaxed);
        Self(Arc::clone(stats))
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::Relaxed);
    }
}

const MINIMUM_INTERVAL: Duration = Duration::from_millis(10);
const SNAPSHOT_MINIMUM: Duration = Duration::from_secs(1);
const SAMPLE_GRANULARITY: Duration = Duration::from_millis(10);
const DEFAULT_BUFFER: usize = 1024;
const MAX_CLIENT_MESSAGE: usize = 64 * 1024;
const MAX_ERRORS_PER_MINUTE: usize = 10;
const DECISIONS_READ: &str = "1000";
const VOLATILE: [&str; 2] = ["age", "remaining"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Topic {
    Signals,
    Outputs,
    Status,
    Metrics,
    Decisions,
}

impl Topic {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "signals" => Some(Self::Signals),
            "outputs" => Some(Self::Outputs),
            "status" => Some(Self::Status),
            "metrics" => Some(Self::Metrics),
            "decisions" => Some(Self::Decisions),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Signals => "signals",
            Self::Outputs => "outputs",
            Self::Status => "status",
            Self::Metrics => "metrics",
            Self::Decisions => "decisions",
        }
    }

    fn role(self) -> &'static str {
        "viewer"
    }

    fn is_event(self) -> bool {
        self == Self::Decisions
    }

    fn default_interval(self) -> Duration {
        match self {
            Self::Status | Self::Metrics => SNAPSHOT_MINIMUM,
            Self::Decisions => Duration::from_millis(200),
            Self::Signals | Self::Outputs => Duration::from_millis(100),
        }
    }

    fn minimum_interval(self) -> Duration {
        match self {
            Self::Status | Self::Metrics => SNAPSHOT_MINIMUM,
            _ => MINIMUM_INTERVAL,
        }
    }
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn now_rfc3339() -> String {
    humantime::format_rfc3339_millis(std::time::SystemTime::now()).to_string()
}

fn new_id() -> String {
    ulid(now_nanos())
}

fn format_interval(d: Duration) -> String {
    joker_units::Duration::from_nanos(u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)).to_string()
}

fn parse_interval(topic: Topic, text: Option<&str>) -> Result<Option<Duration>, Problem> {
    let Some(text) = text else {
        return Ok(None);
    };
    let d = joker_units::Duration::parse(text)
        .map_err(|e| Problem::new("J-E1012", 400, format!("min_interval: {e}")))?;
    let d = Duration::from_nanos(d.as_nanos());
    let minimum = topic.minimum_interval();
    if d < minimum {
        return Err(Problem::new(
            "J-E1905",
            400,
            format!(
                "min_interval of `{}` below {}",
                topic.name(),
                format_interval(minimum)
            ),
        ));
    }
    Ok(Some(d))
}

/// The samples per second of `items` read every `interval`, rounded up
/// (ADR 0074).
fn rate_of(items: usize, interval: Duration) -> u64 {
    let items = u128::try_from(items.max(1)).unwrap_or(u128::MAX);
    let nanos = interval.as_nanos().max(1);
    u64::try_from(items.saturating_mul(1_000_000_000).div_ceil(nanos)).unwrap_or(u64::MAX)
}

/// The `GenerationRef` of a `Joker-Generation` header value.
fn generation_ref(header: Option<&str>) -> Value {
    header
        .and_then(|h| h.split_once(' '))
        .and_then(|(n, hash)| Some(json!({"gen": n.parse::<u64>().ok()?, "hash": hash})))
        .unwrap_or(Value::Null)
}

/// A message on its way to the client.
enum Out {
    Text {
        text: Arc<str>,
        release: Option<Arc<AtomicBool>>,
    },
    Ping,
}

/// The byte-bounded output queue of one connection (§6.6, ADR 0063).
struct Outbox {
    tx: mpsc::UnboundedSender<Out>,
    bytes: Arc<AtomicUsize>,
    capacity: usize,
    refused: usize,
    last_text: Instant,
}

impl Outbox {
    fn new(capacity: usize) -> (Self, mpsc::UnboundedReceiver<Out>, Arc<AtomicUsize>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let bytes = Arc::new(AtomicUsize::new(0));
        (
            Self {
                tx,
                bytes: Arc::clone(&bytes),
                capacity,
                refused: 0,
                last_text: Instant::now(),
            },
            rx,
            bytes,
        )
    }

    /// Queues `value` if it fits; a message larger than the queue still
    /// goes alone into an empty queue. `release` is set until the message
    /// leaves the queue.
    fn push(&mut self, value: &Value, release: Option<&Arc<AtomicBool>>) -> bool {
        self.push_text(Arc::from(value.to_string()), release, false)
    }

    /// Queues `text`; with `force`, even when it does not fit (a replay,
    /// bounded by the replay buffer).
    fn push_text(
        &mut self,
        text: Arc<str>,
        release: Option<&Arc<AtomicBool>>,
        force: bool,
    ) -> bool {
        let n = text.len();
        let held = self.bytes.load(Ordering::Acquire);
        if !force && held > 0 && held.saturating_add(n) > self.capacity {
            self.refused = self.refused.max(n);
            return false;
        }
        self.bytes.fetch_add(n, Ordering::AcqRel);
        if let Some(flag) = release {
            flag.store(true, Ordering::Release);
        }
        let out = Out::Text {
            text,
            release: release.cloned(),
        };
        if self.tx.send(out).is_err() {
            return false;
        }
        self.last_text = Instant::now();
        true
    }

    fn ping(&self) {
        let _ = self.tx.send(Out::Ping);
    }

    /// Full from a refused message until that message would fit (ADR 0063).
    fn is_full(&mut self) -> bool {
        if self.refused == 0 {
            return false;
        }
        let held = self.bytes.load(Ordering::Acquire);
        if held == 0 || held.saturating_add(self.refused) <= self.capacity {
            self.refused = 0;
            return false;
        }
        true
    }
}

fn delivered(out: &Out, bytes: &AtomicUsize) {
    if let Out::Text { text, release } = out {
        bytes.fetch_sub(text.len(), Ordering::AcqRel);
        if let Some(flag) = release {
            flag.store(false, Ordering::Release);
        }
    }
}

struct Subscription {
    topic: Topic,
    sel: Vec<String>,
    interval: Duration,
    next_due: Instant,
    every: bool,
    seq: u64,
    resync: bool,
    last: BTreeMap<String, Value>,
    last_id: Option<String>,
    ring: VecDeque<Value>,
    buffer: usize,
    lost: u64,
    in_flight: Arc<AtomicBool>,
    rate: u64,
    paths: Option<BTreeSet<String>>,
    replay: VecDeque<Replayed>,
}

/// A `data` or `resync` message kept for a replay (§6.7).
struct Replayed {
    seq: u64,
    stamp: u64,
    text: Arc<str>,
}

impl Subscription {
    fn busy(&self) -> bool {
        self.in_flight.load(Ordering::Acquire)
    }

    fn replay_bytes(&self) -> usize {
        self.replay.iter().map(|r| r.text.len()).sum()
    }

    /// Whether `paths` (of `items`) are the ones this subscription resolved.
    fn same_paths(&self, items: &[Value]) -> bool {
        let Some(paths) = &self.paths else {
            return true;
        };
        items.len() == paths.len()
            && items.iter().all(|i| {
                i.get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|p| paths.contains(p))
            })
    }
}

fn paths_of(items: &[Value]) -> BTreeSet<String> {
    items
        .iter()
        .filter_map(|i| i.get("path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn item_key(topic: Topic, item: &Value, index: usize) -> String {
    let field = match topic {
        Topic::Signals | Topic::Outputs => item.get("path"),
        Topic::Decisions => item.get("id"),
        Topic::Status | Topic::Metrics => None,
    };
    field
        .and_then(Value::as_str)
        .map_or_else(|| index.to_string(), str::to_owned)
}

fn stable(value: &Value) -> Value {
    match value {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| !VOLATILE.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), stable(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(stable).collect()),
        other => other.clone(),
    }
}

fn decision_id(item: &Value) -> Option<&str> {
    item.get("id").and_then(Value::as_str)
}

/// Reads `topic`'s current items through the backend, the same call a REST
/// read makes (`spawn_blocking`: a backend call may block).
async fn read_topic(
    backend: &Shared,
    engine: &Engine,
    topic: Topic,
    sel: &[String],
) -> Result<Vec<Value>, Problem> {
    let b = Arc::clone(backend);
    let stats = Arc::clone(&engine.stats);
    let sel = sel.to_vec();
    tokio::task::spawn_blocking(move || {
        let selectors =
            |key: &str| -> Query { sel.iter().map(|s| (key.to_owned(), s.clone())).collect() };
        let list = |body: Value| match body.get("items") {
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        };
        match topic {
            Topic::Signals => b.values(&selectors("sel")).map(|r| list(r.body)),
            Topic::Outputs => b.outputs(&selectors("sel")).map(|r| list(r.body)),
            Topic::Status => b.status().map(|r| vec![r.body]),
            Topic::Metrics => b.metrics().map(|r| vec![with_api(r.body, &stats)]),
            Topic::Decisions => {
                let mut q = selectors("output");
                q.push(("limit".into(), DECISIONS_READ.into()));
                b.decisions(&q).map(|r| list(r.body))
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err(Problem::new("J-E1000", 500, "the handler panicked")))
}

/// `Metrics` with the `api` object of the subscription engine.
pub(crate) fn with_api(mut metrics: Value, stats: &ApiStats) -> Value {
    if let Value::Object(m) = &mut metrics {
        m.insert("api".into(), stats.json());
    }
    metrics
}

/// The subscriptions of one connection and the messages they produce.
struct Session {
    backend: Shared,
    engine: Engine,
    who: Principal,
    role: &'static str,
    id: String,
    subs: BTreeMap<String, Subscription>,
    outbox: Outbox,
    replay_bytes: usize,
    stamp: u64,
    generation: Option<String>,
    checked: Instant,
}

impl Drop for Session {
    fn drop(&mut self) {
        for sub in self.subs.values() {
            self.engine.release(sub);
        }
    }
}

impl Engine {
    fn release(&self, sub: &Subscription) {
        self.stats.subscriptions.fetch_sub(1, Ordering::Relaxed);
        self.stats.release(sub.rate);
    }
}

#[derive(Debug, Default)]
struct SubscribeOptions {
    sel: Vec<String>,
    min_interval: Option<String>,
    mode: Option<String>,
    initial: bool,
    buffer: Option<usize>,
}

/// What follows `resumed` for one subscription.
enum Follow {
    Replay(String, u64),
    Gap(String, u64),
}

impl Session {
    fn new(
        backend: Shared,
        engine: &Engine,
        who: Principal,
        role: &'static str,
    ) -> (Self, mpsc::UnboundedReceiver<Out>, Arc<AtomicUsize>) {
        let (outbox, rx, bytes) = Outbox::new(engine.limits.outbox_bytes);
        (
            Self {
                backend,
                engine: engine.clone(),
                who,
                role,
                id: new_id(),
                subs: BTreeMap::new(),
                outbox,
                replay_bytes: 0,
                stamp: 0,
                generation: None,
                checked: Instant::now(),
            },
            rx,
            bytes,
        )
    }

    /// The shortest period among the groups that own `items` (outputs),
    /// and never less than 10 ms (ADR 0074).
    async fn outputs_floor(&self, items: &[Value]) -> Duration {
        let groups: BTreeSet<String> = items
            .iter()
            .filter_map(|i| i.get("group").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        let b = Arc::clone(&self.backend);
        let status = tokio::task::spawn_blocking(move || b.status())
            .await
            .ok()
            .and_then(Result::ok);
        let shortest = status
            .as_ref()
            .and_then(|r| r.body.get("groups"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|g| {
                g.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| groups.contains(n))
            })
            .filter_map(|g| g.get("period").and_then(Value::as_str))
            .filter_map(|p| joker_units::Duration::parse(p).ok())
            .map(|d| Duration::from_nanos(d.as_nanos()))
            .min();
        shortest.map_or(MINIMUM_INTERVAL, |d| d.max(MINIMUM_INTERVAL))
    }

    /// Accepts a subscription: queues `subscribed` and, with `initial`, its
    /// first `resync`. Returns the new subscription's id.
    async fn subscribe(
        &mut self,
        id: Option<&str>,
        topic_name: &str,
        options: SubscribeOptions,
    ) -> Result<String, Problem> {
        let topic = Topic::parse(topic_name)
            .ok_or_else(|| Problem::new("J-E1902", 400, format!("unknown topic `{topic_name}`")))?;
        if role_rank(self.role) < role_rank(topic.role()) {
            return Err(Problem::new(
                "J-E1002",
                403,
                format!("`{}` needs the role `{}`", topic.name(), topic.role()),
            ));
        }
        if self.subs.len() >= self.engine.limits.max_subscriptions {
            return Err(Problem::new(
                "J-E1903",
                400,
                format!(
                    "at most {} subscriptions per connection",
                    self.engine.limits.max_subscriptions
                ),
            ));
        }
        let asked = parse_interval(topic, options.min_interval.as_deref())?;
        let every = match options.mode.as_deref() {
            None | Some("change") => false,
            Some("interval") => true,
            Some(other) => {
                return Err(Problem::new(
                    "J-E1012",
                    400,
                    format!("unknown mode `{other}`"),
                ));
            }
        };
        let items = read_topic(&self.backend, &self.engine, topic, &options.sel).await?;
        let floor = if topic == Topic::Outputs {
            self.outputs_floor(&items).await
        } else {
            topic.minimum_interval()
        };
        let interval = match asked {
            Some(d) if d < floor => {
                return Err(Problem::new(
                    "J-E1905",
                    400,
                    format!(
                        "min_interval of `{}` below {}, the period of the fastest group it reads",
                        topic.name(),
                        format_interval(floor)
                    ),
                ));
            }
            Some(d) => d,
            None => topic.default_interval().max(floor),
        };
        let resolved = if topic.is_event() { 1 } else { items.len() };
        let rate = rate_of(resolved, interval);
        let budget = self.engine.limits.sampling_budget;
        self.engine.stats.reserve(rate, budget).map_err(|fits| {
            Problem::new(
                "J-E1903",
                400,
                format!(
                    "the sampling budget of the node is exhausted: this subscription needs {rate} samples/s and {fits} still fit (of {budget})"
                ),
            )
        })?;
        self.engine
            .stats
            .subscriptions
            .fetch_add(1, Ordering::Relaxed);
        let sub_id = new_id();
        let paths = matches!(topic, Topic::Signals | Topic::Outputs).then(|| paths_of(&items));
        let mut sub = Subscription {
            topic,
            sel: options.sel,
            interval,
            next_due: Instant::now() + interval,
            every,
            seq: 0,
            resync: options.initial,
            last: BTreeMap::new(),
            last_id: None,
            ring: VecDeque::new(),
            buffer: options.buffer.unwrap_or(DEFAULT_BUFFER).clamp(64, 4096),
            lost: 0,
            in_flight: Arc::new(AtomicBool::new(false)),
            rate,
            paths,
            replay: VecDeque::new(),
        };
        if topic.is_event() {
            sub.last_id = items
                .iter()
                .filter_map(decision_id)
                .max()
                .map(str::to_owned);
        }
        self.outbox.push(
            &json!({
                "op": "subscribed",
                "id": id,
                "sub": sub_id,
                "topic": topic.name(),
                "resolved": items.len(),
                "min_interval": format_interval(interval),
            }),
            None,
        );
        if sub.resync {
            self.send_resync(&sub_id, &mut sub, items);
        }
        self.subs.insert(sub_id.clone(), sub);
        self.trim_replay();
        Ok(sub_id)
    }

    fn unsubscribe(&mut self, sub: &str) {
        if let Some(s) = self.subs.remove(sub) {
            self.remove(&s);
        }
    }

    fn remove(&mut self, sub: &Subscription) {
        self.engine.release(sub);
        self.replay_bytes = self.replay_bytes.saturating_sub(sub.replay_bytes());
    }

    /// Queues a `data` or `resync` message of `sub` with its next `seq`,
    /// and keeps it for a replay.
    fn emit(&mut self, sub: &mut Subscription, msg: &Value) -> bool {
        let text: Arc<str> = Arc::from(msg.to_string());
        if !self
            .outbox
            .push_text(Arc::clone(&text), Some(&sub.in_flight), false)
        {
            return false;
        }
        sub.seq += 1;
        self.stamp += 1;
        self.replay_bytes += text.len();
        sub.replay.push_back(Replayed {
            seq: sub.seq,
            stamp: self.stamp,
            text,
        });
        if sub.replay.len() > self.engine.limits.replay_messages
            && let Some(r) = sub.replay.pop_front()
        {
            self.replay_bytes = self.replay_bytes.saturating_sub(r.text.len());
        }
        true
    }

    /// Drops the oldest kept messages of the session beyond its replay
    /// bytes (ADR 0074).
    fn trim_replay(&mut self) {
        while self.replay_bytes > self.engine.limits.replay_bytes {
            let oldest = self
                .subs
                .values_mut()
                .filter(|s| !s.replay.is_empty())
                .min_by_key(|s| s.replay.front().map_or(u64::MAX, |r| r.stamp));
            let Some(r) = oldest.and_then(|s| s.replay.pop_front()) else {
                self.replay_bytes = 0;
                return;
            };
            self.replay_bytes = self.replay_bytes.saturating_sub(r.text.len());
        }
    }

    fn send_resync(&mut self, sub_id: &str, sub: &mut Subscription, items: Vec<Value>) {
        if sub.busy() {
            return;
        }
        let msg = json!({
            "op": "resync",
            "sub": sub_id,
            "seq": sub.seq + 1,
            "items": items,
        });
        if self.emit(sub, &msg) {
            sub.resync = false;
            if !sub.topic.is_event() {
                sub.last = items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| (item_key(sub.topic, item, i), stable(item)))
                    .collect();
            }
        }
    }

    async fn sample_due(&mut self) {
        let now = Instant::now();
        let due: Vec<String> = self
            .subs
            .iter()
            .filter(|(_, s)| s.next_due <= now && (s.topic.is_event() || !s.busy()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            let Some(mut sub) = self.subs.remove(&id) else {
                continue;
            };
            sub.next_due = now + sub.interval;
            if let Ok(items) = read_topic(&self.backend, &self.engine, sub.topic, &sub.sel).await
                && (sub.same_paths(&items) || self.re_resolve(&id, &mut sub, &items).await)
            {
                if sub.topic.is_event() {
                    self.sample_event(&id, &mut sub, items);
                } else if sub.resync {
                    self.send_resync(&id, &mut sub, items);
                } else {
                    self.sample_state(&id, &mut sub, items);
                }
            }
            self.subs.insert(id, sub);
        }
        self.trim_replay();
    }

    /// The selectors of `sub` resolve to the paths of `items` now: sends
    /// `resolved` before anything with an added path, forgets the removed
    /// ones and updates the reservation (§6.9, ADR 0074). `false` when the
    /// notice did not fit in the queue; the next interval tries again.
    async fn re_resolve(&mut self, sub_id: &str, sub: &mut Subscription, items: &[Value]) -> bool {
        let now = paths_of(items);
        let old = sub.paths.take().unwrap_or_default();
        let added: Vec<&String> = now.difference(&old).collect();
        let removed: Vec<&String> = old.difference(&now).collect();
        let b = Arc::clone(&self.backend);
        let header = tokio::task::spawn_blocking(move || b.headers().generation)
            .await
            .ok()
            .flatten();
        let msg = json!({
            "op": "resolved",
            "sub": sub_id,
            "generation": generation_ref(header.as_deref()),
            "added": added,
            "removed": removed,
        });
        if !self.outbox.push(&msg, None) {
            sub.paths = Some(old);
            return false;
        }
        for path in &removed {
            sub.last.remove(path.as_str());
        }
        let rate = rate_of(now.len(), sub.interval);
        self.engine.stats.take(rate);
        self.engine.stats.release(sub.rate);
        sub.rate = rate;
        sub.paths = Some(now);
        true
    }

    /// Recomputes the role when the running generation changed, ending the
    /// subscriptions the new role cannot keep (§6.9, ADR 0074).
    async fn check_generation(&mut self) {
        if self.checked.elapsed() < self.engine.limits.generation_check {
            return;
        }
        self.checked = Instant::now();
        let b = Arc::clone(&self.backend);
        let who = self.who.clone();
        let Ok((generation, role)) = tokio::task::spawn_blocking(move || {
            (b.headers().generation, role_with(&who, &b.socket_roles()))
        })
        .await
        else {
            return;
        };
        if generation == self.generation {
            return;
        }
        self.generation = generation;
        self.role = role;
        let lost: Vec<String> = self
            .subs
            .iter()
            .filter(|(_, s)| role_rank(role) < role_rank(s.topic.role()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in lost {
            let Some(sub) = self.subs.remove(&id) else {
                continue;
            };
            self.remove(&sub);
            self.outbox.push(
                &json!({"op": "gap", "sub": id, "lost": 0, "reason": "role_changed", "ts": now_rfc3339()}),
                None,
            );
            let p = Problem::new(
                "J-E1002",
                403,
                format!(
                    "`{}` needs the role `{}`",
                    sub.topic.name(),
                    sub.topic.role()
                ),
            );
            self.outbox.push(
                &json!({"op": "error", "id": null, "sub": id, "problem": p.body(None)}),
                None,
            );
        }
    }

    fn sample_state(&mut self, sub_id: &str, sub: &mut Subscription, items: Vec<Value>) {
        let mut changed = Vec::new();
        let mut seen = Vec::new();
        for (i, item) in items.into_iter().enumerate() {
            let key = item_key(sub.topic, &item, i);
            let now = stable(&item);
            if sub.every || sub.last.get(&key) != Some(&now) {
                changed.push(item);
                seen.push((key, now));
            }
        }
        if changed.is_empty() {
            return;
        }
        let msg = json!({
            "op": "data",
            "sub": sub_id,
            "seq": sub.seq + 1,
            "items": changed,
        });
        if self.emit(sub, &msg) {
            sub.last.extend(seen);
        }
    }

    fn sample_event(&mut self, sub_id: &str, sub: &mut Subscription, items: Vec<Value>) {
        let mut fresh: Vec<Value> = items
            .into_iter()
            .filter(|i| {
                decision_id(i).is_some_and(|id| sub.last_id.as_deref().is_none_or(|last| id > last))
            })
            .collect();
        fresh.sort_by(|a, b| decision_id(a).cmp(&decision_id(b)));
        if let Some(id) = fresh.last().and_then(decision_id) {
            sub.last_id = Some(id.to_owned());
        }
        for item in fresh {
            sub.ring.push_back(item);
            if sub.ring.len() > sub.buffer {
                sub.ring.pop_front();
                sub.lost += 1;
                self.engine.stats.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        if sub.lost > 0 {
            let gap = json!({
                "op": "gap",
                "sub": sub_id,
                "lost": sub.lost,
                "reason": "overflow",
                "ts": now_rfc3339(),
            });
            if !self.outbox.push(&gap, None) {
                return;
            }
            sub.lost = 0;
        }
        if sub.ring.is_empty() || sub.busy() {
            return;
        }
        let items: Vec<Value> = sub.ring.iter().cloned().collect();
        let msg = json!({
            "op": "data",
            "sub": sub_id,
            "seq": sub.seq + 1,
            "items": items,
        });
        if self.emit(sub, &msg) {
            sub.ring.clear();
        }
    }

    /// Parks this session for resumption (§6.7, ADR 0074).
    fn park(mut self) {
        let subs = std::mem::take(&mut self.subs);
        for sub in subs.values() {
            self.engine.release(sub);
        }
        self.engine.park(Parked {
            id: self.id.clone(),
            who: self.who.clone(),
            subs,
            at: Instant::now(),
        });
    }

    /// Takes over `parked`: answers `resumed` and replays, resyncs or
    /// declares the gap of each subscription in `wanted` (ADR 0074).
    fn resume(&mut self, id: Option<&str>, parked: Parked, wanted: Vec<(String, u64)>) {
        let mut kept = parked.subs;
        self.id = parked.id;
        let mut results = Vec::with_capacity(wanted.len());
        let mut follow = Vec::with_capacity(wanted.len());
        for (sub_id, last_seq) in wanted {
            let Some(mut sub) = kept.remove(&sub_id) else {
                results.push(json!({"sub": sub_id, "result": "unknown"}));
                continue;
            };
            sub.in_flight = Arc::new(AtomicBool::new(false));
            sub.next_due = Instant::now();
            self.engine
                .stats
                .subscriptions
                .fetch_add(1, Ordering::Relaxed);
            self.engine.stats.take(sub.rate);
            let covered = last_seq == sub.seq
                || (last_seq < sub.seq
                    && sub
                        .replay
                        .front()
                        .is_some_and(|r| r.seq <= last_seq.saturating_add(1)));
            if covered {
                results.push(json!({"sub": sub_id, "result": "replayed"}));
                follow.push(Follow::Replay(sub_id.clone(), last_seq));
            } else {
                results.push(json!({"sub": sub_id, "result": "resync"}));
                if sub.topic.is_event() {
                    follow.push(Follow::Gap(
                        sub_id.clone(),
                        sub.seq.saturating_sub(last_seq),
                    ));
                } else {
                    sub.resync = true;
                }
            }
            self.subs.insert(sub_id, sub);
        }
        self.replay_bytes = self.subs.values().map(Subscription::replay_bytes).sum();
        self.outbox.push(
            &json!({"op": "resumed", "id": id, "session": self.id, "subs": results}),
            None,
        );
        for f in follow {
            match f {
                Follow::Replay(sub_id, last) => {
                    if let Some(sub) = self.subs.get(&sub_id) {
                        for r in sub.replay.iter().filter(|r| r.seq > last) {
                            self.outbox.push_text(Arc::clone(&r.text), None, true);
                        }
                    }
                }
                Follow::Gap(sub_id, lost) => {
                    self.outbox.push(
                        &json!({"op": "gap", "sub": sub_id, "lost": lost, "reason": "resume_unavailable", "ts": now_rfc3339()}),
                        None,
                    );
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ClientMsg {
    Subscribe {
        id: String,
        topic: String,
        #[serde(default)]
        sel: Vec<String>,
        min_interval: Option<String>,
        mode: Option<String>,
        #[serde(default = "default_true")]
        initial: bool,
        buffer: Option<usize>,
    },
    Unsubscribe {
        #[serde(default)]
        id: Option<String>,
        sub: String,
    },
    Resume {
        #[serde(default)]
        id: Option<String>,
        session: String,
        #[serde(default)]
        subs: Vec<ResumeSub>,
    },
    Ping {
        #[serde(default)]
        id: Option<String>,
    },
    List {
        #[serde(default)]
        id: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct ResumeSub {
    sub: String,
    last_seq: u64,
}

fn default_true() -> bool {
    true
}

fn error_json(id: Option<&str>, p: &Problem) -> Value {
    json!({"op": "error", "id": id, "sub": null, "problem": p.body(None)})
}

fn problem_response(p: &Problem) -> Response {
    let status = axum::http::StatusCode::from_u16(p.status)
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    (status, axum::Json(p.body(None))).into_response()
}

/// Why the server ends a connection (ref/09 §6.10).
struct Closing {
    code: u16,
    problem: Option<Problem>,
}

impl Closing {
    fn with(code: u16, problem: &str, detail: &str) -> Self {
        Self {
            code,
            problem: Some(Problem::new(problem, 400, detail)),
        }
    }
}

/// `GET /v1/ws` (ref/09 §6.1): the subprotocol must be `joker.v1`.
pub async fn ws_handler(
    State(backend): State<Shared>,
    Extension(engine): Extension<Engine>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    ws: WebSocketUpgrade,
) -> Response {
    let who = principal(peer);
    ws.protocols(["joker.v1"])
        .max_message_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| handle_socket(socket, backend, engine, who))
}

/// Drains the output queue into the socket. A close preempts a send that
/// the socket does not take, so a client that stops reading cannot hold
/// the close back (ref/09 §6.6 point 4).
async fn writer(
    mut sink: SplitSink<WebSocket, Message>,
    mut rx: mpsc::UnboundedReceiver<Out>,
    bytes: Arc<AtomicUsize>,
    mut close: oneshot::Receiver<Closing>,
    close_timeout: Duration,
) {
    loop {
        let out = tokio::select! {
            biased;
            c = &mut close => return finish(&mut sink, c.ok(), close_timeout).await,
            o = rx.recv() => match o {
                Some(o) => o,
                None => return,
            },
        };
        let msg = match &out {
            Out::Text { text, .. } => Message::Text(String::from(&**text).into()),
            Out::Ping => Message::Ping(Vec::new().into()),
        };
        tokio::select! {
            biased;
            c = &mut close => return finish(&mut sink, c.ok(), close_timeout).await,
            r = sink.send(msg) => {
                if r.is_err() {
                    return;
                }
                delivered(&out, &bytes);
            }
        }
    }
}

async fn finish(
    sink: &mut SplitSink<WebSocket, Message>,
    closing: Option<Closing>,
    close_timeout: Duration,
) {
    let Some(closing) = closing else {
        return;
    };
    let _ = tokio::time::timeout(close_timeout, async {
        if let Some(p) = &closing.problem {
            let msg = json!({"op": "closing", "code": closing.code, "problem": p.body(None)});
            sink.send(Message::Text(msg.to_string().into())).await?;
        }
        let reason = closing.problem.as_ref().map_or("", |p| p.code.as_str());
        sink.send(Message::Close(Some(CloseFrame {
            code: closing.code,
            reason: reason.into(),
        })))
        .await
    })
    .await;
}

async fn handle_socket(socket: WebSocket, backend: Shared, engine: Engine, who: Principal) {
    let _guard = ConnectionGuard::new(&engine.stats);
    let limits = engine.limits;
    let role = role_with(&who, &backend.socket_roles());
    let (mut session, rx, bytes) = Session::new(Arc::clone(&backend), &engine, who.clone(), role);
    let (sink, mut stream) = socket.split();
    let (close_tx, close_rx) = oneshot::channel::<Closing>();
    let mut writer_task = tokio::spawn(writer(sink, rx, bytes, close_rx, limits.close_timeout));

    let info = {
        let b = Arc::clone(&backend);
        let who = who.clone();
        tokio::task::spawn_blocking(move || b.info(&who))
            .await
            .ok()
            .and_then(Result::ok)
    };
    let (boot_id, generation) = info
        .map(|r| {
            (
                r.body["runtime"]["boot_id"].clone(),
                r.body["generation"].clone(),
            )
        })
        .unwrap_or((Value::Null, Value::Null));
    let welcome = json!({
        "op": "welcome",
        "session": session.id,
        "boot_id": boot_id,
        "api": API_VERSION,
        "role": role,
        "generation": generation,
        "heartbeat": format_interval(limits.heartbeat),
        "resume_window": format_interval(limits.resume_window),
        "limits": {"max_subscriptions": limits.max_subscriptions},
    });
    session.outbox.push(&welcome, None);

    let mut last_client = Instant::now();
    let mut full_since: Option<Instant> = None;
    let mut errors: VecDeque<Instant> = VecDeque::new();
    let mut sample_tick = tokio::time::interval(SAMPLE_GRANULARITY);
    sample_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut heartbeat_tick =
        tokio::time::interval_at(Instant::now() + limits.heartbeat, limits.heartbeat);

    let closing = loop {
        tokio::select! {
            incoming = stream.next() => {
                let msg = match incoming {
                    Some(Ok(msg)) => msg,
                    Some(Err(_)) => break Some(Closing { code: 1009, problem: None }),
                    None => break None,
                };
                last_client = Instant::now();
                match msg {
                    Message::Text(raw) => {
                        if !handle_client_message(&raw, &mut session).await {
                            errors.push_back(Instant::now());
                            while errors.front().is_some_and(|t| t.elapsed() > Duration::from_secs(60)) {
                                errors.pop_front();
                            }
                            if errors.len() > MAX_ERRORS_PER_MINUTE {
                                break Some(Closing::with(4000, "J-E1900", "too many protocol errors"));
                            }
                        }
                    }
                    Message::Binary(_) => break Some(Closing::with(4000, "J-E1900", "binary messages are not accepted")),
                    Message::Close(_) => break None,
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
            _ = sample_tick.tick() => {
                session.check_generation().await;
                session.sample_due().await;
                if session.outbox.is_full() {
                    let since = *full_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= limits.slow_client_grace {
                        break Some(Closing::with(4008, "J-E1906", "slow client"));
                    }
                } else {
                    full_since = None;
                }
            }
            _ = heartbeat_tick.tick() => {
                if last_client.elapsed() >= limits.heartbeat_miss {
                    break Some(Closing::with(4002, "J-E1908", "missing heartbeat"));
                }
                session.outbox.ping();
                if session.outbox.last_text.elapsed() >= limits.heartbeat {
                    session.outbox.push(&json!({"op": "heartbeat", "ts": now_rfc3339()}), None);
                }
            }
            _ = &mut writer_task => {
                session.park();
                return;
            }
        }
    };
    match closing {
        Some(c) => {
            let _ = close_tx.send(c);
        }
        None => drop(close_tx),
    }
    session.park();
    let _ = writer_task.await;
}

/// Handles one client message; `false` is a protocol error that counts
/// toward the 4000 close (ref/09 §6.2).
async fn handle_client_message(raw: &str, session: &mut Session) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        let p = Problem::new("J-E1900", 400, "malformed message");
        session.outbox.push(&error_json(None, &p), None);
        return false;
    };
    let id = value.get("id").and_then(Value::as_str).map(str::to_owned);
    let known = ["subscribe", "unsubscribe", "resume", "ping", "list"];
    if !value
        .get("op")
        .and_then(Value::as_str)
        .is_some_and(|op| known.contains(&op))
    {
        let p = Problem::new("J-E1901", 400, "unknown op");
        session.outbox.push(&error_json(id.as_deref(), &p), None);
        return false;
    }
    let msg = match serde_json::from_value::<ClientMsg>(value) {
        Ok(m) => m,
        Err(e) => {
            let p = Problem::new("J-E1900", 400, format!("malformed message: {e}"));
            session.outbox.push(&error_json(id.as_deref(), &p), None);
            return false;
        }
    };
    match msg {
        ClientMsg::Ping { id } => {
            session.outbox.push(&json!({"op": "pong", "id": id}), None);
        }
        ClientMsg::List { id } => {
            let items: Vec<Value> = session
                .subs
                .iter()
                .map(|(sub, s)| {
                    json!({
                        "sub": sub,
                        "topic": s.topic.name(),
                        "sel": s.sel,
                        "min_interval": format_interval(s.interval),
                    })
                })
                .collect();
            session.outbox.push(
                &json!({"op": "subscriptions", "id": id, "items": items}),
                None,
            );
        }
        ClientMsg::Resume {
            id,
            session: wanted,
            subs,
        } => {
            let parked = if session.subs.is_empty() {
                session.engine.unpark(&wanted, &session.who)
            } else {
                None
            };
            match parked {
                Some(parked) => session.resume(
                    id.as_deref(),
                    parked,
                    subs.into_iter().map(|s| (s.sub, s.last_seq)).collect(),
                ),
                None => {
                    let detail = if session.subs.is_empty() {
                        format!("session `{wanted}` cannot be resumed; subscribe again")
                    } else {
                        "`resume` comes before any `subscribe` on a connection".to_owned()
                    };
                    let p = Problem::new("J-E1907", 400, detail);
                    session.outbox.push(&error_json(id.as_deref(), &p), None);
                }
            }
        }
        ClientMsg::Unsubscribe { id, sub } => {
            session.unsubscribe(&sub);
            session
                .outbox
                .push(&json!({"op": "unsubscribed", "id": id, "sub": sub}), None);
        }
        ClientMsg::Subscribe {
            id,
            topic,
            sel,
            min_interval,
            mode,
            initial,
            buffer,
        } => {
            let options = SubscribeOptions {
                sel,
                min_interval,
                mode,
                initial,
                buffer,
            };
            if let Err(p) = session.subscribe(Some(&id), &topic, options).await {
                session.outbox.push(&error_json(Some(&id), &p), None);
            }
        }
    }
    true
}

fn stream_options(raw: Option<&str>) -> Result<(String, SubscribeOptions), Problem> {
    let query = parse_query(
        raw,
        &[
            "topic",
            "sel",
            "min_interval",
            "mode",
            "initial",
            "mono",
            "filter",
            "buffer",
        ],
    )?;
    let mut topic = None;
    let mut options = SubscribeOptions {
        initial: true,
        ..SubscribeOptions::default()
    };
    for (k, v) in query {
        match k.as_str() {
            "topic" => topic = Some(v),
            "sel" => options
                .sel
                .extend(v.split(',').filter(|s| !s.is_empty()).map(str::to_owned)),
            "min_interval" => options.min_interval = Some(v),
            "mode" => options.mode = Some(v),
            "initial" => options.initial = v != "false",
            "buffer" => {
                options.buffer = Some(
                    v.parse()
                        .map_err(|_| Problem::new("J-E1012", 400, "bad `buffer`"))?,
                );
            }
            _ => {}
        }
    }
    let topic = topic.ok_or_else(|| Problem::new("J-E1013", 400, "`topic` is required"))?;
    Ok((topic, options))
}

/// The `<session>.<seq>` of a `Last-Event-ID` header.
fn last_event_id(headers: &HeaderMap) -> Option<(String, u64)> {
    let (session, seq) = headers
        .get("last-event-id")?
        .to_str()
        .ok()?
        .rsplit_once('.')?;
    Some((session.to_owned(), seq.parse().ok()?))
}

/// `GET /v1/stream` (ref/09 §6.11): one subscription per request, over the
/// same engine as `/v1/ws`. `Last-Event-ID` resumes the session it names
/// (ADR 0074). A stream whose queue stays full past the grace is ended,
/// and the session of an ended stream is parked.
pub async fn sse_handler(
    State(backend): State<Shared>,
    Extension(engine): Extension<Engine>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    let (topic, options) = match stream_options(raw.as_deref()) {
        Ok(p) => p,
        Err(p) => return problem_response(&p),
    };
    let who = principal(peer);
    let limits = engine.limits;
    let role = role_with(&who, &backend.socket_roles());
    let (mut session, rx, bytes) = Session::new(backend, &engine, who.clone(), role);
    let asked = last_event_id(&headers);
    let parked = asked
        .as_ref()
        .and_then(|(id, _)| engine.unpark(id, &who))
        .and_then(|p| Some((p.subs.keys().next()?.clone(), p)));
    let sub_id = match (parked, asked) {
        (Some((sub, parked)), Some((_, last))) => {
            session.resume(None, parked, vec![(sub.clone(), last)]);
            sub
        }
        (_, asked) => {
            let sub = match session.subscribe(None, &topic, options).await {
                Ok(s) => s,
                Err(p) => return problem_response(&p),
            };
            if asked.is_some() && session.subs.get(&sub).is_some_and(|s| s.topic.is_event()) {
                session.outbox.push(
                    &json!({"op": "gap", "sub": sub, "reason": "resume_unavailable", "ts": now_rfc3339()}),
                    None,
                );
            }
            sub
        }
    };
    let guard = ConnectionGuard::new(&engine.stats);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let session_id = session.id.clone();
    tokio::spawn(async move {
        let mut full_since: Option<Instant> = None;
        let mut sample_tick = tokio::time::interval(SAMPLE_GRANULARITY);
        sample_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            sample_tick.tick().await;
            if session.outbox.tx.is_closed() || !session.subs.contains_key(&sub_id) {
                break;
            }
            session.check_generation().await;
            session.sample_due().await;
            if session.outbox.is_full() {
                let since = *full_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= limits.slow_client_grace {
                    let _ = stop_tx.send(());
                    break;
                }
            } else {
                full_since = None;
            }
        }
        session.park();
    });
    let events = UnboundedReceiverStream::new(rx).filter_map(move |out| {
        delivered(&out, &bytes);
        let _held = &guard;
        let event = match out {
            Out::Text { text, .. } => {
                let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                let op = parsed["op"].as_str().unwrap_or("data").to_owned();
                let mut event = Event::default().event(op);
                if let Some(seq) = parsed["seq"].as_u64() {
                    event = event.id(format!("{session_id}.{seq}"));
                }
                Some(event.data(&*text))
            }
            Out::Ping => None,
        };
        std::future::ready(event)
    });
    let stream = events
        .take_until(stop_rx)
        .map(Ok::<_, std::convert::Infallible>);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(limits.heartbeat).text("hb"))
        .into_response()
}
