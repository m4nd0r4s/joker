//! The state a daemon keeps across restarts and hands to each generation
//! before its first tick: persisted trips (docs/01 §7) and manual levels
//! (docs/16 A14), loaded once when the controller is built (ADR 0061).

use std::path::Path;
use std::sync::Arc;

use joker_core::ClockQuality;
use joker_core::image::{Intent, NO_TASK, Proposal, Source};
use serde_json::{Map, json};

use crate::decisions::{SharedLog, lock};
use crate::manual::{ManualTable, Restored, RestoredLevel};
use crate::plan::Plan;
use crate::runner::Restore;
use crate::trips::TripTable;
use crate::wire::{input_bits, timestamp};

/// The persisted trips and manual levels of a daemon.
#[derive(Debug)]
pub struct Persistent {
    trips: Arc<TripTable>,
    manual: Arc<ManualTable>,
    restored: Option<Restored>,
    first: bool,
}

impl Persistent {
    /// Loads `state_dir`'s trips and manual levels at `now`, the
    /// pacer's instant at startup: its clock quality decides how A14
    /// restores the manual levels. A state file that cannot be parsed is
    /// an error (ADR 0061).
    pub fn load(state_dir: &Path, now: joker_core::Instant) -> std::io::Result<Self> {
        let (trips, _) = TripTable::load(state_dir.to_owned())?;
        let (manual, restored) = ManualTable::load(
            state_dir.to_owned(),
            now.utc_ns,
            now.mono_ns,
            now.clock == ClockQuality::Synced,
        )?;
        Ok(Self {
            trips: Arc::new(trips),
            manual: Arc::new(manual),
            restored: Some(restored),
            first: true,
        })
    }

    /// The persisted trips.
    pub fn trips(&self) -> Arc<TripTable> {
        Arc::clone(&self.trips)
    }

    /// The manual levels held.
    pub fn manual(&self) -> Arc<ManualTable> {
        Arc::clone(&self.manual)
    }

    /// What the next generation of `plan` restores before its first tick,
    /// at monotonic `now_mono_ns`: every persisted trip of an output of
    /// the plan and, for the first generation of the daemon only, every
    /// manual level held (a later generation starts without manual
    /// levels, and the table is cleared; a failure to persist that is
    /// reported, not fatal to the switch). Levels and trips of outputs the
    /// plan does not have stay persisted but are not restored.
    pub fn restore_for(&mut self, plan: &Plan, now_mono_ns: u64) -> Restore {
        let trips = self
            .trips
            .paths()
            .iter()
            .filter(|path| plan.output(path).is_some())
            .filter_map(|path| plan.id(path))
            .collect();
        let manual = if self.first {
            self.manual
                .all(now_mono_ns)
                .into_iter()
                .filter_map(|(path, level, occ)| {
                    let remaining_ns = occ.until_mono_ns.saturating_sub(now_mono_ns);
                    let o = plan.output(&path)?;
                    let group = plan.groups.iter().position(|g| g.name == o.group)?;
                    let signal = plan.signal(&o.path)?;
                    let proposal = Proposal {
                        writer: plan.writer(&format!("user:manual:{level}"))?,
                        signal: plan.id(&o.path)?,
                        value: input_bits(signal, &occ.value).ok()?,
                        flags: 0,
                        source: Source::Async,
                        ticket: 0,
                        ttl_ns: remaining_ns,
                        intent: Intent::Write,
                        task: NO_TASK,
                    };
                    (remaining_ns > 0).then_some((group, proposal))
                })
                .collect()
        } else {
            if let Err(e) = self.manual.clear(now_mono_ns) {
                eprintln!("jokerd: cannot persist the manual levels: {e}");
            }
            Vec::new()
        };
        Restore { trips, manual }
    }

    /// Records that generation `generation` started with what
    /// [`Persistent::restore_for`] gave it, and, the first time, appends
    /// the A14 restore records to the decision log (`manual_restored`,
    /// `manual_expired`, ADR 0061).
    pub fn started(&mut self, log: &SharedLog, generation: u64, utc_now_ns: i64) {
        self.first = false;
        let Some(restored) = self.restored.take() else {
            return;
        };
        let mut log = lock(log);
        let records = restored
            .synced
            .iter()
            .map(|l| ("manual_restored", Some(true), l))
            .chain(
                restored
                    .degraded
                    .iter()
                    .map(|l| ("manual_restored", Some(false), l)),
            )
            .chain(restored.expired.iter().map(|l| ("manual_expired", None, l)));
        for (record, synced, level) in records {
            let entry = entry(record, synced, level, generation, utc_now_ns);
            if let Err(e) = log.append(entry, true) {
                eprintln!("jokerd: decision log: {e}");
            }
        }
    }
}

fn entry(
    record: &str,
    synced: Option<bool>,
    level: &RestoredLevel,
    generation: u64,
    utc_now_ns: i64,
) -> Map<String, serde_json::Value> {
    let mut m = Map::new();
    m.insert("gen".into(), json!(generation));
    m.insert("record".into(), json!(record));
    m.insert("output".into(), json!(level.output));
    m.insert("level".into(), json!(format!("manual:{}", level.level)));
    m.insert("principal".into(), json!(level.principal));
    if let Some(synced) = synced {
        m.insert("synced".into(), json!(synced));
        m.insert("remaining_ns".into(), json!(level.remaining_ns));
    }
    m.insert("ts".into(), json!(timestamp(utc_now_ns)));
    m
}
