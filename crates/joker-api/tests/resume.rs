//! Resumption after a reconnection (ref/09 §6.7, §6.11, ADR 0074): a
//! dropped session is parked, and `resume` or `Last-Event-ID` takes it over
//! with a replay of what the client missed, a resync when the buffer cannot
//! cover it, and `J-E1907` when it is gone.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{Ws, connect, next_json, send, start};
use joker_api::ws::Limits;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Opens a session with one `signals` subscription and lets it send
/// `resync` (seq 1) and two `data` (seq 2 and 3). Returns the session,
/// the subscription and the open socket.
async fn session_with_three_messages(f: &common::Fixture) -> (String, String, Ws) {
    let mut ws = connect(&f.socket).await;
    let welcome = next_json(&mut ws).await;
    assert_ne!(welcome["resume_window"], "0s", "{welcome}");
    let session = welcome["session"].as_str().unwrap().to_owned();
    send(
        &mut ws,
        json!({"op": "subscribe", "id": "r", "topic": "signals", "sel": ["a.*"], "min_interval": "50ms"}),
    )
    .await;
    let subscribed = next_json(&mut ws).await;
    let sub = subscribed["sub"].as_str().unwrap().to_owned();
    assert_eq!(next_json(&mut ws).await["seq"], 1);
    for seq in 2..=3 {
        f.stub.version.fetch_add(1, Ordering::SeqCst);
        let data = next_json(&mut ws).await;
        assert_eq!(data["seq"], seq, "{data}");
    }
    (session, sub, ws)
}

async fn drop_connection(mut ws: Ws) {
    let _ = ws.close(None).await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(150)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_replays_the_messages_after_last_seq() {
    let f = start(Limits::default()).await;
    let (session, sub, ws) = session_with_three_messages(&f).await;
    drop_connection(ws).await;

    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "resume", "id": "x", "session": session, "subs": [
            {"sub": sub, "last_seq": 1},
            {"sub": "nope", "last_seq": 0},
        ]}),
    )
    .await;
    let resumed = next_json(&mut ws).await;
    assert_eq!(resumed["op"], "resumed", "{resumed}");
    assert_eq!(resumed["id"], "x");
    assert_eq!(resumed["session"], session.as_str());
    assert_eq!(
        resumed["subs"],
        json!([{"sub": sub, "result": "replayed"}, {"sub": "nope", "result": "unknown"}])
    );
    assert_eq!(next_json(&mut ws).await["seq"], 2);
    assert_eq!(next_json(&mut ws).await["seq"], 3);
    f.stub.version.fetch_add(1, Ordering::SeqCst);
    let data = next_json(&mut ws).await;
    assert_eq!(data["op"], "data");
    assert_eq!(
        data["seq"], 4,
        "sampling continues after the replay: {data}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_resyncs_when_the_buffer_no_longer_covers_last_seq() {
    let f = start(Limits {
        replay_messages: 1,
        ..Limits::default()
    })
    .await;
    let (session, sub, ws) = session_with_three_messages(&f).await;
    drop_connection(ws).await;

    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "resume", "session": session, "subs": [{"sub": sub, "last_seq": 1}]}),
    )
    .await;
    let resumed = next_json(&mut ws).await;
    assert_eq!(resumed["subs"], json!([{"sub": sub, "result": "resync"}]));
    let resync = next_json(&mut ws).await;
    assert_eq!(resync["op"], "resync", "{resync}");
    assert_eq!(resync["seq"], 4);
    assert_eq!(resync["items"].as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_that_is_gone_or_a_late_resume_is_refused() {
    let f = start(Limits {
        resume_window: Duration::from_millis(300),
        ..Limits::default()
    })
    .await;
    let (session, sub, ws) = session_with_three_messages(&f).await;
    drop_connection(ws).await;

    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "resume", "session": "01ZZZZZZZZZZZZZZZZZZZZZZZZ", "subs": []}),
    )
    .await;
    let error = next_json(&mut ws).await;
    assert_eq!(error["problem"]["code"], "J-E1907", "{error}");

    send(
        &mut ws,
        json!({"op": "subscribe", "id": "s", "topic": "status", "initial": false}),
    )
    .await;
    assert_eq!(next_json(&mut ws).await["op"], "subscribed");
    send(
        &mut ws,
        json!({"op": "resume", "session": session, "subs": [{"sub": sub, "last_seq": 3}]}),
    )
    .await;
    let error = next_json(&mut ws).await;
    assert_eq!(error["problem"]["code"], "J-E1907", "{error}");
    drop_connection(ws).await;

    tokio::time::sleep(Duration::from_millis(400)).await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "resume", "session": session, "subs": [{"sub": sub, "last_seq": 3}]}),
    )
    .await;
    let error = next_json(&mut ws).await;
    assert_eq!(
        error["problem"]["code"], "J-E1907",
        "outside the window: {error}"
    );
}

/// Opens `GET /v1/stream` with `last_event_id`, reads until `done` holds
/// for the text received so far, and closes the stream.
async fn stream_until(
    socket: &Path,
    last_event_id: Option<&str>,
    done: impl Fn(&str) -> bool,
) -> String {
    let mut stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    let header = last_event_id
        .map(|id| format!("Last-Event-ID: {id}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET /v1/stream?topic=signals&sel=a.*&min_interval=50ms HTTP/1.1\r\nHost: joker\r\n{header}\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut text = String::new();
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !done(&text) {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("the expected events within 5 s")
            .unwrap();
        assert!(n > 0, "the stream ended: {text}");
        text.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    text
}

fn event_ids(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("id: "))
        .map(str::to_owned)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn last_event_id_resumes_an_sse_stream() {
    let f = start(Limits::default()).await;
    let first = stream_until(&f.socket, None, |t| !event_ids(t).is_empty()).await;
    let id = event_ids(&first).remove(0);
    let (session, seq) = id.rsplit_once('.').unwrap();
    assert_eq!(seq, "1");
    f.stub.version.fetch_add(1, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(200)).await;
    f.stub.version.fetch_add(1, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let second = stream_until(&f.socket, Some(&id), |t| !event_ids(t).is_empty()).await;
    assert!(second.contains("event: resumed"), "{second}");
    let ids = event_ids(&second);
    assert_eq!(
        ids[0],
        format!("{session}.2"),
        "the stream picks up after the last event the client saw: {second}"
    );

    let fresh = stream_until(&f.socket, Some("01ZZZZZZZZZZZZZZZZZZZZZZZZ.9"), |t| {
        !event_ids(t).is_empty()
    })
    .await;
    assert!(fresh.contains("event: subscribed"), "{fresh}");
    let fresh_id = event_ids(&fresh).remove(0);
    assert!(fresh_id.ends_with(".1"), "a new session: {fresh}");
    let parsed: Vec<Value> = fresh
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|d| serde_json::from_str(d).ok())
        .collect();
    assert!(parsed.iter().any(|v| v["op"] == "resync"), "{fresh}");
}
