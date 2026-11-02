//! `jk decisions` with a range and `--report csv|pdf` (ADR 0104) against a
//! real `jokerd` subprocess.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::process::{Command, Output};

fn jk(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jk"))
        .args(args)
        .env_remove("JK_NODE")
        .env_remove("JK_ENV")
        .env_remove("JK_MANIFEST")
        .env("TZ", "UTC")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn decisions_reports_cover_the_selection_as_csv_and_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());
    let node = format!("unix:{}", daemon.socket.display());
    let n = |args: &[&str]| {
        let mut all = vec!["--node", node.as_str()];
        all.extend_from_slice(args);
        jk(&all)
    };
    let write = n(&[
        "write",
        "pi.act_led",
        "true",
        "--ttl",
        "30s",
        "--reason",
        "report",
        "--yes",
    ]);
    assert_eq!(write.status.code(), Some(0), "{}", text(&write));

    let listed = n(&["decisions", "--output", "pi.act_led", "--since", "1h"]);
    assert_eq!(listed.status.code(), Some(0), "{}", text(&listed));
    assert!(text(&listed).contains("manual:4"), "{}", text(&listed));
    let none = n(&["decisions", "--from", "2000-01-01 00:00", "--to", "01:00"]);
    assert_eq!(none.status.code(), Some(0), "{}", text(&none));
    assert!(!text(&none).contains("pi.act_led"), "{}", text(&none));

    let csv = n(&["decisions", "--output", "pi.act_led", "--report", "csv"]);
    assert_eq!(csv.status.code(), Some(0), "{}", text(&csv));
    let body = String::from_utf8_lossy(&csv.stdout).to_string();
    let mut lines = body.lines();
    assert_eq!(
        lines.next(),
        Some(
            "ts,output,kind,writer,level,proposed,outcome,reason,applied,active_level,repeats,last_ts,principal,request_id"
        )
    );
    assert!(body.contains(",pi.act_led,"), "{body}");
    assert_eq!(body.matches("ts,output,").count(), 1);

    let pdf = dir.path().join("decisions.pdf");
    let made = n(&[
        "decisions",
        "--report",
        "pdf",
        "--out",
        pdf.to_str().unwrap(),
    ]);
    assert_eq!(made.status.code(), Some(0), "{}", text(&made));
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes).contains("pi.act_led"));
    let again = n(&[
        "decisions",
        "--report",
        "pdf",
        "--out",
        pdf.to_str().unwrap(),
    ]);
    assert_eq!(again.status.code(), Some(1), "{}", text(&again));
    assert!(text(&again).contains("not overwritten"));

    let no_out = n(&["decisions", "--report", "pdf"]);
    assert_eq!(no_out.status.code(), Some(1), "{}", text(&no_out));
    let both = n(&["decisions", "--since", "1h", "--from", "now"]);
    assert_eq!(both.status.code(), Some(1), "{}", text(&both));
    assert!(text(&both).contains("--since and --from"));
}
