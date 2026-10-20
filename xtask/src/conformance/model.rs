//! The case model of a core suite (ref/03 §2.6.1, §2.6.2, §2.6.4), already
//! validated and resolved: every instant is a tick index, every path an
//! index into the fixture, every literal a value in the signal's unit.
//!
//! Only `core`, `alloc` and `joker-core` types are used, so the same model
//! can drive the conformance firmware of an MCU (docs/13 §3, ref/08 §3.2).

use alloc::string::String;
use alloc::vec::Vec;

use joker_core::ValueType;
use joker_core::output::{ClockRequirement, Level, OnRange, WriterKind};
use joker_core::time::ClockQuality;

use super::literal::{OutcomeLit, QualityLit};

/// A feature the M0 core does not implement yet; cases that need one are
/// reported as `skipped (not in M0)`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Feature {
    /// Short name of the feature.
    pub name: &'static str,
    /// The paragraph of the specification that defines it.
    pub spec: &'static str,
}

/// An input of the fixture (`[[signals]]`).
#[derive(Clone, Debug)]
pub struct SignalDef {
    /// Signal path.
    pub path: String,
    /// Signal type.
    pub ty: ValueType,
    /// Declared unit (`"1"` when absent).
    pub unit: String,
    /// `max_age` in nanoseconds.
    pub max_age_ns: Option<u64>,
    /// Features of the declaration that the M0 core lacks.
    pub missing: Vec<Feature>,
}

/// The `safe` of an output.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SafeDef {
    /// A value, as the bits of the signal type.
    Value(u64),
    /// `"relinquish"`.
    Relinquish,
}

/// An output of the fixture (`[[outputs]]`) with the envelope keys the M0
/// core implements.
#[derive(Clone, Debug)]
pub struct OutputDef {
    /// Signal path.
    pub path: String,
    /// Signal type.
    pub ty: ValueType,
    /// Declared unit (`"1"` when absent).
    pub unit: String,
    /// `safe`.
    pub safe: SafeDef,
    /// `safe_ramp`, in unit per second.
    pub safe_ramp: Option<f64>,
    /// Constant `range`, in the unit of the output.
    pub range: Option<(f64, f64)>,
    /// `on_range`.
    pub on_range: OnRange,
    /// `rate`, in unit per second.
    pub rate: Option<f64>,
    /// `max_lease` in nanoseconds.
    pub max_lease_ns: Option<u64>,
    /// `max_manual` in nanoseconds.
    pub max_manual_ns: Option<u64>,
    /// Features of the envelope that the M0 core lacks.
    pub missing: Vec<Feature>,
}

/// A writer of the fixture (`[[writers]]`).
#[derive(Clone, Debug)]
pub struct WriterDef {
    /// Writer id, as written in the cases.
    pub id: String,
    /// Arbiter level.
    pub level: Level,
    /// Kind of writer.
    pub kind: WriterKind,
    /// `lease` (or `ttl` for a manual writer) in nanoseconds.
    pub lease_ns: Option<u64>,
    /// `requires_clock`.
    pub requires_clock: ClockRequirement,
}

impl WriterDef {
    /// Whether the proposals of this writer enter at step 2 (async inbox)
    /// rather than at step 4 (cycle tasks) of the tick (16 §8).
    pub fn is_async(&self) -> bool {
        matches!(
            self.kind,
            WriterKind::Async | WriterKind::Manual | WriterKind::Bridge
        )
    }
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
#[derive(Clone, Debug)]
pub enum ActionKind {
    /// A new sample of an input.
    Input {
        /// Index in [`Suite::signals`].
        signal: usize,
        /// Bits of the value (0 when absent).
        bits: u64,
        /// Code and reason of the sample quality.
        quality: QualityLit,
    },
    /// A proposal of a writer.
    Propose {
        /// Index in [`Suite::outputs`].
        output: usize,
        /// Index in [`Suite::writers`].
        writer: usize,
        /// Bits of the proposed value.
        bits: u64,
    },
    /// The end of a manual write.
    Release {
        /// Index in [`Suite::outputs`].
        output: usize,
        /// Index in [`Suite::writers`].
        writer: usize,
    },
    /// The end of a trip.
    Reset {
        /// Index in [`Suite::outputs`].
        output: usize,
    },
    /// A new clock quality from this tick on.
    Clock(ClockQuality),
    /// A swap to an identical generation at this tick boundary.
    Generation,
}

/// One action of a case.
#[derive(Clone, Debug)]
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

/// What an expectation looks at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// An output, by index in [`Suite::outputs`].
    Output(usize),
    /// An input, by index in [`Suite::signals`].
    Input(usize),
    /// An output read as a signal (its slot).
    OutputSignal(usize),
}

/// The predicates of an expectation (ref/03 §2.5.4, §2.6.4).
#[derive(Clone, Debug, Default)]
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
}

/// An expectation, or a combinator over complete expectations.
#[derive(Clone, Debug)]
pub enum Expect {
    /// A simple expectation.
    Check {
        /// When it holds.
        when: When,
        /// The subject.
        subject: Subject,
        /// What must hold.
        predicates: Predicates,
    },
    /// At least one of the expectations holds.
    AnyOf(Vec<Expect>),
    /// All the expectations hold.
    AllOf(Vec<Expect>),
}

/// A top-level expectation and the text it was written with.
#[derive(Clone, Debug)]
pub struct ExpectEntry {
    /// The resolved expectation.
    pub expect: Expect,
    /// The source, as an inline table, for reports.
    pub source: String,
}

/// A case of a suite.
#[derive(Clone, Debug)]
pub struct Case {
    /// Case id (without the suite prefix).
    pub id: String,
    /// Free text.
    pub name: Option<String>,
    /// Paragraphs the case derives from.
    pub spec: Vec<String>,
    /// Number of ticks the case covers.
    pub ticks: u64,
    /// The timeline, in the order written.
    pub actions: Vec<Action>,
    /// The expectations.
    pub expects: Vec<ExpectEntry>,
    /// Features the case needs that the M0 core lacks.
    pub missing: Vec<Feature>,
    /// Problems found while resolving the case: it does not run.
    pub errors: Vec<Diagnostic>,
}

/// A problem found in a suite file, with its code (ref/03 §2.6, 16 §9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The J-code.
    pub code: &'static str,
    /// Where in the file (`case[2].expect[0]`).
    pub location: String,
    /// What is wrong.
    pub message: String,
}

/// A core suite (`kind = "core"`).
#[derive(Clone, Debug)]
pub struct Suite {
    /// Suite id.
    pub id: String,
    /// Paragraphs of the suite.
    pub spec: Vec<String>,
    /// Period of the group of the cases, in nanoseconds.
    pub period_ns: u64,
    /// Inputs.
    pub signals: Vec<SignalDef>,
    /// Outputs.
    pub outputs: Vec<OutputDef>,
    /// Writers.
    pub writers: Vec<WriterDef>,
    /// The cases.
    pub cases: Vec<Case>,
}
