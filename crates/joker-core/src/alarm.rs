//! Alarms (M9): the lifecycle of docs/02 §11 with the delays, `Unknown`,
//! suppression and shelving of ADR 0040, evaluated in step 3 of the tick.

use joker_expr::{EvalError, Program};

use crate::commit::{CommitError, CommitSink, Outputs, TickInputs};
use crate::image::ProcessImage;
use crate::quality::{Quality, QualityCode, QualityReason};
use crate::registry::{GroupId, SignalId};
use crate::slot::Slot;
use crate::value::NO_TS_SRC;

/// Default `max_shelve`: 24 h (ref/01 §5.15).
pub const DEFAULT_MAX_SHELVE_NS: u64 = 24 * 3600 * 1_000_000_000;

/// The published alarm state, with the codes of ref/00 §6.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AlarmState {
    /// No alarm.
    Normal = 0,
    /// Active, not acknowledged.
    ActiveUnacked = 1,
    /// Active, acknowledged.
    ActiveAcked = 2,
    /// Returned to normal, not acknowledged.
    RtnUnacked = 3,
    /// The condition is `Bad` (docs/16 §3.2).
    Unknown = 4,
}

impl AlarmState {
    /// True in both active states.
    pub const fn is_active(self) -> bool {
        matches!(self, Self::ActiveUnacked | Self::ActiveAcked)
    }
}

/// Alarm severity, with the codes of ref/00 §6.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Severity {
    /// `low`.
    Low = 0,
    /// `medium`.
    Medium = 1,
    /// `high`.
    High = 2,
    /// `critical`.
    Critical = 3,
}

/// One alarm (ref/01 §5.15).
#[derive(Copy, Clone, Debug)]
pub struct AlarmConfig<'a> {
    /// Alarm number in this generation, used in events.
    pub id: u32,
    /// The group that evaluates it.
    pub group: GroupId,
    /// The condition.
    pub when: &'a Program,
    /// Suppression by design.
    pub suppress_when: Option<&'a Program>,
    /// Time the condition must hold before activation, ns.
    pub delay_on_ns: u64,
    /// Time the condition must be false before returning, ns.
    pub delay_off_ns: u64,
    /// Severity.
    pub severity: Severity,
    /// Upper bound of a shelve TTL, ns.
    pub max_shelve_ns: u64,
    /// Outputs tripped on activation.
    pub trip: &'a [SignalId],
    /// The `alarm.<a>.state` signal, if registered.
    pub state_signal: Option<SignalId>,
    /// The `alarm.<a>.active` signal, if registered.
    pub active_signal: Option<SignalId>,
}

impl<'a> AlarmConfig<'a> {
    /// An alarm with no delays, suppression, actions or signals.
    pub const fn new(id: u32, group: GroupId, when: &'a Program, severity: Severity) -> Self {
        Self {
            id,
            group,
            when,
            suppress_when: None,
            delay_on_ns: 0,
            delay_off_ns: 0,
            severity,
            max_shelve_ns: DEFAULT_MAX_SHELVE_NS,
            trip: &[],
            state_signal: None,
            active_signal: None,
        }
    }

    fn state_words(&self) -> usize {
        self.when.state_words() + self.suppress_when.map_or(0, |p| p.state_words())
    }
}

/// Why an alarm changed state.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AlarmCause {
    /// The condition (or its quality) changed.
    Condition,
    /// An operator acknowledged it.
    Ack,
    /// An operator shelved it.
    Shelve,
    /// An operator unshelved it.
    Unshelve,
    /// The shelve TTL ran out.
    ShelveExpired,
}

/// One transition of an alarm: the event series of docs/02 §11.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AlarmEvent {
    /// Alarm number.
    pub alarm: u32,
    /// Monotonic instant.
    pub at_mono_ns: u64,
    /// Published state before.
    pub from: AlarmState,
    /// Published state after.
    pub to: AlarmState,
    /// Why.
    pub cause: AlarmCause,
    /// Severity.
    pub severity: Severity,
    /// Shelved at the time of the event.
    pub shelved: bool,
    /// The condition was `Uncertain`.
    pub uncertain: bool,
}

/// Why an operator command was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AlarmError {
    /// No such alarm.
    Unknown,
    /// The TTL is zero or above `max_shelve`.
    BadTtl,
    /// Not shelved.
    NotShelved,
}

/// What of an alarm survives a restart of the runtime (ADR 0082): the
/// underlying state, with its pending acknowledgement, and the end of a
/// shelve. The `Unknown` overlay and the delay timers do not survive; the
/// next evaluation derives them again.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AlarmSnapshot {
    /// The underlying state; never `Unknown`.
    pub underlying: AlarmState,
    /// Monotonic instant the shelve ends, if shelved.
    pub shelved_until: Option<u64>,
}

/// The runtime state of one alarm.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AlarmRuntime {
    underlying: AlarmState,
    unknown: Option<QualityReason>,
    since: Option<(bool, u64)>,
    shelved_until: Option<u64>,
    published: Option<(u8, u64, u32)>,
    uncertain: bool,
    arena_offset: usize,
}

impl AlarmRuntime {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        underlying: AlarmState::Normal,
        unknown: None,
        since: None,
        shelved_until: None,
        published: None,
        uncertain: false,
        arena_offset: 0,
    };

    /// The published state.
    pub const fn state(&self) -> AlarmState {
        match self.unknown {
            Some(_) => AlarmState::Unknown,
            None => self.underlying,
        }
    }

    /// True while shelved.
    pub const fn shelved(&self) -> bool {
        self.shelved_until.is_some()
    }
}

/// The alarms of a generation.
#[derive(Debug)]
pub struct Alarms<'a> {
    configs: &'a [AlarmConfig<'a>],
    states: &'a mut [AlarmRuntime],
    arena: &'a mut [u64],
    state_words: usize,
}

impl<'a> Alarms<'a> {
    /// Alarms over caller-provided state, one per configuration, all in
    /// `Normal`.
    pub fn new(
        configs: &'a [AlarmConfig<'a>],
        states: &'a mut [AlarmRuntime],
    ) -> Result<Self, CommitError> {
        if states.len() != configs.len() {
            return Err(CommitError::StorageMismatch);
        }
        let mut words = 0;
        for (config, state) in configs.iter().zip(states.iter_mut()) {
            *state = AlarmRuntime {
                arena_offset: words,
                ..AlarmRuntime::EMPTY
            };
            words += config.state_words();
        }
        Ok(Self {
            configs,
            states,
            arena: &mut [],
            state_words: words,
        })
    }

    /// Words of state arena the alarm expressions need.
    pub fn state_words(&self) -> usize {
        self.state_words
    }

    /// Hands in the state arena and zeroes it.
    pub fn set_state_arena(&mut self, arena: &'a mut [u64]) -> Result<(), CommitError> {
        if arena.len() < self.state_words {
            return Err(CommitError::StateArenaTooSmall);
        }
        arena.fill(0);
        self.arena = arena;
        Ok(())
    }

    fn find(&self, alarm: u32) -> Result<usize, AlarmError> {
        self.configs
            .iter()
            .position(|c| c.id == alarm)
            .ok_or(AlarmError::Unknown)
    }

    /// The published state and shelve flag of an alarm.
    pub fn state(&self, alarm: u32) -> Option<(AlarmState, bool)> {
        let s = &self.states[self.find(alarm).ok()?];
        Some((s.state(), s.shelved()))
    }

    /// The state of an alarm that survives a restart.
    pub fn snapshot(&self, alarm: u32) -> Option<AlarmSnapshot> {
        let s = &self.states[self.find(alarm).ok()?];
        Some(AlarmSnapshot {
            underlying: s.underlying,
            shelved_until: s.shelved_until,
        })
    }

    /// Restores an alarm before its first evaluation (ADR 0082), without an
    /// event: the next tick publishes its signals, an active one trips its
    /// outputs as usual, and a restored state leaves it only through the
    /// lifecycle of docs/02 §11. An `Unknown` underlying state restores
    /// `Normal`.
    pub fn restore(&mut self, alarm: u32, snapshot: AlarmSnapshot) -> Result<(), AlarmError> {
        let i = self.find(alarm)?;
        let state = &mut self.states[i];
        state.underlying = match snapshot.underlying {
            AlarmState::Unknown => AlarmState::Normal,
            s => s,
        };
        state.shelved_until = snapshot.shelved_until;
        state.unknown = None;
        state.since = None;
        state.published = None;
        Ok(())
    }

    /// Step 3 for the alarms of the image's group: evaluates conditions and
    /// suppressions, moves the state machines, trips outputs on activation,
    /// publishes the alarm signals and reports events to `sink`.
    pub fn evaluate(
        &mut self,
        image: &ProcessImage<'_>,
        slots: &[Slot],
        outputs: &mut Outputs<'_>,
        sink: &mut impl CommitSink,
    ) -> Result<(), CommitError> {
        let tick = image.tick().ok_or(CommitError::NoTick)?;
        if self.arena.len() < self.state_words {
            return Err(CommitError::StateArenaTooSmall);
        }
        let now = tick.at.mono_ns;
        let inputs = TickInputs::new(image, slots);
        for (config, state) in self.configs.iter().zip(self.states.iter_mut()) {
            if config.group != image.group() {
                continue;
            }
            let base = state.arena_offset;
            let when_words = config.when.state_words();
            let condition = joker_expr::eval(
                config.when,
                &inputs,
                now,
                &mut self.arena[base..base + when_words],
            );
            let suppress = config.suppress_when.map(|p| {
                let start = base + when_words;
                joker_expr::eval(
                    p,
                    &inputs,
                    now,
                    &mut self.arena[start..start + p.state_words()],
                )
            });
            let suppressed = matches!(
                suppress,
                Some(Ok(o)) if o.get_uncertain().is_ok_and(|v| v.as_bool())
            );
            if let Some(until) = state.shelved_until
                && now >= until
            {
                state.shelved_until = None;
                let s = state.state();
                emit(sink, config, state, now, s, s, AlarmCause::ShelveExpired);
            }
            step(config, state, condition, suppressed, now, outputs, sink);
            publish(config, state, slots, now)?;
        }
        Ok(())
    }

    /// Acknowledges an alarm (docs/02 §11): `Active·Unacked` → `Active·Acked`
    /// and `RTN·Unacked` → `Normal`, also while `Unknown` (ADR 0040 §3).
    /// Returns whether anything changed.
    pub fn ack(
        &mut self,
        alarm: u32,
        now: u64,
        sink: &mut impl CommitSink,
    ) -> Result<bool, AlarmError> {
        let i = self.find(alarm)?;
        let (config, state) = (&self.configs[i], &mut self.states[i]);
        let from = state.state();
        let next = match state.underlying {
            AlarmState::ActiveUnacked => AlarmState::ActiveAcked,
            AlarmState::RtnUnacked => AlarmState::Normal,
            _ => return Ok(false),
        };
        state.underlying = next;
        emit(
            sink,
            config,
            state,
            now,
            from,
            state.state(),
            AlarmCause::Ack,
        );
        Ok(true)
    }

    /// Shelves an alarm for `ttl_ns`, at most `max_shelve` (ADR 0040 §6).
    pub fn shelve(
        &mut self,
        alarm: u32,
        ttl_ns: u64,
        now: u64,
        sink: &mut impl CommitSink,
    ) -> Result<(), AlarmError> {
        let i = self.find(alarm)?;
        let (config, state) = (&self.configs[i], &mut self.states[i]);
        if ttl_ns == 0 || ttl_ns > config.max_shelve_ns {
            return Err(AlarmError::BadTtl);
        }
        state.shelved_until = Some(now.saturating_add(ttl_ns));
        let s = state.state();
        emit(sink, config, state, now, s, s, AlarmCause::Shelve);
        Ok(())
    }

    /// Ends a shelve.
    pub fn unshelve(
        &mut self,
        alarm: u32,
        now: u64,
        sink: &mut impl CommitSink,
    ) -> Result<(), AlarmError> {
        let i = self.find(alarm)?;
        let (config, state) = (&self.configs[i], &mut self.states[i]);
        if state.shelved_until.take().is_none() {
            return Err(AlarmError::NotShelved);
        }
        let s = state.state();
        emit(sink, config, state, now, s, s, AlarmCause::Unshelve);
        Ok(())
    }
}

fn emit(
    sink: &mut impl CommitSink,
    config: &AlarmConfig<'_>,
    state: &AlarmRuntime,
    now: u64,
    from: AlarmState,
    to: AlarmState,
    cause: AlarmCause,
) {
    sink.alarm(&AlarmEvent {
        alarm: config.id,
        at_mono_ns: now,
        from,
        to,
        cause,
        severity: config.severity,
        shelved: state.shelved(),
        uncertain: state.uncertain,
    });
}

fn step(
    config: &AlarmConfig<'_>,
    state: &mut AlarmRuntime,
    condition: Result<joker_expr::Output, EvalError>,
    suppressed: bool,
    now: u64,
    outputs: &mut Outputs<'_>,
    sink: &mut impl CommitSink,
) {
    let from = state.state();
    let value = match condition {
        Ok(o) => o.get_uncertain().ok().map(|v| {
            (
                v.as_bool(),
                o.quality().to_bits() & 0xFF == QualityCode::Uncertain as u32,
            )
        }),
        Err(_) => None,
    };
    let Some((value, uncertain)) = value else {
        let reason = match condition {
            Ok(o) => QualityReason::from_u8(((o.quality().to_bits() >> 8) & 0xFF) as u8),
            Err(_) => QualityReason::ConfigError,
        };
        state.since = None;
        if state.unknown.is_none() {
            state.unknown = Some(reason);
            emit(
                sink,
                config,
                state,
                now,
                from,
                AlarmState::Unknown,
                AlarmCause::Condition,
            );
        }
        state.unknown = Some(reason);
        return;
    };
    state.uncertain = uncertain;
    if state.unknown.take().is_some() {
        state.since = None;
        emit(
            sink,
            config,
            state,
            now,
            from,
            state.underlying,
            AlarmCause::Condition,
        );
    }
    if state.since.is_none_or(|(v, _)| v != value) {
        state.since = Some((value, now));
    }
    let held = state.since.map_or(0, |(_, t)| now.saturating_sub(t));
    let next = match (state.underlying, value) {
        (AlarmState::Normal | AlarmState::RtnUnacked, true)
            if held >= config.delay_on_ns && !suppressed =>
        {
            AlarmState::ActiveUnacked
        }
        (AlarmState::ActiveUnacked, false) if held >= config.delay_off_ns => AlarmState::RtnUnacked,
        (AlarmState::ActiveAcked, false) if held >= config.delay_off_ns => AlarmState::Normal,
        (current, _) => current,
    };
    if next != state.underlying {
        let before = state.underlying;
        state.underlying = next;
        emit(
            sink,
            config,
            state,
            now,
            before,
            next,
            AlarmCause::Condition,
        );
    }
    if state.underlying.is_active() {
        for &output in config.trip {
            outputs.trip(output);
        }
    }
}

fn publish(
    config: &AlarmConfig<'_>,
    state: &mut AlarmRuntime,
    slots: &[Slot],
    now: u64,
) -> Result<(), CommitError> {
    let published = state.state();
    let active_quality = match state.unknown {
        Some(reason) => Quality::new(QualityCode::Bad, reason),
        None => Quality::GOOD,
    };
    let active = u64::from(state.underlying.is_active());
    let current = (published as u8, active, active_quality.to_bits());
    if state.published == Some(current) {
        return Ok(());
    }
    state.published = Some(current);
    if let Some(signal) = config.state_signal {
        slots
            .get(signal.index())
            .ok_or(CommitError::MissingSlot(signal))?
            .write(u64::from(published as u8), Quality::GOOD, NO_TS_SRC, now);
    }
    if let Some(signal) = config.active_signal {
        slots
            .get(signal.index())
            .ok_or(CommitError::MissingSlot(signal))?
            .write(active, active_quality, NO_TS_SRC, now);
    }
    Ok(())
}
