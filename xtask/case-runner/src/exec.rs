//! Runs one case against `joker-core` with a virtual clock and hands the
//! observer, for every tick the case covers, the result of every output and
//! the latched sample of every input (ref/03 §2.6.4).
//!
//! The initial state (clock `synced`), the step where each writer kind's
//! proposals enter, the order of the actions at a tick boundary and the
//! generation swap follow ADR 0042.
//!
//! All the storage of the core lives on the stack, in arrays whose lengths
//! are the const parameters of [`run`]: `S` signals (inputs and outputs),
//! `O` outputs, `W` writers and `P` staged proposals per tick. Each tick is
//! split in two: the core part (inputs into slots, latch, staging, commit),
//! which runs inside the [`TickGuard`], and the copy of the results into the
//! [`TickView`].

use joker_core::commit::{CommitError, OutputState};
use joker_core::image::{ImageStorage, RELINQUISH};
use joker_core::output::{OnInterlock, OutputConfig};
use joker_core::quality::{Quality, QualityCode, QualityReason};
use joker_core::registry::{Alias, GroupId, GroupMeta, Registry, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::{Clock, ClockQuality, Instant, VirtualClock};
use joker_core::value::NO_TS_SRC;
use joker_core::{
    Applied, CommitSink, Level, Outcome, OutputResult, Outputs, OutputsStorage, ProcessImage,
    Proposal, RegistryBuilder, SafeValue, Sample, SignalId, Slot, ValueType, WriterConfig,
    WriterId, WriterKind,
};

use crate::model::{ActionKind, Fixture, SafeDef, Script};

/// The name of the only cycle group of a core suite.
pub const GROUP: &str = "case";

/// The capacity of the staging buffer of one tick, in proposals, on every
/// target: a proposal past it is dropped by the core, so the host and the
/// firmware use the same value.
pub const STAGING: usize = 256;

/// Why a case could not run: the fixture does not build in the core.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SetupError {
    /// The code the core returned (`J0203`, `J0210`, …), `J0805` for the
    /// registry, or `J0844`.
    pub code: &'static str,
    /// What failed.
    pub message: &'static str,
}

/// What one output did in one tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutputTick {
    /// The result of the core.
    pub result: OutputResult,
    /// The sample of the output slot after the commit.
    pub sample: Sample,
}

impl OutputTick {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        result: OutputResult {
            tick: 0,
            signal: SignalId(0),
            outcome: Outcome::None,
            applied: Applied::Relinquish,
            level: None,
            tripped: false,
        },
        sample: Sample::INITIAL,
    };
}

/// What the case saw in one tick.
#[derive(Copy, Clone, Debug)]
pub struct TickView<'a> {
    /// Per output, in the order of [`Fixture::outputs`].
    pub outputs: &'a [OutputTick],
    /// Per input, the latched sample, in the order of [`Fixture::signals`].
    pub inputs: &'a [Sample],
}

/// Receives every tick of a case, in order, outside the guard.
pub trait Observer {
    /// Tick `tick` (from 0) has committed.
    fn tick(&mut self, tick: u64, view: &TickView<'_>);
}

/// Runs code that must not allocate: the core part of every tick. The host
/// runner installs a guard that counts allocator calls; a firmware with no
/// allocator linked runs [`Unguarded`].
pub trait TickGuard {
    /// Runs `f` inside the guard.
    fn run<R>(&self, f: impl FnOnce() -> R) -> R;

    /// Allocator calls seen inside guarded code on this thread so far.
    fn violations(&self) -> u32 {
        0
    }
}

/// The identity guard, for a target where nothing can allocate.
#[derive(Copy, Clone, Debug, Default)]
pub struct Unguarded;

impl TickGuard for Unguarded {
    fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        f()
    }
}

#[derive(Debug)]
struct Sink<'a> {
    results: &'a mut [Option<OutputResult>],
    len: usize,
}

impl CommitSink for Sink<'_> {
    fn output(&mut self, result: &OutputResult) {
        if let Some(slot) = self.results.get_mut(self.len) {
            *slot = Some(*result);
            self.len += 1;
        }
    }
}

const fn setup(code: &'static str, message: &'static str) -> SetupError {
    SetupError { code, message }
}

fn quality_of(code: u8, reason: Option<u8>) -> Quality {
    let code = match code {
        0 => QualityCode::Good,
        1 => QualityCode::Uncertain,
        _ => QualityCode::Bad,
    };
    Quality::new(code, QualityReason::from_u8(reason.unwrap_or(0)))
}

/// The parts of a fixture that the core keeps for a whole generation, built
/// once per case.
#[derive(Debug)]
struct Plan<'p> {
    registry: Registry<'p>,
    group: GroupId,
    writers: &'p [WriterConfig],
    configs: &'p [OutputConfig<'static>],
    output_ids: &'p [SignalId],
    input_ids: &'p [SignalId],
    ports: &'p [SignalId],
    slots: &'p [Slot],
}

/// Runs a case and hands every tick to `observer`.
///
/// # Errors
///
/// A [`SetupError`] when the fixture does not fit the capacities, when the
/// core refuses it (`J0203`, `J0210`, …), or when a tick fails in the core.
pub fn run<const S: usize, const O: usize, const W: usize, const P: usize>(
    fixture: &Fixture<'_>,
    script: &Script<'_>,
    guard: &impl TickGuard,
    observer: &mut impl Observer,
) -> Result<(), SetupError> {
    let n_in = fixture.signals.len();
    let n_out = fixture.outputs.len();
    let n_writers = fixture.writers.len();
    if n_in + n_out > S || n_out > O || n_writers > W {
        return Err(setup(
            "J0844",
            "the fixture exceeds the capacity of the runner",
        ));
    }
    let mut metas = [SignalMeta::EMPTY; S];
    let mut aliases: [Alias<'_>; 0] = [];
    let mut groups = [GroupMeta::EMPTY; 1];
    let mut builder = RegistryBuilder::new(&mut metas, &mut aliases, &mut groups);
    let registry_error = |_| setup("J0805", "the registry does not build");
    for s in fixture.signals {
        let mut meta = SignalMeta::input(s.path, s.ty, UnitRef(s.unit));
        meta.max_age_ns = s.max_age_ns;
        builder.signal(meta).map_err(registry_error)?;
    }
    for o in fixture.outputs {
        builder
            .signal(SignalMeta::output(o.path, o.ty, UnitRef(o.unit), GROUP))
            .map_err(registry_error)?;
    }
    builder
        .group(GroupMeta {
            name: GROUP,
            period_ns: fixture.period_ns,
        })
        .map_err(registry_error)?;
    let registry = builder.build().map_err(registry_error)?;
    let missing = || setup("J0844", "a path is not in the registry");
    let group = registry
        .resolve_group(GROUP)
        .ok_or_else(|| setup("J0844", "group missing from the registry"))?;

    let mut input_ids = [SignalId(0); S];
    for (id, s) in input_ids.iter_mut().zip(fixture.signals) {
        *id = registry.resolve(s.path).ok_or_else(missing)?;
    }
    let mut output_ids = [SignalId(0); O];
    for (id, o) in output_ids.iter_mut().zip(fixture.outputs) {
        *id = registry.resolve(o.path).ok_or_else(missing)?;
    }
    let mut order = [0usize; O];
    for (i, slot) in order.iter_mut().enumerate() {
        *slot = i;
    }
    order[..n_out].sort_unstable_by_key(|&i| output_ids[i]);
    let mut configs =
        [OutputConfig::new(SignalId(0), ValueType::F64, group, SafeValue::Relinquish); O];
    for (config, &i) in configs.iter_mut().zip(&order[..n_out]) {
        let o = &fixture.outputs[i];
        let safe = match o.safe {
            SafeDef::Value(bits) => SafeValue::Value(bits),
            SafeDef::Relinquish => SafeValue::Relinquish,
        };
        let mut c = OutputConfig::new(output_ids[i], o.ty, group, safe);
        c.safe_ramp = o.safe_ramp;
        c.range = o.range;
        c.on_range = o.on_range;
        c.rate = o.rate;
        c.on_interlock = OnInterlock::Safe;
        if let Some(ns) = o.max_lease_ns {
            c.max_lease_ns = ns;
        }
        if let Some(ns) = o.max_manual_ns {
            c.max_manual_ns = ns;
        }
        *config = c;
    }
    let mut writers = [WriterConfig::EMPTY; W];
    for (config, w) in writers.iter_mut().zip(fixture.writers) {
        *config = WriterConfig {
            kind: w.kind,
            level: Level::new(w.level)
                .ok_or_else(|| setup("J0844", "a writer level is outside 1-16"))?,
            lease_ns: w.lease_ns,
            requires_clock: w.requires_clock,
        };
    }
    let mut ports = input_ids;
    ports[..n_in].sort_unstable();
    let slots: [Slot; S] = core::array::from_fn(|_| Slot::new());
    let plan = Plan {
        registry,
        group,
        writers: &writers[..n_writers],
        configs: &configs[..n_out],
        output_ids: &output_ids[..n_out],
        input_ids: &input_ids[..n_in],
        ports: &ports[..n_in],
        slots: &slots[..registry.len()],
    };
    let clock = VirtualClock::new(Instant {
        mono_ns: 0,
        utc_ns: 0,
        clock: ClockQuality::Synced,
    });
    let segment = Segment {
        fixture,
        script,
        plan: &plan,
        clock: &clock,
    };
    let mut inherited = [Applied::Relinquish; O];
    let mut has_inherited = false;
    let mut start = 0;
    loop {
        let next = script
            .actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::Generation) && a.tick > start)
            .map(|a| a.tick)
            .min();
        let end = match next {
            Some(g) => g,
            None if script.ticks > start => script.ticks,
            None => break,
        };
        let from = has_inherited.then_some(&inherited[..n_out]);
        let mut last = [Applied::Relinquish; O];
        segment.run::<S, O, P>(start, end, from, &mut last[..n_out], guard, observer)?;
        inherited = last;
        has_inherited = true;
        start = end;
        if next.is_none() {
            break;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Segment<'r> {
    fixture: &'r Fixture<'r>,
    script: &'r Script<'r>,
    plan: &'r Plan<'r>,
    clock: &'r VirtualClock,
}

impl Segment<'_> {
    /// Runs the ticks `start..end` of one generation and writes the last
    /// applied value of every output, in the order of the configs, to
    /// `last`.
    fn run<const S: usize, const O: usize, const P: usize>(
        &self,
        start: u64,
        end: u64,
        inherited: Option<&[Applied]>,
        last: &mut [Applied],
        guard: &impl TickGuard,
        observer: &mut impl Observer,
    ) -> Result<(), SetupError> {
        let plan = self.plan;
        let n_out = plan.configs.len();
        let n_in = plan.ports.len();
        let mut states = [OutputState::EMPTY; O];
        let mut intakes = [None; P];
        let mut outputs = Outputs::new(
            &plan.registry,
            plan.writers,
            OutputsStorage {
                configs: plan.configs,
                states: &mut states[..n_out],
                intakes: &mut intakes,
            },
        )
        .map_err(|e| match e {
            CommitError::Config(c) => setup(c.code, "the core refuses the output configuration"),
            _ => setup("J0844", "the outputs do not build"),
        })?;
        if let Some(values) = inherited {
            for (config, value) in plan.configs.iter().zip(values) {
                outputs.inherit(config.signal, *value);
            }
        }
        let mut snapshot = [Sample::INITIAL; S];
        let mut extenders = [SeqExtender::for_slot(); S];
        let mut proposals = [Proposal::EMPTY; P];
        let mut image = ProcessImage::new(
            plan.group,
            self.fixture.period_ns,
            ImageStorage {
                ports: plan.ports,
                snapshot: &mut snapshot[..n_in],
                extenders: &mut extenders[..n_in],
                proposals: &mut proposals,
            },
        )
        .map_err(|_| setup("J0844", "the process image does not build"))?;
        let mut results = [None; O];
        let mut record = [OutputTick::EMPTY; O];
        let mut inputs = [Sample::INITIAL; S];
        for tick in start..end {
            let mut sink = Sink {
                results: &mut results[..n_out],
                len: 0,
            };
            let core = guard.run(|| self.tick(tick, &mut image, &mut outputs, &mut sink));
            core.map_err(|message| setup("J0844", message))?;
            let results = &sink.results[..sink.len];
            for (out, id) in record.iter_mut().zip(plan.output_ids) {
                let result = results
                    .iter()
                    .flatten()
                    .find(|r| r.signal == *id)
                    .copied()
                    .ok_or_else(|| setup("J0844", "an output has no result in a tick"))?;
                let sample = plan
                    .slots
                    .get(id.index())
                    .and_then(|s| s.try_read().ok())
                    .unwrap_or(Sample::INITIAL);
                *out = OutputTick { result, sample };
            }
            for (sample, id) in inputs.iter_mut().zip(plan.input_ids) {
                *sample = image.sample(*id).copied().unwrap_or(Sample::INITIAL);
            }
            observer.tick(
                tick,
                &TickView {
                    outputs: &record[..n_out],
                    inputs: &inputs[..n_in],
                },
            );
        }
        for (value, config) in last.iter_mut().zip(plan.configs) {
            let view = outputs.view(config.signal);
            *value = view.map_or(Applied::from_safe(config.safe), |v| v.applied);
        }
        Ok(())
    }

    fn tick(
        &self,
        tick: u64,
        image: &mut ProcessImage<'_>,
        outputs: &mut Outputs<'_>,
        sink: &mut Sink<'_>,
    ) -> Result<(), &'static str> {
        let plan = self.plan;
        let now = tick.saturating_mul(self.fixture.period_ns);
        let quality = self.clock.now().clock;
        self.clock.set(Instant {
            mono_ns: now,
            utc_ns: i64::try_from(now).unwrap_or(i64::MAX),
            clock: quality,
        });
        let at_tick = || self.script.actions.iter().filter(move |a| a.tick == tick);
        for action in at_tick() {
            match action.kind {
                ActionKind::Clock(q) => self.clock.set_quality(q),
                ActionKind::Input {
                    signal,
                    bits,
                    quality,
                } => {
                    let id = plan.input_ids.get(signal).ok_or("unknown input")?;
                    let slot = plan.slots.get(id.index()).ok_or("missing slot")?;
                    slot.write(
                        bits,
                        quality_of(quality.code, quality.reason),
                        NO_TS_SRC,
                        now,
                    );
                }
                ActionKind::Reset { output } => {
                    let id = plan.output_ids.get(output).ok_or("unknown output")?;
                    outputs.reset(*id);
                }
                _ => {}
            }
        }
        image
            .latch(tick, self.clock.now(), plan.slots, &plan.registry)
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
                let def = self.fixture.writers.get(writer).ok_or("unknown writer")?;
                if def.is_async() != stage_async {
                    continue;
                }
                let proposal = Proposal {
                    writer: WriterId(u32::try_from(writer).map_err(|_| "too many writers")?),
                    signal: *plan.output_ids.get(output).ok_or("unknown output")?,
                    value: bits,
                    flags,
                    ttl_ns: if def.kind == WriterKind::Manual {
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
            .commit(image, plan.slots, sink)
            .map_err(|_| "commit failed")?;
        image.end_tick().map_err(|_| "end of tick failed")?;
        Ok(())
    }
}
