//! `jk top` (ref/09 §9.1): a real `jokerd` subprocess and a real `jk`
//! subprocess subscribed to `metrics`, `status` and `outputs` over
//! `GET /v1/ws`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

#[test]
fn top_prints_the_group_row() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());

    let buf = common::run_and_collect(
        &[
            "--node",
            &format!("unix:{}", daemon.socket.display()),
            "top",
        ],
        Duration::from_secs(2),
    );
    assert!(buf.contains("default"), "{buf:?}");
    assert!(buf.contains("jitter(max)"), "{buf:?}");
    assert!(buf.contains("pi.act_led"), "{buf:?}");
    assert!(buf.contains("api connections"), "{buf:?}");
}

/// No app writes `pi.act_led`, so it lives in the implicit `default`
/// group (docs/16 A11) rather than a declared `[cycles.*]`.
#[test]
fn top_json_prints_one_metrics_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());

    let buf = common::run_and_collect(
        &[
            "--node",
            &format!("unix:{}", daemon.socket.display()),
            "--json",
            "top",
        ],
        Duration::from_secs(2),
    );
    let v: serde_json::Value = serde_json::from_str(buf.trim()).unwrap();
    assert!(v["at"].is_string(), "{buf:?}");
    assert_eq!(v["groups"][0]["group"], "default");
}
