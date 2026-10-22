//! `jk read --watch` (ref/09 §6): a real `jokerd` subprocess and a real
//! `jk` subprocess talking `GET /v1/ws` over the node's own socket.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

#[test]
fn watch_prints_a_live_line_for_the_selected_signal() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());

    let buf = common::run_and_collect(
        &[
            "--node",
            &format!("unix:{}", daemon.socket.display()),
            "read",
            "pi.act_led",
            "--watch",
        ],
        Duration::from_secs(2),
    );
    assert!(buf.contains("pi.act_led"), "{buf:?}");
}

#[test]
fn watch_json_prints_the_items_array() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());

    let buf = common::run_and_collect(
        &[
            "--node",
            &format!("unix:{}", daemon.socket.display()),
            "--json",
            "read",
            "pi.act_led",
            "--watch",
        ],
        Duration::from_secs(2),
    );
    assert!(buf.trim_start().starts_with('['), "{buf:?}");
    assert!(buf.contains("\"pi.act_led\""), "{buf:?}");
}
