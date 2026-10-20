//! Steps 5–7 of a tick for the outputs of one group: arbiter, envelope and
//! commit, with the outcomes and decision records of ADR 0033 (docs/01 §4,
//! §19; docs/16 §1–§2).

use crate::arbiter::{Intake, OutputArbiter, Refusal, check_writers};
use crate::envelope::{Applied, Effective, Verdict, check_output, evaluate};
use crate::image::{ProcessImage, Proposal, WriterId};
use crate::outcome::{Outcome, RejectReason};
use crate::output::{Band, ConfigError, Level, OutputConfig, WriterConfig, WriterKind};
use crate::quality::Quality;
use crate::registry::{Direction, Registry, SignalId};
use crate::slot::Slot;
use crate::value::NO_TS_SRC;

/// The result of one proposal in one tick (ADR 0033 §1–§3).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WriteResult {
    /// Tick index.
    pub tick: u64,
    /// The proposal, as staged.
    pub proposal: Proposal,
    /// Its outcome.
    pub outcome: Outcome,
    /// The output's applied value in this tick.
    pub applied: Applied,
}

/// The result of one output in one tick: what the trace records and the
/// conformance cases check (ADR 0033 §4, docs/16 §7–§8).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutputResult {
    /// Tick index.
    pub tick: u64,
    /// The output.
    pub signal: SignalId,
    /// The output's outcome in this tick.
    pub outcome: Outcome,
    /// The applied value.
    pub applied: Applied,
    /// The active level; `None` is `"none"`.
    pub level: Option<Level>,
    /// The output is in trip.
    pub tripped: bool,
}

/// What a decision record is about.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RecordKind {
    /// The output's triple (applied value, outcome, active level).
    Output,
    /// One writer's proposals on the output.
    Write(WriterId),
}

/// Why a record was written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    /// A proposal of this writer.
    Proposal(WriterId),
    /// A lease or TTL ran out (`kind = expiry`, ref/09 §5.6).
    Expiry,
    /// The trip was reset (docs/16 E7).
    Reset,
    /// The envelope moved on its own (a ramp, docs/16 A7).
    Envelope,
}

/// Whether the host must persist a record at once (docs/01 §19).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Durability {
    /// fsync now: trips, manual writes, rejections, changes of active level.
    Immediate,
    /// In the next WAL batch.
    Batch,
}

/// A triple that held for a number of ticks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Applied value.
    pub applied: Applied,
    /// Outcome.
    pub outcome: Outcome,
    /// Active level.
    pub level: Option<Level>,
    /// First tick index.
    pub first_tick: u64,
    /// Last tick index.
    pub last_tick: u64,
    /// Number of ticks it held.
    pub repeats: u64,
}

/// One decision record, written on each change (docs/01 §19, ADR 0033 §10).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DecisionRecord {
    /// Output or writer record.
    pub kind: RecordKind,
    /// The output.
    pub output: SignalId,
    /// Tick index of the change.
    pub tick: u64,
    /// Monotonic latch instant of that tick.
    pub at_mono_ns: u64,
    /// New applied value.
    pub applied: Applied,
    /// New outcome.
    pub outcome: Outcome,
    /// New active level.
    pub level: Option<Level>,
    /// Why.
    pub cause: Cause,
    /// What held before, with its repeat count.
    pub previous: Option<Span>,
    /// Persistence class.
    pub durability: Durability,
}

/// Receives the results of a commit. Every method has an empty default.
pub trait CommitSink {
    /// One call per proposal.
    fn write(&mut self, _result: &WriteResult) {}
    /// One call per output of the group, in `SignalId` order.
    fn output(&mut self, _result: &OutputResult) {}
    /// One call per decision record.
    fn decision(&mut self, _record: &DecisionRecord) {}
}

impl CommitSink for () {}

/// Why a commit or a setup call failed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CommitError {
    /// An output or writer configuration error.
    Config(ConfigError),
    /// Storage slices of different lengths.
    StorageMismatch,
    /// Outputs not in strictly increasing `SignalId` order.
    OutputsNotSorted,
    /// An output is not an output signal of the registry, or its group
    /// differs from the registry's.
    NotAnOutput(SignalId),
    /// The intake buffer is smaller than the staged proposals.
    IntakeBufferTooSmall,
    /// The image has no tick in progress.
    NoTick,
    /// The slot table does not cover the outputs.
    MissingSlot(SignalId),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Track {
    span: Span,
    writer: WriterId,
}

/// The state of one output across ticks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutputState {
    arbiter: OutputArbiter,
    last: Applied,
    tripped: bool,
    reset_pending: bool,
    expired_now: bool,
    started: bool,
    output_track: Option<Track>,
    write_tracks: [Option<Track>; Level::COUNT],
}

impl OutputState {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        arbiter: OutputArbiter::new(0),
        last: Applied::Value(0),
        tripped: false,
        reset_pending: false,
        expired_now: false,
        started: false,
        output_track: None,
        write_tracks: [None; Level::COUNT],
    };
}

/// A read-only view of an output.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutputView {
    /// Last applied value.
    pub applied: Applied,
    /// Active level after the last commit.
    pub level: Option<Level>,
    /// In trip.
    pub tripped: bool,
    /// Outcome of the last tick.
    pub outcome: Outcome,
}

/// Fixed storage for [`Outputs`].
#[derive(Debug)]
pub struct OutputsStorage<'a> {
    /// Output configurations, in strictly increasing `SignalId` order.
    pub configs: &'a [OutputConfig<'a>],
    /// One state per configuration.
    pub states: &'a mut [OutputState],
    /// Scratch buffer, at least as long as the staging capacity of any image.
    pub intakes: &'a mut [Option<Intake>],
}

/// The outputs of a generation, with their arbiters and envelopes.
#[derive(Debug)]
pub struct Outputs<'a> {
    writers: &'a [WriterConfig],
    configs: &'a [OutputConfig<'a>],
    states: &'a mut [OutputState],
    intakes: &'a mut [Option<Intake>],
}

impl<'a> Outputs<'a> {
    /// Checks every output and the writer table, and starts every output at
    /// its `safe` value (docs/16 A8).
    pub fn new(
        registry: &Registry<'_>,
        writers: &'a [WriterConfig],
        storage: OutputsStorage<'a>,
    ) -> Result<Self, CommitError> {
        let OutputsStorage {
            configs,
            states,
            intakes,
        } = storage;
        if states.len() != configs.len() {
            return Err(CommitError::StorageMismatch);
        }
        if configs.windows(2).any(|w| w[0].signal >= w[1].signal) {
            return Err(CommitError::OutputsNotSorted);
        }
        for (config, state) in configs.iter().zip(states.iter_mut()) {
            let meta = registry
                .signal(config.signal)
                .filter(|m| m.direction == Direction::Output && m.ty == config.ty)
                .ok_or(CommitError::NotAnOutput(config.signal))?;
            if meta.group_id.is_some_and(|g| g != config.group) {
                return Err(CommitError::NotAnOutput(config.signal));
            }
            let period_ns = registry
                .group(config.group)
                .map(|g| g.period_ns)
                .ok_or(CommitError::NotAnOutput(config.signal))?;
            check_output(config).map_err(CommitError::Config)?;
            check_writers(config, writers, period_ns).map_err(CommitError::Config)?;
            *state = OutputState {
                arbiter: OutputArbiter::new(period_ns),
                last: Applied::from_safe(config.safe),
                ..OutputState::EMPTY
            };
        }
        Ok(Self {
            writers,
            configs,
            states,
            intakes,
        })
    }

    fn find(&self, signal: SignalId) -> Option<usize> {
        self.configs
            .binary_search_by(|c| c.signal.cmp(&signal))
            .ok()
    }

    /// A new generation that keeps an output starts from the last value the
    /// previous generation applied (docs/16 A8). Call before the first commit.
    pub fn inherit(&mut self, signal: SignalId, applied: Applied) -> bool {
        match self.find(signal) {
            Some(i) if !self.states[i].started => {
                self.states[i].last = applied;
                true
            }
            _ => false,
        }
    }

    /// Ends a trip (`jk reset`, docs/16 E7) at the next commit of the owner
    /// group. Returns false if the output is unknown or not in trip.
    pub fn reset(&mut self, signal: SignalId) -> bool {
        match self.find(signal) {
            Some(i) if self.states[i].tripped => {
                self.states[i].reset_pending = true;
                true
            }
            _ => false,
        }
    }

    /// The output configurations.
    pub fn configs(&self) -> &[OutputConfig<'a>] {
        self.configs
    }

    /// The state of an output after the last commit.
    pub fn view(&self, signal: SignalId) -> Option<OutputView> {
        let state = &self.states[self.find(signal)?];
        Some(OutputView {
            applied: state.last,
            level: state.arbiter.active().map(|(l, _)| l),
            tripped: state.tripped,
            outcome: state.output_track.map_or(Outcome::None, |t| t.span.outcome),
        })
    }

    /// The outcome of the last proposal of `writer` on `signal`
    /// (`last_outcome()`, docs/02 §7.1).
    pub fn last_outcome(&self, writer: WriterId, signal: SignalId) -> Option<Outcome> {
        let state = &self.states[self.find(signal)?];
        let level = self.writers.get(writer.index())?.level;
        state.write_tracks[level.index()]
            .filter(|t| t.writer == writer)
            .map(|t| t.span.outcome)
    }

    /// Steps 5–7 for every output owned by the image's group: expires
    /// leases, takes the staged proposals, runs the envelope, writes the
    /// applied values to the output slots, and reports results and decision
    /// records to `sink`. Call between the cycle tasks and `end_tick`.
    pub fn commit(
        &mut self,
        image: &ProcessImage<'_>,
        slots: &[Slot],
        sink: &mut impl CommitSink,
    ) -> Result<(), CommitError> {
        let tick = image.tick().ok_or(CommitError::NoTick)?;
        let proposals = image.proposals();
        if self.intakes.len() < proposals.len() {
            return Err(CommitError::IntakeBufferTooSmall);
        }
        let group = image.group();
        let now = tick.at.mono_ns;
        for (config, state) in self.configs.iter().zip(self.states.iter_mut()) {
            if config.group == group {
                state.expired_now = state.arbiter.expire(now) != 0;
            }
        }
        for (i, proposal) in proposals.iter().enumerate() {
            let intake = match self
                .configs
                .binary_search_by(|c| c.signal.cmp(&proposal.signal))
            {
                Ok(o) if self.configs[o].group == group => {
                    self.states[o]
                        .arbiter
                        .propose(&self.configs[o], self.writers, proposal, now)
                }
                _ => Intake::Refused(Refusal::NotOwner),
            };
            self.intakes[i] = Some(intake);
        }
        for (i, proposal) in proposals.iter().enumerate() {
            if self
                .find(proposal.signal)
                .is_none_or(|o| self.configs[o].group != group)
            {
                let result = WriteResult {
                    tick: tick.index,
                    proposal: *proposal,
                    outcome: Outcome::Rejected(RejectReason::NotOwner),
                    applied: Applied::Value(0),
                };
                sink.write(&result);
                sink.decision(&DecisionRecord {
                    kind: RecordKind::Write(proposal.writer),
                    output: proposal.signal,
                    tick: tick.index,
                    at_mono_ns: now,
                    applied: result.applied,
                    outcome: result.outcome,
                    level: None,
                    cause: Cause::Proposal(proposal.writer),
                    previous: None,
                    durability: Durability::Immediate,
                });
                self.intakes[i] = None;
            }
        }
        for o in 0..self.configs.len() {
            let config = self.configs[o];
            if config.group != group {
                continue;
            }
            let slot = slots
                .get(config.signal.index())
                .ok_or(CommitError::MissingSlot(config.signal))?;
            let state = &mut self.states[o];
            let reset = core::mem::take(&mut state.reset_pending);
            if reset {
                state.tripped = false;
            }
            let active = state.arbiter.active();
            let effective = match active {
                None => Effective::Safe,
                Some((_, entry)) => Effective::Writer {
                    value: entry.value,
                    requires_clock: self.writers[entry.writer.index()].requires_clock,
                },
            };
            let Verdict {
                applied,
                outcome: envelope_outcome,
                tripped,
            } = evaluate(
                &config,
                state.last,
                state.tripped,
                effective,
                tick.at.clock,
                tick.dt_ns,
            );
            let level = active.map(|(l, _)| l);
            let active_writer = active.map(|(_, e)| e.writer);
            let mut output_outcome = None;
            let mut last_outcome = None;
            let mut active_cause = None;
            let mut any_cause = None;
            for (i, proposal) in proposals.iter().enumerate() {
                if proposal.signal != config.signal {
                    continue;
                }
                let Some(intake) = self.intakes[i] else {
                    continue;
                };
                let superseded = proposals[i + 1..].iter().any(|later| {
                    later.signal == proposal.signal && later.writer == proposal.writer
                });
                let outcome = match intake {
                    Intake::Refused(Refusal::NotOwner) => Outcome::Rejected(RejectReason::NotOwner),
                    Intake::Refused(Refusal::WrongType) => {
                        Outcome::Rejected(RejectReason::WrongType)
                    }
                    Intake::Relinquished(_) => Outcome::Accepted,
                    Intake::Placed(own) if superseded => Outcome::Overridden(own),
                    Intake::Placed(own) => match level {
                        Some(active_level) if active_level == own => {
                            envelope_outcome.unwrap_or(Outcome::Accepted)
                        }
                        Some(active_level) => Outcome::Overridden(active_level),
                        None => Outcome::Overridden(own),
                    },
                };
                if Some(proposal.writer) == active_writer
                    && matches!(intake, Intake::Placed(_))
                    && !superseded
                {
                    output_outcome = Some(outcome);
                }
                last_outcome = Some(outcome);
                if Some(proposal.writer) == active_writer {
                    active_cause = Some(Cause::Proposal(proposal.writer));
                }
                any_cause = Some(Cause::Proposal(proposal.writer));
                let result = WriteResult {
                    tick: tick.index,
                    proposal: *proposal,
                    outcome,
                    applied,
                };
                sink.write(&result);
                let writer = self.writers.get(proposal.writer.index());
                let manual = writer.is_some_and(|w| w.kind == WriterKind::Manual);
                let durability = if manual || outcome.is_rejected() || outcome == Outcome::Tripped {
                    Durability::Immediate
                } else {
                    Durability::Batch
                };
                let level = writer.map(|w| own_level(intake, w.level));
                let due = match writer {
                    Some(w) => advance(
                        &mut state.write_tracks[w.level.index()],
                        proposal.writer,
                        applied,
                        outcome,
                        level,
                        tick.index,
                        false,
                    ),
                    None => Some(None),
                };
                if let Some(previous) = due {
                    sink.decision(&DecisionRecord {
                        kind: RecordKind::Write(proposal.writer),
                        output: config.signal,
                        tick: tick.index,
                        at_mono_ns: now,
                        applied,
                        outcome,
                        level,
                        cause: Cause::Proposal(proposal.writer),
                        previous,
                        durability,
                    });
                }
            }
            if tripped {
                state.tripped = true;
            }
            state.last = applied;
            let outcome = output_outcome.or(last_outcome).unwrap_or(Outcome::None);
            let previous_track = state.output_track;
            let output_writer = active_writer.unwrap_or(WriterId(u32::MAX));
            if let Some(previous) = advance(
                &mut state.output_track,
                output_writer,
                applied,
                outcome,
                level,
                tick.index,
                reset || tripped,
            ) {
                let level_changed = previous.is_none_or(|p| p.level != level);
                let manual = level.is_some_and(|l| l.band() == Band::Manual);
                let durability = if level_changed
                    || outcome.is_rejected()
                    || outcome == Outcome::Tripped
                    || tripped
                    || reset
                    || manual
                {
                    Durability::Immediate
                } else {
                    Durability::Batch
                };
                let cause = if reset {
                    Cause::Reset
                } else if let Some(cause) = active_cause {
                    cause
                } else if state.expired_now {
                    Cause::Expiry
                } else {
                    any_cause.unwrap_or(Cause::Envelope)
                };
                sink.decision(&DecisionRecord {
                    kind: RecordKind::Output,
                    output: config.signal,
                    tick: tick.index,
                    at_mono_ns: now,
                    applied,
                    outcome,
                    level,
                    cause,
                    previous,
                    durability,
                });
            }
            if !state.started || previous_track.is_none_or(|t| t.span.applied != applied) {
                slot.write(applied.bits(), Quality::GOOD, NO_TS_SRC, now);
            }
            state.started = true;
            sink.output(&OutputResult {
                tick: tick.index,
                signal: config.signal,
                outcome,
                applied,
                level,
                tripped: state.tripped,
            });
        }
        Ok(())
    }
}

fn own_level(intake: Intake, fallback: Level) -> Level {
    match intake {
        Intake::Placed(level) => level,
        _ => fallback,
    }
}

/// Moves a track to this tick. Returns `Some(previous)` when the triple or
/// the writer changed, or `force` is set (a record is due), and `None` when
/// it repeats.
fn advance(
    track: &mut Option<Track>,
    writer: WriterId,
    applied: Applied,
    outcome: Outcome,
    level: Option<Level>,
    tick: u64,
    force: bool,
) -> Option<Option<Span>> {
    if !force
        && let Some(t) = track
        && t.writer == writer
        && t.span.applied == applied
        && t.span.outcome == outcome
        && t.span.level == level
    {
        t.span.last_tick = tick;
        t.span.repeats += 1;
        return None;
    }
    let previous = track.map(|t| t.span);
    *track = Some(Track {
        writer,
        span: Span {
            applied,
            outcome,
            level,
            first_tick: tick,
            last_tick: tick,
            repeats: 1,
        },
    });
    Some(previous)
}
