//! The store: catalog, WAL, open blocks, segments, checkpoints, retention
//! and queries (docs/01 §12, ADR 0081).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::agg::{Bucket, bucket_start};
use crate::block::{self, BlockKind, Body, Open, Rows};
use crate::codec::{AggEncoder, RawEncoder};
use crate::frame::{self, Reader};
use crate::ingest::{Payload, Record};
use crate::segment::{self, BlockRef, Key, Segments};
use crate::wal::{self, Wal};
use crate::{
    Aggregate, Class, Clock, Event, Fsync, Keep, Sample, SeriesId, SeriesKind, SeriesSpec,
    StoreConfig, StoreStats, Tier, ValueKind,
};

const TAG_SAMPLE: u8 = 1;
const TAG_EVENT: u8 = 2;
const TAG_SNAPSHOT: u8 = 3;
const NO_KEEP: u64 = u64::MAX;
const SNAPSHOT_FRAME: usize = 1 << 20;

/// What opening a store found and repaired.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recovery {
    /// WAL files replayed.
    pub wal_files: u64,
    /// Samples and events replayed from the WAL.
    pub wal_records: u64,
    /// Open-block snapshots restored.
    pub snapshots: u64,
    /// Bytes past the last valid frame of the WAL files (torn writes).
    pub wal_torn_bytes: u64,
    /// Segment files read through their index.
    pub segments_indexed: u64,
    /// Segment files scanned block by block.
    pub segments_scanned: u64,
    /// Bytes cut from torn segment tails.
    pub segment_torn_bytes: u64,
    /// Bytes past the last valid record of the catalog.
    pub catalog_torn_bytes: u64,
    /// Sealed blocks of series the catalog does not know.
    pub orphan_blocks: u64,
}

#[derive(Debug)]
struct TierState {
    open: Option<Open>,
    bucket: Option<Bucket>,
    last_n: u64,
    sealed_n: u64,
    blocks: Vec<BlockRef>,
}

impl TierState {
    fn new(open: Option<Open>) -> Self {
        Self {
            open,
            bucket: None,
            last_n: 0,
            sealed_n: 0,
            blocks: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct SeriesState {
    spec: SeriesSpec,
    next_n: u64,
    tiers: [TierState; 4],
}

fn built(spec: &SeriesSpec, tier: Tier) -> bool {
    spec.keep.get(tier).is_some()
        && (tier == Tier::Raw || matches!(spec.kind, SeriesKind::Scalar(_)))
}

fn empty_open(spec: &SeriesSpec, tier: Tier) -> Option<Open> {
    if !built(spec, tier) {
        return None;
    }
    let body = match (tier, spec.kind) {
        (Tier::Raw, SeriesKind::Event) => Body::Event(Vec::new()),
        (Tier::Raw, SeriesKind::Scalar(_)) => Body::Raw(RawEncoder::default()),
        _ => Body::Agg(AggEncoder::default()),
    };
    Some(Open::new(tier, body))
}

impl SeriesState {
    fn new(spec: SeriesSpec) -> Self {
        let tiers = Tier::ALL.map(|t| TierState::new(empty_open(&spec, t)));
        Self {
            spec,
            next_n: 1,
            tiers,
        }
    }

    fn value_kind(&self) -> ValueKind {
        match self.spec.kind {
            SeriesKind::Scalar(k) => k,
            SeriesKind::Event => ValueKind::Other,
        }
    }

    fn key(&self, tier: Tier) -> Key {
        Key {
            tier,
            class: self.spec.class,
            keep_ns: self.spec.keep.get(tier).unwrap_or(0),
        }
    }
}

struct Inner {
    dir: PathBuf,
    config: StoreConfig,
    clock: Arc<dyn Clock>,
    series: Vec<Option<SeriesState>>,
    by_path: BTreeMap<String, u32>,
    catalog: File,
    wal: Wal,
    segments: Segments,
    last_sync: u64,
    last_checkpoint: u64,
    last_maintenance: u64,
    write_failed: bool,
    stats: StoreStats,
}

/// A historian on a directory. Every method takes `&self`; one writer
/// thread appends ([`crate::Writer`]) while others query.
pub struct Store {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

fn encode_spec(id: u32, spec: &SeriesSpec) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_le_bytes());
    let (kind, value) = match spec.kind {
        SeriesKind::Scalar(v) => (0u8, v.code()),
        SeriesKind::Event => (1u8, 0),
    };
    out.push(kind);
    out.push(value);
    out.push(spec.class.code());
    for k in spec.keep.0 {
        out.extend_from_slice(&k.unwrap_or(NO_KEEP).to_le_bytes());
    }
    frame::put_bytes(&mut out, spec.path.as_bytes());
    out
}

fn decode_spec(payload: &[u8]) -> Option<(u32, SeriesSpec)> {
    let mut r = Reader::new(payload);
    let id = r.u32()?;
    let kind = r.u8()?;
    let value = r.u8()?;
    let class = Class::from_code(r.u8()?)?;
    let mut keep = [None; 4];
    for k in &mut keep {
        let v = r.u64()?;
        *k = (v != NO_KEEP).then_some(v);
    }
    let path = String::from_utf8(r.bytes()?.to_vec()).ok()?;
    let kind = match kind {
        0 => SeriesKind::Scalar(ValueKind::from_code(value)?),
        1 => SeriesKind::Event,
        _ => return None,
    };
    Some((
        id,
        SeriesSpec {
            path,
            kind,
            class,
            keep: Keep(keep),
        },
    ))
}

fn put_sample(out: &mut Vec<u8>, series: u32, n: u64, s: &Sample) {
    out.push(TAG_SAMPLE);
    out.extend_from_slice(&series.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&s.ts_ns.to_le_bytes());
    frame::put_opt_i64(out, s.ts_src_ns);
    out.extend_from_slice(&s.bits.to_le_bytes());
    out.extend_from_slice(&s.quality.to_le_bytes());
    out.extend_from_slice(&s.seq.to_le_bytes());
}

fn put_event(out: &mut Vec<u8>, series: u32, n: u64, e: &Event) {
    out.push(TAG_EVENT);
    out.extend_from_slice(&series.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&e.ts_ns.to_le_bytes());
    frame::put_opt_i64(out, e.ts_src_ns);
    out.extend_from_slice(&e.seq.to_le_bytes());
    frame::put_bytes(out, &e.payload);
}

fn put_snapshot(out: &mut Vec<u8>, series: u32, tier: Tier, state: &TierState) {
    out.push(TAG_SNAPSHOT);
    out.extend_from_slice(&series.to_le_bytes());
    out.push(tier.index() as u8);
    match state.open.as_ref().filter(|o| o.rows > 0) {
        Some(open) => {
            out.push(1);
            let kind = match open.body {
                Body::Raw(_) => BlockKind::Raw,
                Body::Agg(_) => BlockKind::Agg,
                Body::Event(_) => BlockKind::Event,
            };
            out.push(kind as u8);
            out.extend_from_slice(&open.rows.to_le_bytes());
            out.extend_from_slice(&open.n_first.to_le_bytes());
            out.extend_from_slice(&open.n_last.to_le_bytes());
            let (bits, payload) = open.snapshot_payload();
            out.extend_from_slice(&bits.to_le_bytes());
            frame::put_bytes(out, payload);
        }
        None => out.push(0),
    }
    match &state.bucket {
        Some(b) => {
            out.push(1);
            b.encode(out);
        }
        None => out.push(0),
    }
}

/// One record decoded from a WAL frame.
enum WalRecord<'a> {
    Sample(u32, u64, Sample),
    Event(u32, u64, Event),
    Snapshot(Snapshot<'a>),
}

struct Snapshot<'a> {
    series: u32,
    tier: Tier,
    open: Option<SnapshotBlock<'a>>,
    bucket: Option<Bucket>,
}

#[derive(Clone, Copy)]
struct SnapshotBlock<'a> {
    kind: BlockKind,
    rows: u32,
    n_first: u64,
    n_last: u64,
    bits: u64,
    payload: &'a [u8],
}

fn next_record<'a>(r: &mut Reader<'a>) -> Option<WalRecord<'a>> {
    match r.u8()? {
        TAG_SAMPLE => {
            let series = r.u32()?;
            let n = r.u64()?;
            let ts_ns = r.i64()?;
            let ts_src_ns = r.opt_i64()?;
            let bits = r.u64()?;
            let quality = r.u32()?;
            let seq = r.u64()?;
            Some(WalRecord::Sample(
                series,
                n,
                Sample {
                    ts_ns,
                    ts_src_ns,
                    bits,
                    quality,
                    seq,
                },
            ))
        }
        TAG_EVENT => {
            let series = r.u32()?;
            let n = r.u64()?;
            let ts_ns = r.i64()?;
            let ts_src_ns = r.opt_i64()?;
            let seq = r.u64()?;
            let payload = r.bytes()?.to_vec();
            Some(WalRecord::Event(
                series,
                n,
                Event {
                    ts_ns,
                    ts_src_ns,
                    seq,
                    payload,
                },
            ))
        }
        TAG_SNAPSHOT => {
            let series = r.u32()?;
            let tier = Tier::from_index(r.u8()?)?;
            let open = match r.u8()? {
                0 => None,
                _ => {
                    let kind = match r.u8()? {
                        0 => BlockKind::Raw,
                        1 => BlockKind::Agg,
                        2 => BlockKind::Event,
                        _ => return None,
                    };
                    let rows = r.u32()?;
                    let n_first = r.u64()?;
                    let n_last = r.u64()?;
                    let bits = r.u64()?;
                    let payload = r.bytes()?;
                    Some(SnapshotBlock {
                        kind,
                        rows,
                        n_first,
                        n_last,
                        bits,
                        payload,
                    })
                }
            };
            let bucket = match r.u8()? {
                0 => None,
                _ => Some(Bucket::decode(r)?),
            };
            Some(WalRecord::Snapshot(Snapshot {
                series,
                tier,
                open,
                bucket,
            }))
        }
        _ => None,
    }
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

impl Inner {
    fn state(&mut self, id: u32) -> Option<&mut SeriesState> {
        self.series.get_mut(id as usize).and_then(Option::as_mut)
    }

    fn seal(&mut self, id: u32, tier: Tier) {
        let Some(st) = self.series.get_mut(id as usize).and_then(Option::as_mut) else {
            return;
        };
        let key = st.key(tier);
        let ts = &mut st.tiers[tier.index()];
        let Some(open) = ts.open.as_mut() else {
            return;
        };
        if open.rows == 0 {
            return;
        }
        let written = open
            .seal(id)
            .ok_or_else(|| io::Error::other("block does not encode"))
            .and_then(|bytes| self.segments.write(key, &bytes));
        match written {
            Ok(r) => {
                ts.blocks.push(r);
                ts.sealed_n = ts.sealed_n.max(open.n_last);
            }
            Err(_) => {
                self.stats.write_errors += 1;
                self.stats.dropped_write_error += u64::from(open.rows);
                self.write_failed = true;
                self.stats.disk_full = true;
            }
        }
        *open = open.fresh();
    }

    fn apply_sample(&mut self, id: u32, n: u64, s: &Sample) {
        let Some(st) = self.state(id) else { return };
        let kind = match st.spec.kind {
            SeriesKind::Scalar(k) => k,
            SeriesKind::Event => return,
        };
        st.next_n = st.next_n.max(n + 1);
        let mut full = [false; 4];
        let raw = &mut st.tiers[Tier::Raw.index()];
        if n > raw.last_n
            && let Some(open) = raw.open.as_mut()
        {
            open.push_raw(n, s);
            raw.last_n = n;
            full[Tier::Raw.index()] = open.is_full();
        }
        for tier in [Tier::S1, Tier::M1, Tier::M15] {
            let ts = &mut st.tiers[tier.index()];
            let Some(open) = ts.open.as_mut() else {
                continue;
            };
            if n <= ts.last_n {
                continue;
            }
            ts.last_n = n;
            let start = bucket_start(s.ts_ns, tier.resolution_ns());
            match &mut ts.bucket {
                Some(b) if start <= b.start => b.add(kind, n, s),
                slot => {
                    if let Some(b) = slot.take() {
                        open.push_agg(b.n_first, b.n_last, &b.finish(kind));
                        full[tier.index()] = open.is_full();
                    }
                    let mut b = Bucket::new(start);
                    b.add(kind, n, s);
                    *slot = Some(b);
                }
            }
        }
        for tier in Tier::ALL {
            if full[tier.index()] {
                self.seal(id, tier);
            }
        }
    }

    fn apply_event(&mut self, id: u32, n: u64, e: &Event) {
        let Some(st) = self.state(id) else { return };
        if st.spec.kind != SeriesKind::Event {
            return;
        }
        st.next_n = st.next_n.max(n + 1);
        let raw = &mut st.tiers[Tier::Raw.index()];
        let Some(open) = raw.open.as_mut() else {
            return;
        };
        if n <= raw.last_n {
            return;
        }
        open.push_event(n, e);
        raw.last_n = n;
        if open.is_full() {
            self.seal(id, Tier::Raw);
        }
    }

    fn restore(&mut self, snap: &Snapshot<'_>) -> bool {
        let Some(st) = self.state(snap.series) else {
            return false;
        };
        let ts = &mut st.tiers[snap.tier.index()];
        let Some(open) = ts.open.as_mut() else {
            return false;
        };
        let mut restored = false;
        if let Some(SnapshotBlock {
            kind,
            rows,
            n_first,
            n_last,
            bits,
            payload,
        }) = snap.open
            && n_last > ts.last_n
            && let Some(decoded) =
                block::decode_snapshot(kind, n_first, rows as usize, bits, payload)
        {
            match decoded {
                Rows::Raw(rows) => {
                    for (n, s) in rows.iter().filter(|(n, _)| *n > ts.last_n) {
                        open.push_raw(*n, s);
                    }
                }
                Rows::Event(rows) => {
                    for (n, e) in rows.iter().filter(|(n, _)| *n > ts.last_n) {
                        open.push_event(*n, e);
                    }
                }
                Rows::Agg(rows) => {
                    if open.rows == 0 && n_first > ts.sealed_n {
                        for a in &rows {
                            open.push_agg(n_first, n_last, a);
                        }
                        open.n_first = n_first;
                    }
                }
            }
            ts.last_n = ts.last_n.max(n_last);
            restored = true;
        }
        if let Some(b) = snap.bucket
            && b.n_last > ts.last_n
        {
            ts.bucket = Some(b);
            ts.last_n = b.n_last;
            restored = true;
        }
        st.next_n = st.next_n.max(ts_last(st) + 1);
        restored
    }

    fn write_snapshots(&mut self) -> io::Result<()> {
        let mut buf = Vec::new();
        for (id, st) in self.series.iter().enumerate() {
            let Some(st) = st else { continue };
            for tier in Tier::ALL {
                let ts = &st.tiers[tier.index()];
                let has_open = ts.open.as_ref().is_some_and(|o| o.rows > 0);
                if !has_open && ts.bucket.is_none() {
                    continue;
                }
                put_snapshot(&mut buf, id as u32, tier, ts);
                if buf.len() >= SNAPSHOT_FRAME {
                    self.wal.append(&buf)?;
                    buf.clear();
                }
            }
        }
        if !buf.is_empty() {
            self.wal.append(&buf)?;
        }
        Ok(())
    }

    fn checkpoint(&mut self) -> io::Result<()> {
        self.segments.sync()?;
        self.wal.rotate()?;
        self.write_snapshots()?;
        self.wal.sync()?;
        self.stats.fsyncs += 1;
        self.wal.trim()?;
        self.stats.checkpoints += 1;
        self.last_checkpoint = self.clock.mono_ns();
        Ok(())
    }

    fn sync_wal(&mut self) {
        if !self.wal.dirty() {
            return;
        }
        match self.wal.sync() {
            Ok(()) => self.stats.fsyncs += 1,
            Err(_) => {
                self.stats.write_errors += 1;
                self.write_failed = true;
            }
        }
        self.last_sync = self.clock.mono_ns();
    }

    fn total_bytes(&self) -> u64 {
        self.segments.total_bytes() + self.wal.total_bytes()
    }

    fn retention(&mut self) -> io::Result<()> {
        let now = self.clock.utc_ns();
        let mut deleted = BTreeSet::new();
        let expired: Vec<u64> = self
            .segments
            .files
            .iter()
            .filter(|(_, f)| {
                f.t_max != i64::MIN
                    && f.t_max
                        < now.saturating_sub(i64::try_from(f.key.keep_ns).unwrap_or(i64::MAX))
            })
            .map(|(no, _)| *no)
            .collect();
        for no in expired {
            self.segments.delete(no)?;
            deleted.insert(no);
            self.stats.expired_files += 1;
        }
        let limit = self.config.max_disk / 10 * 9;
        let mut total = self.total_bytes();
        while total > limit {
            let victim = self
                .segments
                .files
                .iter()
                .filter(|(_, f)| f.len > 0)
                .min_by_key(|(_, f)| {
                    let group = if f.key.class.is_protected() {
                        4
                    } else {
                        f.key.tier.index()
                    };
                    (group, f.t_max)
                })
                .map(|(no, f)| (*no, f.len));
            let Some((no, len)) = victim else { break };
            self.segments.delete(no)?;
            deleted.insert(no);
            self.stats.evicted_files += 1;
            total = total.saturating_sub(len);
        }
        if !deleted.is_empty() {
            for st in self.series.iter_mut().flatten() {
                for ts in &mut st.tiers {
                    ts.blocks.retain(|b| !deleted.contains(&b.file));
                }
            }
        }
        let critical = match &self.config.probe {
            Some(probe) => probe(&self.dir).is_some_and(|d| d.free.saturating_mul(100) < d.total),
            None => false,
        };
        self.stats.disk_full = critical || self.total_bytes() >= self.config.max_disk;
        self.write_failed = false;
        Ok(())
    }

    fn snapshot_stats(&self) -> StoreStats {
        StoreStats {
            series: self.by_path.len() as u64,
            segment_bytes: self.segments.total_bytes(),
            wal_bytes: self.wal.total_bytes(),
            disk_full: self.stats.disk_full || self.write_failed,
            ..self.stats
        }
    }
}

fn ts_last(st: &SeriesState) -> u64 {
    st.tiers.iter().map(|t| t.last_n).max().unwrap_or(0)
}

/// Blocks to read and the rows of the open block, for one query.
struct Plan {
    files: Vec<(PathBuf, Vec<BlockRef>)>,
    open: Option<Rows>,
}

impl Store {
    /// Opens (or creates) the store in `dir`, recovering what a crash
    /// left: torn tails are cut, the WAL is replayed over the sealed
    /// segments, and a checkpoint starts a fresh WAL file.
    pub fn open(
        dir: &Path,
        config: StoreConfig,
        clock: Arc<dyn Clock>,
    ) -> io::Result<(Self, Recovery)> {
        let mut recovery = Recovery::default();
        fs::create_dir_all(dir)?;
        let wal_dir = dir.join("wal");
        fs::create_dir_all(&wal_dir)?;
        fs::create_dir_all(dir.join("cursors"))?;

        let catalog_path = dir.join("series.cat");
        let bytes = match fs::read(&catalog_path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let (frames, valid) = frame::split(&bytes);
        let mut specs: BTreeMap<u32, SeriesSpec> = BTreeMap::new();
        for f in frames {
            if let Some((id, spec)) = decode_spec(f) {
                specs.insert(id, spec);
            }
        }
        let catalog = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&catalog_path)?;
        if valid < bytes.len() {
            recovery.catalog_torn_bytes = (bytes.len() - valid) as u64;
            catalog.set_len(valid as u64)?;
            catalog.sync_all()?;
        }
        sync_dir(dir)?;

        let mut series: Vec<Option<SeriesState>> = Vec::new();
        let mut by_path = BTreeMap::new();
        for (id, spec) in specs {
            let i = id as usize;
            if series.len() <= i {
                series.resize_with(i + 1, || None);
            }
            by_path.insert(spec.path.clone(), id);
            series[i] = Some(SeriesState::new(spec));
        }

        let (segments, blocks, report) =
            Segments::open(&dir.join("seg"), config.segment_file_bytes)?;
        recovery.segments_indexed = report.indexed;
        recovery.segments_scanned = report.scanned;
        recovery.segment_torn_bytes = report.truncated;
        for b in blocks {
            let Some(st) = series
                .get_mut(b.header.series as usize)
                .and_then(Option::as_mut)
            else {
                recovery.orphan_blocks += 1;
                continue;
            };
            let ts = &mut st.tiers[b.header.tier.index()];
            ts.sealed_n = ts.sealed_n.max(b.header.n_last);
            ts.last_n = ts.sealed_n;
            ts.blocks.push(b);
        }
        for st in series.iter_mut().flatten() {
            for ts in &mut st.tiers {
                ts.blocks
                    .sort_by_key(|b| (b.header.n_first, b.file, b.offset));
            }
            st.next_n = ts_last(st) + 1;
        }

        let found = wal::scan(&wal_dir)?;
        let next_id = found.last().map_or(0, |f| f.id + 1);
        let older = found
            .iter()
            .map(|f| {
                (
                    f.id,
                    wal_dir.join(format!("{:016x}.wal", f.id)),
                    f.bytes.len() as u64,
                )
            })
            .collect();
        let wal = Wal::create(&wal_dir, next_id, older)?;
        let now = clock.mono_ns();
        let mut inner = Inner {
            dir: dir.to_owned(),
            config,
            clock,
            series,
            by_path,
            catalog,
            wal,
            segments,
            last_sync: now,
            last_checkpoint: now,
            last_maintenance: now,
            write_failed: false,
            stats: StoreStats::default(),
        };
        for f in &found {
            recovery.wal_files += 1;
            recovery.wal_torn_bytes += f.torn;
            let (frames, _) = frame::split(&f.bytes);
            for payload in frames {
                let mut r = Reader::new(payload);
                while !r.is_empty() {
                    let Some(record) = next_record(&mut r) else {
                        break;
                    };
                    match record {
                        WalRecord::Sample(id, n, s) => {
                            inner.apply_sample(id, n, &s);
                            recovery.wal_records += 1;
                        }
                        WalRecord::Event(id, n, e) => {
                            inner.apply_event(id, n, &e);
                            recovery.wal_records += 1;
                        }
                        WalRecord::Snapshot(snap) => {
                            if inner.restore(&snap) {
                                recovery.snapshots += 1;
                            }
                        }
                    }
                }
            }
        }
        drop(found);
        inner.segments.sync()?;
        inner.write_snapshots()?;
        inner.wal.sync()?;
        inner.wal.trim()?;
        inner.retention()?;
        Ok((
            Self {
                inner: Mutex::new(inner),
            },
            recovery,
        ))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Declares a series, or updates the class and `keep` of one with the
    /// same path. The catalog is synced before this returns, so a record
    /// of the series is never in the WAL without its definition.
    pub fn define(&self, spec: SeriesSpec) -> io::Result<SeriesId> {
        self.define_all(vec![spec])?
            .pop()
            .ok_or_else(|| io::Error::other("no series defined"))
    }

    /// [`Store::define`] for many series, with one sync of the catalog.
    pub fn define_all(&self, specs: Vec<SeriesSpec>) -> io::Result<Vec<SeriesId>> {
        let mut inner = self.lock();
        let mut out = Vec::with_capacity(specs.len());
        let mut buf = Vec::new();
        let mut changes: Vec<(u32, SeriesSpec, bool)> = Vec::new();
        let mut fresh: BTreeMap<String, u32> = BTreeMap::new();
        let mut next = inner.series.len();
        for spec in specs {
            let known = inner
                .by_path
                .get(&spec.path)
                .or_else(|| fresh.get(&spec.path))
                .copied();
            let Some(id) = known else {
                let id = u32::try_from(next).map_err(|_| io::Error::other("too many series"))?;
                next += 1;
                frame::put(&mut buf, &encode_spec(id, &spec));
                fresh.insert(spec.path.clone(), id);
                changes.push((id, spec, true));
                out.push(SeriesId(id));
                continue;
            };
            let current = changes
                .iter()
                .rev()
                .find(|c| c.0 == id)
                .map(|c| c.1.clone())
                .or_else(|| inner.state(id).map(|s| s.spec.clone()))
                .ok_or_else(|| io::Error::other("catalog out of step"))?;
            out.push(SeriesId(id));
            if current == spec {
                continue;
            }
            if current.kind != spec.kind {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("series `{}` changes kind", spec.path),
                ));
            }
            frame::put(&mut buf, &encode_spec(id, &spec));
            changes.push((id, spec, false));
        }
        if buf.is_empty() {
            return Ok(out);
        }
        for (id, _, new) in &changes {
            if !new {
                for tier in Tier::ALL {
                    inner.seal(*id, tier);
                }
            }
        }
        inner.catalog.write_all(&buf)?;
        inner.catalog.sync_data()?;
        for (id, spec, new) in changes {
            if new {
                let i = id as usize;
                if inner.series.len() <= i {
                    inner.series.resize_with(i + 1, || None);
                }
                inner.by_path.insert(spec.path.clone(), id);
                inner.series[i] = Some(SeriesState::new(spec));
                continue;
            }
            if let Some(st) = inner.state(id) {
                for tier in Tier::ALL {
                    let ts = &mut st.tiers[tier.index()];
                    match (ts.open.is_some(), built(&spec, tier)) {
                        (false, true) => {
                            ts.open = empty_open(&spec, tier);
                            ts.last_n = st.next_n.saturating_sub(1);
                        }
                        (true, false) => {
                            ts.open = None;
                            ts.bucket = None;
                        }
                        _ => {}
                    }
                }
                st.spec = spec;
            }
        }
        Ok(out)
    }

    /// Changes the `fsync` policy and `max_disk` (a new generation's
    /// `[telemetry.store]`).
    pub fn set_policy(&self, fsync: Fsync, fsync_interval_ns: u64, max_disk: u64) {
        let mut inner = self.lock();
        inner.config.fsync = fsync;
        inner.config.fsync_interval_ns = fsync_interval_ns;
        inner.config.max_disk = max_disk;
    }

    /// The series with that path.
    pub fn series(&self, path: &str) -> Option<SeriesId> {
        self.lock().by_path.get(path).copied().map(SeriesId)
    }

    /// The definition of a series.
    pub fn spec(&self, id: SeriesId) -> Option<SeriesSpec> {
        self.lock().state(id.0).map(|s| s.spec.clone())
    }

    /// Every series, by path.
    pub fn list(&self) -> Vec<(String, SeriesId)> {
        self.lock()
            .by_path
            .iter()
            .map(|(p, id)| (p.clone(), SeriesId(*id)))
            .collect()
    }

    /// The store sequence number of the last record of a series; 0 when
    /// it has none.
    pub fn last_n(&self, id: SeriesId) -> u64 {
        self.lock()
            .state(id.0)
            .map_or(0, |s| s.next_n.saturating_sub(1))
    }

    /// Appends a batch as one WAL frame and returns how many records were
    /// accepted. While the disk is critical only alarms and decisions are;
    /// the others are counted in [`StoreStats::dropped_disk_full`]. A
    /// failed WAL write drops the batch and counts it. The WAL is synced
    /// after the batch with `always`, and with `batch` when the batch holds
    /// an alarm or a decision.
    pub fn append(&self, records: &[Record]) -> usize {
        let mut inner = self.lock();
        let critical = inner.stats.disk_full || inner.write_failed;
        let mut payload = Vec::with_capacity(records.len() * 48);
        let mut accepted: Vec<(u32, u64, usize)> = Vec::with_capacity(records.len());
        let mut urgent = false;
        for (i, record) in records.iter().enumerate() {
            let id = record.series.0;
            let Some(st) = inner.state(id) else { continue };
            let class = st.spec.class;
            let fits = matches!(
                (&record.payload, st.spec.kind),
                (Payload::Sample(_), SeriesKind::Scalar(_))
                    | (Payload::Event(_), SeriesKind::Event)
            );
            if !fits {
                continue;
            }
            if critical && !class.is_protected() {
                inner.stats.dropped_disk_full += 1;
                continue;
            }
            let n = st.next_n;
            st.next_n += 1;
            urgent |= class.is_protected();
            match &record.payload {
                Payload::Sample(s) => put_sample(&mut payload, id, n, s),
                Payload::Event(e) => put_event(&mut payload, id, n, e),
            }
            accepted.push((id, n, i));
        }
        if accepted.is_empty() {
            return 0;
        }
        if inner.wal.append(&payload).is_err() {
            inner.stats.write_errors += 1;
            inner.stats.dropped_write_error += accepted.len() as u64;
            inner.write_failed = true;
            return 0;
        }
        for &(id, n, i) in &accepted {
            match &records[i].payload {
                Payload::Sample(s) => inner.apply_sample(id, n, s),
                Payload::Event(e) => inner.apply_event(id, n, e),
            }
        }
        inner.stats.records += accepted.len() as u64;
        let sync = match inner.config.fsync {
            Fsync::Always => true,
            Fsync::Batch => urgent,
            Fsync::Never => false,
        };
        if sync {
            inner.sync_wal();
        }
        if inner.wal.len() >= inner.config.wal_file_bytes && inner.checkpoint().is_err() {
            inner.stats.write_errors += 1;
            inner.write_failed = true;
        }
        accepted.len()
    }

    /// Housekeeping, called often by the writer thread: the batch sync,
    /// checkpoints, and every `maintenance_interval` the retention by time
    /// and size and the disk probe.
    pub fn maintain(&self) {
        let mut inner = self.lock();
        let now = inner.clock.mono_ns();
        if inner.config.fsync == Fsync::Batch
            && now.saturating_sub(inner.last_sync) >= inner.config.fsync_interval_ns
        {
            inner.sync_wal();
            inner.last_sync = now;
        }
        if now.saturating_sub(inner.last_checkpoint) >= inner.config.checkpoint_interval_ns {
            if inner.checkpoint().is_err() {
                inner.stats.write_errors += 1;
                inner.write_failed = true;
            }
            inner.last_checkpoint = now;
        }
        if now.saturating_sub(inner.last_maintenance) >= inner.config.maintenance_interval_ns {
            inner.last_maintenance = now;
            if inner.retention().is_err() {
                inner.stats.write_errors += 1;
            }
        }
    }

    /// Runs retention and the disk probe now.
    pub fn enforce_retention(&self) -> io::Result<()> {
        self.lock().retention()
    }

    /// Syncs the WAL now.
    pub fn sync(&self) -> io::Result<()> {
        let mut inner = self.lock();
        inner.wal.sync()?;
        inner.stats.fsyncs += 1;
        Ok(())
    }

    /// Checkpoints now: syncs the segments, moves the open blocks into a
    /// fresh WAL file and deletes the older ones.
    pub fn checkpoint(&self) -> io::Result<()> {
        self.lock().checkpoint()
    }

    /// A clean shutdown: a checkpoint, then every segment file is closed
    /// with its index.
    pub fn close(&self) -> io::Result<()> {
        let mut inner = self.lock();
        inner.checkpoint()?;
        inner.segments.close_all()
    }

    /// The counters.
    pub fn stats(&self) -> StoreStats {
        self.lock().snapshot_stats()
    }

    /// Every WAL and segment file with the bytes known to be on stable
    /// storage: what survives a power cut (crash tests).
    pub fn durability(&self) -> Vec<(PathBuf, u64)> {
        let inner = self.lock();
        let mut out = inner.wal.durability();
        out.extend(inner.segments.durability());
        out
    }

    /// The instants a tier of a series covers: earliest and latest row.
    pub fn coverage(&self, id: SeriesId, tier: Tier) -> Option<(i64, i64)> {
        let mut inner = self.lock();
        let ts = &inner.state(id.0)?.tiers[tier.index()];
        let mut lo = ts.blocks.iter().map(|b| b.header.t_min).min();
        let mut hi = ts.blocks.iter().map(|b| b.header.t_max).max();
        if let Some(open) = ts.open.as_ref().filter(|o| o.rows > 0) {
            lo = Some(lo.map_or(open.t_min, |l| l.min(open.t_min)));
            hi = Some(hi.map_or(open.t_max, |h| h.max(open.t_max)));
        }
        Some((lo?, hi?))
    }

    fn plan(&self, id: SeriesId, tier: Tier, from: i64, to: i64, after_n: u64) -> Option<Plan> {
        let mut inner = self.lock();
        let paths: BTreeMap<u64, PathBuf> = inner
            .segments
            .files
            .iter()
            .map(|(no, f)| (*no, f.path.clone()))
            .collect();
        let st = inner.state(id.0)?;
        let ts = &st.tiers[tier.index()];
        let mut files: Vec<(PathBuf, Vec<BlockRef>)> = Vec::new();
        for b in ts
            .blocks
            .iter()
            .filter(|b| b.header.t_max >= from && b.header.t_min <= to && b.header.n_last > after_n)
        {
            let Some(path) = paths.get(&b.file) else {
                continue;
            };
            match files.last_mut() {
                Some((p, refs)) if p == path => refs.push(*b),
                _ => files.push((path.clone(), vec![*b])),
            }
        }
        let open = ts.open.as_ref().filter(|o| o.rows > 0).and_then(|o| {
            let (bits, payload) = o.snapshot_payload();
            let kind = match o.body {
                Body::Raw(_) => BlockKind::Raw,
                Body::Agg(_) => BlockKind::Agg,
                Body::Event(_) => BlockKind::Event,
            };
            block::decode_snapshot(kind, o.n_first, o.rows as usize, bits, payload)
        });
        Some(Plan { files, open })
    }

    fn rows(plan: Plan) -> io::Result<Vec<Rows>> {
        let mut out = Vec::new();
        for (path, refs) in &plan.files {
            let blocks = match segment::read_blocks(path, refs) {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            for (header, payload) in blocks {
                if let Some(rows) = block::decode(&header, &payload) {
                    out.push(rows);
                }
            }
        }
        out.extend(plan.open);
        Ok(out)
    }

    /// Raw samples of a scalar series with `from ≤ ts ≤ to` and a store
    /// sequence number above `after_n`, in that order, at most `limit`.
    pub fn read_raw(
        &self,
        id: SeriesId,
        from: i64,
        to: i64,
        after_n: u64,
        limit: usize,
    ) -> io::Result<Vec<(u64, Sample)>> {
        let Some(plan) = self.plan(id, Tier::Raw, from, to, after_n) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for rows in Self::rows(plan)? {
            let Rows::Raw(rows) = rows else { continue };
            for (n, s) in rows {
                if n > after_n && s.ts_ns >= from && s.ts_ns <= to {
                    out.push((n, s));
                    if out.len() >= limit {
                        return Ok(out);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Closed aggregates of a tier with `from ≤ ts ≤ to`, at most `limit`.
    pub fn read_agg(
        &self,
        id: SeriesId,
        tier: Tier,
        from: i64,
        to: i64,
        limit: usize,
    ) -> io::Result<Vec<Aggregate>> {
        if tier == Tier::Raw {
            return Ok(Vec::new());
        }
        let Some(plan) = self.plan(id, tier, from, to, 0) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for rows in Self::rows(plan)? {
            let Rows::Agg(rows) = rows else { continue };
            for a in rows {
                if a.ts_ns >= from && a.ts_ns <= to {
                    out.push(a);
                    if out.len() >= limit {
                        return Ok(out);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Events of an event series, as [`Store::read_raw`].
    pub fn read_events(
        &self,
        id: SeriesId,
        from: i64,
        to: i64,
        after_n: u64,
        limit: usize,
    ) -> io::Result<Vec<(u64, Event)>> {
        let Some(plan) = self.plan(id, Tier::Raw, from, to, after_n) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for rows in Self::rows(plan)? {
            let Rows::Event(rows) = rows else { continue };
            for (n, e) in rows {
                if n > after_n && e.ts_ns >= from && e.ts_ns <= to {
                    out.push((n, e));
                    if out.len() >= limit {
                        return Ok(out);
                    }
                }
            }
        }
        Ok(out)
    }

    /// How the values of a series read as numbers.
    pub fn value_kind(&self, id: SeriesId) -> Option<ValueKind> {
        self.lock().state(id.0).map(|s| s.value_kind())
    }

    /// The persistent cursor of a consumer (store-and-forward).
    pub fn cursor(&self, consumer: &str) -> io::Result<BTreeMap<String, u64>> {
        let dir = self.lock().dir.join("cursors");
        crate::cursor::load(&dir, consumer)
    }

    /// Replaces the persistent cursor of a consumer.
    pub fn set_cursor(&self, consumer: &str, cursor: &BTreeMap<String, u64>) -> io::Result<()> {
        let dir = self.lock().dir.join("cursors");
        crate::cursor::save(&dir, consumer, cursor)
    }
}
