//! The command sequences of learning track parts 1–2 against a local
//! runtime started with `jk dev` (docs/07, docs/26 §6).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn jk(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jk"))
        .current_dir(dir)
        .args(args)
        .env_remove("JK_NODE")
        .env_remove("JK_ENV")
        .env_remove("JK_MANIFEST")
        .output()
        .unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

struct Dev(Child);

impl Drop for Dev {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn part_one_and_two() {
    let tmp = tempfile::tempdir().unwrap();
    let created = jk(tmp.path(), &["new", "bench"]);
    assert_eq!(code(&created), 0, "{}", text(&created));
    let dir = tmp.path().join("bench");
    assert_eq!(code(&jk(tmp.path(), &["new", "bench"])), 1);

    let check = jk(&dir, &["check", "--env", "sim"]);
    assert_eq!(code(&check), 0, "{}", text(&check));
    assert!(text(&check).contains("✓ manifest"));
    let bare = jk(&dir, &["check"]);
    assert_eq!(code(&bare), 2);
    assert!(text(&bare).contains("J0101"));
    assert_eq!(
        code(&jk(
            &dir,
            &["check", "--env", "sim", "--manifest", "x.toml"]
        )),
        1
    );

    let socket = dir.join(".joker/dev.sock");
    let dev = Dev(Command::new(env!("CARGO_BIN_EXE_jk"))
        .current_dir(&dir)
        .args(["dev", "--env", "sim"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap());
    let start = Instant::now();
    while !socket.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(300));
    let node = format!("unix:{}", socket.display());
    let n = |args: &[&str]| {
        let mut all = vec!["--node", node.as_str()];
        all.extend_from_slice(args);
        jk(&dir, &all)
    };

    let read = n(&["read", "pi.**"]);
    assert_eq!(code(&read), 0, "{}", text(&read));
    assert!(text(&read).contains("pi.act_led"));
    let json = n(&["--json", "read", "pi.act_led"]);
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["items"][0]["quality"], "good");

    let write = n(&[
        "write",
        "pi.act_led",
        "true",
        "--ttl",
        "30s",
        "--reason",
        "trail",
        "--yes",
    ]);
    assert_eq!(code(&write), 0, "{}", text(&write));
    assert!(text(&write).contains("accepted"));
    let no_ttl = n(&[
        "write",
        "pi.act_led",
        "true",
        "--ttl",
        "500ms",
        "--reason",
        "x",
        "--yes",
    ]);
    assert_eq!(code(&no_ttl), 1, "{}", text(&no_ttl));
    assert!(text(&no_ttl).contains("J-E1103"));

    let apply = n(&["--env", "sim", "apply", "--confirm", "30s", "-m", "trail"]);
    assert_eq!(code(&apply), 0, "{}", text(&apply));
    assert!(text(&apply).contains("confirmation pending"));
    assert_eq!(code(&n(&["confirm"])), 0);
    let again = n(&["confirm"]);
    assert_eq!(code(&again), 3, "{}", text(&again));
    let gens = n(&["--json", "generations"]);
    let v: serde_json::Value = serde_json::from_slice(&gens.stdout).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 2);
    assert_eq!(code(&n(&["rollback"])), 0);
    let decisions = n(&["decisions", "--limit", "3"]);
    assert_eq!(code(&decisions), 0);
    assert!(text(&decisions).contains("pi.act_led"));
    assert_eq!(code(&n(&["status"])), 0);

    let rec = n(&["record", "--for", "1s", "--out", "bench.jktrace"]);
    assert_eq!(code(&rec), 0, "{}", text(&rec));
    let replayed = jk(&dir, &["--env", "sim", "replay", "bench.jktrace", "--diff"]);
    assert_eq!(code(&replayed), 0, "{}", text(&replayed));
    assert!(
        text(&replayed).contains("state of light · 0 divergence(s)"),
        "{}",
        text(&replayed)
    );
    let dump = n(&["trace-dump", "--last", "500ms", "--out", "ring.jktrace"]);
    assert_eq!(code(&dump), 0, "{}", text(&dump));
    std::fs::write(
        dir.join("joker.toml"),
        std::fs::read_to_string(dir.join("joker.toml"))
            .unwrap()
            .replace("500 ms", "100 ms"),
    )
    .unwrap();
    let changed = jk(&dir, &["--env", "sim", "replay", "bench.jktrace"]);
    assert_eq!(code(&changed), 6, "{}", text(&changed));
    drop(dev);

    let gone = n(&["status"]);
    assert_eq!(code(&gone), 4, "{}", text(&gone));
}
