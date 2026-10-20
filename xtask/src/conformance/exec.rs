//! Runs one case against `joker-core` with a virtual clock and records, for
//! every tick the case covers, the result of every output and the latched
//! sample of every input (ref/03 §2.6.4).
//!
//! The initial state (clock `synced`), the step where each writer kind's
//! proposals enter, the order of the actions at a tick boundary and the
//! generation swap follow ADR 0042.
//!
//! The executor uses only `core`, `alloc` and `joker-core`. Each tick is
//! split in two: the core part (inputs into slots, latch, staging, commit),
//! which must not allocate, and the recording, which copies the results out.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use joker_core::commit::OutputState;
use joker_core::image::{ImageStorage, RELINQUISH};
use joker_core::output::{OnInterlock, OutputConfig};
use joker_core::quality::{Quality, QualityCode, QualityReason};
use joker_core::registry::{Alias, GroupMeta, OwnedRegistry, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::{Clock, ClockQuality, Instant, VirtualClock};
use joker_core::value::NO_TS_SRC;
use joker_core::{
    Applied, CommitSink, OutputResult, Outputs, OutputsStorage, ProcessImage, Proposal, SafeValue,
    Sample, SignalId, Slot, WriterConfig, WriterId,
};

use super::model::{ActionKind, Case, SafeDef, Suite};

/// The name of the only cycle group of a core suite.
pub const GROUP: &str = "case";

/// Capacity of the staging buffer of one tick, in proposals.
const STAGING: usize = 256;

/// Why a case could not run: the fixture does not build in the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupError {
    /// The code the core returned (`J0203`, `J0210`, …) or `J0844`.
    pub code: &'static str,
    /// What failed.
    pub message: String,
}

/// What one output did in one tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutputTick {
    /// The result of the core.
    pub result: OutputResult,
    /// The sample of the output slot after the commit.
    pub sample: Sample,
}

/// What the case saw in one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickRecord {
    /// Per output, in the order of [`Suite::outputs`].
    pub outputs: Vec<OutputTick>,
    /// Per input, the latched sample, in the order of [`Suite::signals`].
    pub inputs: Vec<Sample>,
}

/// The full timeline of a case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    /// One record per tick, from tick 0.
    pub ticks: Vec<TickRecord>,
}

/// Runs code that must not allocate. The default is the identity; a host
/// can install a guard (`--features no-alloc-guard`).
pub trait TickGuard {
    /// Runs `f` inside the guard.
    fn run<R>(&self, f: impl FnOnce() -> R) -> R;
}

/// No guard.
#[derive(Copy, Clone, Debug, Default)]
pub struct Unguarded;

impl TickGuard for Unguarded {
    fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        f()
    }
}

#[derive(Debug)]
struct Sink {
    results: Vec<OutputResult>,
}

impl CommitSink for Sink {
    fn output(&mut self, result: &OutputResult) {
        if self.results.len() < self.results.capacity() {
            self.results.push(*result);
        }
    }
}

/// The parts of a suite that the core keeps for a whole generation.
#[derive(Debug)]
struct Plan<'s> {
    registry: OwnedRegistry<'s>,
    writers: Vec<WriterConfig>,
    configs: Vec<OutputConfig<'static>>,
    output_ids: Vec<SignalId>,
    input_ids: Vec<SignalId>,
    ports: Vec<SignalId>,
}

fn setup(code: &'static str, message: impl Into<String>) -> SetupError {
    SetupError {
        code,
        message: message.into(),
    }
}

fn plan(suite: &Suite) -> Result<Plan<'_>, SetupError> {
    let mut signals: Vec<SignalMeta<'_>> = Vec::new();
    for s in &suite.signals {
        let mut meta = SignalMeta::input(&s.path, s.ty, UnitRef(&s.unit));
        meta.max_age_ns = s.max_age_ns;
        signals.push(meta);
    }
    for o in &suite.outputs {
        signals.push(SignalMeta::output(&o.path, o.ty, UnitRef(&o.unit), GROUP));
    }
    let groups = vec![GroupMeta {
        name: GROUP,
        period_ns: suite.period_ns,
    }];
    let registry = OwnedRegistry::build(signals, Vec::<Alias<'_>>::new(), groups)
        .map_err(|e| setup("J0805", alloc::format!("registry: {e:?}")))?;
    let view = registry.as_registry();
    let resolve = |path: &str| {
        view.resolve(path).ok_or_else(|| {
            setup(
                "J0844",
                alloc::format!("path `{path}` is not in the registry"),
            )
        })
    };
    let group = view
        .resolve_group(GROUP)
        .ok_or_else(|| setup("J0844", "group missing from the registry"))?;
    let input_ids = suite
        .signals
        .iter()
        .map(|s| resolve(&s.path))
        .collect::<Result<Vec<_>, _>>()?;
    let output_ids = suite
        .outputs
        .iter()
        .map(|o| resolve(&o.path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut order: Vec<usize> = (0..suite.outputs.len()).collect();
    order.sort_by_key(|&i| output_ids[i]);
    let mut configs = Vec::new();
    for &i in &order {
        let o = &suite.outputs[i];
        let safe = match o.safe {
            SafeDef::Value(bits) => SafeValue::Value(bits),
            SafeDef::Relinquish => SafeValue::Relinquish,
        };
        let mut config = OutputConfig::new(output_ids[i], o.ty, group, safe);
        config.safe_ramp = o.safe_ramp;
        config.range = o.range;
        config.on_range = o.on_range;
        config.rate = o.rate;
        config.on_interlock = OnInterlock::Safe;
        if let Some(ns) = o.max_lease_ns {
            config.max_lease_ns = ns;
        }
        if let Some(ns) = o.max_manual_ns {
            config.max_manual_ns = ns;
        }
        configs.push(config);
    }
    let writers = suite
        .writers
        .iter()
        .map(|w| WriterConfig {
            kind: w.kind,
            level: w.level,
            lease_ns: w.lease_ns,
            requires_clock: w.requires_clock,
        })
        .collect();
    let mut ports = input_ids.clone();
    ports.sort_unstable();
    Ok(Plan {
        registry,
        writers,
        configs,
        output_ids,
        input_ids,
        ports,
    })
}

fn quality_of(code: u8, reason: Option<u8>) -> Quality {
    let code = match code {
        0 => QualityCode::Good,
        1 => QualityCode::Uncertain,
        _ => QualityCode::Bad,
    };
    Quality::new(code, QualityReason::from_u8(reason.unwrap_or(0)))
}

/// Runs a case and returns its timeline.
pub fn run(suite: &Suite, case: &Case, guard: &impl TickGuard) -> Result<Timeline, SetupError> {
    let plan = plan(suite)?;
    let registry = plan.registry.as_registry();
    let slots: Vec<Slot> = (0..registry.len()).map(|_| Slot::new()).collect();
    let clock = VirtualClock::new(Instant {
        mono_ns: 0,
        utc_ns: 0,
        clock: ClockQuality::Synced,
    });
    let mut boundaries: Vec<u64> = case
        .actions
        .iter()
        .filter(|a| matches!(a.kind, ActionKind::Generation) && a.tick > 0)
        .map(|a| a.tick)
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries.push(case.ticks);
    let mut timeline = Timeline {
        ticks: Vec::with_capacity(usize::try_from(case.ticks).unwrap_or(0)),
    };
    let mut inherited: Option<Vec<Applied>> = None;
    let mut start = 0;
    for end in boundaries {
        if end <= start {
            continue;
        }
        let segment = Segment {
            suite,
            case,
            plan: &plan,
            slots: &slots,
            clock: &clock,
        };
        inherited = Some(segment.run(start, end, inherited.as_deref(), guard, &mut timeline)?);
        start = end;
    }
    Ok(timeline)
}

#[derive(Debug)]
struct Segment<'r> {
    suite: &'r Suite,
    case: &'r Case,
    plan: &'r Plan<'r>,
    slots: &'r [Slot],
    clock: &'r VirtualClock,
}

impl Segment<'_> {
    /// Runs the ticks `start..end` of one generation and returns the last
    /// applied value of every output, in the order of the configs.
    fn run(
        &self,
        start: u64,
        end: u64,
        inherited: Option<&[Applied]>,
        guard: &impl TickGuard,
        timeline: &mut Timeline,
    ) -> Result<Vec<Applied>, SetupError> {
        let plan = self.plan;
        let registry = plan.registry.as_registry();
        let period = self.suite.period_ns;
        let mut states = vec![OutputState::EMPTY; plan.configs.len()];
        let mut intakes = vec![None; STAGING];
        let mut outputs = Outputs::new(
            &registry,
            &plan.writers,
            OutputsStorage {
                configs: &plan.configs,
                states: &mut states,
                intakes: &mut intakes,
            },
        )
        .map_err(|e| match e {
            joker_core::commit::CommitError::Config(c) => setup(c.code, alloc::format!("{e:?}")),
            other => setup("J0844", alloc::format!("{other:?}")),
        })?;
        if let Some(values) = inherited {
            for (config, value) in plan.configs.iter().zip(values) {
                outputs.inherit(config.signal, *value);
            }
        }
        let group = registry
            .resolve_group(GROUP)
            .ok_or_else(|| setup("J0844", "group missing from the registry"))?;
        let mut snapshot = vec![Sample::INITIAL; plan.ports.len()];
        let mut extenders = vec![SeqExtender::for_slot(); plan.ports.len()];
        let mut proposals = vec![Proposal::EMPTY; STAGING];
        let mut image = ProcessImage::new(
            group,
            period,
            ImageStorage {
                ports: &plan.ports,
                snapshot: &mut snapshot,
                extenders: &mut extenders,
                proposals: &mut proposals,
            },
        )
        .map_err(|e| setup("J0844", alloc::format!("process image: {e:?}")))?;
        let mut sink = Sink {
            results: Vec::with_capacity(plan.configs.len()),
        };
        let mut last = Vec::with_capacity(plan.configs.len());
        for tick in start..end {
            sink.results.clear();
            let core = guard.run(|| self.tick(tick, &mut image, &mut outputs, &mut sink));
            core.map_err(|e| setup("J0844", e))?;
            let record = self.record(&image, &sink)?;
            timeline.ticks.push(record);
        }
        for config in &plan.configs {
            let view = outputs.view(config.signal);
            last.push(view.map_or(Applied::from_safe(config.safe), |v| v.applied));
        }
        Ok(last)
    }

    fn tick(
        &self,
        tick: u64,
        image: &mut ProcessImage<'_>,
        outputs: &mut Outputs<'_>,
        sink: &mut Sink,
    ) -> Result<(), &'static str> {
        let plan = self.plan;
        let registry = plan.registry.as_registry();
        let period = self.suite.period_ns;
        let now = tick.saturating_mul(period);
        let quality = self.clock.now().clock;
        self.clock.set(Instant {
            mono_ns: now,
            utc_ns: i64::try_from(now).unwrap_or(i64::MAX),
            clock: quality,
        });
        let at_tick = || self.case.actions.iter().filter(move |a| a.tick == tick);
        for action in at_tick() {
            match &action.kind {
                ActionKind::Clock(q) => self.clock.set_quality(*q),
                ActionKind::Input {
                    signal,
                    bits,
                    quality,
                } => {
                    let id = plan.input_ids[*signal];
                    let slot = self.slots.get(id.index()).ok_or("missing slot")?;
                    slot.write(
                        *bits,
                        quality_of(quality.code, quality.reason),
                        NO_TS_SRC,
                        now,
                    );
                }
                ActionKind::Reset { output } => {
                    outputs.reset(plan.output_ids[*output]);
                }
                _ => {}
            }
        }
        image
            .latch(tick, self.clock.now(), self.slots, &registry)
            .map_err(|_| "latch failed")?;
        for stage_async in [true, false] {
            for action in at_tick() {
                let (output, writer, bits, flags) = match action.kind {
                    ActionKind::Propose {
                        output,
                        writer,
                        bits,
                    } => (output, writer, bits, 0),
                    ActionKind::Release { output, writer } => (output, writer, 0, RELINQUISH),
                    _ => continue,
                };
                let def = &self.suite.writers[writer];
                if def.is_async() != stage_async {
                    continue;
                }
                let proposal = Proposal {
                    writer: WriterId(u32::try_from(writer).map_err(|_| "too many writers")?),
                    signal: plan.output_ids[output],
                    value: bits,
                    flags,
                    ttl_ns: if def.kind == joker_core::WriterKind::Manual {
                        def.lease_ns.unwrap_or(0)
                    } else {
                        0
                    },
                    ..Proposal::EMPTY
                };
                let staged = if stage_async {
                    image.stage_async(proposal)
                } else {
                    image.stage_cycle(proposal)
                };
                staged.map_err(|_| "staging failed")?;
            }
        }
        outputs
            .commit(image, self.slots, sink)
            .map_err(|_| "commit failed")?;
        image.end_tick().map_err(|_| "end of tick failed")?;
        Ok(())
    }

    fn record(&self, image: &ProcessImage<'_>, sink: &Sink) -> Result<TickRecord, SetupError> {
        let plan = self.plan;
        let mut outputs = Vec::with_capacity(self.suite.outputs.len());
        for id in &plan.output_ids {
            let result = sink
                .results
                .iter()
                .find(|r| r.signal == *id)
                .copied()
                .ok_or_else(|| setup("J0844", "an output has no result in a tick"))?;
            let sample = self
                .slots
                .get(id.index())
                .and_then(|s| s.try_read().ok())
                .unwrap_or(Sample::INITIAL);
            outputs.push(OutputTick { result, sample });
        }
        let inputs = plan
            .input_ids
            .iter()
            .map(|id| image.sample(*id).copied().unwrap_or(Sample::INITIAL))
            .collect();
        Ok(TickRecord { outputs, inputs })
    }
}
