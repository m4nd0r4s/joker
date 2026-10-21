//! Compares a timeline with the expectations of a case (ref/03 §2.5.4,
//! §2.6.4). A failed expectation is `J0842`, with the tick, the value seen
//! and the value expected.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use joker_case_runner::check::{Seen, View, holds, view};
use joker_case_runner::{Fixture, SequenceDigest};
use joker_core::{Outcome, RejectReason, Sample};

use super::exec::Timeline;
use super::literal::{self, QUALITY_REASONS, REJECT_REASONS};
use super::model::{Case, Expect, Expected, Predicates, Subject, Suite};

/// A failed expectation.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    /// Index of the expectation in the case.
    pub index: usize,
    /// The expectation as written.
    pub source: String,
    /// The first tick where it failed.
    pub tick: u64,
    /// That tick as an instant (`"2200ms"`).
    pub at: String,
    /// The subject path.
    pub subject: String,
    /// What was expected, in canonical literals.
    pub expected: String,
    /// What the core produced in that tick.
    pub actual: String,
}

/// The canonical text of a write result (ref/00 §4.4).
pub fn outcome_text(outcome: Outcome) -> String {
    let reason = |r: RejectReason| {
        REJECT_REASONS
            .get(usize::from(r as u8).wrapping_sub(1))
            .copied()
            .unwrap_or("unknown")
    };
    match outcome {
        Outcome::None => "none".to_string(),
        Outcome::Accepted => "accepted".to_string(),
        Outcome::ClampedRange => "clamped_range".to_string(),
        Outcome::ClampedRate => "clamped_rate".to_string(),
        Outcome::Suppressed => "suppressed".to_string(),
        Outcome::Overridden(level) => format!("overridden({})", literal::level_name(level.get())),
        Outcome::Deferred => "deferred".to_string(),
        Outcome::Shadowed => "shadowed".to_string(),
        Outcome::Rejected(r) => format!("rejected({})", reason(r)),
        Outcome::Tripped => "tripped".to_string(),
    }
}

/// The canonical text of a sample quality (ref/00 §4.3).
pub fn quality_text(sample: &Sample) -> String {
    let q = sample.quality();
    let code = q.code() as u8;
    let reason = q.reason().to_u8();
    let code = ["good", "uncertain", "bad"]
        .get(usize::from(code))
        .copied()
        .unwrap_or("bad");
    match QUALITY_REASONS.get(usize::from(reason)) {
        Some(&"none") => code.to_string(),
        Some(r) => format!("{code}({r})"),
        None => format!("{code}({reason})"),
    }
}

fn level_text(level: Option<u8>) -> String {
    level.map_or_else(|| "none".to_string(), literal::level_name)
}

fn seen_text(seen: Seen, unit: &str) -> String {
    let unit = if unit == "1" { "" } else { unit };
    let with_unit = |x: String| {
        if unit.is_empty() {
            x
        } else {
            format!("{x} {unit}")
        }
    };
    match seen {
        Seen::Number(x) => with_unit(format!("{x}")),
        Seen::Integer(i) => with_unit(format!("{i}")),
        Seen::Bool(b) => format!("{b}"),
        Seen::Relinquish => "relinquish".to_string(),
        Seen::NoValue => "no value".to_string(),
    }
}

fn expected_text(e: Expected, unit: &str) -> String {
    match e {
        Expected::Number(x) => seen_text(Seen::Number(x), unit),
        Expected::Integer(i) => seen_text(Seen::Integer(i), unit),
        Expected::Bool(b) => format!("{b}"),
        Expected::Relinquish => "relinquish".to_string(),
    }
}

fn subject_info<'s>(suite: &'s Suite, subject: &Subject) -> (&'s str, &'s str) {
    let pick = |path: &'s str, unit: &'s str| (path, unit);
    match subject {
        Subject::Output(i) | Subject::OutputSignal(i) => suite
            .outputs
            .get(*i)
            .map_or(("?", ""), |o| pick(&o.path, &o.unit)),
        Subject::Input(i) => suite
            .signals
            .get(*i)
            .map_or(("?", ""), |s| pick(&s.path, &s.unit)),
        Subject::Calc(i) => suite
            .calcs
            .get(*i)
            .map_or(("?", ""), |c| pick(&c.path, &c.unit)),
        Subject::Alarm(i) => suite.alarms.get(*i).map_or(("?", ""), |a| pick(&a.id, "")),
        Subject::Commit(i) => suite
            .atomic
            .get(*i)
            .and_then(|g| g.commit.as_ref())
            .map_or(("?", ""), |c| pick(&c.0, "")),
    }
}

fn describe_expected(p: &Predicates, unit: &str) -> String {
    let mut parts = Vec::new();
    if let Some(e) = p.equals {
        let mut text = format!("equals {}", expected_text(e, unit));
        if let Some(t) = p.tolerance {
            text.push_str(&format!(" ± {}", seen_text(Seen::Number(t), unit)));
        }
        parts.push(text);
    }
    if let Some(x) = p.min {
        parts.push(format!("min {}", seen_text(Seen::Number(x), unit)));
    }
    if let Some(x) = p.max {
        parts.push(format!("max {}", seen_text(Seen::Number(x), unit)));
    }
    if let Some(q) = &p.quality {
        let code = ["good", "uncertain", "bad"]
            .get(usize::from(q.code))
            .copied()
            .unwrap_or("?");
        match q.reason.and_then(|r| QUALITY_REASONS.get(usize::from(r))) {
            Some(r) => parts.push(format!("quality {code}({r})")),
            None => parts.push(format!("quality {code}")),
        }
    }
    if let Some(o) = &p.outcome {
        parts.push(format!("outcome {}", literal::outcome_text(o)));
    }
    if let Some(l) = p.level {
        parts.push(format!("level {}", level_text(l)));
    }
    if let Some(s) = p.state {
        parts.push(format!("state {}", state_text(s)));
    }
    if let Some(a) = p.active {
        parts.push(format!("active {a}"));
    }
    parts.join(", ")
}

fn state_text(state: u8) -> &'static str {
    literal::ALARM_STATES
        .get(usize::from(state))
        .copied()
        .unwrap_or("unknown")
}

fn describe_actual(p: &Predicates, v: &View<'_>, unit: &str) -> String {
    if let Some(a) = v.alarm {
        return format!("state {}, active {}", state_text(a.state), a.active);
    }
    let mut parts = Vec::new();
    if p.equals.is_some() || p.min.is_some() || p.max.is_some() || v.output.is_some() {
        let key = if v.output.is_some() {
            "applied"
        } else {
            "value"
        };
        parts.push(format!("{key} {}", seen_text(v.seen, unit)));
    }
    if let Some(o) = v.output {
        parts.push(format!("outcome {}", outcome_text(o.result.outcome)));
        parts.push(format!(
            "level {}",
            level_text(o.result.level.map(|l| l.get()))
        ));
        if o.result.tripped {
            parts.push("tripped".to_string());
        }
    }
    if p.quality.is_some() || v.output.is_none() {
        parts.push(format!("quality {}", quality_text(v.sample)));
    }
    parts.join(", ")
}

fn check(
    suite: &Suite,
    fixture: &Fixture<'_>,
    timeline: &Timeline,
    expect: &Expect,
    index: usize,
    source: &str,
) -> Option<Failure> {
    match expect {
        Expect::Check {
            when,
            subject,
            predicates,
        } => {
            let (path, unit) = subject_info(suite, subject);
            let (first, last) = when.bounds();
            for tick in first..=last {
                let failure = |actual: String| Failure {
                    index,
                    source: source.to_string(),
                    tick,
                    at: literal::format_ns(tick.saturating_mul(suite.period_ns)),
                    subject: path.to_string(),
                    expected: describe_expected(predicates, unit),
                    actual,
                };
                let Some(record) = timeline
                    .ticks
                    .get(usize::try_from(tick).unwrap_or(usize::MAX))
                else {
                    return Some(failure("the case did not reach this tick".to_string()));
                };
                let record = record.view();
                let Some(v) = view(fixture, *subject, &record) else {
                    return Some(failure("no record for the subject".to_string()));
                };
                if !holds(predicates, &v) {
                    return Some(failure(describe_actual(predicates, &v, unit)));
                }
            }
            None
        }
        Expect::AllOf(list) => list
            .iter()
            .find_map(|e| check(suite, fixture, timeline, e, index, source)),
        Expect::AnyOf(list) => {
            let failures: Vec<Failure> = list
                .iter()
                .filter_map(|e| check(suite, fixture, timeline, e, index, source))
                .collect();
            if failures.len() < list.len() {
                return None;
            }
            let first = failures.first()?.clone();
            let expected = failures
                .iter()
                .map(|f| format!("{} at {}: {}", f.subject, f.at, f.expected))
                .collect::<Vec<_>>()
                .join(" | ");
            let actual = failures
                .iter()
                .map(|f| format!("{} at {}: {}", f.subject, f.at, f.actual))
                .collect::<Vec<_>>()
                .join(" | ");
            Some(Failure {
                expected: format!("any of: {expected}"),
                actual,
                ..first
            })
        }
    }
}

/// Checks every expectation of the case against the timeline.
pub fn evaluate(suite: &Suite, case: &Case, timeline: &Timeline) -> Vec<Failure> {
    let view = suite.view();
    let fixture = view.fixture();
    case.expects
        .iter()
        .enumerate()
        .filter_map(|(i, e)| check(suite, &fixture, timeline, &e.expect, i, &e.source))
        .collect()
}

/// The digest of the full per-tick result sequence of the case, the bytes
/// the determinism gate compares between targets and runs (16 §7, ref/03
/// §2.6.4): [`SequenceDigest`], the same code the QEMU firmware runs.
pub fn sequence_digest(timeline: &Timeline) -> u64 {
    let mut digest = SequenceDigest::new();
    for (t, record) in timeline.ticks.iter().enumerate() {
        digest.tick(t as u64, &record.outputs);
    }
    digest.value()
}
