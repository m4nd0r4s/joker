//! The case model of a core suite (ref/03 §2.6.1, §2.6.2, §2.6.4), already
//! validated and resolved: every instant is a tick index, every path an
//! index into the fixture, every literal a value in the signal's unit.
//!
//! The plain data of a case (actions, predicates, windows) is the model of
//! `joker-case-runner`, which drives the core on every target; this module
//! adds the owned fixture, the expectation tree and the diagnostics that
//! the host runner reports, and the borrowed views the executor takes.

use alloc::string::String;
use alloc::vec::Vec;

use joker_case_runner::Fixture;
use joker_core::ValueType;
use joker_core::output::{ClockRequirement, Level, OnRange, WriterKind};

pub use joker_case_runner::model::{
    Action, ActionKind, Expected, Node, OutputSpec, Predicates, SafeDef, SignalSpec, Subject, When,
    WriterSpec,
};

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

/// The borrowed fixture of a suite, as the executor of `joker-case-runner`
/// takes it.
#[derive(Debug)]
pub struct FixtureView<'s> {
    period_ns: u64,
    signals: Vec<SignalSpec<'s>>,
    outputs: Vec<OutputSpec<'s>>,
    writers: Vec<WriterSpec>,
}

impl FixtureView<'_> {
    /// The fixture.
    pub fn fixture(&self) -> Fixture<'_> {
        Fixture {
            period_ns: self.period_ns,
            signals: &self.signals,
            outputs: &self.outputs,
            writers: &self.writers,
        }
    }
}

impl Suite {
    /// The borrowed fixture of the suite.
    pub fn view(&self) -> FixtureView<'_> {
        FixtureView {
            period_ns: self.period_ns,
            signals: self
                .signals
                .iter()
                .map(|s| SignalSpec {
                    path: &s.path,
                    ty: s.ty,
                    unit: &s.unit,
                    max_age_ns: s.max_age_ns,
                })
                .collect(),
            outputs: self
                .outputs
                .iter()
                .map(|o| OutputSpec {
                    path: &o.path,
                    ty: o.ty,
                    unit: &o.unit,
                    safe: o.safe,
                    safe_ramp: o.safe_ramp,
                    range: o.range,
                    on_range: o.on_range,
                    rate: o.rate,
                    max_lease_ns: o.max_lease_ns,
                    max_manual_ns: o.max_manual_ns,
                })
                .collect(),
            writers: self
                .writers
                .iter()
                .map(|w| WriterSpec {
                    level: w.level.get(),
                    kind: w.kind,
                    lease_ns: w.lease_ns,
                    requires_clock: w.requires_clock,
                })
                .collect(),
        }
    }
}

impl Expect {
    /// Appends the expectation in prefix order ([`Node`]).
    pub fn flatten_into(&self, out: &mut Vec<Node>) {
        match self {
            Self::Check {
                when,
                subject,
                predicates,
            } => out.push(Node::Check {
                when: *when,
                subject: *subject,
                predicates: *predicates,
            }),
            Self::AnyOf(list) | Self::AllOf(list) => {
                let n = u32::try_from(list.len()).unwrap_or(u32::MAX);
                out.push(match self {
                    Self::AnyOf(_) => Node::AnyOf(n),
                    _ => Node::AllOf(n),
                });
                for e in list {
                    e.flatten_into(out);
                }
            }
        }
    }
}

impl Case {
    /// The expectations of the case in prefix order.
    pub fn nodes(&self) -> Vec<Node> {
        let mut out = Vec::new();
        for e in &self.expects {
            e.expect.flatten_into(&mut out);
        }
        out
    }
}
