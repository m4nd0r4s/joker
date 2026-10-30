//! `jk board probe` and `jk board check --node` against a running
//! `jokerd` (ref/03 §2.3.7, ref/09 §5.2).

#![allow(clippy::unwrap_used)]

mod common;

use std::process::Command;

fn jk(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_jk"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn text(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn a_probed_draft_checks_locally_and_on_its_node() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());
    let node = format!("unix:{}", daemon.socket.display());

    let probe = jk(
        dir.path(),
        &["--node", &node, "board", "probe", "--name", "this-host"],
    );
    assert_eq!(probe.status.code(), Some(0), "{}", text(&probe));
    assert!(
        text(&probe).contains("fields to review"),
        "{}",
        text(&probe)
    );
    let draft = dir.path().join("boards/this-host.board.toml");
    let content = std::fs::read_to_string(&draft).unwrap();
    assert!(content.contains("name    = \"this-host\""), "{content}");

    let again = jk(
        dir.path(),
        &["--node", &node, "board", "probe", "--name", "this-host"],
    );
    assert_eq!(again.status.code(), Some(1), "{}", text(&again));
    assert_eq!(std::fs::read_to_string(&draft).unwrap(), content);

    let local = jk(
        dir.path(),
        &["board", "check", "boards/this-host.board.toml"],
    );
    assert_eq!(local.status.code(), Some(0), "{}", text(&local));

    let on_node = jk(
        dir.path(),
        &[
            "--node",
            &node,
            "board",
            "check",
            "boards/this-host.board.toml",
        ],
    );
    assert_eq!(
        on_node.status.code(),
        Some(0),
        "{}{content}",
        text(&on_node)
    );
    assert!(text(&on_node).contains("\"# REVIEW\" lines unresolved"));

    let wrong = content.replacen("cpus    = ", "cpus    = 1", 1);
    std::fs::write(dir.path().join("wrong.board.toml"), wrong).unwrap();
    let refused = jk(
        dir.path(),
        &["--node", &node, "board", "check", "wrong.board.toml"],
    );
    assert_eq!(refused.status.code(), Some(2), "{}", text(&refused));
    assert!(text(&refused).contains("J0829"), "{}", text(&refused));
    common::terminate(daemon);
}
