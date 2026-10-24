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
//! **Not built:** resumption after a reconnection (§6.7), the
//! generation-swap `resolved` notice (§6.9), the node sampling budget (§6.6
//! point 5), the per-group minimum of `outputs`, and the topics without a
//! data source in this build (refused with `J-E1902`).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use axum::Extension;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, RawQuery, State};
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
        }
    }
}

/// The counters of the subscription engine (`Metrics.api`, ref/09 §5.2).
#[derive(Debug, Default)]
pub struct ApiStats {
    connections: AtomicU64,
    subscriptions: AtomicU64,
    dropped: AtomicU64,
}

impl ApiStats {
    /// The `api` object of `Metrics`: open connections, live subscriptions
    /// and the items dropped from event rings (`joker.api.dropped`).
    pub fn json(&self) -> Value {
        json!({
            "connections": self.connections.load(Ordering::Relaxed),
            "subscriptions": self.subscriptions.load(Ordering::Relaxed),
            "dropped": self.dropped.load(Ordering::Relaxed),
        })
    }
}

/// What the subscription engine needs besides the backend.
#[derive(Debug, Clone, Default)]
pub struct Engine {
    /// Its limits.
    pub limits: Limits,
    /// Its counters.
    pub stats: Arc<ApiStats>,
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

fn parse_interval(topic: Topic, text: Option<&str>) -> Result<Duration, Problem> {
    let Some(text) = text else {
        return Ok(topic.default_interval());
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
    Ok(d)
}

/// A message on its way to the client.
enum Out {
    Text {
        text: String,
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
        let text = value.to_string();
        let n = text.len();
        let held = self.bytes.load(Ordering::Acquire);
        if held > 0 && held.saturating_add(n) > self.capacity {
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
}

impl Subscription {
    fn busy(&self) -> bool {
        self.in_flight.load(Ordering::Acquire)
    }
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
    role: &'static str,
    id: String,
    subs: BTreeMap<String, Subscription>,
    outbox: Outbox,
}

impl Drop for Session {
    fn drop(&mut self) {
        let n = u64::try_from(self.subs.len()).unwrap_or(u64::MAX);
        self.engine
            .stats
            .subscriptions
            .fetch_sub(n, Ordering::Relaxed);
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

impl Session {
    fn new(
        backend: Shared,
        engine: &Engine,
        role: &'static str,
    ) -> (Self, mpsc::UnboundedReceiver<Out>, Arc<AtomicUsize>) {
        let (outbox, rx, bytes) = Outbox::new(engine.limits.outbox_bytes);
        (
            Self {
                backend,
                engine: engine.clone(),
                role,
                id: new_id(),
                subs: BTreeMap::new(),
                outbox,
            },
            rx,
            bytes,
        )
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
        let interval = parse_interval(topic, options.min_interval.as_deref())?;
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
        self.engine
            .stats
            .subscriptions
            .fetch_add(1, Ordering::Relaxed);
        let items = match read_topic(&self.backend, &self.engine, topic, &options.sel).await {
            Ok(items) => items,
            Err(p) => {
                self.engine
                    .stats
                    .subscriptions
                    .fetch_sub(1, Ordering::Relaxed);
                return Err(p);
            }
        };
        let sub_id = new_id();
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
        Ok(sub_id)
    }

    fn unsubscribe(&mut self, sub: &str) {
        if self.subs.remove(sub).is_some() {
            self.engine
                .stats
                .subscriptions
                .fetch_sub(1, Ordering::Relaxed);
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
        if self.outbox.push(&msg, Some(&sub.in_flight)) {
            sub.seq += 1;
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
            if let Ok(items) = read_topic(&self.backend, &self.engine, sub.topic, &sub.sel).await {
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
        if self.outbox.push(&msg, Some(&sub.in_flight)) {
            sub.seq += 1;
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
        if self.outbox.push(&msg, Some(&sub.in_flight)) {
            sub.seq += 1;
            sub.ring.clear();
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
            Out::Text { text, .. } => Message::Text(text.clone().into()),
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
    let (mut session, rx, bytes) = Session::new(Arc::clone(&backend), &engine, role);
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
        "resume_window": "0s",
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
            _ = &mut writer_task => return,
        }
    };
    match closing {
        Some(c) => {
            let _ = close_tx.send(c);
        }
        None => drop(close_tx),
    }
    drop(session);
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
        ClientMsg::Resume { id } => {
            let p = Problem::new("J-E1907", 400, "no session to resume");
            session.outbox.push(&error_json(id.as_deref(), &p), None);
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

/// `GET /v1/stream` (ref/09 §6.11): one subscription per request, over the
/// same engine as `/v1/ws`. A stream whose queue stays full past the grace
/// is ended.
pub async fn sse_handler(
    State(backend): State<Shared>,
    Extension(engine): Extension<Engine>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    RawQuery(raw): RawQuery,
) -> Response {
    let (topic, options) = match stream_options(raw.as_deref()) {
        Ok(p) => p,
        Err(p) => return problem_response(&p),
    };
    let who = principal(peer);
    let limits = engine.limits;
    let role = role_with(&who, &backend.socket_roles());
    let (mut session, rx, bytes) = Session::new(backend, &engine, role);
    let sub_id = match session.subscribe(None, &topic, options).await {
        Ok(s) => s,
        Err(p) => return problem_response(&p),
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
                return;
            }
            session.sample_due().await;
            if session.outbox.is_full() {
                let since = *full_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= limits.slow_client_grace {
                    let _ = stop_tx.send(());
                    return;
                }
            } else {
                full_since = None;
            }
        }
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
                Some(event.data(text))
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
