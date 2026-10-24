//! A stalled client does not change cycle jitter (AUR-243, ref/09 §6.6): a
//! periodic thread that stands in for a cycle group publishes into the
//! stub backend while the subscription engine serves clients that never
//! read and clients that read everything at the minimum interval. Its
//! start lateness under that load stays within the noise of the same
//! thread with no client at all, and every stalled client is closed with
//! 4008.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::{Stub, connect, next_json, send, start};
use futures_util::StreamExt;
use joker_api::ws::Limits;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

const PERIOD: Duration = Duration::from_millis(2);
const PHASE: Duration = Duration::from_millis(1500);

/// Runs a periodic thread for `PHASE` and returns its sorted start
/// lateness, one per tick.
fn cycle(stub: Arc<Stub>) -> std::thread::JoinHandle<Vec<Duration>> {
    std::thread::spawn(move || {
        let start = Instant::now();
        let mut deadline = start + PERIOD;
        let mut late = Vec::with_capacity(1024);
        while deadline < start + PHASE {
            if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            late.push(Instant::now().saturating_duration_since(deadline));
            stub.version.fetch_add(1, Ordering::Release);
            deadline += PERIOD;
        }
        late.sort();
        late
    })
}

fn quantile(sorted: &[Duration], q: f64) -> Duration {
    let i = ((sorted.len() as f64 - 1.0) * q).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_clients_do_not_change_the_jitter_of_a_cycle_thread() {
    let f = start(Limits {
        outbox_bytes: 64 * 1024,
        slow_client_grace: Duration::from_millis(500),
        ..Limits::default()
    })
    .await;
    f.stub.big.store(true, Ordering::SeqCst);
    f.stub.decisions_per_read.store(50, Ordering::SeqCst);

    let baseline = tokio::task::spawn_blocking({
        let stub = Arc::clone(&f.stub);
        move || cycle(stub).join().unwrap()
    })
    .await
    .unwrap();

    let mut stalled = Vec::new();
    for _ in 0..8 {
        let mut ws = connect(&f.socket).await;
        let _welcome = next_json(&mut ws).await;
        for i in 0..8 {
            send(
                &mut ws,
                json!({"op": "subscribe", "id": format!("s{i}"), "topic": "signals", "sel": ["a.*"], "min_interval": "10ms"}),
            )
            .await;
        }
        send(
            &mut ws,
            json!({"op": "subscribe", "id": "d", "topic": "decisions", "buffer": 64, "min_interval": "10ms"}),
        )
        .await;
        stalled.push(ws);
    }
    let reading = Arc::new(AtomicBool::new(true));
    let mut readers = Vec::new();
    for _ in 0..4 {
        let mut ws = connect(&f.socket).await;
        let _welcome = next_json(&mut ws).await;
        send(
            &mut ws,
            json!({"op": "subscribe", "id": "r", "topic": "signals", "sel": ["a.*"], "min_interval": "10ms"}),
        )
        .await;
        let reading = Arc::clone(&reading);
        readers.push(tokio::spawn(async move {
            let mut n = 0u64;
            while reading.load(Ordering::Relaxed) {
                match tokio::time::timeout(Duration::from_millis(100), ws.next()).await {
                    Ok(Some(Ok(_))) => n += 1,
                    Ok(_) => break,
                    Err(_) => {}
                }
            }
            n
        }));
    }

    let loaded = tokio::task::spawn_blocking({
        let stub = Arc::clone(&f.stub);
        move || cycle(stub).join().unwrap()
    })
    .await
    .unwrap();
    reading.store(false, Ordering::Relaxed);
    let mut delivered = 0;
    for r in readers {
        delivered += r.await.unwrap();
    }
    assert!(
        delivered > 100,
        "the readers were served: {delivered} messages"
    );

    let expected = (PHASE.as_micros() / PERIOD.as_micros()) as usize;
    assert!(
        loaded.len() * 10 >= expected * 9,
        "{} of {expected} ticks",
        loaded.len()
    );
    let (base99, load99) = (quantile(&baseline, 0.99), quantile(&loaded, 0.99));
    let bound = (base99 * 3).max(base99 + Duration::from_millis(5));
    assert!(
        load99 <= bound,
        "p99 start lateness rose from {base99:?} to {load99:?} (bound {bound:?})"
    );

    for mut ws in stalled {
        let mut code = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while let Ok(Some(Ok(m))) = tokio::time::timeout_at(deadline, ws.next()).await {
            if let Message::Close(frame) = m {
                code = frame.map(|f| u16::from(f.code));
                break;
            }
        }
        assert_eq!(code, Some(4008), "a stalled client is closed with 4008");
    }
}
