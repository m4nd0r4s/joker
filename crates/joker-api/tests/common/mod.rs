//! A stub [`Backend`] whose data the tests drive, and the helpers that
//! serve it on a Unix socket and talk WebSocket and HTTP to it.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use joker_api::model::{
    ActivateRequest, BulkResetRequest, GenerationProposal, InvokeActionRequest, ManualWriteRequest,
    MethodCallRequest, MissingBlobsRequest, ReasonBody, ReleaseRequest, ResetTripRequest,
    RollbackRequest, TraceRequest,
};
use joker_api::ws::Limits;
use joker_api::{Backend, Headers, Principal, Problem, Query, Reply};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

#[derive(Default)]
pub struct Stub {
    pub version: AtomicU64,
    pub big: AtomicBool,
    pub decisions: AtomicU64,
    pub decisions_per_read: AtomicU64,
    pub generation: AtomicU64,
    pub added: AtomicBool,
    pub removed: AtomicBool,
    pub reads: AtomicU64,
    pub metrics: Mutex<Option<Value>>,
}

fn none<T>() -> Result<T, Problem> {
    Err(Problem::new("J-E1005", 404, "not in the stub"))
}

impl Backend for Stub {
    fn headers(&self) -> Headers {
        let g = self.generation.load(Ordering::SeqCst);
        Headers {
            generation: (g > 0).then(|| format!("{g} sha256:{g:064}")),
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
        Ok(Reply::ok(json!({"node": "stub", "devices": [], "groups": [
            {"name": "fast", "period": "20ms"},
            {"name": "slow", "period": "1s"},
        ]})))
    }
    fn metrics(&self) -> Result<Reply, Problem> {
        let set = self.metrics.lock().unwrap().clone();
        Ok(Reply::ok(
            set.unwrap_or_else(|| json!({"at": "now", "groups": []})),
        ))
    }
    fn signals(&self, _query: &Query) -> Result<Reply, Problem> {
        none()
    }
    fn signal(&self, _path: &str) -> Result<Reply, Problem> {
        none()
    }
    fn values(&self, _query: &Query) -> Result<Reply, Problem> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let v = self.version.load(Ordering::SeqCst);
        if self.big.load(Ordering::SeqCst) {
            self.version.fetch_add(1, Ordering::SeqCst);
            let pad = "x".repeat(8 * 1024);
            return Ok(Reply::ok(json!({"items": [
                {"path": "a.big", "value": v, "seq": v, "pad": pad},
            ]})));
        }
        let mut items = vec![json!(
            {"path": "a.changes", "value": v, "seq": v, "age": format!("{}ms", rand_age())}
        )];
        if !self.removed.load(Ordering::SeqCst) {
            items.push(json!(
                {"path": "a.still", "value": 1, "seq": 1, "age": format!("{}ms", rand_age())}
            ));
        }
        if self.added.load(Ordering::SeqCst) {
            items.push(json!({"path": "a.added", "value": 7, "seq": 1}));
        }
        Ok(Reply::ok(json!({"items": items})))
    }
    fn outputs(&self, query: &Query) -> Result<Reply, Problem> {
        let all = [
            json!({"path": "o.fast", "group": "fast", "applied": 1}),
            json!({"path": "o.slow", "group": "slow", "applied": 2}),
        ];
        let sel: Vec<&str> = query
            .iter()
            .filter(|(k, _)| k == "sel")
            .map(|(_, v)| v.as_str())
            .collect();
        let items: Vec<Value> = all
            .into_iter()
            .filter(|o| sel.is_empty() || sel.iter().any(|s| *s == "o.*" || o["path"] == *s))
            .collect();
        Ok(Reply::ok(json!({"items": items})))
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
    fn logs(&self, _query: &Query) -> Result<Reply, Problem> {
        Ok(Reply::ok(json!({"items": [], "next_cursor": null})))
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

    fn call_method(
        &self,
        _who: &Principal,
        _instance: &str,
        _method: &str,
        _request: MethodCallRequest,
    ) -> Result<Reply, Problem> {
        none()
    }
}

pub fn rand_age() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        % 1000
}

pub struct Fixture {
    pub socket: PathBuf,
    pub stub: Arc<Stub>,
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

pub async fn start(limits: Limits) -> Fixture {
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

pub type Ws = WebSocketStream<tokio::net::UnixStream>;

pub async fn connect(socket: &Path) -> Ws {
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

pub async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

pub async fn next_json(ws: &mut Ws) -> Value {
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

pub async fn texts_for(ws: &mut Ws, window: Duration) -> Vec<Value> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + window;
    while let Ok(Some(Ok(m))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Message::Text(t) = m {
            out.push(serde_json::from_str(&t).unwrap());
        }
    }
    out
}

pub fn short() -> Limits {
    Limits {
        heartbeat: Duration::from_millis(200),
        heartbeat_miss: Duration::from_secs(60),
        ..Limits::default()
    }
}

/// `GET path` over the socket with HTTP/1.1: the status, the
/// `content-type` and the body.
pub async fn http_get(socket: &Path, path: &str) -> (u16, String, String) {
    let mut stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: joker\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8(raw).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    let content_type = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-type")
                .then(|| v.trim().to_owned())
        })
        .unwrap_or_default();
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(body)
    } else {
        body.to_owned()
    };
    (status, content_type, body)
}

fn dechunk(mut body: &str) -> String {
    let mut out = String::new();
    loop {
        let (size, rest) = body.split_once("\r\n").unwrap();
        let size = usize::from_str_radix(size.trim(), 16).unwrap();
        if size == 0 {
            return out;
        }
        out.push_str(&rest[..size]);
        body = &rest[size + 2..];
    }
}
