//! Durability: a store that is dropped without closing, a process that is
//! killed, and a power cut that loses every byte not yet synced (docs/01
//! §12: "at most 1 s is lost in a power outage" with `fsync = "batch"`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::*;
use joker_store::{Class, Event, Fsync, Keep, Payload, Record, SeriesId, Store, StoreConfig, Tier};

fn keep() -> Keep {
    Keep::DEFAULT
        .with(Tier::S1, 86_400 * SEC)
        .with(Tier::M1, 86_400 * SEC)
}

fn read_all(store: &Store, id: SeriesId) -> Vec<joker_store::Sample> {
    store
        .read_raw(id, i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|(_, s)| s)
        .collect()
}

#[test]
fn restarts_without_a_clean_close_lose_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let mut expected: Vec<Vec<joker_store::Sample>> = vec![Vec::new(); 3];
    let mut i = 0u64;
    for round in 0..6 {
        let (store, recovery) = Store::open(dir.path(), small_config(), clock.clone()).unwrap();
        if round > 0 {
            assert!(recovery.wal_records > 0 || recovery.snapshots > 0);
        }
        let ids: Vec<_> = (0..3)
            .map(|k| store.define(scalar(&format!("s{k}"), keep())).unwrap())
            .collect();
        for (k, id) in ids.iter().enumerate() {
            same(
                &read_all(&store, *id),
                &expected[k],
                &format!("round {round}, series {k}"),
            );
        }
        for _ in 0..(2_000 + round * 777) {
            let batch: Vec<_> = ids
                .iter()
                .enumerate()
                .map(|(k, id)| {
                    let s = sample(T0 + (i * SEC / 10) as i64, value(i + k as u64), i);
                    expected[k].push(s);
                    rec(*id, s)
                })
                .collect();
            store.append(&batch);
            i += 1;
        }
        if round % 2 == 1 {
            store.checkpoint().unwrap();
        }
        drop(store);
    }
    let (store, _) = Store::open(dir.path(), small_config(), clock).unwrap();
    let id = store.series("s1").unwrap();
    same(&read_all(&store, id), &expected[1], "final");
    let agg = store
        .read_agg(id, Tier::M1, i64::MIN, i64::MAX, usize::MAX)
        .unwrap();
    let direct = joker_store::aggregate(
        joker_store::ValueKind::F64,
        &expected[1]
            .iter()
            .enumerate()
            .map(|(n, s)| (n as u64 + 1, *s))
            .collect::<Vec<_>>(),
        60 * SEC,
    );
    assert_eq!(
        agg.len(),
        direct.len() - 1,
        "aggregates survive every restart"
    );
    for (a, b) in agg.iter().zip(&direct) {
        assert_eq!(
            (a.ts_ns, a.count, a.min, a.max),
            (b.ts_ns, b.count, b.min, b.max)
        );
    }
}

/// Emulates a power cut: every file keeps only the bytes known to be on
/// stable storage, followed by a torn write.
fn power_cut(store: Store) {
    let durable = store.durability();
    drop(store);
    for (path, synced) in durable {
        let Ok(mut f) = OpenOptions::new().append(true).open(&path) else {
            continue;
        };
        f.set_len(synced).unwrap();
        f.write_all(&[0xA5; 37]).unwrap();
    }
}

fn power_cut_run(policy: Fsync, seconds_ms: u64) -> (i64, i64, usize) {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let config = StoreConfig {
        fsync: policy,
        ..small_config()
    };
    let (store, _) = Store::open(dir.path(), config.clone(), clock.clone()).unwrap();
    let ids: Vec<_> = (0..20)
        .map(|k| store.define(scalar(&format!("inv{k}.p"), keep())).unwrap())
        .collect();
    let alarms = store.define(events("alarms", Class::Alarm)).unwrap();
    let mut appended: Vec<Vec<joker_store::Sample>> = vec![Vec::new(); ids.len()];
    let mut alarm_events = Vec::new();
    let mut t = 0u64;
    let step_ms = 10;
    while t < seconds_ms {
        let mut batch = Vec::new();
        for ms in 0..step_ms {
            let now = T0 + ((t + ms) * 1_000_000) as i64;
            for (k, id) in ids.iter().enumerate() {
                let s = sample(now, value(t + ms + k as u64), t + ms);
                appended[k].push(s);
                batch.push(rec(*id, s));
            }
        }
        if t.is_multiple_of(1_330) {
            let e = Event {
                ts_ns: T0 + (t * 1_000_000) as i64,
                ts_src_ns: None,
                seq: t,
                payload: b"{\"kind\":\"transition\"}".to_vec(),
            };
            alarm_events.push(e.clone());
            batch.push(Record {
                series: alarms,
                payload: Payload::Event(e),
            });
        }
        store.append(&batch);
        clock.advance(step_ms * 1_000_000);
        t += step_ms;
        store.maintain();
    }
    power_cut(store);

    let (store, recovery) = Store::open(dir.path(), config, clock).unwrap();
    assert!(
        recovery.wal_torn_bytes > 0,
        "the torn write was found and cut"
    );
    let mut worst = i64::MAX;
    let mut kept = 0;
    for (k, id) in ids.iter().enumerate() {
        let got = read_all(&store, *id);
        same(
            &got,
            &appended[k][..got.len().min(appended[k].len())],
            "what survives is a prefix, without holes",
        );
        kept += got.len();
        worst = worst.min(got.last().map_or(T0, |s| s.ts_ns));
    }
    let events: Vec<_> = store
        .read_events(alarms, i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|(_, e)| e)
        .collect();
    assert_eq!(events, alarm_events, "alarm records are synced at once");
    let last = appended[0].last().unwrap().ts_ns;
    (last, worst, kept)
}

#[test]
fn a_power_cut_with_batch_loses_at_most_one_second() {
    for run_ms in [2_340, 5_000, 7_770, 12_010] {
        let (last, survived, _) = power_cut_run(Fsync::Batch, run_ms);
        let lost = last - survived;
        assert!(lost <= SEC as i64, "{run_ms} ms run lost {lost} ns");
    }
}

#[test]
fn a_power_cut_with_always_loses_nothing() {
    let (last, survived, _) = power_cut_run(Fsync::Always, 3_210);
    assert_eq!(last, survived);
}

#[test]
fn a_corrupt_segment_tail_is_cut_and_rebuilt_from_the_wal() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let config = StoreConfig {
        wal_file_bytes: u64::MAX,
        ..small_config()
    };
    let (store, _) = Store::open(dir.path(), config, clock.clone()).unwrap();
    let id = store.define(scalar("meter.p", Keep::DEFAULT)).unwrap();
    let batch: Vec<_> = (0..20_000u64)
        .map(|i| rec(id, sample(T0 + (i * SEC) as i64, value(i), i)))
        .collect();
    store.append(&batch);
    let expected = read_all(&store, id);
    let segments: Vec<_> = store
        .durability()
        .into_iter()
        .filter(|(p, _)| p.extension().is_some_and(|e| e == "seg"))
        .collect();
    drop(store);
    let (path, _) = segments.last().unwrap();
    let len = std::fs::metadata(path).unwrap().len();
    let f = OpenOptions::new().write(true).open(path).unwrap();
    f.set_len(len - 100).unwrap();
    let (store, recovery) = Store::open(dir.path(), small_config(), clock).unwrap();
    assert!(recovery.segment_torn_bytes > 0);
    same(&read_all(&store, id), &expected, "after the cut");
}

const CHILD_DIR: &str = "JOKER_STORE_CRASH_CHILD";

/// The child of [`a_killed_process_loses_nothing_it_acknowledged`]: writes
/// until it is killed, printing what each append made durable against a
/// process crash.
#[test]
fn crash_child() {
    let Ok(dir) = std::env::var(CHILD_DIR) else {
        return;
    };
    let config = StoreConfig {
        fsync: Fsync::Never,
        ..small_config()
    };
    let (store, _) = Store::open(Path::new(&dir), config, ManualClock::at(T0)).unwrap();
    let id = store.define(scalar("meter.p", keep())).unwrap();
    let mut out = std::io::stdout().lock();
    let mut i = store.last_n(id);
    loop {
        let batch: Vec<_> = (0..500)
            .map(|k| {
                rec(
                    id,
                    sample(T0 + ((i + k) * 1_000_000) as i64, value(i + k), i + k),
                )
            })
            .collect();
        store.append(&batch);
        i += 500;
        store.maintain();
        writeln!(out, "acked {i}").unwrap();
        out.flush().unwrap();
    }
}

#[test]
fn a_killed_process_loses_nothing_it_acknowledged() {
    let dir = tempfile::tempdir().unwrap();
    let mut acked_total = 0;
    for round in 0..3 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_child", "--nocapture", "--test-threads=1"])
            .env(CHILD_DIR, dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut last = 0u64;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(n) = line.strip_prefix("acked ").and_then(|n| n.parse().ok()) {
                    last = n;
                }
            }
            last
        });
        std::thread::sleep(Duration::from_millis(300 + round * 150));
        child.kill().unwrap();
        child.wait().unwrap();
        let acked = reader.join().unwrap();
        assert!(acked > acked_total, "the child made progress");
        acked_total = acked;
        let (store, recovery) =
            Store::open(dir.path(), small_config(), ManualClock::at(T0)).unwrap();
        assert_eq!(recovery.orphan_blocks, 0);
        let id = store.series("meter.p").unwrap();
        let got = read_all(&store, id);
        assert!(
            got.len() as u64 >= acked,
            "round {round}: {} < {acked}",
            got.len()
        );
        assert!(
            got.iter().enumerate().all(|(k, s)| s.seq == k as u64),
            "no holes, no duplicates"
        );
        store.close().unwrap();
    }
}
