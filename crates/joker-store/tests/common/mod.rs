//! Helpers shared by the integration tests.

#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use joker_store::{
    Class, Clock, Keep, Payload, Record, Sample, SeriesId, SeriesKind, SeriesSpec, StoreConfig,
    ValueKind,
};

/// A clock the test moves by hand.
#[derive(Debug, Default)]
pub struct ManualClock {
    pub mono: AtomicU64,
    pub utc: AtomicI64,
}

impl ManualClock {
    pub fn at(utc: i64) -> Arc<Self> {
        Arc::new(Self {
            mono: AtomicU64::new(0),
            utc: AtomicI64::new(utc),
        })
    }

    pub fn advance(&self, ns: u64) {
        self.mono.fetch_add(ns, Ordering::SeqCst);
        self.utc.fetch_add(ns as i64, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn mono_ns(&self) -> u64 {
        self.mono.load(Ordering::SeqCst)
    }

    fn utc_ns(&self) -> i64 {
        self.utc.load(Ordering::SeqCst)
    }
}

pub const T0: i64 = 1_790_000_000_000_000_000;
pub const SEC: u64 = 1_000_000_000;

pub fn scalar(path: &str, keep: Keep) -> SeriesSpec {
    SeriesSpec {
        path: path.to_owned(),
        kind: SeriesKind::Scalar(ValueKind::F64),
        class: Class::Telemetry,
        keep,
    }
}

pub fn events(path: &str, class: Class) -> SeriesSpec {
    SeriesSpec {
        path: path.to_owned(),
        kind: SeriesKind::Event,
        class,
        keep: Keep::DEFAULT,
    }
}

pub fn sample(ts: i64, v: f64, seq: u64) -> Sample {
    Sample {
        ts_ns: ts,
        ts_src_ns: None,
        bits: v.to_bits(),
        quality: 0,
        seq,
    }
}

pub fn rec(series: SeriesId, s: Sample) -> Record {
    Record {
        series,
        payload: Payload::Sample(s),
    }
}

pub fn small_config() -> StoreConfig {
    StoreConfig {
        wal_file_bytes: 256 << 10,
        segment_file_bytes: 64 << 10,
        maintenance_interval_ns: u64::MAX,
        checkpoint_interval_ns: u64::MAX,
        ..StoreConfig::default()
    }
}

pub fn value(i: u64) -> f64 {
    230.0 + ((i as f64) * 0.01).sin() * 3.0 + (i % 17) as f64 * 0.001
}

/// Asserts two sample lists are equal, reporting only the first difference.
pub fn same(got: &[Sample], want: &[Sample], what: &str) {
    if let Some(i) = got.iter().zip(want).position(|(a, b)| a != b) {
        panic!(
            "{what}: first difference at {i}: {:?} vs {:?}",
            got[i], want[i]
        );
    }
    assert_eq!(got.len(), want.len(), "{what}: lengths differ");
}
