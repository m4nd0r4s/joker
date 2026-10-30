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

#[test]
fn a_raw_line_filter_skips_lines_before_they_are_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("decisions.jsonl");
    std::fs::write(
        &path,
        "{\"output\":\"a\",\"seq\":1}\nnot json at all\n{\"output\":\"b\",\"seq\":2}\n{\"output\":\"a\",\"seq\":3}\n",
    )
    .unwrap();
    let needle: &[u8] = b"\"output\":\"a\"";
    let mut got = Vec::new();
    DecisionLog::scan_filtered(&path, true, &[needle], |e| {
        got.push(e["seq"].as_u64().unwrap());
        true
    })
    .unwrap();
    assert_eq!(got, [3, 1]);
}

#[test]
fn a_needle_far_from_its_line_end_is_found_across_skipped_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("decisions.jsonl");
    let text: String = (1..=6)
        .map(|i| {
            let output = if i % 3 == 0 { "rare" } else { "busy" };
            format!(
                "{{\"output\":\"{output}\",\"pad\":\"{}\",\"seq\":{i}}}\n",
                "x".repeat(150_000)
            )
        })
        .collect();
    std::fs::write(&path, text).unwrap();
    let needle: &[u8] = b"\"output\":\"rare\"";
    let mut got = Vec::new();
    DecisionLog::scan_filtered(&path, true, &[needle], |e| {
        got.push(e["seq"].as_u64().unwrap());
        true
    })
    .unwrap();
    assert_eq!(got, [6, 3]);
}

fn entry(i: u64) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    m.insert("record".into(), serde_json::json!("output"));
    m.insert("i".into(), serde_json::json!(i));
    m.insert("pad".into(), serde_json::json!("x".repeat(80)));
    m
}

fn segment_names(dir: &tempfile::TempDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n != "decisions.jsonl")
        .collect();
    names.sort();
    names
}

#[test]
fn a_full_current_file_is_rotated_and_seq_continues_across_segments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("decisions.jsonl");
    let mut log = DecisionLog::with_bounds(&path, 1_000, 1 << 30).unwrap();
    for i in 0..30 {
        assert_eq!(log.append(entry(i), false).unwrap(), i);
    }
    log.sync().unwrap();
    let names = segment_names(&dir);
    assert!(names.len() >= 2, "{names:?}");
    assert_eq!(names[0], "decisions.00000000000000000000.jsonl");
    let all = DecisionLog::read_all(&path).unwrap();
    let seqs: Vec<u64> = all.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, (0..30).collect::<Vec<_>>());
    let mut newest = Vec::new();
    DecisionLog::scan(&path, true, |e| {
        newest.push(e["seq"].as_u64().unwrap());
        true
    })
    .unwrap();
    assert_eq!(newest, (0..30).rev().collect::<Vec<_>>());

    drop(log);
    let mut reopened = DecisionLog::with_bounds(&path, 1_000, 1 << 30).unwrap();
    assert_eq!(reopened.append(entry(30), true).unwrap(), 30);
}

#[test]
fn reopening_after_a_rotation_continues_from_the_newest_segment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("decisions.jsonl");
    let mut log = DecisionLog::with_bounds(&path, 1_000, 1 << 30).unwrap();
    for i in 0..9 {
        log.append(entry(i), true).unwrap();
    }
    drop(log);
    std::fs::rename(&path, jokerd::decisions::segment_path(&path, 0)).unwrap();
    let mut log = DecisionLog::with_bounds(&path, 1_000, 1 << 30).unwrap();
    assert_eq!(log.append(entry(9), true).unwrap(), 9);
}

#[test]
fn the_oldest_segments_go_past_the_bound_and_the_gap_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("decisions.jsonl");
    let mut log = DecisionLog::with_bounds(&path, 1_000, 2_500).unwrap();
    for i in 0..200 {
        log.append(entry(i), false).unwrap();
    }
    log.sync().unwrap();
    let total: u64 = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert!(total <= 2_500 + 1_200, "{total}");
    let all = DecisionLog::read_all(&path).unwrap();
    let retention: Vec<&Value> = all.iter().filter(|e| e["record"] == "retention").collect();
    assert!(!retention.is_empty());
    let first_kept = all
        .iter()
        .map(|e| e["seq"].as_u64().unwrap())
        .min()
        .unwrap();
    let removed_to = retention
        .iter()
        .map(|e| e["to_seq"].as_u64().unwrap())
        .max()
        .unwrap();
    assert_eq!(removed_to + 1, first_kept);
    assert!(retention.iter().all(|e| e["bytes"].as_u64().unwrap() > 0));
}
