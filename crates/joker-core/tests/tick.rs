//! End-to-end ticks through registry, slots, process image, arbiter and
//! envelope, driven by a virtual clock.

#![allow(clippy::unwrap_used)]

use joker_core::commit::{Cause, Durability, RecordKind};
use joker_core::image::{ImageStorage, RELINQUISH};
use joker_core::output::{ClockRequirement, OnRange};
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::Instant;
use joker_core::{
    Applied, ClockQuality, CommitSink, DecisionRecord, Level, Outcome, OutputConfig, OutputResult,
    Outputs, OutputsStorage, ProcessImage, Proposal, RegistryBuilder, RejectReason, SafeValue,
    Sample, SignalId, Slot, ValueType, WriteResult, WriterConfig, WriterId, WriterKind,
};

const MS: u64 = 1_000_000;
const PERIOD: u64 = 100 * MS;

#[derive(Default)]
struct Log {
    writes: Vec<WriteResult>,
    outputs: Vec<OutputResult>,
    decisions: Vec<DecisionRecord>,
}

impl CommitSink for Log {
    fn write(&mut self, r: &WriteResult) {
        self.writes.push(*r);
    }
    fn output(&mut self, r: &OutputResult) {
        self.outputs.push(*r);
    }
    fn decision(&mut self, r: &DecisionRecord) {
        self.decisions.push(*r);
    }
}

struct Harness {
    slots: &'static [Slot],
    image: ProcessImage<'static>,
    outputs: Outputs<'static>,
    registry: joker_core::Registry<'static>,
    clock: ClockQuality,
}

fn leak<T>(v: Vec<T>) -> &'static mut [T] {
    Box::leak(v.into_boxed_slice())
}

impl Harness {
    fn new(writers: Vec<WriterConfig>, configure: impl FnOnce(&mut OutputConfig<'static>)) -> Self {
        let signals = leak(vec![SignalMeta::EMPTY; 2]);
        let aliases = leak(vec![Alias::EMPTY; 0]);
        let groups = leak(vec![GroupMeta::EMPTY; 1]);
        let mut b = RegistryBuilder::new(signals, aliases, groups);
        b.group(GroupMeta {
            name: "fast",
            period_ns: PERIOD,
        })
        .unwrap();
        b.signal(SignalMeta::output(
            "x.p_set",
            ValueType::F64,
            UnitRef("kW"),
            "fast",
        ))
        .unwrap();
        b.signal(SignalMeta::input("x.f", ValueType::F64, UnitRef("Hz")))
            .unwrap();
        let registry = b.build().unwrap();
        let out = registry.resolve("x.p_set").unwrap();
        let slots: &'static [Slot] = leak((0..2).map(|_| Slot::new()).collect());
        let mut config = OutputConfig::new(
            out,
            ValueType::F64,
            registry.resolve_group("fast").unwrap(),
            SafeValue::Value(0f64.to_bits()),
        );
        config.range = Some((-100.0, 100.0));
        config.rate = Some(10.0);
        configure(&mut config);
        let configs: &'static [OutputConfig<'static>] = leak(vec![config]);
        let writers: &'static [WriterConfig] = leak(writers);
        let outputs = Outputs::new(
            &registry,
            writers,
            OutputsStorage {
                configs,
                states: leak(vec![joker_core::commit::OutputState::EMPTY; 1]),
                intakes: leak(vec![None; 8]),
            },
        )
        .unwrap();
        let image = ProcessImage::new(
            registry.resolve_group("fast").unwrap(),
            PERIOD,
            ImageStorage {
                ports: leak(vec![]),
                snapshot: leak(Vec::<Sample>::new()),
                extenders: leak(Vec::<SeqExtender>::new()),
                proposals: leak(vec![Proposal::EMPTY; 8]),
            },
        )
        .unwrap();
        Self {
            slots,
            image,
            outputs,
            registry,
            clock: ClockQuality::Synced,
        }
    }

    fn out(&self) -> SignalId {
        self.registry.resolve("x.p_set").unwrap()
    }

    fn tick(&mut self, k: u64, proposals: &[(u32, f64, bool)]) -> Log {
        let mut log = Log::default();
        let at = Instant {
            mono_ns: k * PERIOD,
            utc_ns: 0,
            clock: self.clock,
        };
        self.image.latch(k, at, self.slots, &self.registry).unwrap();
        for &(writer, value, _) in proposals.iter().filter(|p| !p.2) {
            self.image
                .stage_async(proposal(writer, self.out(), value))
                .unwrap();
        }
        for &(writer, value, _) in proposals.iter().filter(|p| p.2) {
            self.image
                .stage_cycle(proposal(writer, self.out(), value))
                .unwrap();
        }
        self.outputs
            .commit(&self.image, self.slots, &mut log)
            .unwrap();
        self.image.end_tick().unwrap();
        log
    }

    fn stage_raw(&mut self, k: u64, raw: &[Proposal]) -> Log {
        let mut log = Log::default();
        self.image
            .latch(k, Instant::mono(k * PERIOD), self.slots, &self.registry)
            .unwrap();
        for p in raw {
            self.image.stage_cycle(*p).unwrap();
        }
        self.outputs
            .commit(&self.image, self.slots, &mut log)
            .unwrap();
        self.image.end_tick().unwrap();
        log
    }
}

fn proposal(writer: u32, signal: SignalId, value: f64) -> Proposal {
    Proposal {
        writer: WriterId(writer),
        signal,
        value: value.to_bits(),
        ..Proposal::EMPTY
    }
}

fn app(level: &str, lease_ms: u64) -> WriterConfig {
    WriterConfig {
        kind: WriterKind::Cycle,
        level: Level::parse(level).unwrap(),
        lease_ns: Some(lease_ms * MS),
        requires_clock: ClockRequirement::Any,
    }
}

fn manual(ttl_ms: u64) -> WriterConfig {
    WriterConfig {
        kind: WriterKind::Manual,
        level: Level::MANUAL_DEFAULT,
        lease_ns: Some(ttl_ms * MS),
        requires_clock: ClockRequirement::Any,
    }
}

fn kw(x: f64) -> Applied {
    Applied::Value(x.to_bits())
}

fn summary(log: &Log) -> (Outcome, Applied, Option<u8>) {
    let o = log.outputs[0];
    (o.outcome, o.applied, o.level.map(|l| l.get()))
}

#[test]
fn docs16_rate_example() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |_| {});
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])),
        (Outcome::ClampedRate, kw(1.0), Some(13))
    );
    for k in 1..20 {
        assert_eq!(
            summary(&h.tick(k, &[])),
            (Outcome::None, kw(1.0 + k as f64), Some(13)),
            "tick {k}"
        );
    }
    assert_eq!(summary(&h.tick(20, &[])), (Outcome::None, kw(0.0), None));
    assert_eq!(summary(&h.tick(22, &[])), (Outcome::None, kw(0.0), None));
    let slot = h.slots[h.out().index()].try_read().unwrap();
    assert_eq!(slot.get(), Ok(0f64.to_bits()));
}

#[test]
fn first_applied_value_is_safe_and_inherited_across_generations() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| c.rate = None);
    assert_eq!(summary(&h.tick(0, &[])), (Outcome::None, kw(0.0), None));
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |_| {});
    assert!(h.outputs.inherit(h.out(), kw(30.0)));
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])),
        (Outcome::ClampedRate, kw(31.0), Some(13))
    );
    assert!(!h.outputs.inherit(h.out(), kw(0.0)));
}

#[test]
fn manual_write_overrides_the_app_and_expires() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000), manual(500)], |c| {
        c.rate = None
    });
    assert_eq!(
        summary(&h.tick(0, &[(0, 10.0, true)])),
        (Outcome::Accepted, kw(10.0), Some(13))
    );
    let log = h.tick(1, &[(1, 42.0, false), (0, 10.0, true)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(42.0), Some(4)));
    assert_eq!(log.writes[0].outcome, Outcome::Accepted);
    assert_eq!(
        log.writes[1].outcome,
        Outcome::Overridden(Level::MANUAL_DEFAULT)
    );
    for k in 2..6 {
        let log = h.tick(k, &[(0, 10.0, true)]);
        assert_eq!(
            summary(&log),
            (
                Outcome::Overridden(Level::MANUAL_DEFAULT),
                kw(42.0),
                Some(4)
            )
        );
    }
    assert_eq!(
        summary(&h.tick(6, &[(0, 10.0, true)])),
        (Outcome::Accepted, kw(10.0), Some(13))
    );
    assert_eq!(
        h.outputs.last_outcome(WriterId(0), h.out()),
        Some(Outcome::Accepted)
    );
}

#[test]
fn not_owner_and_wrong_type_are_rejected_without_renewal() {
    let mut h = Harness::new(
        vec![app("optimizer:13", 300), manual(10_000), manual(10_000)],
        |c| c.rate = None,
    );
    h.tick(0, &[(1, 5.0, false)]);
    let log = h.tick(1, &[(2, 6.0, false)]);
    assert_eq!(
        summary(&log),
        (Outcome::Rejected(RejectReason::NotOwner), kw(5.0), Some(4))
    );
    let log = h.tick(2, &[(0, f64::NAN, true)]);
    assert_eq!(
        log.writes[0].outcome,
        Outcome::Rejected(RejectReason::WrongType)
    );
    assert_eq!(
        summary(&log),
        (Outcome::Rejected(RejectReason::WrongType), kw(5.0), Some(4))
    );
    let release = Proposal {
        flags: RELINQUISH,
        ..proposal(1, h.out(), 0.0)
    };
    let log = h.stage_raw(3, &[release, proposal(2, h.out(), 7.0)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(7.0), Some(4)));
    assert_eq!(log.writes[0].outcome, Outcome::Accepted);
}

#[test]
fn trip_holds_safe_until_reset() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| {
        c.rate = None;
        c.on_range = OnRange::Trip;
    });
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])),
        (Outcome::Accepted, kw(50.0), Some(13))
    );
    let log = h.tick(1, &[(0, 150.0, true)]);
    assert_eq!(summary(&log), (Outcome::Tripped, kw(0.0), Some(13)));
    assert!(log.outputs[0].tripped);
    let trip = log
        .decisions
        .iter()
        .find(|d| d.kind == RecordKind::Output)
        .unwrap();
    assert_eq!(trip.durability, Durability::Immediate);
    let log = h.tick(2, &[(0, 50.0, true)]);
    assert_eq!(summary(&log), (Outcome::Tripped, kw(0.0), Some(13)));
    assert!(h.outputs.reset(h.out()));
    let log = h.tick(3, &[]);
    assert_eq!(summary(&log), (Outcome::None, kw(50.0), Some(13)));
    let reset = log
        .decisions
        .iter()
        .find(|d| d.kind == RecordKind::Output)
        .unwrap();
    assert_eq!(reset.cause, Cause::Reset);
    assert!(!h.outputs.reset(h.out()));
}

#[test]
fn reset_is_recorded_even_when_nothing_else_changes() {
    let mut h = Harness::new(vec![app("optimizer:13", 300)], |c| {
        c.rate = None;
        c.on_range = OnRange::Trip;
    });
    h.tick(0, &[(0, 150.0, true)]);
    for k in 1..6 {
        h.tick(k, &[]);
    }
    assert!(h.outputs.reset(h.out()));
    let log = h.tick(6, &[]);
    assert_eq!(summary(&log), (Outcome::None, kw(0.0), None));
    let reset: Vec<_> = log
        .decisions
        .iter()
        .filter(|d| d.kind == RecordKind::Output)
        .collect();
    assert_eq!(reset.len(), 1);
    assert_eq!(reset[0].cause, Cause::Reset);
    assert_eq!(reset[0].durability, Durability::Immediate);
}

#[test]
fn unsynced_clock_rejects_writers_that_require_it() {
    let mut w = app("optimizer:13", 2_000);
    w.requires_clock = ClockRequirement::Synced;
    let mut h = Harness::new(vec![w], |c| c.rate = None);
    h.tick(0, &[(0, 50.0, true)]);
    h.clock = ClockQuality::Unsynced;
    let log = h.tick(1, &[(0, 60.0, true)]);
    assert_eq!(
        summary(&log),
        (
            Outcome::Rejected(RejectReason::ClockUnsynced),
            kw(0.0),
            Some(13)
        )
    );
    h.clock = ClockQuality::Synced;
    assert_eq!(
        summary(&h.tick(2, &[])),
        (Outcome::None, kw(60.0), Some(13))
    );
}

#[test]
fn decision_records_are_written_on_change_with_repeat_counts() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| c.rate = None);
    let mut records = Vec::new();
    for k in 0..5 {
        records.extend(h.tick(k, &[(0, 10.0, true)]).decisions);
    }
    let outputs: Vec<_> = records
        .iter()
        .filter(|d| d.kind == RecordKind::Output)
        .collect();
    let writes: Vec<_> = records
        .iter()
        .filter(|d| d.kind == RecordKind::Write(WriterId(0)))
        .collect();
    assert_eq!(outputs.len(), 1);
    assert_eq!(writes.len(), 1);
    assert_eq!(outputs[0].previous, None);
    assert_eq!(outputs[0].durability, Durability::Immediate);
    assert_eq!(writes[0].durability, Durability::Batch);
    let log = h.tick(5, &[(0, 20.0, true)]);
    let change = log
        .decisions
        .iter()
        .find(|d| d.kind == RecordKind::Output)
        .unwrap();
    let previous = change.previous.unwrap();
    assert_eq!(
        (previous.first_tick, previous.last_tick, previous.repeats),
        (0, 4, 5)
    );
    assert_eq!(change.cause, Cause::Proposal(WriterId(0)));
    assert_eq!(change.durability, Durability::Batch);
    let mut quiet = Vec::new();
    for k in 6..30 {
        quiet.extend(h.tick(k, &[]).decisions);
    }
    let expiry: Vec<_> = quiet
        .iter()
        .filter(|d| d.kind == RecordKind::Output)
        .collect();
    assert_eq!(expiry.len(), 2);
    assert_eq!(expiry[0].outcome, Outcome::None);
    assert_eq!(expiry[1].cause, Cause::Expiry);
    assert_eq!(expiry[1].level, None);
    assert_eq!(expiry[1].durability, Durability::Immediate);
}

#[test]
fn earlier_proposals_of_a_writer_in_a_tick_are_overridden_by_its_last() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| c.rate = None);
    let log = h.tick(0, &[(0, 10.0, true), (0, 20.0, true)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(20.0), Some(13)));
    assert_eq!(
        log.writes[0].outcome,
        Outcome::Overridden(Level::new(13).unwrap())
    );
    assert_eq!(log.writes[1].outcome, Outcome::Accepted);
}
