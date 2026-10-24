//! The subscription engine of `GET /v1/ws` (ref/09 §6) against a stub
//! backend whose data the tests drive: sampling at `min_interval` with only
//! the changes, ping frames and heartbeats, the subscription limit, counted
//! event drops and the 4008 close of a client that stops reading.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{connect, next_json, send, short, start, texts_for};
use futures_util::StreamExt;
use joker_api::ws::Limits;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

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
    assert_eq!(
        api["subscriptions"], 0,
        "the first snapshot is read before the subscription counts: {resync}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subscription_beyond_the_sampling_budget_is_refused_with_what_fits() {
    let f = start(Limits {
        sampling_budget: 50,
        ..Limits::default()
    })
    .await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    for i in 0..3 {
        send(
            &mut ws,
            json!({"op": "subscribe", "id": format!("s{i}"), "topic": "signals", "sel": ["a.*"], "min_interval": "100ms", "initial": false}),
        )
        .await;
    }
    let replies = [
        next_json(&mut ws).await,
        next_json(&mut ws).await,
        next_json(&mut ws).await,
    ];
    assert_eq!(replies[0]["op"], "subscribed");
    assert_eq!(replies[1]["op"], "subscribed");
    assert_eq!(replies[2]["op"], "error", "{:?}", replies[2]);
    assert_eq!(replies[2]["problem"]["code"], "J-E1903");
    let detail = replies[2]["problem"]["detail"].as_str().unwrap();
    assert!(
        detail.contains("20 samples/s") && detail.contains("10 still fit"),
        "{detail}"
    );

    let (_, _, body) = common::http_get(&f.socket, "/v1/metrics").await;
    let metrics: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(metrics["api"]["sampled_per_s"], 40, "{metrics}");
    assert_eq!(metrics["api"]["subscriptions"], 2, "{metrics}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outputs_sample_no_faster_than_their_fastest_group() {
    let f = start(Limits::default()).await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "subscribe", "id": "slow", "topic": "outputs", "sel": ["o.slow"], "min_interval": "100ms"}),
    )
    .await;
    let error = next_json(&mut ws).await;
    assert_eq!(error["op"], "error");
    assert_eq!(error["problem"]["code"], "J-E1905");
    assert!(
        error["problem"]["detail"].as_str().unwrap().contains("1s"),
        "{error}"
    );

    send(
        &mut ws,
        json!({"op": "subscribe", "id": "default", "topic": "outputs", "sel": ["o.slow"], "initial": false}),
    )
    .await;
    let subscribed = next_json(&mut ws).await;
    assert_eq!(subscribed["op"], "subscribed", "{subscribed}");
    assert_eq!(subscribed["min_interval"], "1s");

    send(
        &mut ws,
        json!({"op": "subscribe", "id": "both", "topic": "outputs", "sel": ["o.*"], "min_interval": "20ms", "initial": false}),
    )
    .await;
    let subscribed = next_json(&mut ws).await;
    assert_eq!(subscribed["op"], "subscribed", "{subscribed}");
    assert_eq!(subscribed["resolved"], 2);
    assert_eq!(subscribed["min_interval"], "20ms");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_generation_swap_sends_resolved_before_the_new_paths() {
    let f = start(Limits::default()).await;
    let mut ws = connect(&f.socket).await;
    let _welcome = next_json(&mut ws).await;
    send(
        &mut ws,
        json!({"op": "subscribe", "id": "r", "topic": "signals", "sel": ["a.*"], "min_interval": "50ms"}),
    )
    .await;
    let _subscribed = next_json(&mut ws).await;
    let resync = next_json(&mut ws).await;
    assert_eq!(resync["op"], "resync");

    f.stub.generation.store(2, Ordering::SeqCst);
    f.stub.added.store(true, Ordering::SeqCst);
    f.stub.removed.store(true, Ordering::SeqCst);
    let resolved = next_json(&mut ws).await;
    assert_eq!(resolved["op"], "resolved", "{resolved}");
    assert_eq!(resolved["sub"], resync["sub"]);
    assert_eq!(resolved["generation"]["gen"], 2);
    assert_eq!(resolved["added"], json!(["a.added"]));
    assert_eq!(resolved["removed"], json!(["a.still"]));
    let data = next_json(&mut ws).await;
    assert_eq!(data["op"], "data", "{data}");
    assert_eq!(data["items"].as_array().unwrap().len(), 1, "{data}");
    assert_eq!(data["items"][0]["path"], "a.added");
    let quiet = texts_for(&mut ws, Duration::from_millis(300)).await;
    assert!(quiet.is_empty(), "the surviving path continues: {quiet:?}");
}
