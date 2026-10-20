//! Shared harness: one group, one input `x.f` and one output `x.p_set`,
//! driven tick by tick with a virtual clock.

#![allow(dead_code, clippy::unwrap_used)]

use joker_core::image::ImageStorage;
use joker_core::output::ClockRequirement;
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::Instant;
use joker_core::trace::{SegmentWriter, TickRecord, TraceResult};
use joker_core::{
    Applied, ClockQuality, CommitSink, DecisionRecord, Level, Outcome, OutputConfig, OutputResult,
    Outputs, OutputsStorage, ProcessImage, Proposal, Quality, RegistryBuilder, SafeValue, Sample,
    SignalId, Slot, ValueType, WriteResult, WriterConfig, WriterId, WriterKind,
};

pub const MS: u64 = 1_000_000;
pub const PERIOD: u64 = 100 * MS;

#[derive(Default)]
pub struct Log {
    pub writes: Vec<WriteResult>,
    pub outputs: Vec<OutputResult>,
    pub decisions: Vec<DecisionRecord>,
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

pub struct Harness {
    pub slots: &'static [Slot],
    pub image: ProcessImage<'static>,
    pub outputs: Outputs<'static>,
    pub registry: joker_core::Registry<'static>,
    pub clock: ClockQuality,
}

pub fn leak<T>(v: Vec<T>) -> &'static mut [T] {
    Box::leak(v.into_boxed_slice())
}

impl Harness {
    pub fn new(
        writers: Vec<WriterConfig>,
        configure: impl FnOnce(&mut OutputConfig<'static>),
    ) -> Self {
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
                ports: leak(vec![registry.resolve("x.f").unwrap()]),
                snapshot: leak(vec![Sample::INITIAL; 1]),
                extenders: leak(vec![SeqExtender::for_slot(); 1]),
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

    pub fn out(&self) -> SignalId {
        self.registry.resolve("x.p_set").unwrap()
    }

    pub fn tick(&mut self, k: u64, proposals: &[(u32, f64, bool)]) -> Log {
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

    pub fn stage_raw(&mut self, k: u64, raw: &[Proposal]) -> Log {
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

pub fn proposal(writer: u32, signal: SignalId, value: f64) -> Proposal {
    Proposal {
        writer: WriterId(writer),
        signal,
        value: value.to_bits(),
        ..Proposal::EMPTY
    }
}

pub fn app(level: &str, lease_ms: u64) -> WriterConfig {
    WriterConfig {
        kind: WriterKind::Cycle,
        level: Level::parse(level).unwrap(),
        lease_ns: Some(lease_ms * MS),
        requires_clock: ClockRequirement::Any,
    }
}

pub fn manual(ttl_ms: u64) -> WriterConfig {
    WriterConfig {
        kind: WriterKind::Manual,
        level: Level::MANUAL_DEFAULT,
        lease_ns: Some(ttl_ms * MS),
        requires_clock: ClockRequirement::Any,
    }
}

pub fn kw(x: f64) -> Applied {
    Applied::Value(x.to_bits())
}

pub fn summary(log: &Log) -> (Outcome, Applied, Option<u8>) {
    let o = log.outputs[0];
    (o.outcome, o.applied, o.level.map(|l| l.get()))
}

impl Harness {
    pub fn input(&self) -> SignalId {
        self.registry.resolve("x.f").unwrap()
    }

    pub fn set_input(&self, value: f64, quality: Quality, k: u64) {
        self.slots[self.input().index()].write(value.to_bits(), quality, i64::MIN, k * PERIOD);
    }

    pub fn record(&mut self, k: u64, staged: &[Proposal], segment: &mut SegmentWriter<'_>) -> Log {
        let mut log = Log::default();
        let at = Instant {
            mono_ns: k * PERIOD,
            utc_ns: (k * PERIOD) as i64,
            clock: self.clock,
        };
        self.image.latch(k, at, self.slots, &self.registry).unwrap();
        for p in staged
            .iter()
            .filter(|p| p.source == joker_core::image::Source::Async)
        {
            self.image.stage_async(*p).unwrap();
        }
        for p in staged
            .iter()
            .filter(|p| p.source == joker_core::image::Source::Cycle)
        {
            self.image.stage_cycle(*p).unwrap();
        }
        self.outputs
            .commit(&self.image, self.slots, &mut log)
            .unwrap();
        let results: Vec<TraceResult> = log.outputs.iter().map(TraceResult::from).collect();
        segment
            .push(&TickRecord {
                index: k,
                at,
                inputs: self.image.snapshot(),
                proposals: self.image.proposals(),
                results: &results,
            })
            .unwrap();
        self.image.end_tick().unwrap();
        log
    }
}
