//! The store's behaviour on a healthy disk: round trips, tiers, events,
//! retention by time and size, the critical disk, bounded ingestion and
//! cursors.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::*;
use joker_store::{
    Class, DiskSpace, Event, Ingest, Keep, Payload, Record, Store, StoreConfig, Tier, ValueKind,
    Writer, aggregate,
};

#[test]
fn samples_round_trip_through_sealed_blocks_and_a_clean_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let (store, _) = Store::open(dir.path(), small_config(), clock.clone()).unwrap();
    let a = store.define(scalar("meter.p", Keep::DEFAULT)).unwrap();
    let b = store.define(scalar("meter.q", Keep::DEFAULT)).unwrap();
    let mut expected = Vec::new();
    for chunk in 0..50u64 {
        let mut batch = Vec::new();
        for i in chunk * 200..(chunk + 1) * 200 {
            let s = sample(T0 + (i * SEC) as i64 + (i % 3) as i64 * 1_000, value(i), i);
            expected.push(s);
            batch.push(rec(a, s));
            batch.push(rec(b, sample(s.ts_ns, -value(i), i)));
        }
        assert_eq!(store.append(&batch), 400);
    }
    let read: Vec<_> = store
        .read_raw(a, i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|(_, s)| s)
        .collect();
    assert_eq!(read, expected);
    store.close().unwrap();
    drop(store);

    let (store, recovery) = Store::open(dir.path(), small_config(), clock).unwrap();
    assert_eq!(
        recovery.segments_scanned, 0,
        "a clean close leaves every index"
    );
    assert_eq!(
        recovery.wal_records, 0,
        "a clean close leaves only snapshots"
    );
    let read: Vec<_> = store
        .read_raw(a, i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|(_, s)| s)
        .collect();
    assert_eq!(read, expected);
    assert_eq!(store.last_n(a), 10_000);
    let stats = store.stats();
    let per_sample = stats.segment_bytes as f64 / 20_000.0;
    assert!(per_sample < 10.0, "{per_sample} bytes per sample");
}

#[test]
fn ranges_limits_and_sequence_numbers_page_through_a_series() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = Store::open(dir.path(), small_config(), ManualClock::at(T0)).unwrap();
    let a = store.define(scalar("pcs.p_set", Keep::DEFAULT)).unwrap();
    let batch: Vec<_> = (0..3000u64)
        .map(|i| rec(a, sample(T0 + (i * SEC) as i64, value(i), i)))
        .collect();
    store.append(&batch);
    let from = T0 + 1000 * SEC as i64;
    let to = T0 + 1999 * SEC as i64;
    let mut after = 0;
    let mut got = Vec::new();
    loop {
        let page = store.read_raw(a, from, to, after, 128).unwrap();
        let Some(&(n, _)) = page.last() else { break };
        after = n;
        got.extend(page);
    }
    assert_eq!(got.len(), 1000);
    assert_eq!(got[0].1.seq, 1000);
    assert_eq!(got[999].1.seq, 1999);
    assert!(got.windows(2).all(|w| w[0].0 + 1 == w[1].0));
}

#[test]
fn aggregate_tiers_match_the_raw_samples() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = Store::open(dir.path(), small_config(), ManualClock::at(T0)).unwrap();
    let keep = Keep::DEFAULT
        .with(Tier::S1, 86_400 * SEC)
        .with(Tier::M1, 400 * 86_400 * SEC)
        .with(Tier::M15, 400 * 86_400 * SEC);
    let a = store.define(scalar("meter.f", keep)).unwrap();
    let mut raw = Vec::new();
    for i in 0..20_000u64 {
        let mut s = sample(T0 + (i * SEC / 4) as i64, value(i), i);
        if i % 97 == 0 {
            s.quality = 0x0302;
            s.bits = f64::NAN.to_bits();
        }
        raw.push((i + 1, s));
    }
    let records: Vec<_> = raw.iter().map(|(_, s)| rec(a, *s)).collect();
    for chunk in records.chunks(1000) {
        store.append(chunk);
    }
    for (tier, step) in [
        (Tier::S1, SEC),
        (Tier::M1, 60 * SEC),
        (Tier::M15, 900 * SEC),
    ] {
        let stored = store
            .read_agg(a, tier, i64::MIN, i64::MAX, usize::MAX)
            .unwrap();
        let mut direct = aggregate(ValueKind::F64, &raw, step);
        direct.pop();
        assert_eq!(stored.len(), direct.len(), "{tier:?}");
        for (x, y) in stored.iter().zip(&direct) {
            assert_eq!(
                (x.ts_ns, x.count, x.valid, x.quality),
                (y.ts_ns, y.count, y.valid, y.quality)
            );
            assert_eq!((x.min, x.max, x.last), (y.min, y.max, y.last));
            assert!((x.mean - y.mean).abs() < 1e-9);
            assert!(!x.mean.is_nan(), "a Bad sample never becomes a number");
        }
    }
    assert!(store.coverage(a, Tier::M15).is_some());
}

#[test]
fn events_round_trip_and_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let (store, _) = Store::open(dir.path(), small_config(), clock.clone()).unwrap();
    let e = store
        .define(events("alarm:grid_overfreq", Class::Alarm))
        .unwrap();
    let mut expected = Vec::new();
    let mut batch = Vec::new();
    for i in 0..2_000u64 {
        let ev = Event {
            ts_ns: T0 + i as i64,
            ts_src_ns: None,
            seq: i,
            payload: format!("{{\"kind\":\"transition\",\"to\":\"active_unacked\",\"i\":{i}}}")
                .into_bytes(),
        };
        expected.push(ev.clone());
        batch.push(Record {
            series: e,
            payload: Payload::Event(ev),
        });
        if batch.len() == 100 {
            store.append(&batch);
            batch.clear();
        }
    }
    drop(store);
    let (store, recovery) = Store::open(dir.path(), small_config(), clock).unwrap();
    assert!(recovery.wal_records > 0);
    let got: Vec<_> = store
        .read_events(e, i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|(_, e)| e)
        .collect();
    assert_eq!(got, expected);
}

#[test]
fn retention_by_time_deletes_whole_files_past_their_keep() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let (store, _) = Store::open(dir.path(), small_config(), clock.clone()).unwrap();
    let short = store
        .define(scalar("short", Keep([Some(3600 * SEC), None, None, None])))
        .unwrap();
    let long = store
        .define(scalar(
            "long",
            Keep([Some(30 * 86_400 * SEC), None, None, None]),
        ))
        .unwrap();
    for hour in 0..6u64 {
        let batch: Vec<_> = (0..3600u64)
            .flat_map(|s| {
                let i = hour * 3600 + s;
                let smp = sample(T0 + (i * SEC) as i64, value(i), i);
                [rec(short, smp), rec(long, smp)]
            })
            .collect();
        store.append(&batch);
    }
    clock
        .utc
        .store(T0 + (6 * 3600 * SEC) as i64, Ordering::SeqCst);
    store.checkpoint().unwrap();
    store.enforce_retention().unwrap();
    let cutoff = T0 + (5 * 3600 * SEC) as i64;
    let kept = store
        .read_raw(short, i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap();
    assert!(store.stats().expired_files > 0);
    let first = kept.first().unwrap().1.ts_ns;
    assert!(
        first >= cutoff - (3600 * SEC) as i64,
        "kept more than keep plus one file span"
    );
    assert!(first > T0, "the oldest hours are gone");
    let all = store
        .read_raw(long, i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap();
    assert_eq!(all.len(), 6 * 3600);
}

#[test]
fn retention_by_size_drops_the_oldest_raw_telemetry_first() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let config = StoreConfig {
        max_disk: 512 << 10,
        ..small_config()
    };
    let (store, _) = Store::open(dir.path(), config, clock.clone()).unwrap();
    let keep = Keep::DEFAULT.with(Tier::M1, 400 * 86_400 * SEC);
    let series: Vec<_> = (0..8)
        .map(|k| store.define(scalar(&format!("inv{k}.p"), keep)).unwrap())
        .collect();
    let alarms = store.define(events("alarm:log", Class::Alarm)).unwrap();
    let mut alarm_count = 0;
    for step in 0..40u64 {
        let mut batch = Vec::new();
        for i in step * 1000..(step + 1) * 1000 {
            for (k, s) in series.iter().enumerate() {
                batch.push(rec(
                    *s,
                    sample(
                        T0 + (i * SEC) as i64,
                        value(i * 7 + k as u64) * (i as f64).cos(),
                        i,
                    ),
                ));
            }
        }
        batch.push(Record {
            series: alarms,
            payload: Payload::Event(Event {
                ts_ns: T0 + (step * 1000 * SEC) as i64,
                ts_src_ns: None,
                seq: step,
                payload: b"{\"kind\":\"transition\"}".to_vec(),
            }),
        });
        alarm_count += 1;
        store.append(&batch);
        store.enforce_retention().unwrap();
    }
    let stats = store.stats();
    assert!(stats.evicted_files > 0);
    assert!(
        stats.segment_bytes + stats.wal_bytes <= 512 << 10,
        "{} bytes on disk",
        stats.segment_bytes + stats.wal_bytes
    );
    let raw = store
        .read_raw(series[0], i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap();
    let agg = store
        .read_agg(series[0], Tier::M1, i64::MIN, i64::MAX, usize::MAX)
        .unwrap();
    assert!(
        raw.first().unwrap().1.ts_ns > T0,
        "the oldest raw went first"
    );
    assert!(
        agg.first().unwrap().ts_ns < raw.first().unwrap().1.ts_ns,
        "aggregates outlive raw"
    );
    let kept = store
        .read_events(alarms, i64::MIN, i64::MAX, 0, usize::MAX)
        .unwrap();
    assert_eq!(kept.len(), alarm_count, "alarms are deleted last");
}

#[test]
fn a_critical_disk_keeps_alarms_and_decisions_and_counts_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let full = Arc::new(AtomicBool::new(false));
    let probe_full = Arc::clone(&full);
    let config = StoreConfig {
        probe: Some(Arc::new(move |_| {
            Some(DiskSpace {
                free: if probe_full.load(Ordering::SeqCst) {
                    5
                } else {
                    500
                },
                total: 1000,
            })
        })),
        ..small_config()
    };
    let (store, _) = Store::open(dir.path(), config, ManualClock::at(T0)).unwrap();
    let t = store.define(scalar("meter.p", Keep::DEFAULT)).unwrap();
    let d = store.define(events("decisions", Class::Decision)).unwrap();
    let event = |seq| Record {
        series: d,
        payload: Payload::Event(Event {
            ts_ns: T0,
            ts_src_ns: None,
            seq,
            payload: b"{}".to_vec(),
        }),
    };
    assert_eq!(store.append(&[rec(t, sample(T0, 1.0, 1)), event(1)]), 2);
    full.store(true, Ordering::SeqCst);
    store.enforce_retention().unwrap();
    assert!(store.stats().disk_full);
    assert_eq!(store.append(&[rec(t, sample(T0 + 1, 2.0, 2)), event(2)]), 1);
    assert_eq!(store.stats().dropped_disk_full, 1);
    full.store(false, Ordering::SeqCst);
    store.enforce_retention().unwrap();
    assert!(!store.stats().disk_full);
    assert_eq!(store.append(&[rec(t, sample(T0 + 2, 3.0, 3))]), 1);
    let values: Vec<u64> = store
        .read_raw(t, i64::MIN, i64::MAX, 0, 10)
        .unwrap()
        .iter()
        .map(|(_, s)| s.seq)
        .collect();
    assert_eq!(values, vec![1, 3]);
    assert_eq!(
        store
            .read_events(d, i64::MIN, i64::MAX, 0, 10)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn full_queues_drop_and_count_without_blocking_and_priority_has_its_own() {
    let ingest = Ingest::new(2, 3);
    let r = |seq| rec(joker_store::SeriesId(0), sample(T0, 0.0, seq));
    let accepted = (0..10).filter(|i| ingest.push(r(*i))).count();
    assert_eq!(accepted, 3);
    assert!(ingest.push_priority(r(100)));
    assert!(ingest.push_priority(r(101)));
    assert!(!ingest.push_priority(r(102)));
    let stats = ingest.stats();
    assert_eq!(
        (
            stats.dropped_telemetry,
            stats.dropped_priority,
            stats.pending
        ),
        (7, 1, 5)
    );
    let mut out = Vec::new();
    ingest.drain(&mut out, 3);
    let order: Vec<u64> = out
        .iter()
        .map(|r| match &r.payload {
            Payload::Sample(s) => s.seq,
            Payload::Event(e) => e.seq,
        })
        .collect();
    assert_eq!(order, vec![100, 101, 0], "priority first");
}

#[test]
fn the_writer_thread_drains_the_queues_into_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = Store::open(
        dir.path(),
        StoreConfig::default(),
        Arc::new(joker_store::SystemClock::new()),
    )
    .unwrap();
    let store = Arc::new(store);
    let a = store.define(scalar("meter.p", Keep::DEFAULT)).unwrap();
    let ingest = Arc::new(Ingest::new(1024, 65_536));
    let writer = Writer::spawn(Arc::clone(&store), Arc::clone(&ingest)).unwrap();
    for i in 0..50_000u64 {
        while !ingest.push(rec(a, sample(T0 + i as i64, value(i), i))) {
            std::thread::yield_now();
        }
    }
    writer.stop();
    assert_eq!(
        store
            .read_raw(a, i64::MIN, i64::MAX, 0, usize::MAX)
            .unwrap()
            .len(),
        50_000 - ingest.stats().dropped_telemetry as usize
    );
}

#[test]
fn cursors_persist_per_consumer() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let (store, _) = Store::open(dir.path(), small_config(), clock.clone()).unwrap();
    let a = store.define(scalar("meter.p", Keep::DEFAULT)).unwrap();
    let batch: Vec<_> = (0..100)
        .map(|i| rec(a, sample(T0 + i, 0.0, i as u64)))
        .collect();
    store.append(&batch);
    let page = store.read_raw(a, i64::MIN, i64::MAX, 0, 40).unwrap();
    let mut cursor = BTreeMap::new();
    cursor.insert("meter.p".to_owned(), page.last().unwrap().0);
    store.set_cursor("cloud", &cursor).unwrap();
    drop(store);
    let (store, _) = Store::open(dir.path(), small_config(), clock).unwrap();
    let cursor = store.cursor("cloud").unwrap();
    let rest = store
        .read_raw(a, i64::MIN, i64::MAX, cursor["meter.p"], usize::MAX)
        .unwrap();
    assert_eq!(rest.len(), 60);
    assert_eq!(rest[0].1.seq, 40);
}

#[test]
fn a_new_keep_adds_a_tier_and_keeps_what_was_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = Store::open(dir.path(), small_config(), ManualClock::at(T0)).unwrap();
    let a = store.define(scalar("meter.p", Keep::DEFAULT)).unwrap();
    store.append(
        &(0..500u64)
            .map(|i| rec(a, sample(T0 + (i * SEC) as i64, 1.0, i)))
            .collect::<Vec<_>>(),
    );
    let again = store
        .define(scalar(
            "meter.p",
            Keep::DEFAULT.with(Tier::M1, 86_400 * SEC),
        ))
        .unwrap();
    assert_eq!(a, again);
    store.append(
        &(500..1000u64)
            .map(|i| rec(a, sample(T0 + (i * SEC) as i64, 2.0, i)))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        store
            .read_raw(a, i64::MIN, i64::MAX, 0, usize::MAX)
            .unwrap()
            .len(),
        1000
    );
    let agg = store
        .read_agg(a, Tier::M1, i64::MIN, i64::MAX, usize::MAX)
        .unwrap();
    assert!(!agg.is_empty());
    assert!(agg.iter().all(|x| x.mean == 2.0));
}

#[test]
fn many_series_are_defined_with_one_sync_and_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::at(T0);
    let (store, _) = Store::open(dir.path(), small_config(), clock.clone()).unwrap();
    let specs: Vec<_> = (0..2000)
        .map(|k| scalar(&format!("dev{k}.p"), Keep::DEFAULT))
        .collect();
    let ids = store.define_all(specs.clone()).unwrap();
    assert_eq!(ids.len(), 2000);
    assert_eq!(store.define_all(specs).unwrap(), ids);
    store.append(&[rec(ids[1999], sample(T0, 1.0, 1))]);
    drop(store);
    let (store, _) = Store::open(dir.path(), small_config(), clock).unwrap();
    assert_eq!(store.series("dev1999.p"), Some(ids[1999]));
    assert_eq!(store.list().len(), 2000);
    assert_eq!(
        store
            .read_raw(ids[1999], i64::MIN, i64::MAX, 0, 10)
            .unwrap()
            .len(),
        1
    );
}
