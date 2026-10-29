//! `DecisionLog::scan`: reading the decision log a line at a time, newest
//! first or oldest first, and stopping early, so that `GET /v1/decisions`
//! never holds the whole file (a 260 MB log killed `jokerd` on the bench Pi
//! by running it out of memory).

#![allow(clippy::unwrap_used)]

use jokerd::decisions::DecisionLog;
use serde_json::Value;

fn log_with(lines: usize, pad: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let text: String = (1..=lines)
        .map(|i| format!("{{\"seq\":{i},\"pad\":\"{}\"}}\n", "x".repeat(pad)))
        .collect();
    std::fs::write(dir.path().join("decisions.jsonl"), text).unwrap();
    dir
}

fn seqs(dir: &tempfile::TempDir, newest_first: bool, take: usize) -> Vec<u64> {
    let mut out = Vec::new();
    DecisionLog::scan(
        &dir.path().join("decisions.jsonl"),
        newest_first,
        |e: Value| {
            out.push(e["seq"].as_u64().unwrap());
            out.len() < take
        },
    )
    .unwrap();
    out
}

#[test]
fn newest_first_reads_from_the_end_and_stops_early() {
    let dir = log_with(1000, 10);
    assert_eq!(seqs(&dir, true, 3), [1000, 999, 998]);
    assert_eq!(seqs(&dir, false, 3), [1, 2, 3]);
}

#[test]
fn lines_longer_than_a_block_are_read_whole() {
    let dir = log_with(5, 100_000);
    assert_eq!(seqs(&dir, true, 10), [5, 4, 3, 2, 1]);
}

#[test]
fn a_last_line_without_a_newline_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("decisions.jsonl");
    std::fs::write(&path, "{\"seq\":1}\n{\"seq\":2}").unwrap();
    let mut got = Vec::new();
    DecisionLog::scan(&path, true, |e| {
        got.push(e["seq"].as_u64().unwrap());
        true
    })
    .unwrap();
    assert_eq!(got, [2, 1]);
}
