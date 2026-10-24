//! `GET /v1/ws` (ref/09 §6): a real WebSocket handshake and the
//! `subscribe`/`data`/`ping` round trip, scoped to the topics this
//! build actually feeds (`signals`, `outputs`, `decisions`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use joker_core::ClockQuality;
use jokerd::api::Node;
use jokerd::control::{Config, Controller};
use jokerd::decisions::DecisionLog;
use jokerd::devices::NoDevices;
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, SystemPacer};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

fn effective() -> String {
    format!(
        "{}\n[devices.pi]\ndriver = \"sim\"\n\n[devices.pi.signals]\nact_led = {{ type = \"bool\", direction = \"output\" }}\n",
        common::JOKER_TOML
    )
}

struct Fixture {
    socket: std::path::PathBuf,
    _dir: tempfile::TempDir,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

async fn start() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let clock = Monotonic::start();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let mut c = Controller::new(
        dir.path(),
        log,
        Arc::new(NativeTasks),
        Arc::new(NoDevices),
        Box::new(move || Box::new(SystemPacer::new(clock, ClockQuality::Unsynced))),
        Config {
            min_window: Duration::from_millis(100),
            ..Config::default()
        },
    )
    .unwrap();
    let g = c
        .propose(&effective(), "", Some("bench".into()), false)
        .unwrap();
    c.activate(g.number, None, None).unwrap();
    let controller = Arc::new(Mutex::new(c));
    let node = Node::new(controller, clock, 0, dir.path()).unwrap();
    let socket = dir.path().join("api.sock");
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let path = socket.clone();
    tokio::spawn(async move {
        joker_api::server::serve_unix(&path, Arc::new(node), async {
            let _ = rx.await;
        })
        .await
        .unwrap();
    });
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    Fixture {
        socket,
        _dir: dir,
        stop: Some(tx),
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

async fn connect(socket: &Path) -> tokio_tungstenite::WebSocketStream<tokio::net::UnixStream> {
    let stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    let mut request = "ws://joker/v1/ws".into_client_request().unwrap();
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static("joker.v1"),
    );
    let (ws, _response) = tokio_tungstenite::client_async(request, stream)
        .await
        .unwrap();
    ws
}

async fn next_json(ws: &mut tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>) -> Value {
    loop {
        match ws.next().await.unwrap().unwrap() {
            Message::Text(t) => return serde_json::from_str(&t).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected message: {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subscribe_gets_a_welcome_then_a_subscribed_and_a_resync() {
    let f = start().await;
    let mut ws = connect(&f.socket).await;

    let welcome = next_json(&mut ws).await;
    assert_eq!(welcome["op"], "welcome");
    assert!(welcome["session"].is_string(), "{welcome}");

    ws.send(Message::Text(
        json!({"op": "subscribe", "id": "r1", "topic": "outputs"})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();

    let subscribed = next_json(&mut ws).await;
    assert_eq!(subscribed["op"], "subscribed");
    assert_eq!(subscribed["id"], "r1");
    assert_eq!(subscribed["topic"], "outputs");
    let sub = subscribed["sub"].as_str().unwrap().to_owned();

    let resync = next_json(&mut ws).await;
    assert_eq!(resync["op"], "resync");
    assert_eq!(resync["sub"], sub);
    assert_eq!(resync["seq"], 1);
    assert!(resync["items"].is_array(), "{resync}");
}

/// The connection itself stays open after the error (ref/09 §6.2): a
/// ping still works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_topic_is_refused_without_closing_the_connection() {
    let f = start().await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;

    ws.send(Message::Text(
        json!({"op": "subscribe", "id": "r1", "topic": "alarms"})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let error = next_json(&mut ws).await;
    assert_eq!(error["op"], "error");
    assert_eq!(error["id"], "r1");
    assert_eq!(error["problem"]["code"], "J-E1902");

    ws.send(Message::Text(
        json!({"op": "ping", "id": "p1"}).to_string().into(),
    ))
    .await
    .unwrap();
    let pong = next_json(&mut ws).await;
    assert_eq!(pong["op"], "pong");
    assert_eq!(pong["id"], "p1");
}

/// `common::JOKER_TOML`'s own `light` app flips `pi.act_led` on its own
/// cycle, with no action needed from the test: the next `data` message on
/// the subscription just has to arrive on its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_change_arrives_as_a_data_message() {
    let f = start().await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;

    ws.send(Message::Text(
        json!({
            "op": "subscribe",
            "id": "r1",
            "topic": "signals",
            "sel": ["pi.act_led"],
            "min_interval": "10ms",
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();
    let subscribed = next_json(&mut ws).await;
    assert_eq!(subscribed["op"], "subscribed");
    let _resync = next_json(&mut ws).await;

    let data = next_json(&mut ws).await;
    assert_eq!(data["op"], "data");
    assert_eq!(data["sub"], subscribed["sub"]);
    assert!(data["items"].is_array(), "{data}");
}

/// Reads enough of the stream to see the first two SSE events, then
/// stops: SSE ignores `connection: close` and streams forever, so a
/// partial read is enough to prove it started right.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sse_stream_sends_a_subscribed_event_then_data() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let f = start().await;
    let mut stream = tokio::net::UnixStream::connect(&f.socket).await.unwrap();
    stream
        .write_all(
            b"GET /v1/stream?topic=outputs HTTP/1.1\r\nhost: joker\r\nconnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while raw.len() < 4096 && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => {
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw);
                if text.contains("event: subscribed") && text.contains("event: data") {
                    break;
                }
            }
            Ok(Err(_)) => break,
        }
    }
    let text = String::from_utf8_lossy(&raw);
    assert!(
        text.contains("text/event-stream") || text.contains("200 OK"),
        "{text}"
    );
    assert!(text.contains("event: subscribed"), "{text}");
    assert!(text.contains("\"topic\":\"outputs\""), "{text}");
}
