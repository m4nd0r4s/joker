//! `jk logs` (ref/09 §9.1, `GET /v1/logs`): a real `jokerd` subprocess
//! and a real `jk` subprocess reading the node's log ring.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

#[test]
fn logs_prints_the_daemon_start() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());
    let node = format!("unix:{}", daemon.socket.display());

    let buf = common::run_and_collect(&["--node", &node, "logs"], Duration::from_secs(2));
    let line = buf
        .lines()
        .find(|l| l.contains("jokerd") && l.contains("started"))
        .unwrap_or_else(|| panic!("{buf:?}"));
    assert!(line.contains("info"), "{line}");

    let json = common::run_and_collect(
        &[
            "--node", &node, "--json", "logs", "--source", "jokerd", "--level", "info",
        ],
        Duration::from_secs(2),
    );
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    let items = v["items"].as_array().unwrap();
    assert!(!items.is_empty(), "{json}");
    assert!(items.iter().all(|e| e["source"] == "jokerd"), "{json}");
}
