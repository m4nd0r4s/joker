//! Atomic groups through the commit stage: all or none, proportional rate
//! and the commit register (docs/02 §7.3, ref/01 §5.16, ADR 0039).

#![allow(clippy::unwrap_used)]

use joker_core::commit::{CommitError, OutputState};
use joker_core::image::{ImageStorage, Source};
use joker_core::output::{AtomicGroup, ClockRequirement, OnRange, OnRate};
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::Instant;
use joker_core::{
    Applied, CommitSink, IntakeSlot, Level, Outcome, OutputConfig, OutputResult, Outputs,
    OutputsStorage, ProcessImage, Proposal, Registry, RegistryBuilder, RejectReason, SafeValue,
    Sample, SignalId, Slot, ValueType, WriterConfig, WriterId, WriterKind,
};

const MS: u64 = 1_000_000;
const PERIOD: u64 = 100 * MS;
const LEFT: SignalId = SignalId(1);
const RIGHT: SignalId = SignalId(2);
const APPLY: SignalId = SignalId(0);

fn leak<T>(v: Vec<T>) -> &'static mut [T] {
    Box::leak(v.into_boxed_slice())
}

#[derive(Default)]
struct Results(Vec<OutputResult>);

impl CommitSink for Results {
    fn output(&mut self, r: &OutputResult) {
        self.0.push(*r);
    }
}

struct Rig {
    registry: Registry<'static>,
    slots: &'static [Slot],
    image: ProcessImage<'static>,
    outputs: Outputs<'static>,
}

fn registry() -> Registry<'static> {
    let mut b = RegistryBuilder::new(
        leak(vec![SignalMeta::EMPTY; 3]),
        leak(vec![Alias::EMPTY; 0]),
        leak(vec![GroupMeta::EMPTY; 2]),
    );
    for (name, period) in [("fast", PERIOD), ("slow", 10 * PERIOD)] {
        b.group(GroupMeta {
            name,
            period_ns: period,
        })
        .unwrap();
    }
    let ms = UnitRef("m/s");
    b.signal(SignalMeta::output("v.left", ValueType::F64, ms, "fast"))
        .unwrap();
    b.signal(SignalMeta::output("v.right", ValueType::F64, ms, "fast"))
        .unwrap();
    b.signal(SignalMeta::output(
        "v.apply",
        ValueType::U64,
        UnitRef::ONE,
        "fast",
    ))
    .unwrap();
    let r = b.build().unwrap();
    assert_eq!(
        (
            r.resolve("v.apply"),
            r.resolve("v.left"),
            r.resolve("v.right")
        ),
        (Some(APPLY), Some(LEFT), Some(RIGHT))
    );
    r
}

fn rig(configure: impl Fn(&mut OutputConfig<'static>), on_rate: OnRate) -> Rig {
    let registry = registry();
    let fast = registry.resolve_group("fast").unwrap();
    let configs = leak(
        [LEFT, RIGHT]
            .into_iter()
            .map(|s| {
                let mut c =
                    OutputConfig::new(s, ValueType::F64, fast, SafeValue::Value(0f64.to_bits()));
                c.rate = Some(1.0);
                configure(&mut c);
                c
            })
            .collect(),
    );
    let writers: &'static [WriterConfig] = leak(vec![WriterConfig {
        kind: WriterKind::Cycle,
        level: Level::parse("control:12").unwrap(),
        lease_ns: Some(10_000 * MS),
        requires_clock: ClockRequirement::Any,
    }]);
    let mut outputs = Outputs::new(
        &registry,
        writers,
        OutputsStorage {
            configs,
            states: leak(vec![OutputState::EMPTY; 2]),
            intakes: leak(vec![IntakeSlot::EMPTY; 8]),
        },
    )
    .unwrap();
    let groups: &'static [AtomicGroup<'static>] = leak(vec![AtomicGroup {
        outputs: leak(vec![LEFT, RIGHT]),
        order: leak(vec![RIGHT, LEFT]),
        on_rate,
        commit: Some((APPLY, 1)),
    }]);
    outputs.set_atomic_groups(groups).unwrap();
    let image = ProcessImage::new(
        fast,
        PERIOD,
        ImageStorage {
            ports: leak(vec![]),
            snapshot: leak(Vec::<Sample>::new()),
            extenders: leak(Vec::<SeqExtender>::new()),
            proposals: leak(vec![Proposal::EMPTY; 8]),
        },
    )
    .unwrap();
    Rig {
        registry,
        slots: leak((0..3).map(|_| Slot::new()).collect()),
        image,
        outputs,
    }
}

impl Rig {
    fn tick(&mut self, k: u64, left: f64, right: f64) -> Vec<(Outcome, f64)> {
        self.image
            .latch(k, Instant::mono(k * PERIOD), self.slots, &self.registry)
            .unwrap();
        for (signal, value) in [(LEFT, left), (RIGHT, right)] {
            self.image
                .stage_cycle(Proposal {
                    writer: WriterId(0),
                    signal,
                    value: value.to_bits(),
                    source: Source::Cycle,
                    ..Proposal::EMPTY
                })
                .unwrap();
        }
        let mut results = Results::default();
        self.outputs
            .commit(&self.image, self.slots, &mut results)
            .unwrap();
        self.image.end_tick().unwrap();
        results
            .0
            .iter()
            .map(|r| match r.applied {
                Applied::Value(bits) => (r.outcome, f64::from_bits(bits)),
                Applied::Relinquish => (r.outcome, f64::NAN),
            })
            .collect()
    }

    fn commits(&self) -> u64 {
        self.slots[APPLY.index()].try_read().unwrap().seq()
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn proportional_rate_keeps_the_ratio() {
    let mut r = rig(|_| {}, OnRate::Proportional);
    let out = r.tick(0, 1.0, 0.5);
    assert_eq!(
        (out[0].0, out[1].0),
        (Outcome::ClampedRate, Outcome::ClampedRate)
    );
    assert!(close(out[0].1, 0.1) && close(out[1].1, 0.05), "{out:?}");
    let out = r.tick(1, 1.0, 0.5);
    assert!(close(out[0].1, 0.2) && close(out[1].1, 0.1), "{out:?}");
}

#[test]
fn independent_rate_limits_each_member() {
    let mut r = rig(|_| {}, OnRate::Independent);
    let out = r.tick(0, 1.0, 0.05);
    assert_eq!(out[0], (Outcome::ClampedRate, 0.1));
    assert_eq!(out[1], (Outcome::Accepted, 0.05));
}

#[test]
fn one_failure_blocks_the_whole_group() {
    let mut r = rig(
        |c| {
            c.rate = None;
            if c.signal == RIGHT {
                c.range = Some((-0.4, 0.4));
                c.on_range = OnRange::Reject;
            }
        },
        OnRate::Independent,
    );
    let out = r.tick(0, 0.3, 0.2);
    assert_eq!(out, [(Outcome::Accepted, 0.3), (Outcome::Accepted, 0.2)]);
    assert_eq!(r.commits(), 1);
    let out = r.tick(1, 0.35, 1.0);
    assert_eq!(
        out,
        [
            (Outcome::Rejected(RejectReason::AtomicGroup), 0.3),
            (Outcome::Rejected(RejectReason::Range), 0.2)
        ]
    );
    assert_eq!(r.commits(), 1);
    let out = r.tick(2, 0.3, 0.2);
    assert_eq!(out, [(Outcome::Accepted, 0.3), (Outcome::Accepted, 0.2)]);
    assert_eq!(r.commits(), 1);
    r.tick(3, 0.1, 0.2);
    assert_eq!(r.commits(), 2);
}

#[test]
fn group_configuration_is_checked() {
    let registry = registry();
    let fast = registry.resolve_group("fast").unwrap();
    let slow = registry.resolve_group("slow").unwrap();
    let configs: &'static [OutputConfig<'static>] = leak(vec![
        OutputConfig::new(LEFT, ValueType::F64, fast, SafeValue::Value(0)),
        OutputConfig::new(RIGHT, ValueType::F64, slow, SafeValue::Value(0)),
    ]);
    let mut registry_slow = RegistryBuilder::new(
        leak(vec![SignalMeta::EMPTY; 3]),
        leak(vec![Alias::EMPTY; 0]),
        leak(vec![GroupMeta::EMPTY; 2]),
    );
    for (name, period) in [("fast", PERIOD), ("slow", 10 * PERIOD)] {
        registry_slow
            .group(GroupMeta {
                name,
                period_ns: period,
            })
            .unwrap();
    }
    let ms = UnitRef("m/s");
    registry_slow
        .signal(SignalMeta::output("v.left", ValueType::F64, ms, "fast"))
        .unwrap();
    registry_slow
        .signal(SignalMeta::output("v.right", ValueType::F64, ms, "slow"))
        .unwrap();
    registry_slow
        .signal(SignalMeta::output(
            "v.apply",
            ValueType::U64,
            UnitRef::ONE,
            "fast",
        ))
        .unwrap();
    let registry_slow = registry_slow.build().unwrap();
    let mut outputs = Outputs::new(
        &registry_slow,
        leak(vec![]),
        OutputsStorage {
            configs,
            states: leak(vec![OutputState::EMPTY; 2]),
            intakes: leak(vec![IntakeSlot::EMPTY; 2]),
        },
    )
    .unwrap();
    let code = |groups: Vec<AtomicGroup<'static>>, outputs: &mut Outputs<'static>| match outputs
        .set_atomic_groups(leak(groups))
    {
        Err(CommitError::Config(e)) => e.code,
        other => panic!("{other:?}"),
    };
    let group = |outputs: Vec<SignalId>, order: Vec<SignalId>, commit| AtomicGroup {
        outputs: leak(outputs),
        order: leak(order),
        on_rate: OnRate::Independent,
        commit,
    };
    assert_eq!(
        code(vec![group(vec![LEFT, RIGHT], vec![], None)], &mut outputs),
        "J0272"
    );
    let mut r = rig(|_| {}, OnRate::Independent);
    assert_eq!(
        code(
            vec![
                group(vec![LEFT, RIGHT], vec![], None),
                group(vec![RIGHT, LEFT], vec![], None)
            ],
            &mut r.outputs
        ),
        "J0270"
    );
    assert_eq!(
        code(
            vec![group(vec![LEFT, RIGHT], vec![LEFT, LEFT], None)],
            &mut r.outputs
        ),
        "J0273"
    );
    assert_eq!(
        code(
            vec![group(vec![LEFT, RIGHT], vec![], Some((LEFT, 1)))],
            &mut r.outputs
        ),
        "J0274"
    );
    assert_eq!(
        r.outputs
            .set_atomic_groups(leak(vec![group(vec![LEFT], vec![], None)])),
        Err(CommitError::AtomicTooSmall)
    );
}
