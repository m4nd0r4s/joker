//! A resolved core case as borrowed, fixed data (ref/03 §2.6.1, §2.6.2,
//! §2.6.4): every instant is a tick index, every path an index into the
//! fixture, every literal a value in the unit of its signal.
//!
//! Every type is `Copy` and can be written as a `static`, so the firmware of
//! an MCU embeds the cases as data, and the host runner builds the same view
//! over the suites it reads.

use joker_core::ValueType;
use joker_core::alarm::Severity;
use joker_core::output::{ClockRequirement, OnInterlock, OnRange, OnRate, WriterKind};
use joker_core::time::ClockQuality;

/// An input of the fixture (`[[signals]]`).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SignalSpec<'a> {
    /// Signal path.
    pub path: &'a str,
    /// Signal type.
    pub ty: ValueType,
    /// Declared unit (`"1"` when absent).
    pub unit: &'a str,
    /// `max_age` in nanoseconds.
    pub max_age_ns: Option<u64>,
}

/// The `safe` of an output.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SafeDef {
    /// A value, as the bits of the signal type.
    Value(u64),
    /// `"relinquish"`.
    Relinquish,
}

/// One limit of a `range` (16 E6).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum LimitSpec<'a> {
    /// A constant, in the unit of the output.
    Const(f64),
    /// An expression evaluated in every tick, in the unit of the output.
    Expr(&'a str),
}

/// An output of the fixture (`[[outputs]]`) with the envelope keys the
/// core implements (ref/01 §5.10).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OutputSpec<'a> {
    /// Signal path.
    pub path: &'a str,
    /// Signal type.
    pub ty: ValueType,
    /// Declared unit (`"1"` when absent).
    pub unit: &'a str,
    /// `safe`.
    pub safe: SafeDef,
    /// `safe_ramp`, in unit per second.
    pub safe_ramp: Option<f64>,
    /// `range`: two constants, or limits of which at least one is an
    /// expression (16 E6).
    pub range: Option<(LimitSpec<'a>, LimitSpec<'a>)>,
    /// `on_range`.
    pub on_range: OnRange,
    /// `rate`, in unit per second.
    pub rate: Option<f64>,
    /// `max_lease` in nanoseconds.
    pub max_lease_ns: Option<u64>,
    /// `max_manual` in nanoseconds.
    pub max_manual_ns: Option<u64>,
    /// `interlock`, the permissive of step 2.
    pub interlock: Option<&'a str>,
    /// `permit_uncertain`.
    pub permit_uncertain: bool,
    /// `on_interlock` (16 E3).
    pub on_interlock: OnInterlock,
    /// `grace` of `requires_good`, in nanoseconds (16 E4).
    pub grace_ns: u64,
    /// `reset_when` (16 E7).
    pub reset_when: Option<&'a str>,
    /// `deadband`, a difference in the unit of the output.
    pub deadband: Option<f64>,
    /// `min_interval` in nanoseconds.
    pub min_interval_ns: u64,
    /// `max_actuations`: the count and the window in nanoseconds.
    pub max_actuations: Option<(u32, u64)>,
}

/// A derived signal (`[[calc]]`, ref/01 §5.14), with the type and unit the
/// loader inferred from its expression.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CalcSpec<'a> {
    /// Signal path (`calc.<name>`).
    pub path: &'a str,
    /// The expression.
    pub expr: &'a str,
    /// The inferred type.
    pub ty: ValueType,
    /// The inferred unit (`"1"` when dimensionless).
    pub unit: &'a str,
}

/// An alarm (`[[alarms]]`, ref/01 §5.15).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AlarmSpec<'a> {
    /// The path of its `state` signal (`alarm.<id>.state`).
    pub state_path: &'a str,
    /// The path of its `active` signal (`alarm.<id>.active`).
    pub active_path: &'a str,
    /// `when`.
    pub when: &'a str,
    /// `suppress_when`.
    pub suppress_when: Option<&'a str>,
    /// `severity`.
    pub severity: Severity,
    /// `delay_on` in nanoseconds.
    pub delay_on_ns: u64,
    /// `delay_off` in nanoseconds.
    pub delay_off_ns: u64,
    /// `max_shelve` in nanoseconds.
    pub max_shelve_ns: Option<u64>,
}

/// The commit register of an atomic group (ref/01 §5.16).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommitSpec<'a> {
    /// Signal path.
    pub path: &'a str,
    /// Its type.
    pub ty: ValueType,
    /// The bits written after the writes of the group.
    pub value: u64,
}

/// An atomic group (`[[atomic]]`, ref/01 §5.16); its members are in
/// [`Fixture::members`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AtomicSpec<'a> {
    /// `on_rate`.
    pub on_rate: OnRate,
    /// `commit`.
    pub commit: Option<CommitSpec<'a>>,
}

/// A pair of indices that links two lists of the fixture: (owner, item).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Link<'a> {
    /// Index of the owner: an output, an alarm or an atomic group.
    pub owner: usize,
    /// The item: a path (`requires_good`) or an output index.
    pub item: Item<'a>,
}

/// The item of a [`Link`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Item<'a> {
    /// A path or a selector.
    Path(&'a str),
    /// An index in [`Fixture::outputs`].
    Output(usize),
}

/// A writer of the fixture (`[[writers]]`), in the order of the file: its
/// index is its `WriterId`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WriterSpec {
    /// Arbiter level, 1 to 16.
    pub level: u8,
    /// Kind of writer.
    pub kind: WriterKind,
    /// `lease` (or `ttl` for a manual writer) in nanoseconds.
    pub lease_ns: Option<u64>,
    /// `requires_clock`.
    pub requires_clock: ClockRequirement,
}

impl WriterSpec {
    /// Whether the proposals of this writer enter at step 2 (async inbox)
    /// rather than at step 4 (cycle tasks) of the tick (16 §8, ADR 0042).
    pub fn is_async(&self) -> bool {
        matches!(
            self.kind,
            WriterKind::Async | WriterKind::Manual | WriterKind::Bridge
        )
    }
}

/// The fixture of a suite: what the core keeps for a whole generation.
#[derive(Copy, Clone, Debug)]
pub struct Fixture<'a> {
    /// Period of the group of the cases, in nanoseconds.
    pub period_ns: u64,
    /// Inputs.
    pub signals: &'a [SignalSpec<'a>],
    /// Outputs.
    pub outputs: &'a [OutputSpec<'a>],
    /// Writers.
    pub writers: &'a [WriterSpec],
    /// Derived signals, in evaluation order (dependencies first).
    pub calcs: &'a [CalcSpec<'a>],
    /// Alarms.
    pub alarms: &'a [AlarmSpec<'a>],
    /// Atomic groups.
    pub atomic: &'a [AtomicSpec<'a>],
    /// `requires_good` of each output: (output, path or selector).
    pub requires_good: &'a [Link<'a>],
    /// `actions.trip` of each alarm: (alarm, output).
    pub trips: &'a [Link<'a>],
    /// Members of each atomic group, in `outputs` order: (group, output).
    pub members: &'a [Link<'a>],
    /// `order` of each atomic group that declares one: (group, output).
    pub order: &'a [Link<'a>],
}

/// A quality literal: the code, and the reason when one is given.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct QualityLit {
    /// Index in good, uncertain, bad.
    pub code: u8,
    /// The numeric reason (ref/00 §4.3).
    pub reason: Option<u8>,
}

/// An outcome predicate: the result code and, for `rejected` and
/// `overridden`, the detail byte when one is given (ref/00 §4.4).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutcomeLit {
    /// The result code ([`joker_core::Outcome::code`]).
    pub code: u8,
    /// The detail ([`joker_core::Outcome::detail`]): the reject reason or
    /// the level.
    pub detail: Option<u8>,
}

/// A value written in a case, already encoded for its signal.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Expected {
    /// A number in the unit of the signal.
    Number(f64),
    /// An exact integer (`i64` and `u64` signals).
    Integer(i128),
    /// A boolean.
    Bool(bool),
    /// `"relinquish"`.
    Relinquish,
}

/// What an action does, at the latch of its tick (ref/03 §2.6.2).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ActionKind {
    /// A new sample of an input.
    Input {
        /// Index in [`Fixture::signals`].
        signal: usize,
        /// Bits of the value (0 when absent).
        bits: u64,
        /// Code and reason of the sample quality.
        quality: QualityLit,
    },
    /// A proposal of a writer.
    Propose {
        /// Index in [`Fixture::outputs`].
        output: usize,
        /// Index in [`Fixture::writers`].
        writer: usize,
        /// Bits of the proposed value.
        bits: u64,
    },
    /// The end of a manual write.
    Release {
        /// Index in [`Fixture::outputs`].
        output: usize,
        /// Index in [`Fixture::writers`].
        writer: usize,
    },
    /// The end of a trip.
    Reset {
        /// Index in [`Fixture::outputs`].
        output: usize,
    },
    /// A new clock quality from this tick on.
    Clock(ClockQuality),
    /// A swap to an identical generation at this tick boundary.
    Generation,
}

/// One action of a case.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Action {
    /// Tick index (`at / period`).
    pub tick: u64,
    /// What it does.
    pub kind: ActionKind,
}

/// The ticks an expectation covers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum When {
    /// One tick.
    At(u64),
    /// Every tick of the closed interval.
    During(u64, u64),
}

impl When {
    /// The first and the last tick covered.
    pub fn bounds(self) -> (u64, u64) {
        match self {
            Self::At(t) => (t, t),
            Self::During(a, b) => (a, b),
        }
    }
}

/// What an expectation looks at.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// An output, by index in [`Fixture::outputs`].
    Output(usize),
    /// An input, by index in [`Fixture::signals`].
    Input(usize),
    /// An output read as a signal (its slot).
    OutputSignal(usize),
    /// A derived signal, by index in [`Fixture::calcs`].
    Calc(usize),
    /// An alarm, by index in [`Fixture::alarms`].
    Alarm(usize),
    /// The commit register of an atomic group, by index in
    /// [`Fixture::atomic`].
    Commit(usize),
}

/// The predicates of an expectation (ref/03 §2.5.4, §2.6.4).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Predicates {
    /// `equals`.
    pub equals: Option<Expected>,
    /// `min`, in the unit of the subject.
    pub min: Option<f64>,
    /// `max`, in the unit of the subject.
    pub max: Option<f64>,
    /// `tolerance`, in the unit of the subject.
    pub tolerance: Option<f64>,
    /// `quality`.
    pub quality: Option<QualityLit>,
    /// `outcome`.
    pub outcome: Option<OutcomeLit>,
    /// `level`: `Some(None)` is `"none"`.
    pub level: Option<Option<u8>>,
    /// `state` of an alarm, as its code (ref/00 §6).
    pub state: Option<u8>,
    /// `active` of an alarm.
    pub active: Option<bool>,
}

/// One node of the expectations of a case, in prefix order: a combinator is
/// followed by its `n` complete sub-expectations.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Node {
    /// A simple expectation.
    Check {
        /// When it holds.
        when: When,
        /// The subject.
        subject: Subject,
        /// What must hold.
        predicates: Predicates,
    },
    /// At least one of the next `n` expectations holds.
    AnyOf(u32),
    /// All the next `n` expectations hold.
    AllOf(u32),
}

/// What a case does and expects, over the fixture of its suite.
#[derive(Copy, Clone, Debug)]
pub struct Script<'a> {
    /// Number of ticks the case covers.
    pub ticks: u64,
    /// The timeline, in the order written.
    pub actions: &'a [Action],
    /// The top-level expectations, each a complete prefix-order tree.
    pub expects: &'a [Node],
}
