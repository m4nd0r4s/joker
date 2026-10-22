//! WebSocket and Server-Sent Events subscriptions (ref/09 §6): live
//! push over the same [`Backend`] reads the REST endpoints already
//! serve, sampled on a per-subscription timer so a slow client never
//! touches the core — nothing here ever reaches an `rt` thread; every
//! read goes through `Backend`, the same path `GET /v1/values` etc.
//! already use.
//!
//! **Scoped to what has a real data source today**: the `signals` and
//! `outputs` state topics, and `decisions` (treated as sampled too,
//! for lack of a push-capable event queue reaching this layer — a
//! simplification from §6.5's "each item is delivered, or its drop is
//! declared", noted where it matters below). The other ref/09 §6.4
//! topics (`status`, `metrics`, `signal_trace`, `alarms`, `events`,
//! `manual`, `intents`, `jobs`, `invocations`, `generations`, `audit`,
//! `logs`) have no backing data source in this build; `subscribe`
//! refuses them with `J-E1902`, the same code an unknown topic gets,
//! rather than accepting and never feeding them.
//!
//! **Also not built**: resumption after a reconnection (§6.7 — no
//! session store), the generation-swap `resolved` notice (§6.9), and
//! the node-wide sampling-rate budget (§6.5's 1,000,000 samples/s,
//! `J-E1903`). Each is a real gap against ref/09, not something this
//! module pretends to cover.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query as AxumQuery, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::backend::{Principal, Query};
use crate::id::ulid;
use crate::problem::Problem;
use crate::server::{API_VERSION, Peer, Shared, principal};

/// How long a client may go without a message before the server closes
/// (ref/09 §6.8).
const HEARTBEAT_MISS: Duration = Duration::from_secs(45);
/// How often the server pings (ref/09 §6.8).
const HEARTBEAT_EVERY: Duration = Duration::from_secs(15);
/// How long a full per-connection output queue is tolerated before the
/// server gives up on a slow client (ref/09 §6.6 point 4).
const SLOW_CLIENT_GRACE: Duration = Duration::from_secs(30);
/// Bounded output queue depth. ref/09 sizes this in bytes (1 MiB); this
/// build counts messages instead, a simplification noted in the module
/// doc.
const OUTBOX_CAPACITY: usize = 1024;
/// Event-topic ring size (ref/09 §6.6 point 3: 64-4,096, default 1,024).
const DEFAULT_BUFFER: usize = 1024;
/// `J-E1905`, ref/09 §6.5.
const MINIMUM_INTERVAL: Duration = Duration::from_millis(10);
/// Topics with a real data source today (module doc).
const KNOWN_TOPICS: [&str; 3] = ["signals", "outputs", "decisions"];

fn default_interval(topic: &str) -> Duration {
    match topic {
        "decisions" => Duration::from_millis(200),
        _ => Duration::from_millis(100),
    }
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// A millisecond Unix timestamp, as a plain decimal string: `joker-api`
/// has no RFC 3339 formatter (that is `jokerd::wire::timestamp`, a
/// layer up), and adding one just for a heartbeat's `ts` field is not
/// worth a new dependency.
fn now_ms_string() -> String {
    (now_nanos() / 1_000_000).to_string()
}

fn new_sub_id() -> String {
    ulid(now_nanos())
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
        #[serde(default = "default_true")]
        initial: bool,
        buffer: Option<usize>,
    },
    Unsubscribe {
        id: String,
        sub: String,
    },
    Ping {
        id: String,
    },
    List {
        id: String,
    },
}

fn default_true() -> bool {
    true
}

struct Subscription {
    topic: String,
    sel: Vec<String>,
    seq: u64,
    #[allow(dead_code, reason = "carried for a future per-sub ring, not read yet")]
    buffer: usize,
}

fn problem_json(p: &Problem) -> Value {
    p.body(None)
}

fn problem_response(p: &Problem) -> Response {
    let status = axum::http::StatusCode::from_u16(p.status)
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    (status, axum::Json(p.body(None))).into_response()
}

fn error_msg(id: Option<&str>, sub: Option<&str>, p: &Problem) -> Message {
    text(&json!({"op": "error", "id": id, "sub": sub, "problem": problem_json(p)}))
}

fn text(v: &Value) -> Message {
    Message::Text(v.to_string().into())
}

/// Reads `topic`'s current items through `Backend`, the same call a
/// REST read of it would make (`spawn_blocking`: a `Backend` call may
/// block on a lock).
async fn read_topic(backend: &Shared, topic: &str, sel: &[String]) -> Result<Value, Problem> {
    let query: Query = if sel.is_empty() {
        Vec::new()
    } else {
        vec![("sel".to_owned(), sel.join(","))]
    };
    let b = Arc::clone(backend);
    let topic = topic.to_owned();
    tokio::task::spawn_blocking(move || match topic.as_str() {
        "signals" => b.values(&query),
        "outputs" => b.outputs(&query),
        "decisions" => b.decisions(&query),
        _ => Err(Problem::new("J-E1902", 400, "unknown topic")),
    })
    .await
    .unwrap_or_else(|_| Err(Problem::new("J-E1000", 500, "the handler panicked")))
    .map(|r| r.body)
}

fn parse_interval(text: &str) -> Result<Duration, Problem> {
    let d = joker_units::Duration::parse(text)
        .map_err(|e| Problem::new("J-E1012", 400, format!("min_interval: {e}")))?;
    let ns = d.as_nanos();
    if ns < MINIMUM_INTERVAL.as_nanos() as u64 {
        return Err(Problem::new("J-E1905", 400, "min_interval below 10ms"));
    }
    Ok(Duration::from_nanos(ns))
}

/// `GET /v1/ws` (ref/09 §6.1): the subprotocol must be `joker.v1`.
pub async fn ws_handler(
    State(backend): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    ws: WebSocketUpgrade,
) -> Response {
    let who = principal(peer);
    ws.protocols(["joker.v1"])
        .on_upgrade(move |socket| handle_socket(socket, backend, who))
}

async fn handle_socket(socket: WebSocket, backend: Shared, who: Principal) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Message>(OUTBOX_CAPACITY);

    let session = new_sub_id();
    let info = {
        let b = Arc::clone(&backend);
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
        "session": session,
        "boot_id": boot_id,
        "api": API_VERSION,
        "role": "admin",
        "generation": generation,
        "heartbeat": "15s",
        "resume_window": "0s",
        "limits": {"max_subscriptions": 64},
    });
    if tx.send(text(&welcome)).await.is_err() {
        return;
    }

    // The writer: drains `rx` into the socket, so a subscription's
    // sampler never blocks on the network (ref/09 §6.6).
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut subs: HashMap<String, Subscription> = HashMap::new();
    let mut last_client_activity = tokio::time::Instant::now();
    let mut full_since: Option<tokio::time::Instant> = None;
    // `interval()` fires immediately on its first tick; skip that one
    // so sampling and heartbeats start a full period from now, not at
    // t=0.
    let mut sample_tick = tokio::time::interval(MINIMUM_INTERVAL);
    sample_tick.tick().await;
    let mut heartbeat_tick = tokio::time::interval(HEARTBEAT_EVERY);
    heartbeat_tick.tick().await;

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                last_client_activity = tokio::time::Instant::now();
                match msg {
                    Message::Text(raw) => {
                        if !handle_client_message(&raw, &backend, &tx, &mut subs).await {
                            break;
                        }
                    }
                    Message::Binary(_) | Message::Close(_) => break, // 4000, ref/09 §6.1
                    _ => {}
                }
            }
            _ = sample_tick.tick() => {
                let due: Vec<String> = subs.keys().cloned().collect();
                for sub_id in due {
                    sample_one(&backend, &mut subs, &sub_id, &tx).await;
                }
                if tx.capacity() == 0 {
                    full_since.get_or_insert_with(tokio::time::Instant::now);
                } else {
                    full_since = None;
                }
                if full_since.is_some_and(|t| t.elapsed() >= SLOW_CLIENT_GRACE) {
                    let _ = tx.send(text(&json!({
                        "op": "closing",
                        "code": 4008,
                        "problem": {"code": "J-E1906", "detail": "slow client"},
                    }))).await;
                    break;
                }
            }
            _ = heartbeat_tick.tick() => {
                if last_client_activity.elapsed() >= HEARTBEAT_MISS {
                    let _ = tx.send(text(&json!({
                        "op": "closing",
                        "code": 4002,
                        "problem": {"code": "J-E1908", "detail": "missing heartbeat"},
                    }))).await;
                    break;
                }
                if tx.send(text(&json!({"op": "heartbeat", "ts": now_ms_string()}))).await.is_err() {
                    break;
                }
            }
        }
    }
    drop(tx);
    let _ = writer.await;
}

/// Handles one client message; `false` means the connection should
/// close (a protocol-level violation, ref/09 §6.1/§6.2).
async fn handle_client_message(
    raw: &str,
    backend: &Shared,
    tx: &mpsc::Sender<Message>,
    subs: &mut HashMap<String, Subscription>,
) -> bool {
    let Ok(msg) = serde_json::from_str::<ClientMsg>(raw) else {
        let _ = tx
            .send(error_msg(
                None,
                None,
                &Problem::new("J-E1900", 400, "malformed message"),
            ))
            .await;
        return true;
    };
    match msg {
        ClientMsg::Ping { id } => {
            let _ = tx.send(text(&json!({"op": "pong", "id": id}))).await;
        }
        ClientMsg::List { id } => {
            let items: Vec<Value> = subs
                .iter()
                .map(|(sub, s)| json!({"sub": sub, "topic": s.topic, "sel": s.sel}))
                .collect();
            let _ = tx
                .send(text(
                    &json!({"op": "subscriptions", "id": id, "items": items}),
                ))
                .await;
        }
        ClientMsg::Unsubscribe { id, sub } => {
            subs.remove(&sub);
            let _ = tx
                .send(text(&json!({"op": "unsubscribed", "id": id, "sub": sub})))
                .await;
        }
        ClientMsg::Subscribe {
            id,
            topic,
            sel,
            min_interval,
            initial,
            buffer,
        } => {
            if !KNOWN_TOPICS.contains(&topic.as_str()) {
                let _ = tx
                    .send(error_msg(
                        Some(&id),
                        None,
                        &Problem::new("J-E1902", 400, format!("unknown topic `{topic}`")),
                    ))
                    .await;
                return true;
            }
            let interval = match min_interval.as_deref() {
                Some(t) => match parse_interval(t) {
                    Ok(d) => d,
                    Err(p) => {
                        let _ = tx.send(error_msg(Some(&id), None, &p)).await;
                        return true;
                    }
                },
                None => default_interval(&topic),
            };
            let sub_id = new_sub_id();
            let mut sub = Subscription {
                topic: topic.clone(),
                sel,
                seq: 0,
                buffer: buffer.unwrap_or(DEFAULT_BUFFER).clamp(64, 4096),
            };
            let resolved = read_topic(backend, &topic, &sub.sel)
                .await
                .ok()
                .and_then(|b| b["items"].as_array().map(Vec::len))
                .unwrap_or(0);
            let _ = tx
                .send(text(&json!({
                    "op": "subscribed",
                    "id": id,
                    "sub": sub_id,
                    "topic": topic,
                    "resolved": resolved,
                    "min_interval": joker_units::Duration::from_nanos(interval.as_nanos() as u64).to_string(),
                })))
                .await;
            if initial && let Ok(body) = read_topic(backend, &sub.topic, &sub.sel).await {
                sub.seq += 1;
                let _ = tx
                    .send(text(&json!({
                        "op": "resync",
                        "sub": sub_id,
                        "seq": sub.seq,
                        "items": body["items"],
                    })))
                    .await;
            }
            subs.insert(sub_id, sub);
        }
    }
    true
}

async fn sample_one(
    backend: &Shared,
    subs: &mut HashMap<String, Subscription>,
    sub_id: &str,
    tx: &mpsc::Sender<Message>,
) {
    let Some(sub) = subs.get_mut(sub_id) else {
        return;
    };
    let Ok(body) = read_topic(backend, &sub.topic, &sub.sel).await else {
        return;
    };
    sub.seq += 1;
    let msg = text(&json!({
        "op": "data",
        "sub": sub_id,
        "seq": sub.seq,
        "items": body["items"],
    }));
    if tx.try_send(msg).is_err() {
        let _ = tx.try_send(text(&json!({
            "op": "gap",
            "sub": sub_id,
            "lost": 1,
            "reason": "overflow",
        })));
    }
}

#[derive(Debug, Deserialize)]
pub struct StreamParams {
    topic: String,
    #[serde(default)]
    sel: Option<String>,
    min_interval: Option<String>,
    #[serde(default = "default_true")]
    initial: bool,
}

/// `GET /v1/stream` (ref/09 §6.11): one subscription per request, the
/// same engine as `/v1/ws`'s `subscribe`.
pub async fn sse_handler(
    State(backend): State<Shared>,
    AxumQuery(params): AxumQuery<StreamParams>,
) -> Response {
    if !KNOWN_TOPICS.contains(&params.topic.as_str()) {
        return problem_response(&Problem::new(
            "J-E1902",
            400,
            format!("unknown topic `{}`", params.topic),
        ));
    }
    let interval = match params.min_interval.as_deref() {
        Some(t) => match parse_interval(t) {
            Ok(d) => d,
            Err(p) => return problem_response(&p),
        },
        None => default_interval(&params.topic),
    };
    let sel: Vec<String> = params
        .sel
        .map(|s| s.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let sub_id = new_sub_id();
    let topic = params.topic;
    let initial = params.initial;

    let (tx, rx) = mpsc::channel::<Event>(64);
    tokio::spawn(async move {
        let _ = tx
            .send(
                Event::default()
                    .event("subscribed")
                    .data(json!({"op": "subscribed", "sub": sub_id, "topic": topic}).to_string()),
            )
            .await;
        let mut seq = 0u64;
        if initial && let Ok(body) = read_topic(&backend, &topic, &sel).await {
            seq += 1;
            let ev = Event::default()
                .id(format!("{sub_id}.{seq}"))
                .event("data")
                .data(
                    json!({"op": "data", "sub": sub_id, "seq": seq, "items": body["items"]})
                        .to_string(),
                );
            if tx.send(ev).await.is_err() {
                return;
            }
        }
        let mut tick = tokio::time::interval(interval);
        loop {
            tick.tick().await;
            let Ok(body) = read_topic(&backend, &topic, &sel).await else {
                continue;
            };
            seq += 1;
            let ev = Event::default()
                .id(format!("{sub_id}.{seq}"))
                .event("data")
                .data(
                    json!({"op": "data", "sub": sub_id, "seq": seq, "items": body["items"]})
                        .to_string(),
                );
            if tx.send(ev).await.is_err() {
                return;
            }
        }
    });

    Sse::new(ReceiverStream::new(rx).map(Ok::<_, std::convert::Infallible>))
        .keep_alive(KeepAlive::new().interval(HEARTBEAT_EVERY).text("hb"))
        .into_response()
}
