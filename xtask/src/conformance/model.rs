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

use alloc::format;

use joker_case_runner::Fixture;
use joker_core::ValueType;
use joker_core::alarm::Severity;
use joker_core::output::{ClockRequirement, Level, OnInterlock, OnRange, OnRate, WriterKind};

pub use joker_case_runner::model::{
    Action, ActionKind, AlarmSpec, AtomicSpec, CalcSpec, CommitSpec, Expected, Item, LimitSpec,
    Link, Node, OutputSpec, Predicates, SafeDef, SignalSpec, Subject, When, WriterSpec,
};

/// A feature the core does not implement yet, or that this runner does not
/// drive; cases that need one are reported as skipped.
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
    /// Features of the declaration that the core lacks or this runner does not drive.
    pub missing: Vec<Feature>,
}

/// One limit of a `range` (16 E6).
#[derive(Clone, Debug, PartialEq)]
pub enum Limit {
    /// A constant, in the unit of the output.
    Const(f64),
    /// An expression, evaluated in every tick.
    Expr(String),
}

impl Limit {
    fn spec(&self) -> LimitSpec<'_> {
        match self {
            Self::Const(x) => LimitSpec::Const(*x),
            Self::Expr(src) => LimitSpec::Expr(src),
        }
    }
}

/// An output of the fixture (`[[outputs]]`) with the envelope keys the core
/// implements (ref/01 §5.10).
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
    /// `range`: constant limits, or at least one expression (16 E6).
    pub range: Option<(Limit, Limit)>,
    /// `on_range`.
    pub on_range: OnRange,
    /// `rate`, in unit per second.
    pub rate: Option<f64>,
    /// `max_lease` in nanoseconds.
    pub max_lease_ns: Option<u64>,
    /// `max_manual` in nanoseconds.
    pub max_manual_ns: Option<u64>,
    /// `interlock`, the permissive of step 2.
    pub interlock: Option<String>,
    /// `permit_uncertain`.
    pub permit_uncertain: bool,
    /// `on_interlock` (16 E3).
    pub on_interlock: OnInterlock,
    /// `requires_good`, as paths or selectors.
    pub requires_good: Vec<String>,
    /// `grace` in nanoseconds (16 E4).
    pub grace_ns: u64,
    /// `reset_when` (16 E7).
    pub reset_when: Option<String>,
    /// `deadband`, a difference in the unit of the output.
    pub deadband: Option<f64>,
    /// `min_interval` in nanoseconds.
    pub min_interval_ns: u64,
    /// `max_actuations`: count and window in nanoseconds.
    pub max_actuations: Option<(u32, u64)>,
    /// Features of the envelope that the core lacks.
    pub missing: Vec<Feature>,
}

/// A derived signal of the fixture (`[[calc]]`, ref/01 §5.14), with the
/// type and unit the expression checker inferred.
#[derive(Clone, Debug)]
pub struct CalcDef {
    /// Signal path (`calc.<name>`).
    pub path: String,
    /// The expression.
    pub expr: String,
    /// The inferred type.
    pub ty: ValueType,
    /// The inferred unit (`"1"` when dimensionless).
    pub unit: String,
}

/// An alarm of the fixture (`[[alarms]]`, ref/01 §5.15).
#[derive(Clone, Debug)]
pub struct AlarmDef {
    /// Alarm id.
    pub id: String,
    /// `alarm.<id>.state`.
    pub state_path: String,
    /// `alarm.<id>.active`.
    pub active_path: String,
    /// `when`.
    pub when: String,
    /// `suppress_when`.
    pub suppress_when: Option<String>,
    /// `severity`.
    pub severity: Severity,
    /// `delay_on` in nanoseconds.
    pub delay_on_ns: u64,
    /// `delay_off` in nanoseconds.
    pub delay_off_ns: u64,
    /// `max_shelve` in nanoseconds.
    pub max_shelve_ns: Option<u64>,
    /// `actions.trip`, as indices in [`Suite::outputs`].
    pub trip: Vec<usize>,
}

impl AlarmDef {
    /// An alarm with the paths of its signals.
    pub fn new(id: String, when: String, severity: Severity) -> Self {
        Self {
            state_path: format!("alarm.{id}.state"),
            active_path: format!("alarm.{id}.active"),
            id,
            when,
            suppress_when: None,
            severity,
            delay_on_ns: 0,
            delay_off_ns: 0,
            max_shelve_ns: None,
            trip: Vec::new(),
        }
    }
}

/// An atomic group of the fixture (`[[atomic]]`, ref/01 §5.16).
#[derive(Clone, Debug)]
pub struct AtomicDef {
    /// Members, as indices in [`Suite::outputs`].
    pub outputs: Vec<usize>,
    /// `order`, as indices in [`Suite::outputs`]; empty when absent.
    pub order: Vec<usize>,
    /// `on_rate`.
    pub on_rate: OnRate,
    /// `commit`: the register path, its type and the bits of its value.
    pub commit: Option<(String, ValueType, u64)>,
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
    /// Features the case needs that the core lacks or this runner does not drive.
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
    /// Derived signals, in evaluation order (dependencies first).
    pub calcs: Vec<CalcDef>,
    /// Alarms.
    pub alarms: Vec<AlarmDef>,
    /// Atomic groups.
    pub atomic: Vec<AtomicDef>,
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
    calcs: Vec<CalcSpec<'s>>,
    alarms: Vec<AlarmSpec<'s>>,
    atomic: Vec<AtomicSpec<'s>>,
    requires_good: Vec<Link<'s>>,
    trips: Vec<Link<'s>>,
    members: Vec<Link<'s>>,
    order: Vec<Link<'s>>,
}

impl FixtureView<'_> {
    /// The fixture.
    pub fn fixture(&self) -> Fixture<'_> {
        Fixture {
            period_ns: self.period_ns,
            signals: &self.signals,
            outputs: &self.outputs,
            writers: &self.writers,
            calcs: &self.calcs,
            alarms: &self.alarms,
            atomic: &self.atomic,
            requires_good: &self.requires_good,
            trips: &self.trips,
            members: &self.members,
            order: &self.order,
            instances: &[],
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
                    range: o.range.as_ref().map(|(a, b)| (a.spec(), b.spec())),
                    on_range: o.on_range,
                    rate: o.rate,
                    max_lease_ns: o.max_lease_ns,
                    max_manual_ns: o.max_manual_ns,
                    interlock: o.interlock.as_deref(),
                    permit_uncertain: o.permit_uncertain,
                    on_interlock: o.on_interlock,
                    grace_ns: o.grace_ns,
                    reset_when: o.reset_when.as_deref(),
                    deadband: o.deadband,
                    min_interval_ns: o.min_interval_ns,
                    max_actuations: o.max_actuations,
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
            calcs: self
                .calcs
                .iter()
                .map(|c| CalcSpec {
                    path: &c.path,
                    expr: &c.expr,
                    ty: c.ty,
                    unit: &c.unit,
                })
                .collect(),
            alarms: self
                .alarms
                .iter()
                .map(|a| AlarmSpec {
                    state_path: &a.state_path,
                    active_path: &a.active_path,
                    when: &a.when,
                    suppress_when: a.suppress_when.as_deref(),
                    severity: a.severity,
                    delay_on_ns: a.delay_on_ns,
                    delay_off_ns: a.delay_off_ns,
                    max_shelve_ns: a.max_shelve_ns,
                })
                .collect(),
            atomic: self
                .atomic
                .iter()
                .map(|g| AtomicSpec {
                    on_rate: g.on_rate,
                    commit: g.commit.as_ref().map(|(path, ty, value)| CommitSpec {
                        path,
                        ty: *ty,
                        value: *value,
                    }),
                })
                .collect(),
            requires_good: self
                .outputs
                .iter()
                .enumerate()
                .flat_map(|(owner, o)| {
                    o.requires_good.iter().map(move |p| Link {
                        owner,
                        item: Item::Path(p),
                    })
                })
                .collect(),
            trips: links(self.alarms.iter().map(|a| &a.trip)),
            members: links(self.atomic.iter().map(|g| &g.outputs)),
            order: links(self.atomic.iter().map(|g| &g.order)),
        }
    }
}

fn links<'s>(lists: impl Iterator<Item = &'s Vec<usize>>) -> Vec<Link<'s>> {
    lists
        .enumerate()
        .flat_map(|(owner, list)| {
            list.iter().map(move |&o| Link {
                owner,
                item: Item::Output(o),
            })
        })
        .collect()
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
