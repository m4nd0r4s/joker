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

#[test]
fn watch_exits_non_zero_when_the_node_goes_away() {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());
    let mut jk = Command::new(env!("CARGO_BIN_EXE_jk"))
        .args([
            "--node",
            &format!("unix:{}", daemon.socket.display()),
            "read",
            "pi.act_led",
            "--watch",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(jk.try_wait().unwrap().is_none(), "jk ended while watching");
    drop(daemon);

    let mut status = None;
    for _ in 0..100 {
        if let Some(s) = jk.try_wait().unwrap() {
            status = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let Some(status) = status else {
        let _ = jk.kill();
        panic!("jk kept watching a node that is gone");
    };
    let mut stderr = String::new();
    jk.stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(4), "{stderr}");
    assert!(
        stderr.contains("closed") || stderr.contains("lost"),
        "{stderr}"
    );
}
