//! Steps 5–7 of a tick for the outputs of one group: arbiter, envelope and
//! commit, with the outcomes and decision records of ADR 0033 (docs/01 §4,
//! §19; docs/16 §1–§2).

use crate::arbiter::{Intake, LevelEntry, OutputArbiter, Refusal, RemoteWriter, check_writers};
use joker_expr::{EvalError, Inputs, Program, Reading};

use crate::envelope::{Applied, Context, Effective, Verdict, check_output, evaluate_in};
use crate::image::{Intent, ProcessImage, Proposal, WriterId};
use crate::outcome::{Outcome, RejectReason};
use crate::output::{
    AtomicGroup, Band, Bound, ClockRequirement, ConfigError, Level, OnRate, OutputConfig,
    WriterConfig, WriterKind,
};
use crate::quality::Quality;
use crate::quality::{QualityCode, QualityReason};
use crate::registry::{Direction, GroupId, Registry, SignalId};
use crate::slot::Slot;
use crate::value::NO_TS_SRC;
use crate::value::Sample;

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
    /// The writer's level was freed so that this principal could take it
    /// (`kind = takeover`, docs/16 A13).
    Takeover(WriterId),
    /// The writer's level was freed by this principal with `force`.
    ForcedRelease(WriterId),
}

/// A change of a manual level, for the persistence of docs/16 A14. The host
/// stores it with fsync (with the deadline in UTC) and replays it with
/// [`Outputs::restore_manual`] before the first tick after a restart.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ManualRecord {
    /// The output.
    pub output: SignalId,
    /// The manual level.
    pub level: Level,
    /// The principal's writer.
    pub writer: WriterId,
    /// What happened.
    pub change: ManualChange,
}

/// What happened to a manual level.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ManualChange {
    /// Written or renewed: the value and the monotonic deadline.
    Set {
        /// Value bits.
        value: u64,
        /// `lease_until`, monotonic ns.
        lease_until: u64,
    },
    /// Freed: relinquished, taken over, force-released or expired.
    Cleared(Cause),
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
    /// One call per change of a manual level (docs/16 A14).
    fn manual(&mut self, _record: &ManualRecord) {}
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
    /// Not a manual writer, or a value the output cannot take.
    NotManual(WriterId),
    /// The output has already been committed once.
    AlreadyStarted,
    /// The state arena is missing or smaller than [`Outputs::state_words`].
    StateArenaTooSmall,
    /// An atomic group without members.
    AtomicTooSmall,
}

/// Most members an atomic group may have.
pub const MAX_ATOMIC_MEMBERS: usize = 16;

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
    arena_offset: usize,
    bad_since: Option<u64>,
    last_actuation: Option<u64>,
    pending: Option<Pending>,
    output_track: Option<Track>,
    write_tracks: [Option<Track>; Level::COUNT],
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Pending {
    verdict: Verdict,
    reset: bool,
    ring: usize,
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
        arena_offset: 0,
        bad_since: None,
        last_actuation: None,
        pending: None,
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
    remote: &'a [RemoteWriter],
    atomic: &'a [AtomicGroup<'a>],
    arena: &'a mut [u64],
    state_words: usize,
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
        let mut state_words = 0usize;
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
                arena_offset: state_words,
                ..OutputState::EMPTY
            };
            state_words += config.state_words();
        }
        Ok(Self {
            writers,
            remote: &[],
            atomic: &[],
            arena: &mut [],
            state_words,
            configs,
            states,
            intakes,
        })
    }

    /// Words of state arena the outputs need: the state of their expressions
    /// and their actuation budgets. Zero for outputs without either.
    pub fn state_words(&self) -> usize {
        self.state_words
    }

    /// Hands in the state arena, at least [`Outputs::state_words`] long, and
    /// zeroes it (the initial state of every expression, ADR 0034).
    pub fn set_state_arena(&mut self, arena: &'a mut [u64]) -> Result<(), CommitError> {
        if arena.len() < self.state_words {
            return Err(CommitError::StateArenaTooSmall);
        }
        arena.fill(0);
        self.arena = arena;
        Ok(())
    }

    /// Sets the atomic groups (M20, ref/01 §5.16): at least two members,
    /// each an output of this generation (`NotAnOutput`), in one group only
    /// (`J0270`), all with the same owner group (`J0272`), an `order` that is
    /// empty or a permutation of the members (`J0273`), and a commit register
    /// that is neither a member nor an output with an envelope (`J0274`).
    pub fn set_atomic_groups(&mut self, groups: &'a [AtomicGroup<'a>]) -> Result<(), CommitError> {
        let err = |code, output| {
            CommitError::Config(ConfigError {
                code,
                output: Some(output),
                writer: None,
            })
        };
        for (g, atomic) in groups.iter().enumerate() {
            let Some(&first) = atomic.outputs.first() else {
                return Err(CommitError::AtomicTooSmall);
            };
            if atomic.outputs.len() < 2 || atomic.outputs.len() > MAX_ATOMIC_MEMBERS {
                return Err(CommitError::AtomicTooSmall);
            }
            let owner = self
                .find(first)
                .map(|o| self.configs[o].group)
                .ok_or(CommitError::NotAnOutput(first))?;
            for (i, &member) in atomic.outputs.iter().enumerate() {
                let o = self.find(member).ok_or(CommitError::NotAnOutput(member))?;
                if self.configs[o].group != owner {
                    return Err(err("J0272", member));
                }
                let repeated = atomic.outputs[..i].contains(&member)
                    || groups[..g]
                        .iter()
                        .any(|other| other.outputs.contains(&member));
                if repeated {
                    return Err(err("J0270", member));
                }
            }
            let permutation = atomic.order.len() == atomic.outputs.len()
                && atomic.order.iter().all(|m| atomic.outputs.contains(m))
                && atomic
                    .order
                    .iter()
                    .enumerate()
                    .all(|(i, m)| !atomic.order[..i].contains(m));
            if !atomic.order.is_empty() && !permutation {
                return Err(err("J0273", first));
            }
            if let Some((signal, _)) = atomic.commit
                && (atomic.outputs.contains(&signal) || self.find(signal).is_some())
            {
                return Err(err("J0274", signal));
            }
        }
        self.atomic = groups;
        Ok(())
    }

    /// Step 7 for one atomic group owned by `group` (ADR 0039). Returns
    /// whether any member applies a new value.
    fn apply_atomic(&mut self, atomic: &AtomicGroup<'_>, group: GroupId) -> bool {
        let mut members = [0usize; MAX_ATOMIC_MEMBERS];
        let mut n = 0;
        for &member in atomic.outputs {
            match self.find(member) {
                Some(o) if self.configs[o].group == group && n < MAX_ATOMIC_MEMBERS => {
                    members[n] = o;
                    n += 1;
                }
                _ => return false,
            }
        }
        let members = &members[..n];
        let failed = members.iter().any(|&o| {
            self.states[o].pending.is_some_and(|p| {
                matches!(
                    p.verdict.outcome,
                    Some(Outcome::Rejected(_)) | Some(Outcome::Tripped)
                )
            })
        });
        if failed {
            for &o in members {
                let last = self.states[o].last;
                if let Some(p) = self.states[o].pending.as_mut()
                    && matches!(
                        p.verdict.outcome,
                        Some(
                            Outcome::Accepted
                                | Outcome::ClampedRange
                                | Outcome::ClampedRate
                                | Outcome::Suppressed
                        )
                    )
                {
                    p.verdict.applied = last;
                    p.verdict.outcome = Some(Outcome::Rejected(RejectReason::AtomicGroup));
                }
            }
        } else if atomic.on_rate == OnRate::Proportional {
            self.scale_atomic(members);
        }
        members.iter().any(|&o| {
            self.states[o]
                .pending
                .is_some_and(|p| p.verdict.applied != self.states[o].last)
        })
    }

    /// The proportional rate of ADR 0039 §4.
    fn scale_atomic(&mut self, members: &[usize]) {
        let mut steps: [Option<(f64, f64, f64)>; MAX_ATOMIC_MEMBERS] = [None; MAX_ATOMIC_MEMBERS];
        for (slot, &o) in steps.iter_mut().zip(members) {
            let state = &self.states[o];
            let ty = self.configs[o].ty;
            *slot = (|| {
                let p = state.pending?;
                let Applied::Value(last) = state.last else {
                    return None;
                };
                let Applied::Value(applied) = p.verdict.applied else {
                    return None;
                };
                let last = ty.bits_to_f64(last)?;
                let requested = ty.bits_to_f64(p.verdict.requested?)? - last;
                let allowed = ty.bits_to_f64(applied)? - last;
                Some((last, requested, allowed))
            })();
        }
        let fraction = steps
            .iter()
            .flatten()
            .filter(|(_, requested, _)| *requested != 0.0)
            .map(|(_, requested, allowed)| allowed / requested)
            .fold(1.0f64, f64::min);
        if fraction >= 1.0 {
            return;
        }
        for (step, &o) in steps.iter().zip(members) {
            let ty = self.configs[o].ty;
            if let Some((last, requested, _)) = *step
                && requested != 0.0
                && let Some(bits) = ty.f64_to_bits(last + fraction * requested)
                && let Some(p) = self.states[o].pending.as_mut()
            {
                p.verdict.applied = Applied::Value(bits);
                if p.verdict.outcome != Some(Outcome::ClampedRange) {
                    p.verdict.outcome = Some(Outcome::ClampedRate);
                }
            }
        }
    }

    /// Sets the remote writers of an MCU's outputs (docs/16 A12), indexed by
    /// `WriterId::remote_slot`.
    pub fn set_remote_writers(&mut self, remote: &'a [RemoteWriter]) {
        self.remote = remote;
    }

    /// The writer occupying a level of an output, for the takeover and
    /// release checks of the API (docs/16 A13).
    pub fn occupant(&self, signal: SignalId, level: Level) -> Option<LevelEntry> {
        self.states[self.find(signal)?].arbiter.level(level)
    }

    /// Restores a manual level saved before a restart (docs/16 A14): before
    /// the first commit, the level of manual writer `writer` holds `value`
    /// for `remaining_ns` from `now_mono`. The host computes `remaining_ns`
    /// from the UTC deadline, or from the TTL remaining when the clock is not
    /// synced (and then raises `manual_restored`).
    pub fn restore_manual(
        &mut self,
        signal: SignalId,
        writer: WriterId,
        value: u64,
        remaining_ns: u64,
        now_mono: u64,
    ) -> Result<Level, CommitError> {
        let o = self.find(signal).ok_or(CommitError::NotAnOutput(signal))?;
        let config = &self.configs[o];
        let w = self
            .writers
            .get(writer.index())
            .filter(|w| w.kind == WriterKind::Manual && !writer.is_remote())
            .ok_or(CommitError::NotManual(writer))?;
        if !config.accepts(value) {
            return Err(CommitError::NotManual(writer));
        }
        let state = &mut self.states[o];
        if state.started {
            return Err(CommitError::AlreadyStarted);
        }
        state.arbiter.restore(
            w.level,
            LevelEntry {
                writer,
                value,
                lease_until: now_mono.saturating_add(remaining_ns.min(config.max_manual_ns)),
            },
        );
        Ok(w.level)
    }

    fn level_of(&self, writer: WriterId) -> Option<Level> {
        writer_level(self.writers, self.remote, writer)
    }

    fn is_manual(&self, writer: WriterId) -> bool {
        writer_is_manual(self.writers, writer)
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
        let level = self.level_of(writer)?;
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
        if self.arena.len() < self.state_words {
            return Err(CommitError::StateArenaTooSmall);
        }
        let group = image.group();
        let now = tick.at.mono_ns;
        let inputs = TickInputs { image, slots };
        for o in 0..self.configs.len() {
            if self.configs[o].group != group {
                continue;
            }
            let signal = self.configs[o].signal;
            let mut freed: [Option<(Level, LevelEntry)>; Level::COUNT] = [None; Level::COUNT];
            let mask = self.states[o].arbiter.expire_each(now, |level, entry| {
                freed[level.index()] = Some((level, entry));
            });
            self.states[o].expired_now = mask != 0;
            let applied = self.states[o].last;
            for (level, entry) in freed.into_iter().flatten() {
                if self.is_manual(entry.writer) {
                    sink.manual(&ManualRecord {
                        output: signal,
                        level,
                        writer: entry.writer,
                        change: ManualChange::Cleared(Cause::Expiry),
                    });
                }
                let track = &mut self.states[o].write_tracks[level.index()];
                let previous = advance(
                    track,
                    entry.writer,
                    applied,
                    Outcome::None,
                    None,
                    tick.index,
                    true,
                )
                .flatten();
                sink.decision(&DecisionRecord {
                    kind: RecordKind::Write(entry.writer),
                    output: signal,
                    tick: tick.index,
                    at_mono_ns: now,
                    applied,
                    outcome: Outcome::None,
                    level: None,
                    cause: Cause::Expiry,
                    previous,
                    durability: Durability::Immediate,
                });
            }
        }
        for (i, proposal) in proposals.iter().enumerate() {
            let intake = match self
                .configs
                .binary_search_by(|c| c.signal.cmp(&proposal.signal))
            {
                Ok(o) if self.configs[o].group == group => {
                    if proposal.writer.is_remote() {
                        match self.remote.get(proposal.writer.remote_slot()) {
                            Some(remote) => self.states[o].arbiter.propose_remote(
                                &self.configs[o],
                                remote,
                                proposal,
                                now,
                            ),
                            None => Intake::Refused(Refusal::NotOwner),
                        }
                    } else {
                        self.states[o].arbiter.propose(
                            &self.configs[o],
                            self.writers,
                            proposal,
                            now,
                        )
                    }
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
        let writers = self.writers;
        let remote = self.remote;
        for o in 0..self.configs.len() {
            let config = self.configs[o];
            if config.group != group {
                continue;
            }
            let state = &mut self.states[o];
            let words = config.state_words();
            let arena = &mut self.arena[state.arena_offset..state.arena_offset + words];
            let facts = facts(&config, &inputs, now, arena);
            let all_good = config
                .requires_good
                .iter()
                .all(|&s| inputs.sample(s).quality().is_good());
            state.bad_since = if all_good {
                None
            } else {
                state.bad_since.or(Some(now))
            };
            let quality = state
                .bad_since
                .is_none_or(|since| now.saturating_sub(since) < config.grace_ns);
            let reset = core::mem::take(&mut state.reset_pending) || (state.tripped && facts.reset);
            if reset {
                state.tripped = false;
            }
            let budget = config.max_actuations.map(|b| {
                let ring = &arena[facts.words..];
                let len = ring.first().map_or(0, |w| (*w & 0xFFFF_FFFF) as usize);
                let used = ring
                    .iter()
                    .skip(1)
                    .take(len)
                    .filter(|&&t| now.saturating_sub(t) < b.window_ns)
                    .count();
                used >= b.count as usize
            });
            let ctx = Context {
                clock: tick.at.clock,
                dt_ns: tick.dt_ns,
                now_ns: now,
                permit: facts.permit,
                quality,
                range: facts.range,
                last_actuation: state.last_actuation,
                budget_exhausted: budget.unwrap_or(false),
            };
            let active = state.arbiter.active();
            let effective = match active {
                None => Effective::Safe,
                Some((_, entry)) => Effective::Writer {
                    value: entry.value,
                    requires_clock: writer_clock(writers, entry.writer),
                },
            };
            state.pending = Some(Pending {
                verdict: evaluate_in(&config, state.last, state.tripped, effective, &ctx),
                reset,
                ring: state.arena_offset + facts.words,
            });
        }
        let atomic_groups = self.atomic;
        for atomic in atomic_groups {
            if self.apply_atomic(atomic, group)
                && let Some((signal, value)) = atomic.commit
            {
                slots
                    .get(signal.index())
                    .ok_or(CommitError::MissingSlot(signal))?
                    .write(value, Quality::GOOD, NO_TS_SRC, now);
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
            let Some(Pending {
                verdict:
                    Verdict {
                        applied,
                        outcome: envelope_outcome,
                        tripped,
                        ..
                    },
                reset,
                ring,
            }) = state.pending.take()
            else {
                continue;
            };
            let active = state.arbiter.active();
            if applied != state.last {
                state.last_actuation = Some(now);
                if let Some(b) = config.max_actuations {
                    record_actuation(&mut self.arena[ring..], b.count as usize, now);
                }
            }
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
                let manual = writer_is_manual(writers, proposal.writer);
                let track_level = writer_level(writers, remote, proposal.writer);
                let write_cause = match (intake, proposal.intent) {
                    (Intake::Relinquished(_), Intent::Takeover { by }) => Cause::Takeover(by),
                    (Intake::Relinquished(_), Intent::ForcedRelease { by }) => {
                        Cause::ForcedRelease(by)
                    }
                    _ => Cause::Proposal(proposal.writer),
                };
                let durability = if manual
                    || outcome.is_rejected()
                    || outcome == Outcome::Tripped
                    || write_cause != Cause::Proposal(proposal.writer)
                {
                    Durability::Immediate
                } else {
                    Durability::Batch
                };
                let level = track_level.map(|l| own_level(intake, l));
                let forced = write_cause != Cause::Proposal(proposal.writer);
                let due = match track_level {
                    Some(l) => advance(
                        &mut state.write_tracks[l.index()],
                        proposal.writer,
                        applied,
                        outcome,
                        level,
                        tick.index,
                        forced,
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
                        cause: write_cause,
                        previous,
                        durability,
                    });
                }
                if manual {
                    let change = match intake {
                        Intake::Placed(l) => state.arbiter.level(l).map(|e| {
                            (
                                l,
                                ManualChange::Set {
                                    value: e.value,
                                    lease_until: e.lease_until,
                                },
                            )
                        }),
                        Intake::Relinquished(Some(l)) => {
                            Some((l, ManualChange::Cleared(write_cause)))
                        }
                        _ => None,
                    };
                    if let Some((level, change)) = change {
                        sink.manual(&ManualRecord {
                            output: config.signal,
                            level,
                            writer: proposal.writer,
                            change,
                        });
                    }
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

/// The latched snapshot of a group, with the slots as fallback, as the
/// expression VM reads it (docs/02 §6.1).
struct TickInputs<'x> {
    image: &'x ProcessImage<'x>,
    slots: &'x [Slot],
}

impl TickInputs<'_> {
    fn sample(&self, signal: SignalId) -> Sample {
        if let Some(sample) = self.image.sample(signal) {
            return *sample;
        }
        match self.slots.get(signal.index()).map(|s| s.read(0)) {
            Some(Ok(sample)) => sample,
            _ => Sample::new(
                0,
                Quality::new(QualityCode::Bad, QualityReason::NotConnected),
                NO_TS_SRC,
                0,
                0,
            ),
        }
    }
}

impl Inputs for TickInputs<'_> {
    fn read(&self, signal: u32) -> Reading {
        let sample = self.sample(SignalId(signal));
        Reading {
            raw: sample.transport_bits(),
            quality: joker_expr::Quality::from_bits(sample.quality().to_bits()),
            acquired: sample.ts_acq(),
        }
    }
}

/// What the expressions of an output say in this tick (ADR 0038 §1–§2).
struct Facts {
    permit: bool,
    range: Option<(f64, f64)>,
    reset: bool,
    words: usize,
}

fn run(
    program: Option<&Program>,
    inputs: &dyn Inputs,
    now: u64,
    arena: &mut [u64],
    cursor: &mut usize,
) -> Option<Result<joker_expr::Output, EvalError>> {
    let program = program?;
    let words = program.state_words();
    let state = arena.get_mut(*cursor..*cursor + words)?;
    *cursor += words;
    Some(joker_expr::eval(program, inputs, now, state))
}

fn facts(config: &OutputConfig<'_>, inputs: &dyn Inputs, now: u64, arena: &mut [u64]) -> Facts {
    let [interlock, low, high, reset_when] = config.programs();
    let mut cursor = 0;
    let interlock = run(interlock, inputs, now, arena, &mut cursor);
    let low = run(low, inputs, now, arena, &mut cursor);
    let high = run(high, inputs, now, arena, &mut cursor);
    let reset_when = run(reset_when, inputs, now, arena, &mut cursor);
    let usable = |out: Option<Result<joker_expr::Output, EvalError>>| match out {
        Some(Ok(o)) if config.permit_uncertain => o.get_uncertain().ok().map(|v| v.as_f64()),
        Some(Ok(o)) => o.get().ok().map(|v| v.as_f64()),
        _ => None,
    };
    let mut permit = match interlock {
        None => true,
        Some(Ok(o)) => o.permits(config.permit_uncertain),
        Some(Err(_)) => false,
    };
    let range = match config.range_exprs {
        None => config.range,
        Some((lo, hi)) => {
            let bound = |b: Bound<'_>, value| match b {
                Bound::Const(c) => Some(c),
                Bound::Expr(_) => value,
            };
            match (bound(lo, usable(low)), bound(hi, usable(high))) {
                (Some(l), Some(h)) if l <= h => Some((l, h)),
                _ => {
                    permit = false;
                    None
                }
            }
        }
    };
    let reset = matches!(reset_when, Some(Ok(o)) if o.get().is_ok_and(|v| v.as_bool()));
    Facts {
        permit,
        range,
        reset,
        words: cursor,
    }
}

fn record_actuation(ring: &mut [u64], capacity: usize, now: u64) {
    if capacity == 0 || ring.len() < capacity + 1 {
        return;
    }
    let len = (ring[0] & 0xFFFF_FFFF) as usize;
    let head = (ring[0] >> 32) as usize % capacity;
    ring[1 + head] = now;
    let len = (len + 1).min(capacity);
    let head = (head + 1) % capacity;
    ring[0] = ((head as u64) << 32) | len as u64;
}

fn writer_level(
    writers: &[WriterConfig],
    remote: &[RemoteWriter],
    writer: WriterId,
) -> Option<Level> {
    if writer.is_remote() {
        remote.get(writer.remote_slot()).map(|r| r.level)
    } else {
        writers.get(writer.index()).map(|w| w.level)
    }
}

fn writer_clock(writers: &[WriterConfig], writer: WriterId) -> ClockRequirement {
    if writer.is_remote() {
        return ClockRequirement::Any;
    }
    writers
        .get(writer.index())
        .map_or(ClockRequirement::Any, |w| w.requires_clock)
}

fn writer_is_manual(writers: &[WriterConfig], writer: WriterId) -> bool {
    !writer.is_remote()
        && writers
            .get(writer.index())
            .is_some_and(|w| w.kind == WriterKind::Manual)
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
