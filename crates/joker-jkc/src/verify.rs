//! Class conformance suites (docs/14 §7.3, ref/03 §2.6.3) against a
//! simulated instance: every member a simulated signal, the `safe` of each
//! `out` its `@safe_hint`, one writer `test:<suite>` at `control:12` with a
//! lease of three periods, and the core of `joker-core` driven tick by tick
//! through `joker-case-runner`, as the core suites are.
//!
//! The runner holds its own writes as ref/03 §2.6.3 asks: after a method or
//! a proposal writes an `out`, it re-proposes the value its level holds in
//! every later tick (a synthetic method `x = own(x)`, ADR 0041), until a
//! `lease_expire`, which relinquishes the member in that tick.

use std::collections::{BTreeMap, BTreeSet};

use joker_case_runner::check::{Seen, holds, view};
use joker_case_runner::exec::{OutputTick, TickView};
use joker_case_runner::{
    Action, ActionKind, CalcSpec, Expected, Fixture, InstanceSpec, MethodSpec, Observer,
    OutputSpec, ParamSpec, Predicates, QualityLit, SafeDef, Script, SignalSpec, StmtSpec, Subject,
    Unguarded, WriterSpec,
};
use joker_core::image::MAX_METHOD_ARGS;
use joker_core::output::{ClockRequirement, OnInterlock, OnRange};
use joker_core::{Sample, ValueType, WriterKind};
use joker_units::{Duration, Literal, Unit};
use toml::{Table, Value};

use crate::ast::Direction;
use crate::contract::{ContractFile, Statement, Ty};
use crate::expr::reads;

/// The period of a simulated instance's group when the suite names none.
pub const DEFAULT_PERIOD_NS: u64 = 100_000_000;

/// The level of the test writer on a simulated instance (`control:12`).
pub const TEST_LEVEL: u8 = 12;

const S: usize = 96;
const O: usize = 24;
const W: usize = 2;
const P: usize = 96;

/// Why a suite cannot run at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteError {
    /// The registry code.
    pub code: &'static str,
    /// What is wrong.
    pub message: String,
}

fn err(code: &'static str, message: impl Into<String>) -> SuiteError {
    SuiteError {
        code,
        message: message.into(),
    }
}

/// What happened to one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Every expectation held.
    Passed,
    /// The expectations that failed (`J0842`).
    Failed(Vec<String>),
    /// Not run, with the code and the reason (`J0846`).
    Skipped(&'static str, String),
}

/// One case of a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseReport {
    /// The case id.
    pub id: String,
    /// What happened.
    pub status: Status,
}

/// The report of a suite against one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The class reference.
    pub class: String,
    /// The capabilities of the instance.
    pub capabilities: Vec<String>,
    /// Every case, in the order of the file.
    pub cases: Vec<CaseReport>,
}

impl Report {
    /// The cases that passed.
    pub fn passed(&self) -> usize {
        self.count(|s| matches!(s, Status::Passed))
    }

    /// The cases that failed.
    pub fn failed(&self) -> usize {
        self.count(|s| matches!(s, Status::Failed(_)))
    }

    /// The cases skipped.
    pub fn skipped(&self) -> usize {
        self.count(|s| matches!(s, Status::Skipped(..)))
    }

    fn count(&self, f: impl Fn(&Status) -> bool) -> usize {
        self.cases.iter().filter(|c| f(&c.status)).count()
    }

    /// The report as `jk class verify` prints it.
    pub fn render(&self) -> String {
        let mut out = format!(
            "{} · simulated instance · capabilities [{}]\n",
            self.class,
            self.capabilities.join(", ")
        );
        for c in &self.cases {
            let status = match &c.status {
                Status::Passed => "ok".to_owned(),
                Status::Failed(_) => "FAILED".to_owned(),
                Status::Skipped(code, why) => format!("skipped ({code}: {why})"),
            };
            let dots = ".".repeat(40usize.saturating_sub(c.id.len()).max(3));
            out.push_str(&format!("  {} {dots} {status}\n", c.id));
            if let Status::Failed(why) = &c.status {
                for w in why {
                    out.push_str(&format!("      {w}\n"));
                }
            }
        }
        out.push_str(&format!(
            "{} cases · {} ok · {} failed · {} skipped\n",
            self.cases.len(),
            self.passed(),
            self.failed(),
            self.skipped()
        ));
        out
    }
}

#[derive(Debug, Clone)]
struct Member {
    path: String,
    ty: ValueType,
    class_ty: Ty,
    unit: String,
}

#[derive(Debug, Clone)]
struct Query {
    path: String,
    expr: String,
    ty: ValueType,
    unit: String,
}

#[derive(Debug, Clone)]
struct Method {
    params: Vec<(String, ValueType, String, Ty)>,
    stmts: Vec<(usize, String)>,
    writes: Vec<usize>,
}

/// A simulated instance of a class with some of its capabilities.
#[derive(Debug)]
struct Instance<'c> {
    contract: &'c ContractFile,
    inputs: Vec<(String, Member)>,
    outputs: Vec<(String, Member, u64)>,
    queries: Vec<(String, Query)>,
    methods: Vec<(String, Method)>,
    holds: usize,
}

fn value_type(ty: &Ty) -> Option<ValueType> {
    Some(match ty {
        Ty::Bool => ValueType::Bool,
        Ty::I64 => ValueType::I64,
        Ty::U64 => ValueType::U64,
        Ty::F64 => ValueType::F64,
        Ty::F32 => ValueType::F32,
        Ty::Enum(_) => ValueType::Enum,
        Ty::Bits(_) => ValueType::Bits,
        _ => return None,
    })
}

fn rewrite(expr: &str, names: &BTreeSet<String>) -> Result<String, SuiteError> {
    let ast = joker_expr::parse(expr)
        .map_err(|e| err("J0844", format!("expression `{expr}`: {}", e.kind)))?;
    let mut out = String::new();
    let mut last = 0;
    for r in reads(&ast, expr) {
        let head = r.path.split('.').next().unwrap_or("");
        if names.contains(head) {
            out.push_str(expr.get(last..r.range.start).unwrap_or(""));
            out.push_str("i.");
            out.push_str(&r.path);
            last = r.range.end;
        }
    }
    out.push_str(expr.get(last..).unwrap_or(""));
    Ok(out)
}

impl<'c> Instance<'c> {
    fn new(
        contract: &'c ContractFile,
        capabilities: &BTreeSet<String>,
    ) -> Result<Self, SuiteError> {
        let c = &contract.contract;
        let has = |cap: &Option<String>| cap.as_ref().is_none_or(|k| capabilities.contains(k));
        if !c.class_members.is_empty() {
            return Err(err(
                "J0846",
                "a class with class members is not simulated by this runner",
            ));
        }
        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for m in c.members.iter().filter(|m| has(&m.capability)) {
            let ty = value_type(&m.ty).ok_or_else(|| {
                err(
                    "J0846",
                    format!(
                        "member `{}`: a `{}` is not simulated",
                        m.name,
                        m.ty.render()
                    ),
                )
            })?;
            names.insert(m.name.clone());
            let member = Member {
                path: format!("i.{}", m.name),
                ty,
                class_ty: m.ty.clone(),
                unit: m.unit.clone().unwrap_or_else(|| "1".into()),
            };
            match m.direction {
                Direction::Out => {
                    let hint = m.safe_hint.as_deref().ok_or_else(|| {
                        err(
                            "J0846",
                            format!(
                                "`out {}` has no `@safe_hint`: a simulated instance takes its safe from it",
                                m.name
                            ),
                        )
                    })?;
                    let safe = encode_text(contract, hint, &member)
                        .map_err(|e| err("J0846", format!("`@safe_hint` of `{}`: {e}", m.name)))?;
                    outputs.push((m.name.clone(), member, safe));
                }
                Direction::In | Direction::Param => inputs.push((m.name.clone(), member)),
            }
        }
        let available: Vec<_> = c.queries.iter().filter(|q| has(&q.capability)).collect();
        for q in &available {
            names.insert(q.name.clone());
        }
        let mut queries = Vec::new();
        let mut placed: BTreeSet<String> = BTreeSet::new();
        while placed.len() < available.len() {
            let before = placed.len();
            for q in &available {
                if placed.contains(&q.name) {
                    continue;
                }
                let ast = joker_expr::parse(&q.expr)
                    .map_err(|e| err("J0844", format!("query `{}`: {}", q.name, e.kind)))?;
                let ready = reads(&ast, &q.expr).iter().all(|r| {
                    let head = r.path.split('.').next().unwrap_or("");
                    !available.iter().any(|o| o.name == head) || placed.contains(head)
                });
                if ready {
                    let ty = value_type(&q.ty).ok_or_else(|| {
                        err("J0846", format!("query `{}`: not simulated", q.name))
                    })?;
                    queries.push((
                        q.name.clone(),
                        Query {
                            path: format!("i.{}", q.name),
                            expr: rewrite(&q.expr, &names)?,
                            ty,
                            unit: q.unit.clone().unwrap_or_else(|| "1".into()),
                        },
                    ));
                    placed.insert(q.name.clone());
                }
            }
            if placed.len() == before {
                return Err(err("J0844", "the queries depend on each other in a cycle"));
            }
        }
        let output_index = |name: &str| outputs.iter().position(|(n, _, _)| n == name);
        let mut methods = Vec::new();
        for m in c.methods.iter().filter(|m| has(&m.capability)) {
            let mut params = Vec::new();
            for p in &m.params {
                let ty = value_type(&p.ty).ok_or_else(|| {
                    err(
                        "J0846",
                        format!("method `{}`: argument not simulated", m.name),
                    )
                })?;
                params.push((
                    p.name.clone(),
                    ty,
                    p.unit.clone().unwrap_or_else(|| "1".into()),
                    p.ty.clone(),
                ));
            }
            let mut stmts = Vec::new();
            let mut writes = Vec::new();
            for s in &m.body {
                match s {
                    Statement::Assign { target, expr } => {
                        let o = output_index(target).ok_or_else(|| {
                            err(
                                "J0844",
                                format!("method `{}` writes unknown `{target}`", m.name),
                            )
                        })?;
                        stmts.push((o, rewrite(expr, &names)?));
                        writes.push(o);
                    }
                    Statement::Call { .. } => {
                        return Err(err(
                            "J0846",
                            "a method that calls class members is not simulated by this runner",
                        ));
                    }
                }
            }
            methods.push((
                m.name.clone(),
                Method {
                    params,
                    stmts,
                    writes,
                },
            ));
        }
        let holds = methods.len();
        for (o, (name, _, _)) in outputs.iter().enumerate() {
            methods.push((
                format!("__hold_{name}"),
                Method {
                    params: Vec::new(),
                    stmts: vec![(o, format!("own(i.{name})"))],
                    writes: vec![o],
                },
            ));
        }
        Ok(Self {
            contract,
            inputs,
            outputs,
            queries,
            methods,
            holds,
        })
    }

    fn member(&self, name: &str) -> Option<(Subject, &Member)> {
        if let Some(i) = self.outputs.iter().position(|(n, _, _)| n == name) {
            return self.outputs.get(i).map(|(_, m, _)| (Subject::Output(i), m));
        }
        let i = self.inputs.iter().position(|(n, _)| n == name)?;
        self.inputs.get(i).map(|(_, m)| (Subject::Input(i), m))
    }
}

fn encode_number(value: f64, ty: ValueType) -> Result<u64, String> {
    match ty {
        ValueType::F64 | ValueType::F32 => Ok(value.to_bits()),
        ValueType::I64 if value.fract() == 0.0 => Ok((value as i64) as u64),
        ValueType::U64 | ValueType::Bits | ValueType::Enum
            if value.fract() == 0.0 && value >= 0.0 =>
        {
            Ok(value as u64)
        }
        _ => Err(format!("{value} does not fit the member")),
    }
}

fn number_in(text: &str, unit: &str) -> Result<f64, String> {
    match Literal::parse(text).map_err(|e| format!("`{text}`: {}", e.kind.message()))? {
        Literal::Number(n) => Ok(n),
        Literal::Quantity(q) => {
            let to = Unit::parse(unit).map_err(|e| e.kind.message().to_owned())?;
            q.value_in(&to)
                .map_err(|_| format!("`{text}` is not in a unit of `{unit}`"))
        }
    }
}

fn encode_text(contract: &ContractFile, text: &str, m: &Member) -> Result<u64, String> {
    match &m.class_ty {
        Ty::Bool => match text {
            "true" => Ok(1),
            "false" => Ok(0),
            _ => Err(format!("`{text}` is not a bool")),
        },
        Ty::Enum(name) => contract
            .contract
            .enums
            .iter()
            .find(|e| &e.name == name)
            .and_then(|e| e.variants.iter().find(|(v, _)| v == text))
            .and_then(|(_, v)| u64::try_from(*v).ok())
            .ok_or_else(|| format!("`{text}` is not a variant of `{name}`")),
        _ => encode_number(number_in(text, &m.unit)?, m.ty),
    }
}

fn encode(contract: &ContractFile, value: &Value, m: &Member) -> Result<u64, String> {
    match value {
        Value::Boolean(b) if m.ty == ValueType::Bool => Ok(u64::from(*b)),
        Value::Integer(i) => encode_number(*i as f64, m.ty),
        Value::Float(f) => encode_number(*f, m.ty),
        Value::String(s) => encode_text(contract, s, m),
        other => Err(format!("`{}` is not a value of the member", show(other))),
    }
}

fn expected(contract: &ContractFile, value: &Value, m: &Member) -> Result<Expected, String> {
    let bits = encode(contract, value, m)?;
    Ok(match m.ty {
        ValueType::Bool => Expected::Bool(bits == 1),
        ValueType::F64 | ValueType::F32 => Expected::Number(f64::from_bits(bits)),
        ValueType::I64 => Expected::Integer(i128::from(bits as i64)),
        _ => Expected::Integer(i128::from(bits)),
    })
}

fn difference_in(value: &Value, unit: &str) -> Result<f64, String> {
    match value {
        Value::Integer(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        Value::String(s) => match Literal::parse(s).map_err(|e| e.kind.message().to_owned())? {
            Literal::Number(n) => Ok(n),
            Literal::Quantity(q) => {
                let to = Unit::parse(unit).map_err(|e| e.kind.message().to_owned())?;
                q.unit
                    .difference()
                    .convert(q.value, &to.difference())
                    .map_err(|_| format!("`{s}` is not in a unit of `{unit}`"))
            }
        },
        other => Err(format!("`{}` is not a number", show(other))),
    }
}

fn show(value: &Value) -> String {
    match value {
        Value::String(s) => format!("{s:?}"),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Datetime(d) => format!("{d:?}"),
        Value::Array(a) => format!("[{}]", a.iter().map(show).collect::<Vec<_>>().join(", ")),
        Value::Table(t) => format!(
            "{{ {} }}",
            t.iter()
                .map(|(k, v)| format!("{k} = {}", show(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

const QUALITY_CODES: [&str; 3] = ["good", "uncertain", "bad"];

const QUALITY_REASONS: [&str; 20] = [
    "none",
    "overridden",
    "local_override",
    "stale",
    "not_confirmed",
    "substituted",
    "out_of_distribution",
    "sensor_cal",
    "clock_unsynced",
    "initial",
    "frozen",
    "partial_write",
    "comm_failure",
    "device_fault",
    "out_of_range",
    "config_error",
    "not_connected",
    "nan",
    "tripped",
    "deadline",
];

fn quality(text: &str) -> Result<QualityLit, String> {
    let (head, reason) = match text.split_once('(') {
        Some((h, r)) => (h, Some(r.trim_end_matches(')'))),
        None => (text, None),
    };
    let code = QUALITY_CODES
        .iter()
        .position(|c| *c == head)
        .ok_or_else(|| format!("`{text}` is not a quality"))?;
    let reason = match reason {
        None => None,
        Some(r) => Some(
            QUALITY_REASONS
                .iter()
                .position(|q| *q == r)
                .and_then(|p| u8::try_from(p).ok())
                .ok_or_else(|| format!("`{r}` is not a quality reason"))?,
        ),
    };
    Ok(QualityLit {
        code: u8::try_from(code).unwrap_or(2),
        reason,
    })
}

const BANDS: [(&str, u8, u8); 6] = [
    ("safety", 1, 2),
    ("manual", 3, 5),
    ("supervisor", 6, 8),
    ("control", 9, 12),
    ("optimizer", 13, 15),
    ("schedule", 16, 16),
];

fn level(text: &str) -> Result<Option<u8>, String> {
    if text == "none" {
        return Ok(None);
    }
    let bad = || format!("`{text}` is not an arbiter level");
    let (band, n) = text.split_once(':').ok_or_else(bad)?;
    let n: u8 = n.parse().map_err(|_| bad())?;
    BANDS
        .iter()
        .find(|(b, lo, hi)| *b == band && (*lo..=*hi).contains(&n))
        .map(|_| Some(n))
        .ok_or_else(bad)
}

fn duration_ns(text: &str) -> Result<u64, String> {
    Duration::parse(text.trim())
        .map(Duration::as_nanos)
        .map_err(|e| format!("`{text}`: {}", e.kind.message()))
}

fn interval_ns(text: &str) -> Result<(u64, u64), String> {
    let (a, b) = text
        .split_once("..")
        .ok_or_else(|| format!("`{text}` is not an interval `a .. b`"))?;
    let (a, b) = (duration_ns(a)?, duration_ns(b)?);
    if a > b {
        return Err(format!("`{text}`: the interval is reversed"));
    }
    Ok((a, b))
}

#[derive(Debug, Clone, Copy)]
enum When {
    At(u64),
    During(u64, u64),
    Within(u64, u64),
}

impl When {
    fn last(self) -> u64 {
        match self {
            Self::At(t) => t,
            Self::During(_, b) | Self::Within(_, b) => b,
        }
    }
}

#[derive(Debug, Clone)]
enum Expect {
    Check {
        text: String,
        when: When,
        subject: Subject,
        predicates: Predicates,
    },
    AnyOf(Vec<Expect>),
    AllOf(Vec<Expect>),
}

enum Parsed<T> {
    Ok(T),
    Skip(String),
}

struct Case<'s> {
    instance: &'s Instance<'s>,
    period: u64,
    ticks: u64,
}

impl Case<'_> {
    fn tick(&self, ns: u64) -> u64 {
        ns / self.period
    }

    fn tick_up(&self, ns: u64) -> u64 {
        ns.div_ceil(self.period)
    }

    fn when(&self, t: &Table) -> Result<When, String> {
        let keys: Vec<&str> = ["at", "during", "within", "before"]
            .into_iter()
            .filter(|k| t.contains_key(*k))
            .collect();
        let text = |k: &str| t.get(k).and_then(Value::as_str).unwrap_or("");
        match keys.as_slice() {
            ["at"] => Ok(When::At(self.tick(duration_ns(text("at"))?))),
            ["during"] => {
                let (a, b) = interval_ns(text("during"))?;
                Ok(When::During(self.tick_up(a), self.tick(b)))
            }
            ["within"] => {
                let (a, b) = interval_ns(text("within"))?;
                Ok(When::Within(self.tick_up(a), self.tick(b)))
            }
            ["before"] => Ok(When::Within(0, self.tick(duration_ns(text("before"))?))),
            _ => Err("an expectation has exactly one of at, during, within, before".into()),
        }
    }

    fn expect(&self, value: &Value) -> Result<Parsed<Expect>, String> {
        let t = value.as_table().ok_or("an expectation is a table")?;
        for (key, combine) in [("any_of", true), ("all_of", false)] {
            if let Some(list) = t.get(key) {
                let list = list.as_array().ok_or("`any_of`/`all_of` is a list")?;
                let mut out = Vec::new();
                for item in list {
                    match self.expect(item)? {
                        Parsed::Ok(e) => out.push(e),
                        Parsed::Skip(why) => return Ok(Parsed::Skip(why)),
                    }
                }
                return Ok(Parsed::Ok(if combine {
                    Expect::AnyOf(out)
                } else {
                    Expect::AllOf(out)
                }));
            }
        }
        for unsupported in [
            "health", "event", "action", "count", "fields", "result", "outcome",
        ] {
            if t.contains_key(unsupported) {
                return Ok(Parsed::Skip(format!(
                    "`{unsupported}` is not observable on this simulated instance"
                )));
            }
        }
        let c = &self.instance.contract.contract;
        let (subject, member, name) = if let Some(m) = t.get("member").and_then(Value::as_str) {
            match self.instance.member(m) {
                Some((s, member)) => (s, member.clone(), m),
                None if c.member(m).is_some() => {
                    return Ok(Parsed::Skip(format!(
                        "member `{m}` belongs to a capability the instance does not have"
                    )));
                }
                None => return Err(format!("J0844: unknown member `{m}`")),
            }
        } else if let Some(q) = t.get("query").and_then(Value::as_str) {
            match self.instance.queries.iter().position(|(n, _)| n == q) {
                Some(i) => {
                    let query = &self.instance.queries[i].1;
                    let ty = c.query(q).map(|d| d.ty.clone()).unwrap_or(Ty::Bool);
                    (
                        Subject::Calc(i),
                        Member {
                            path: query.path.clone(),
                            ty: query.ty,
                            class_ty: ty,
                            unit: query.unit.clone(),
                        },
                        q,
                    )
                }
                None if c.query(q).is_some() => {
                    return Ok(Parsed::Skip(format!(
                        "query `{q}` belongs to a capability the instance does not have"
                    )));
                }
                None => return Err(format!("J0844: unknown query `{q}`")),
            }
        } else {
            return Err("an expectation names a `member` or a `query`".into());
        };
        let contract = self.instance.contract;
        let mut p = Predicates::default();
        if let Some(v) = t.get("equals") {
            p.equals = Some(expected(contract, v, &member)?);
        }
        if let Some(v) = t.get("min") {
            p.min = Some(difference_in(v, &member.unit)?);
        }
        if let Some(v) = t.get("max") {
            p.max = Some(difference_in(v, &member.unit)?);
        }
        if let Some(v) = t.get("tolerance") {
            p.tolerance = Some(difference_in(v, &member.unit)?);
        }
        if let Some(v) = t.get("quality").and_then(Value::as_str) {
            p.quality = Some(quality(v)?);
        }
        if let Some(v) = t.get("level").and_then(Value::as_str) {
            if !matches!(subject, Subject::Output(_)) {
                return Err(format!("`level` needs an `out` member, not `{name}`"));
            }
            p.level = Some(level(v)?);
        }
        let when = self.when(t)?;
        Ok(Parsed::Ok(Expect::Check {
            text: show(value),
            when,
            subject,
            predicates: p,
        }))
    }
}

#[derive(Debug, Default)]
struct Recorder {
    ticks: Vec<(Vec<OutputTick>, Vec<Sample>, Vec<Sample>)>,
}

impl Observer for Recorder {
    fn tick(&mut self, _tick: u64, view: &TickView<'_>) {
        self.ticks.push((
            view.outputs.to_vec(),
            view.inputs.to_vec(),
            view.calcs.to_vec(),
        ));
    }
}

fn evaluate(e: &Expect, fixture: &Fixture<'_>, rec: &Recorder, failures: &mut Vec<String>) -> bool {
    match e {
        Expect::AnyOf(list) => {
            let mut inner = Vec::new();
            let ok = list.iter().any(|x| evaluate(x, fixture, rec, &mut inner));
            if !ok {
                failures.push(format!("none of `any_of` holds: {}", inner.join("; ")));
            }
            ok
        }
        Expect::AllOf(list) => {
            let mut ok = true;
            for x in list {
                ok &= evaluate(x, fixture, rec, failures);
            }
            ok
        }
        Expect::Check {
            text,
            when,
            subject,
            predicates,
        } => {
            let at = |t: u64| -> Option<(bool, Seen)> {
                let (o, i, c) = rec.ticks.get(usize::try_from(t).ok()?)?;
                let tv = TickView {
                    outputs: o,
                    inputs: i,
                    calcs: c,
                    alarms: &[],
                    commits: &[],
                };
                let v = view(fixture, *subject, &tv)?;
                Some((holds(predicates, &v), v.seen))
            };
            let (ok, seen) = match *when {
                When::At(t) => at(t).map_or((false, None), |(ok, s)| (ok, Some((t, s)))),
                When::During(a, b) => {
                    let mut result = (a <= b, None);
                    for t in a..=b {
                        match at(t) {
                            Some((true, _)) => {}
                            Some((false, s)) => {
                                result = (false, Some((t, s)));
                                break;
                            }
                            None => {
                                result = (false, None);
                                break;
                            }
                        }
                    }
                    result
                }
                When::Within(a, b) => {
                    let mut last = None;
                    let mut ok = false;
                    for t in a..=b {
                        if let Some((hit, s)) = at(t) {
                            last = Some((t, s));
                            if hit {
                                ok = true;
                                break;
                            }
                        }
                    }
                    (ok, last)
                }
            };
            if !ok {
                let seen = match seen {
                    Some((t, s)) => format!("tick {t}: saw {s:?}"),
                    None => "no tick in the window".into(),
                };
                failures.push(format!("J0842 {text}: {seen}"));
            }
            ok
        }
    }
}

/// Runs the suite `text` (file `file_name`) of `class` against a simulated
/// instance with `capabilities`.
pub fn verify(
    file_name: &str,
    text: &str,
    class: &ContractFile,
    capabilities: &[String],
) -> Result<Report, SuiteError> {
    let doc: Table = toml::from_str(text).map_err(|e| err("J0801", e.to_string()))?;
    if doc.get("schema").and_then(Value::as_integer) != Some(1) {
        return Err(err("J0801", "unsupported schema version (v1 reads 1)"));
    }
    let suite = doc
        .get("suite")
        .and_then(Value::as_table)
        .ok_or_else(|| err("J0801", "missing `[suite]`"))?;
    if suite.get("kind").and_then(Value::as_str) != Some("class") {
        return Err(err("J0845", "a class suite has `kind = \"class\"`"));
    }
    let reference = class.contract.reference();
    if suite.get("class").and_then(Value::as_str) != Some(reference.as_str()) {
        return Err(err(
            "J0845",
            format!("`class` is not `{reference}`, the class of the file"),
        ));
    }
    let name = class.contract.name();
    let major = class.contract.version.major;
    if file_name != format!("{name}.conformance.toml")
        && file_name != format!("{name}@{major}.conformance.toml")
    {
        return Err(err(
            "J0845",
            format!("`{file_name}` is not the suite of `{name}.jkc`"),
        ));
    }
    let period = match suite.get("period").and_then(Value::as_str) {
        Some(p) => duration_ns(p).map_err(|e| err("J0801", e))?,
        None => DEFAULT_PERIOD_NS,
    };
    if period == 0 {
        return Err(err("J0801", "`period` is zero"));
    }
    let wanted: BTreeSet<String> = capabilities.iter().cloned().collect();
    let caps: BTreeSet<String> = class
        .contract
        .capabilities
        .iter()
        .filter(|c| wanted.contains(*c))
        .cloned()
        .collect();
    let instance = Instance::new(class, &caps)?;
    let strings = |t: &Table, key: &str| -> Vec<String> {
        t.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let suite_requires = strings(suite, "requires");
    let mut report = Report {
        class: reference.clone(),
        capabilities: caps.iter().cloned().collect(),
        cases: Vec::new(),
    };
    let cases = doc
        .get("case")
        .and_then(Value::as_array)
        .ok_or_else(|| err("J0801", "a suite has at least one `[[case]]`"))?;
    let mut ids = BTreeSet::new();
    for case in cases {
        let case = case
            .as_table()
            .ok_or_else(|| err("J0801", "a case is a table"))?;
        let id = case
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| err("J0801", "a case has an `id`"))?
            .to_owned();
        if !ids.insert(id.clone()) {
            return Err(err("J0805", format!("case id `{id}` is used twice")));
        }
        let requires: Vec<String> = suite_requires
            .iter()
            .cloned()
            .chain(strings(case, "requires"))
            .collect();
        let missing: Vec<&String> = requires.iter().filter(|r| !caps.contains(*r)).collect();
        let status = if !missing.is_empty() {
            Status::Skipped(
                "J0846",
                format!(
                    "the instance does not have {}",
                    missing
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        } else {
            run_case(&instance, period, case)
                .map_err(|e| err("J0844", format!("case `{id}`: {e}")))?
        };
        report.cases.push(CaseReport { id, status });
    }
    Ok(report)
}

fn run_case(instance: &Instance<'_>, period: u64, case: &Table) -> Result<Status, String> {
    let mut c = Case {
        instance,
        period,
        ticks: 0,
    };
    let contract = instance.contract;
    let mut actions: Vec<(u64, usize, ActionKind)> = Vec::new();
    let mut last = 0u64;
    let empty = Vec::new();
    let steps = case.get("do").and_then(Value::as_array).unwrap_or(&empty);
    let mut writes: Vec<(u64, usize, bool)> = Vec::new();
    for (seq, step) in steps.iter().enumerate() {
        let t = step.as_table().ok_or("an action is a table")?;
        let at = c.tick(duration_ns(
            t.get("at")
                .and_then(Value::as_str)
                .ok_or("an action has `at`")?,
        )?);
        last = last.max(at);
        if let Some(m) = t.get("method").and_then(Value::as_str) {
            let Some(index) = instance.methods[..instance.holds]
                .iter()
                .position(|(n, _)| n == m)
            else {
                if contract.contract.method(m).is_some() {
                    return Ok(Status::Skipped(
                        "J0846",
                        format!("method `{m}` belongs to a capability the instance does not have"),
                    ));
                }
                return Err(format!("unknown method `{m}`"));
            };
            let method = &instance.methods[index].1;
            let mut args = [0u64; MAX_METHOD_ARGS];
            let given = t.get("args").and_then(Value::as_table);
            for (slot, (pname, ty, unit, class_ty)) in args.iter_mut().zip(&method.params) {
                let v = given
                    .and_then(|g| g.get(pname))
                    .ok_or_else(|| format!("method `{m}`: missing argument `{pname}`"))?;
                let member = Member {
                    path: String::new(),
                    ty: *ty,
                    class_ty: class_ty.clone(),
                    unit: unit.clone(),
                };
                *slot = encode(contract, v, &member)?;
            }
            for o in &method.writes {
                writes.push((at, *o, true));
            }
            actions.push((
                at,
                seq,
                ActionKind::Method {
                    instance: 0,
                    method: u16::try_from(index).map_err(|_| "too many methods")?,
                    writer: 0,
                    args,
                    n_args: u8::try_from(method.params.len()).map_err(|_| "too many arguments")?,
                },
            ));
        } else if let Some(m) = t.get("propose").and_then(Value::as_str) {
            let o = instance
                .outputs
                .iter()
                .position(|(n, _, _)| n == m)
                .ok_or_else(|| format!("`{m}` is not an `out` member of the instance"))?;
            let v = t.get("value").ok_or("a proposal has a `value`")?;
            let bits = encode(contract, v, &instance.outputs[o].1)?;
            writes.push((at, o, true));
            actions.push((
                at,
                seq,
                ActionKind::Propose {
                    output: o,
                    writer: 0,
                    bits,
                },
            ));
        } else if let Some(m) = t.get("lease_expire").and_then(Value::as_str) {
            let o = instance
                .outputs
                .iter()
                .position(|(n, _, _)| n == m)
                .ok_or_else(|| format!("`{m}` is not an `out` member of the instance"))?;
            writes.push((at, o, false));
            actions.push((
                at,
                seq,
                ActionKind::Release {
                    output: o,
                    writer: 0,
                },
            ));
        } else if let Some(m) = t.get("input").and_then(Value::as_str) {
            let Some(i) = instance.inputs.iter().position(|(n, _)| n == m) else {
                if contract.contract.member(m).is_some() {
                    return Ok(Status::Skipped(
                        "J0846",
                        format!("member `{m}` belongs to a capability the instance does not have"),
                    ));
                }
                return Err(format!("`{m}` is not an `in` or `param` member"));
            };
            let q = match t.get("quality").and_then(Value::as_str) {
                Some(q) => quality(q)?,
                None => QualityLit {
                    code: 0,
                    reason: None,
                },
            };
            let bits = match t.get("value") {
                Some(v) => encode(contract, v, &instance.inputs[i].1)?,
                None if q.code == 2 => 0,
                None => return Err("an input without `bad` quality has a `value`".into()),
            };
            actions.push((
                at,
                seq,
                ActionKind::Input {
                    signal: i,
                    bits,
                    quality: q,
                },
            ));
        } else if t.contains_key("action") {
            return Ok(Status::Skipped(
                "J0846",
                "actions are not invoked on this simulated instance".into(),
            ));
        } else {
            return Err("an action has one of method, propose, action, lease_expire, input".into());
        }
    }
    let empty_expect = Vec::new();
    let mut expects = Vec::new();
    for e in case
        .get("expect")
        .and_then(Value::as_array)
        .unwrap_or(&empty_expect)
    {
        match c.expect(e)? {
            Parsed::Ok(x) => expects.push(x),
            Parsed::Skip(why) => return Ok(Status::Skipped("J0846", why)),
        }
    }
    if expects.is_empty() {
        return Err("a case has at least one expectation".into());
    }
    fn last_tick(e: &Expect) -> u64 {
        match e {
            Expect::Check { when, .. } => when.last(),
            Expect::AnyOf(l) | Expect::AllOf(l) => l.iter().map(last_tick).max().unwrap_or(0),
        }
    }
    let horizon = expects.iter().map(last_tick).max().unwrap_or(0).max(last);
    c.ticks = match case.get("duration").and_then(Value::as_str) {
        Some(d) => c.tick(duration_ns(d)?).max(horizon + 1),
        None => horizon + 2,
    };
    let mut held: BTreeMap<usize, u64> = BTreeMap::new();
    let mut timeline: Vec<(u64, u8, usize, ActionKind)> = Vec::new();
    for tick in 0..c.ticks {
        for (o, since) in &held {
            if *since < tick {
                timeline.push((
                    tick,
                    0,
                    *o,
                    ActionKind::Method {
                        instance: 0,
                        method: u16::try_from(instance.holds + o)
                            .map_err(|_| "too many methods")?,
                        writer: 0,
                        args: [0; MAX_METHOD_ARGS],
                        n_args: 0,
                    },
                ));
            }
        }
        for (at, o, start) in writes.iter().filter(|(at, _, _)| *at == tick) {
            if *start {
                held.insert(*o, *at);
            } else {
                held.remove(o);
            }
        }
    }
    for (at, seq, kind) in actions {
        timeline.push((at, 1, seq, kind));
    }
    timeline.sort_by_key(|(t, prio, seq, _)| (*t, *prio, *seq));
    let script_actions: Vec<Action> = timeline
        .iter()
        .map(|(t, _, _, kind)| Action {
            tick: *t,
            kind: *kind,
        })
        .collect();
    let signals: Vec<SignalSpec<'_>> = instance
        .inputs
        .iter()
        .map(|(_, m)| SignalSpec {
            path: &m.path,
            ty: m.ty,
            unit: &m.unit,
            max_age_ns: None,
        })
        .collect();
    let outputs: Vec<OutputSpec<'_>> = instance
        .outputs
        .iter()
        .map(|(_, m, safe)| OutputSpec {
            path: &m.path,
            ty: m.ty,
            unit: &m.unit,
            safe: SafeDef::Value(*safe),
            safe_ramp: None,
            range: None,
            on_range: OnRange::Clamp,
            rate: None,
            max_lease_ns: None,
            max_manual_ns: None,
            interlock: None,
            permit_uncertain: false,
            on_interlock: OnInterlock::Safe,
            grace_ns: 0,
            reset_when: None,
            deadband: None,
            min_interval_ns: 0,
            max_actuations: None,
        })
        .collect();
    let calcs: Vec<CalcSpec<'_>> = instance
        .queries
        .iter()
        .map(|(_, q)| CalcSpec {
            path: &q.path,
            expr: &q.expr,
            ty: q.ty,
            unit: &q.unit,
        })
        .collect();
    let params: Vec<Vec<ParamSpec<'_>>> = instance
        .methods
        .iter()
        .map(|(_, m)| {
            m.params
                .iter()
                .map(|(n, ty, unit, _)| ParamSpec {
                    name: n,
                    ty: *ty,
                    unit,
                })
                .collect()
        })
        .collect();
    let stmts: Vec<Vec<StmtSpec<'_>>> = instance
        .methods
        .iter()
        .map(|(_, m)| {
            m.stmts
                .iter()
                .map(|(o, e)| StmtSpec::Assign {
                    output: *o,
                    expr: e,
                })
                .collect()
        })
        .collect();
    let methods: Vec<MethodSpec<'_>> = params
        .iter()
        .zip(&stmts)
        .map(|(p, s)| MethodSpec {
            params: p,
            stmts: s,
        })
        .collect();
    let instances = [InstanceSpec { methods: &methods }];
    let writers = [WriterSpec {
        level: TEST_LEVEL,
        kind: WriterKind::Cycle,
        lease_ns: Some(3 * period),
        requires_clock: ClockRequirement::Any,
    }];
    let fixture = Fixture {
        period_ns: period,
        signals: &signals,
        outputs: &outputs,
        writers: &writers,
        calcs: &calcs,
        alarms: &[],
        atomic: &[],
        requires_good: &[],
        trips: &[],
        members: &[],
        order: &[],
        instances: &instances,
    };
    let script = Script {
        ticks: c.ticks,
        actions: &script_actions,
        expects: &[],
    };
    let needs =
        joker_case_runner::needs(&fixture).map_err(|e| format!("{}: {}", e.code, e.message))?;
    let mut programs: Vec<Option<joker_expr::Program>> = vec![None; needs.programs];
    let mut arena = vec![0u64; needs.arena + 64];
    let mut recorder = Recorder::default();
    joker_case_runner::run::<S, O, W, P>(
        &fixture,
        &script,
        &mut programs,
        &mut arena,
        &Unguarded,
        &mut recorder,
    )
    .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let mut failures = Vec::new();
    for e in &expects {
        evaluate(e, &fixture, &recorder, &mut failures);
    }
    Ok(if failures.is_empty() {
        Status::Passed
    } else {
        Status::Failed(failures)
    })
}
