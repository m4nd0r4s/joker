//! The subscription engine of `GET /v1/ws` (ref/09 §6) against a stub
//! backend whose data the tests drive: sampling at `min_interval` with only
//! the changes, ping frames and heartbeats, the subscription limit, counted
//! event drops and the 4008 close of a client that stops reading.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use joker_api::model::{
    ActivateRequest, BulkResetRequest, GenerationProposal, InvokeActionRequest, ManualWriteRequest,
    MissingBlobsRequest, ReasonBody, ReleaseRequest, ResetTripRequest, RollbackRequest,
    TraceRequest,
};
use joker_api::ws::Limits;
use joker_api::{Backend, Headers, Principal, Problem, Query, Reply};
use serde_json::{Value, json};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

#[derive(Default)]
struct Stub {
    version: AtomicU64,
    big: AtomicBool,
    decisions: AtomicU64,
    decisions_per_read: AtomicU64,
}

fn none<T>() -> Result<T, Problem> {
    Err(Problem::new("J-E1005", 404, "not in the stub"))
}

impl Backend for Stub {
    fn headers(&self) -> Headers {
        Headers {
            generation: None,
            clock: "unsynced".into(),
        }
    }
    fn info(&self, _who: &Principal) -> Result<Reply, Problem> {
        Ok(Reply::ok(
            json!({"runtime": {"boot_id": "B"}, "generation": null}),
        ))
    }
    fn health(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }
    fn status(&self) -> Result<Reply, Problem> {
        Ok(Reply::ok(json!({"node": "stub", "devices": []})))
    }
    fn metrics(&self) -> Result<Reply, Problem> {
        Ok(Reply::ok(json!({"at": "now", "groups": []})))
    }
    fn signals(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }
    fn signal(&self, _path: &str) -> Result<Reply, Problem> {
        none()
    }
    fn values(&self, _query: &Query) -> Result<Reply, Problem> {
        let v = self.version.load(Ordering::SeqCst);
        if self.big.load(Ordering::SeqCst) {
            self.version.fetch_add(1, Ordering::SeqCst);
            let pad = "x".repeat(8 * 1024);
            return Ok(Reply::ok(json!({"items": [
                {"path": "a.big", "value": v, "seq": v, "pad": pad},
            ]})));
        }
        Ok(Reply::ok(json!({"items": [
            {"path": "a.changes", "value": v, "seq": v, "age": format!("{}ms", rand_age())},
            {"path": "a.still", "value": 1, "seq": 1, "age": format!("{}ms", rand_age())},
        ]})))
    }
    fn outputs(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }
    fn output(&self, _path: &str) -> Result<Reply, Problem> {
        none()
    }
    fn manual_write(
        &self,
        _who: &Principal,
        _path: &str,
        _request: ManualWriteRequest,
    ) -> Result<Reply, Problem> {
        none()
    }
    fn release(
        &self,
        _who: &Principal,
        _path: &str,
        _request: ReleaseRequest,
    ) -> Result<Reply, Problem> {
        none()
    }
    fn reset_trip(
        &self,
        _who: &Principal,
        _path: &str,
        _request: ResetTripRequest,
    ) -> Result<Reply, Problem> {
        none()
    }
    fn trips(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }
    fn reset_trips(&self, _who: &Principal, _request: BulkResetRequest) -> Result<Reply, Problem> {
        none()
    }
    fn manual(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }
    fn write_ticket(&self, _id: &str) -> Result<Reply, Problem> {
        none()
    }
    fn decisions(&self, _query: &Query) -> Result<Reply, Problem> {
        let n = self.decisions_per_read.load(Ordering::SeqCst);
        let end = self.decisions.fetch_add(n, Ordering::SeqCst) + n;
        let items: Vec<Value> = (end.saturating_sub(1000)..end)
            .rev()
            .map(|i| json!({"id": format!("{i:026}"), "output": "a.out"}))
            .collect();
        Ok(Reply::ok(json!({"items": items})))
    }
    fn generations(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }
    fn create_generation(
        &self,
        _who: &Principal,
        _request: GenerationProposal,
        _dry_run: bool,
    ) -> Result<Reply, Problem> {
        none()
    }
    fn missing_blobs(&self, _request: MissingBlobsRequest) -> Result<Reply, Problem> {
        none()
    }
    fn has_blob(&self, _digest: &str) -> Result<bool, Problem> {
        Ok(false)
    }
    fn upload_blob(
        &self,
        _who: &Principal,
        _digest: &str,
        _kind: Option<&str>,
        _bytes: &[u8],
    ) -> Result<Reply, Problem> {
        none()
    }
    fn generation(&self, _selector: &str) -> Result<Reply, Problem> {
        none()
    }
    fn activate(
        &self,
        _who: &Principal,
        _selector: &str,
        _if_match: Option<&str>,
        _request: ActivateRequest,
    ) -> Result<Reply, Problem> {
        none()
    }
    fn confirm(
        &self,
        _who: &Principal,
        _selector: &str,
        _request: ReasonBody,
    ) -> Result<Reply, Problem> {
        none()
    }
    fn rollback(
        &self,
        _who: &Principal,
        _if_match: Option<&str>,
        _request: RollbackRequest,
    ) -> Result<Reply, Problem> {
        none()
    }
    fn traces(&self) -> Result<Reply, Problem> {
        none()
    }
    fn create_trace(&self, _who: &Principal, _request: TraceRequest) -> Result<Reply, Problem> {
        none()
    }
    fn trace(&self, _id: &str) -> Result<Reply, Problem> {
        none()
    }
    fn trace_file(&self, _id: &str) -> Result<Vec<u8>, Problem> {
        none()
    }
    fn actions(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }
    fn invoke_action(
        &self,
        _who: &Principal,
        _target: &str,
        _action: &str,
        _idempotency_key: Option<&str>,
        _request: InvokeActionRequest,
    ) -> Result<Reply, Problem> {
        none()
    }

    fn invocations(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }

    fn invocation(&self, _id: &str) -> Result<Reply, Problem> {
        none()
    }

    fn cancel_invocation(
        &self,
        _who: &Principal,
        _id: &str,
        _request: ReasonBody,
    ) -> Result<Reply, Problem> {
        none()
    }
}

fn rand_age() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        % 1000
}

struct Fixture {
    socket: PathBuf,
    stub: Arc<Stub>,
    _dir: tempfile::TempDir,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

async fn start(limits: Limits) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("api.sock");
    let stub = Arc::new(Stub::default());
    let listener = joker_api::server::bind_unix(&socket).unwrap();
    let app = joker_api::server::router_with(Arc::clone(&stub) as Arc<dyn Backend>, limits);
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        joker_api::server::serve_router(listener, app, async {
            let _ = rx.await;
        })
        .await
        .unwrap();
    });
    Fixture {
        socket,
        stub,
        _dir: dir,
        stop: Some(tx),
    }
}

type Ws = WebSocketStream<tokio::net::UnixStream>;

async fn connect(socket: &Path) -> Ws {
    let stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    let mut request = "ws://joker/v1/ws".into_client_request().unwrap();
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static("joker.v1"),
    );
    let (ws, _) = tokio_tungstenite::client_async(request, stream)
        .await
        .unwrap();
    ws
}

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

async fn next_json(ws: &mut Ws) -> Value {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a message within 5 s")
            .unwrap()
            .unwrap()
        {
            Message::Text(t) => return serde_json::from_str(&t).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected message: {other:?}"),
        }
    }
}

async fn texts_for(ws: &mut Ws, window: Duration) -> Vec<Value> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + window;
    while let Ok(Some(Ok(m))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Message::Text(t) = m {
            out.push(serde_json::from_str(&t).unwrap());
        }
    }
    out
}

fn short() -> Limits {
    Limits {
        heartbeat: Duration::from_millis(200),
        heartbeat_miss: Duration::from_secs(60),
        ..Limits::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn welcome_reports_the_role_of_the_caller_and_the_limits() {
    let f = start(Limits::default()).await;
    let mut ws = connect(&f.socket).await;
    let welcome = next_json(&mut ws).await;
    assert_eq!(welcome["op"], "welcome");
    assert_eq!(welcome["role"], "admin", "{welcome}");
    assert_eq!(welcome["limits"]["max_subscriptions"], 64);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_state_subscription_sends_only_changes_at_its_interval() {
    let f = start(Limits::default()).await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "subscribe", "id": "r", "topic": "signals", "sel": ["a.*"], "min_interval": "200ms"}),
    )
    .await;
    let subscribed = next_json(&mut ws).await;
    assert_eq!(subscribed["op"], "subscribed");
    assert_eq!(subscribed["min_interval"], "200ms");
    let resync = next_json(&mut ws).await;
    assert_eq!(resync["op"], "resync");
    assert_eq!(resync["items"].as_array().unwrap().len(), 2);

    let quiet = texts_for(&mut ws, Duration::from_millis(700)).await;
    assert!(quiet.is_empty(), "nothing changed, yet: {quiet:?}");

    f.stub.version.fetch_add(1, Ordering::SeqCst);
    let start = tokio::time::Instant::now();
    let data = next_json(&mut ws).await;
    assert_eq!(data["op"], "data");
    assert_eq!(data["seq"], 2);
    let items = data["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{data}");
    assert_eq!(items[0]["path"], "a.changes");
    assert!(start.elapsed() <= Duration::from_millis(400));

    f.stub.version.fetch_add(1, Ordering::SeqCst);
    f.stub.version.fetch_add(1, Ordering::SeqCst);
    let burst = texts_for(&mut ws, Duration::from_millis(500)).await;
    assert_eq!(
        burst.len(),
        1,
        "two changes within one interval coalesce: {burst:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_min_interval_below_the_minimum_is_refused() {
    let f = start(Limits::default()).await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "subscribe", "id": "r", "topic": "metrics", "min_interval": "100ms"}),
    )
    .await;
    let error = next_json(&mut ws).await;
    assert_eq!(error["op"], "error");
    assert_eq!(error["problem"]["code"], "J-E1905");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_server_pings_and_sends_heartbeats_when_idle() {
    let f = start(short()).await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    let mut pings = 0;
    let mut heartbeats = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(900);
    while let Ok(Some(Ok(m))) = tokio::time::timeout_at(deadline, ws.next()).await {
        match m {
            Message::Ping(_) => pings += 1,
            Message::Text(t) => {
                let v: Value = serde_json::from_str(&t).unwrap();
                if v["op"] == "heartbeat" {
                    assert!(v["ts"].as_str().unwrap().ends_with('Z'), "{v}");
                    heartbeats += 1;
                }
            }
            _ => {}
        }
    }
    assert!(pings >= 2, "{pings} pings");
    assert!(heartbeats >= 2, "{heartbeats} heartbeats");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscriptions_beyond_the_limit_are_refused() {
    let f = start(Limits {
        max_subscriptions: 2,
        ..Limits::default()
    })
    .await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    for i in 0..3 {
        send(
            &mut ws,
            json!({"op": "subscribe", "id": format!("r{i}"), "topic": "status", "initial": false}),
        )
        .await;
    }
    let replies: Vec<Value> = vec![
        next_json(&mut ws).await,
        next_json(&mut ws).await,
        next_json(&mut ws).await,
    ];
    assert_eq!(replies[0]["op"], "subscribed");
    assert_eq!(replies[1]["op"], "subscribed");
    assert_eq!(replies[2]["op"], "error");
    assert_eq!(replies[2]["id"], "r2");
    assert_eq!(replies[2]["problem"]["code"], "J-E1903");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_stops_reading_is_closed_with_4008_and_its_drops_are_counted() {
    let f = start(Limits {
        outbox_bytes: 32 * 1024,
        slow_client_grace: Duration::from_millis(300),
        ..Limits::default()
    })
    .await;
    f.stub.big.store(true, Ordering::SeqCst);
    f.stub.decisions_per_read.store(200, Ordering::SeqCst);
    let mut slow = connect(&f.socket).await;
    let _welcome = next_json(&mut slow).await;
    for i in 0..8 {
        send(
            &mut slow,
            json!({"op": "subscribe", "id": format!("s{i}"), "topic": "signals", "sel": ["a.*"], "min_interval": "10ms"}),
        )
        .await;
    }
    send(
        &mut slow,
        json!({"op": "subscribe", "id": "d", "topic": "decisions", "buffer": 64, "min_interval": "10ms"}),
    )
    .await;

    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut close = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut closing = None;
    while let Ok(Some(Ok(m))) = tokio::time::timeout_at(deadline, slow.next()).await {
        match m {
            Message::Close(frame) => {
                close = frame;
                break;
            }
            Message::Text(t) => {
                let v: Value = serde_json::from_str(&t).unwrap();
                if v["op"] == "closing" {
                    closing = Some(v);
                }
            }
            _ => {}
        }
    }
    let close = close.expect("a close frame");
    assert_eq!(u16::from(close.code), 4008);
    let closing = closing.expect("a closing message before the close frame");
    assert_eq!(closing["code"], 4008);
    assert_eq!(closing["problem"]["code"], "J-E1906");

    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "subscribe", "id": "m", "topic": "metrics"}),
    )
    .await;
    let _subscribed = next_json(&mut ws).await;
    let resync = next_json(&mut ws).await;
    let api = &resync["items"][0]["api"];
    assert!(api["dropped"].as_u64().unwrap() > 0, "{resync}");
    assert_eq!(api["connections"], 1, "{resync}");
    assert_eq!(api["subscriptions"], 1, "{resync}");
}
