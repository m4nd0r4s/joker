//! The node's historian (docs/01 §12, ADR 0081, ADR 0082): one
//! `joker-store` per daemon, under `<state>/store`, fed by a recorder per
//! generation that samples the signals `[telemetry.record]` catches, and by
//! the alarm journal. Producers only push into the store's bounded queues;
//! the store's own thread writes (ADR 0013).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use joker_core::ValueType;
use joker_manifest::model::Manifest;
use joker_manifest::value::{Node, Table};
use joker_store::{
    Class, DiskProbe, Fsync, Ingest, Keep, Payload, Record, Sample, SeriesId, SeriesKind,
    SeriesSpec, Store, StoreConfig, SystemClock, Tier, ValueKind, Writer,
};

use crate::pacer::Monotonic;
use crate::plan::{Plan, PlanError};
use crate::shared::Shared;

/// The series of the alarm journal (ADR 0082): not a signal path, so it
/// never collides with one.
pub const ALARM_JOURNAL: &str = "journal:alarms";
/// Room in the priority queue (alarms and decisions).
pub const PRIORITY_QUEUE: usize = 16_384;
/// Room in the telemetry queue: 4.5 MB of records, allocated and touched
/// when the historian opens. `svc/record` is its only producer and pushes
/// at most one record per recorded signal every [`RECORDER_PERIOD`], so
/// it holds 650 ms of 1,000 signals recorded `on_change` while
/// `svc/store` is busy, and 320 ms at the 200 k samples/s of docs/01 §21.
/// A full queue drops and counts (ADR 0013).
pub const TELEMETRY_QUEUE: usize = 65_536;
/// How often the recorder looks at the slots.
pub const RECORDER_PERIOD: Duration = Duration::from_millis(10);

/// What one recorded signal keeps (ref/01 §5.17.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRule {
    /// The canonical path.
    pub path: String,
    /// `every`, ns; `None` is `on_change`.
    pub every_ns: Option<u64>,
    /// `keep` per tier.
    pub keep: Keep,
}

/// `[telemetry.store]` (ref/01 §5.17.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorePolicy {
    /// `fsync`.
    pub fsync: Fsync,
    /// `fsync_interval`, ns.
    pub fsync_interval_ns: u64,
    /// `max_disk`; `None` is 25% of the partition.
    pub max_disk: Option<u64>,
}

impl Default for StorePolicy {
    fn default() -> Self {
        Self {
            fsync: Fsync::Batch,
            fsync_interval_ns: joker_store::SECOND_NS,
            max_disk: None,
        }
    }
}

/// The `[telemetry]` of a generation, resolved against its signals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TelemetryPlan {
    /// One rule per recorded signal, by path.
    pub records: Vec<RecordRule>,
    /// The store policy.
    pub store: StorePolicy,
}

fn text<'t>(table: &'t Table, key: &str) -> Option<&'t str> {
    table.get(key).and_then(|e| e.node.as_str())
}

fn duration(text: &str, what: &str) -> Result<u64, PlanError> {
    joker_units::Duration::parse(text)
        .map(joker_units::Duration::as_nanos)
        .map_err(|e| PlanError::new(Some(e.code()), format!("{what} `{text}`")))
}

fn keep_of(rule: &Table, what: &str) -> Result<Keep, PlanError> {
    let Some(keep) = rule.get("keep").and_then(|e| e.node.as_table()) else {
        return Ok(Keep::DEFAULT);
    };
    let mut out = Keep([None; 4]);
    for (layer, item) in keep {
        let tier = Tier::from_name(layer).ok_or_else(|| {
            PlanError::new(Some("J0197"), format!("{what}: unknown layer `{layer}`"))
        })?;
        let text = item.node.as_str().ok_or_else(|| {
            PlanError::new(
                Some("J0152"),
                format!("{what}.keep.{layer} is not a duration"),
            )
        })?;
        out = out.with(tier, duration(text, what)?);
    }
    Ok(out)
}

fn rank(selector: &str) -> (u32, usize) {
    if !selector.contains('*') {
        return (u32::MAX, usize::MAX);
    }
    let literal = selector.split('.').filter(|s| !s.contains('*')).count() as u32;
    (literal, selector.find('*').unwrap_or(selector.len()))
}

fn rules_of(table: &Table, out: &mut Vec<(String, Table)>) {
    for (selector, entry) in table {
        if let Some(rule) = entry.node.as_table() {
            out.push((selector.clone(), rule.clone()));
        }
    }
}

/// Resolves `[telemetry.record]`, the `record` shorthand of `[signals]`
/// and of `[devices.<d>.signals]`, and `[telemetry.store]` against `plan`: for each signal, the most
/// specific entry (ref/01 §5.17.1). Scalar signals only; the wide types
/// and the event and block shapes are not recorded in 1.0 (ADR 0082).
pub fn telemetry_plan(manifest: &Manifest, plan: &Plan) -> Result<TelemetryPlan, PlanError> {
    let telemetry = manifest.pending.get("telemetry").and_then(Node::as_table);
    let mut entries = Vec::new();
    if let Some(record) = telemetry
        .and_then(|t| t.get("record"))
        .and_then(|e| e.node.as_table())
    {
        rules_of(record, &mut entries);
    }
    if let Some(signals) = manifest.pending.get("signals").and_then(Node::as_table) {
        for (selector, entry) in signals {
            if let Some(rule) = entry
                .node
                .as_table()
                .and_then(|t| t.get("record"))
                .and_then(|e| e.node.as_table())
            {
                entries.push((selector.clone(), rule.clone()));
            }
        }
    }
    for (device, d) in &manifest.devices {
        for (name, signal) in &d.signals {
            if let Some(rule) = signal.value.get("record").and_then(Node::as_table) {
                entries.push((format!("{device}.{name}"), rule.clone()));
            }
        }
    }
    let mut records = Vec::new();
    for signal in &plan.signals {
        if matches!(
            signal.ty,
            ValueType::String | ValueType::Bytes | ValueType::Record
        ) {
            continue;
        }
        let best = entries
            .iter()
            .filter(|(sel, _)| crate::wire::matches(sel, &signal.path))
            .max_by_key(|(sel, _)| rank(sel));
        let Some((selector, rule)) = best else {
            continue;
        };
        let what = format!("telemetry.record.\"{selector}\"");
        let every_ns = text(rule, "every")
            .map(|t| duration(t, &what))
            .transpose()?;
        records.push(RecordRule {
            path: signal.path.clone(),
            every_ns,
            keep: keep_of(rule, &what)?,
        });
    }
    let mut store = StorePolicy::default();
    if let Some(t) = telemetry
        .and_then(|t| t.get("store"))
        .and_then(|e| e.node.as_table())
    {
        if let Some(f) = text(t, "fsync") {
            store.fsync = Fsync::from_name(f).ok_or_else(|| {
                PlanError::new(Some("J0145"), format!("telemetry.store.fsync `{f}`"))
            })?;
        }
        if let Some(i) = text(t, "fsync_interval") {
            store.fsync_interval_ns = duration(i, "telemetry.store.fsync_interval")?;
        }
        if let Some(m) = text(t, "max_disk") {
            store.max_disk = Some(joker_manifest::grammar::size(m).ok_or_else(|| {
                PlanError::new(Some("J0145"), format!("telemetry.store.max_disk `{m}`"))
            })?);
        }
    }
    Ok(TelemetryPlan { records, store })
}

/// How the bits of a signal read as a number in the aggregates.
pub fn value_kind(ty: ValueType) -> ValueKind {
    match ty {
        ValueType::F64 => ValueKind::F64,
        ValueType::F32 => ValueKind::F32,
        ValueType::I64 => ValueKind::I64,
        ValueType::U64 => ValueKind::U64,
        _ => ValueKind::Other,
    }
}

#[cfg(target_os = "linux")]
fn probe() -> Option<DiskProbe> {
    Some(Arc::new(|dir: &Path| {
        let s = nix::sys::statvfs::statvfs(dir).ok()?;
        let block = s.fragment_size() as u64;
        Some(joker_store::DiskSpace {
            free: (s.blocks_available() as u64).saturating_mul(block),
            total: (s.blocks() as u64).saturating_mul(block),
        })
    }))
}

#[cfg(not(target_os = "linux"))]
fn probe() -> Option<DiskProbe> {
    None
}

/// The historian of a daemon.
pub struct Historian {
    store: Arc<Store>,
    ingest: Arc<Ingest>,
    writer: Mutex<Option<Writer>>,
    alarms: SeriesId,
    probe: Option<DiskProbe>,
    dir: PathBuf,
}

impl std::fmt::Debug for Historian {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Historian").finish_non_exhaustive()
    }
}

impl Historian {
    /// Opens the store in `dir`, recovering it, and starts its writer.
    pub fn open(dir: &Path) -> io::Result<Self> {
        Self::open_with(dir, probe())
    }

    /// [`Historian::open`] with another disk probe (tests).
    pub fn open_with(dir: &Path, probe: Option<DiskProbe>) -> io::Result<Self> {
        let config = StoreConfig {
            probe: probe.clone(),
            ..StoreConfig::default()
        };
        let (store, recovery) = Store::open(dir, config, Arc::new(SystemClock::new()))?;
        if recovery.wal_torn_bytes > 0 || recovery.segment_torn_bytes > 0 {
            eprintln!("jokerd: historian recovered: {recovery:?}");
        }
        let store = Arc::new(store);
        let alarms = store.define(SeriesSpec {
            path: ALARM_JOURNAL.into(),
            kind: SeriesKind::Event,
            class: Class::Alarm,
            keep: Keep([
                Some(400 * 86_400 * joker_store::SECOND_NS),
                None,
                None,
                None,
            ]),
        })?;
        let ingest = Arc::new(Ingest::new(PRIORITY_QUEUE, TELEMETRY_QUEUE));
        let writer = Writer::spawn(Arc::clone(&store), Arc::clone(&ingest))?;
        Ok(Self {
            store,
            ingest,
            writer: Mutex::new(Some(writer)),
            alarms,
            probe,
            dir: dir.to_owned(),
        })
    }

    /// The store, for queries.
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// The queues producers push into.
    pub fn ingest(&self) -> &Arc<Ingest> {
        &self.ingest
    }

    /// The alarm journal's series.
    pub fn alarm_series(&self) -> SeriesId {
        self.alarms
    }

    /// Applies a generation's `[telemetry.store]`; without `max_disk`, a
    /// quarter of the partition, or 8 GiB when it cannot be measured.
    pub fn configure(&self, policy: &StorePolicy) {
        let max_disk = policy.max_disk.unwrap_or_else(|| {
            self.probe
                .as_ref()
                .and_then(|p| p(&self.dir))
                .map_or(8 << 30, |d| d.total / 4)
        });
        self.store
            .set_policy(policy.fsync, policy.fsync_interval_ns, max_disk);
    }

    /// Defines the series of a generation's recorded signals.
    pub fn series_for(&self, plan: &Plan) -> io::Result<Vec<(usize, SeriesId, Option<u64>)>> {
        let mut specs = Vec::with_capacity(plan.telemetry.records.len());
        let mut slots = Vec::with_capacity(plan.telemetry.records.len());
        for rule in &plan.telemetry.records {
            let (Some(id), Some(signal)) = (
                plan.id(&rule.path),
                plan.signals.iter().find(|s| s.path == rule.path),
            ) else {
                continue;
            };
            specs.push(SeriesSpec {
                path: rule.path.clone(),
                kind: SeriesKind::Scalar(value_kind(signal.ty)),
                class: Class::Telemetry,
                keep: rule.keep,
            });
            slots.push((id.index(), rule.every_ns));
        }
        let ids = self.store.define_all(specs)?;
        Ok(slots
            .into_iter()
            .zip(ids)
            .map(|((slot, every), id)| (slot, id, every))
            .collect())
    }

    /// Stops the writer after it drained the queues, and closes the store.
    pub fn stop(&self) {
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(w) = writer {
            w.stop();
            if let Err(e) = self.store.close() {
                eprintln!("jokerd: historian: {e}");
            }
        }
    }
}

impl Drop for Historian {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Watch {
    slot: usize,
    series: SeriesId,
    every_ns: Option<u64>,
    next_due: u64,
    last_seq: u64,
}

/// The thread of a generation that samples its recorded signals
/// (`svc/record`): `every` takes the last value at its period, `on_change`
/// each new `seq` seen at the recorder's period (ADR 0082).
#[derive(Debug)]
pub struct Recorder {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Starts recording `watches` (slot index, series, `every`) of
    /// `shared` into `ingest`.
    pub fn spawn(
        shared: Arc<Shared>,
        ingest: Arc<Ingest>,
        watches: Vec<(usize, SeriesId, Option<u64>)>,
        utc_origin_ns: i64,
        clock: Monotonic,
    ) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let mut watches: Vec<Watch> = watches
            .into_iter()
            .map(|(slot, series, every_ns)| Watch {
                slot,
                series,
                every_ns,
                next_due: 0,
                last_seq: 0,
            })
            .collect();
        let handle = std::thread::Builder::new()
            .name("svc/record".into())
            .stack_size(crate::shared::SERVICE_STACK)
            .spawn(move || {
                while !flag.load(Ordering::Acquire) {
                    let now = clock.now_ns();
                    for w in &mut watches {
                        if let Some(every) = w.every_ns {
                            if now < w.next_due {
                                continue;
                            }
                            w.next_due = now.saturating_add(every);
                        }
                        let Some(sample) = shared.slots.get(w.slot).and_then(|s| s.read(8).ok())
                        else {
                            continue;
                        };
                        if sample.seq() == 0 || (w.every_ns.is_none() && sample.seq() == w.last_seq)
                        {
                            continue;
                        }
                        w.last_seq = sample.seq();
                        ingest.push(Record {
                            series: w.series,
                            payload: Payload::Sample(Sample {
                                ts_ns: utc_origin_ns.saturating_add(sample.ts_acq() as i64),
                                ts_src_ns: sample.ts_src(),
                                bits: sample.transport_bits(),
                                quality: sample.quality().to_bits(),
                                seq: sample.seq(),
                            }),
                        });
                    }
                    std::thread::sleep(RECORDER_PERIOD);
                }
            })?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Stops the thread.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Recorded series by path, for the API.
pub type SeriesIndex = BTreeMap<String, SeriesId>;
