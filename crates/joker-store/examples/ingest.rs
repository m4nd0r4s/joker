//! Measures the ingestion rate and the compressed size of the historian on
//! this machine (AUR-287: target 200 k samples/s on a CM4 with eMMC).
//!
//! ```sh
//! cargo run --release -p joker-store --example ingest -- [dir] [series] [seconds]
//! ```
//!
//! Each series is sampled once per second of simulated time, with the mix
//! of a process site: analog values with a register resolution, setpoints
//! that rarely change, counters, booleans and a share of full-precision
//! noise. Two runs are reported: appends straight into the store in
//! batches, and the production path, a producer thread pushing into the
//! bounded queues drained by the writer thread.

use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

use joker_store::{
    Class, Fsync, Ingest, Keep, Payload, Record, Sample, SeriesId, SeriesKind, SeriesSpec, Store,
    StoreConfig, SystemClock, Tier, ValueKind, Writer,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Signal {
    id: SeriesId,
    profile: u8,
    value: f64,
    seq: u64,
}

impl Signal {
    fn sample(&mut self, rng: &mut Rng, ts: i64) -> Sample {
        self.seq += 1;
        let bits = match self.profile {
            0..=3 => {
                self.value += (rng.unit() - 0.5) * 0.3;
                ((self.value * 10.0).round() / 10.0).to_bits()
            }
            4..=6 => {
                if rng.next().is_multiple_of(600) {
                    self.value = (rng.next() % 100) as f64;
                }
                self.value.to_bits()
            }
            7 => {
                self.value += (rng.next() % 3) as f64;
                (self.value as u64).to_le()
            }
            8 => u64::from(rng.next().is_multiple_of(900)),
            _ => (230.0 + rng.unit()).to_bits(),
        };
        Sample {
            ts_ns: ts + (rng.next() % 50_000) as i64,
            ts_src_ns: None,
            bits,
            quality: 0,
            seq: self.seq,
        }
    }
}

fn open(dir: &std::path::Path, series: usize) -> Result<(Arc<Store>, Vec<Signal>), Box<dyn Error>> {
    let config = StoreConfig {
        fsync: Fsync::Batch,
        ..StoreConfig::default()
    };
    let (store, _) = Store::open(dir, config, Arc::new(SystemClock::new()))?;
    let keep = Keep::DEFAULT
        .with(Tier::M1, 400 * 86_400 * joker_store::SECOND_NS)
        .with(Tier::M15, 400 * 86_400 * joker_store::SECOND_NS);
    let mut signals = Vec::with_capacity(series);
    for k in 0..series {
        {
            let profile = (k % 10) as u8;
            let kind = match profile {
                7 => ValueKind::U64,
                8 => ValueKind::Other,
                _ => ValueKind::F64,
            };
            let id = store.define(SeriesSpec {
                path: format!("site.dev{}.sig{}", k / 50, k % 50),
                kind: SeriesKind::Scalar(kind),
                class: Class::Telemetry,
                keep,
            })?;
            signals.push(Signal {
                id,
                profile,
                value: 20.0 + k as f64,
                seq: 0,
            });
        }
    }
    Ok((Arc::new(store), signals))
}

fn report(name: &str, store: &Store, samples: u64, secs: f64) -> Result<(), Box<dyn Error>> {
    store.close()?;
    let stats = store.stats();
    println!(
        "{name}: {samples} samples in {secs:.2} s = {:.0} samples/s; segments {:.2} B/sample (raw + 1m + 15m), {} fsyncs, {} checkpoints",
        samples as f64 / secs,
        stats.segment_bytes as f64 / samples as f64,
        stats.fsyncs,
        stats.checkpoints,
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let base = args
        .get(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("joker-store-bench"));
    let series: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5_000);
    let seconds: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
    let t0 = 1_790_000_000_000_000_000i64;

    let dir = base.join("direct");
    let _ = std::fs::remove_dir_all(&dir);
    let (store, mut signals) = open(&dir, series)?;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut batch = Vec::with_capacity(4096);
    let started = Instant::now();
    let mut samples = 0u64;
    for second in 0..seconds {
        let ts = t0 + (second * joker_store::SECOND_NS) as i64;
        for s in &mut signals {
            batch.push(Record {
                series: s.id,
                payload: Payload::Sample(s.sample(&mut rng, ts)),
            });
            if batch.len() == 4096 {
                samples += store.append(&batch) as u64;
                batch.clear();
                store.maintain();
            }
        }
    }
    samples += store.append(&batch) as u64;
    report("direct", &store, samples, started.elapsed().as_secs_f64())?;

    let dir = base.join("queued");
    let _ = std::fs::remove_dir_all(&dir);
    let (store, mut signals) = open(&dir, series)?;
    let ingest = Arc::new(Ingest::new(4096, 262_144));
    let writer = Writer::spawn(Arc::clone(&store), Arc::clone(&ingest))?;
    let started = Instant::now();
    for second in 0..seconds {
        let ts = t0 + (second * joker_store::SECOND_NS) as i64;
        for s in &mut signals {
            let record = Record {
                series: s.id,
                payload: Payload::Sample(s.sample(&mut rng, ts)),
            };
            while !ingest.push(record.clone()) {
                std::thread::yield_now();
            }
        }
    }
    writer.stop();
    let stats = ingest.stats();
    report(
        "queued",
        &store,
        stats.queued,
        started.elapsed().as_secs_f64(),
    )?;
    println!(
        "queued: {} drops counted while the producer outran the writer (the producer retried them)",
        stats.dropped_telemetry
    );
    let _ = std::fs::remove_dir_all(&base);
    Ok(())
}
