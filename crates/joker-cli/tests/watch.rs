//! `jk read --watch` (ref/09 §6): a real `jokerd` subprocess and a real
//! `jk` subprocess talking `GET /v1/ws` over the node's own socket.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// `jokerd`'s binary, built alongside this test but in a sibling crate:
/// `CARGO_BIN_EXE_jokerd` is only set for binaries of *this* package.
/// The workspace's target directory is a fixed property of the
/// workspace, not of where this test's own binary happens to land (this
/// environment's cargo layout places test binaries outside the usual
/// `target/debug/deps/`), so it is found from the workspace root
/// instead: `CARGO_MANIFEST_DIR` is `<root>/crates/joker-cli`.
fn jokerd_bin() -> PathBuf {
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target")
                .to_owned()
        });
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    target_dir.join(profile).join(if cfg!(windows) {
        "jokerd.exe"
    } else {
        "jokerd"
    })
}

const MANIFEST: &str = "[project]\n\
     name  = \"watch-test\"\n\
     joker = \"1.0\"\n\
     mode  = \"dev\"\n\
     \n\
     [outputs.\"pi.act_led\"]\n\
     safe = false\n\
     \n\
     [cycles.main]\n\
     period = \"20ms\"\n\
     \n\
     [devices.pi]\n\
     driver = \"sim\"\n\
     \n\
     [devices.pi.signals]\n\
     act_led = { type = \"bool\", direction = \"output\" }\n";

struct Daemon {
    child: Child,
    socket: std::path::PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_jokerd(dir: &Path) -> Daemon {
    std::fs::write(dir.join("joker.toml"), MANIFEST).unwrap();
    let socket = dir.join("api.sock");
    let child = Command::new(jokerd_bin())
        .args([
            "--state-dir",
            dir.to_str().unwrap(),
            "--manifest",
            dir.join("joker.toml").to_str().unwrap(),
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100));
    Daemon { child, socket }
}

#[test]
fn watch_prints_a_live_line_for_the_selected_signal() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = spawn_jokerd(dir.path());

    let mut jk = Command::new(env!("CARGO_BIN_EXE_jk"))
        .args([
            "--node",
            &format!("unix:{}", daemon.socket.display()),
            "read",
            "pi.act_led",
            "--watch",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = jk.stdout.take().unwrap();
    let collected = std::thread::spawn(move || {
        let mut out = BufReader::new(stdout);
        let mut buf = String::new();
        let mut line = String::new();
        while out.read_line(&mut line).unwrap_or(0) > 0 {
            buf.push_str(&line);
            line.clear();
        }
        buf
    });

    std::thread::sleep(Duration::from_secs(2));
    let _ = jk.kill();
    let _ = jk.wait();
    let buf = collected.join().unwrap_or_default();
    assert!(buf.contains("pi.act_led"), "{buf:?}");
}

#[test]
fn watch_json_prints_the_items_array() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = spawn_jokerd(dir.path());

    let mut jk = Command::new(env!("CARGO_BIN_EXE_jk"))
        .args([
            "--node",
            &format!("unix:{}", daemon.socket.display()),
            "--json",
            "read",
            "pi.act_led",
            "--watch",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = jk.stdout.take().unwrap();
    let collected = std::thread::spawn(move || {
        let mut out = BufReader::new(stdout);
        let mut buf = String::new();
        let mut line = String::new();
        while out.read_line(&mut line).unwrap_or(0) > 0 {
            buf.push_str(&line);
            line.clear();
        }
        buf
    });

    std::thread::sleep(Duration::from_secs(2));
    let _ = jk.kill();
    let _ = jk.wait();
    let buf = collected.join().unwrap_or_default();
    assert!(buf.trim_start().starts_with('['), "{buf:?}");
    assert!(buf.contains("\"pi.act_led\""), "{buf:?}");
}
