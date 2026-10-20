//! Compares a timeline with the expectations of a case (ref/03 §2.5.4,
//! §2.6.4). A failed expectation is `J0842`, with the tick, the value seen
//! and the value expected.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use joker_core::{Applied, Outcome, RejectReason, Sample, ValueType};

use super::exec::{OutputTick, Timeline};
use super::literal::{self, QUALITY_REASONS, REJECT_REASONS};
use super::model::{Case, Expect, Expected, Predicates, Subject, Suite, When};

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

/// A decoded value of a signal.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Seen {
    Number(f64),
    Integer(i128),
    Bool(bool),
    Relinquish,
    NoValue,
}

fn decode(bits: u64, ty: ValueType) -> Seen {
    match ty {
        ValueType::Bool if bits <= 1 => Seen::Bool(bits == 1),
        ValueType::F64 | ValueType::F32 => Seen::Number(f64::from_bits(bits)),
        ValueType::I64 => Seen::Integer(i128::from(bits as i64)),
        _ => Seen::Integer(i128::from(bits)),
    }
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

fn as_f64(seen: Seen) -> Option<f64> {
    match seen {
        Seen::Number(x) => Some(x),
        Seen::Integer(i) => Some(i as f64),
        _ => None,
    }
}

fn equals(seen: Seen, expected: Expected, tolerance: Option<f64>) -> bool {
    match (seen, expected) {
        (Seen::Bool(a), Expected::Bool(b)) => a == b,
        (Seen::Relinquish, Expected::Relinquish) => true,
        (Seen::Integer(a), Expected::Integer(b)) if tolerance.is_none() => a == b,
        (seen, Expected::Number(_) | Expected::Integer(_)) => {
            let Some(a) = as_f64(seen) else {
                return false;
            };
            let b = match expected {
                Expected::Number(x) => x,
                Expected::Integer(i) => i as f64,
                _ => return false,
            };
            match tolerance {
                Some(t) => (a - b).abs() <= t,
                None => a == b,
            }
        }
        _ => false,
    }
}

struct View<'a> {
    seen: Seen,
    sample: &'a Sample,
    output: Option<&'a OutputTick>,
}

fn view<'a>(
    suite: &Suite,
    subject: &Subject,
    tick: &'a super::exec::TickRecord,
) -> Option<View<'a>> {
    match subject {
        Subject::Output(i) => {
            let o = tick.outputs.get(*i)?;
            let seen = match o.result.applied {
                Applied::Relinquish => Seen::Relinquish,
                Applied::Value(bits) => decode(bits, suite.outputs.get(*i)?.ty),
            };
            Some(View {
                seen,
                sample: &o.sample,
                output: Some(o),
            })
        }
        Subject::OutputSignal(i) => {
            let o = tick.outputs.get(*i)?;
            Some(View {
                seen: sample_value(&o.sample, suite.outputs.get(*i)?.ty),
                sample: &o.sample,
                output: None,
            })
        }
        Subject::Input(i) => {
            let s = tick.inputs.get(*i)?;
            Some(View {
                seen: sample_value(s, suite.signals.get(*i)?.ty),
                sample: s,
                output: None,
            })
        }
    }
}

fn sample_value(sample: &Sample, ty: ValueType) -> Seen {
    match sample.get_uncertain() {
        Ok(bits) => decode(bits, ty),
        Err(_) => Seen::NoValue,
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
        parts.push(format!("outcome {}", o.text()));
    }
    if let Some(l) = p.level {
        parts.push(format!("level {}", level_text(l)));
    }
    parts.join(", ")
}

fn describe_actual(p: &Predicates, v: &View<'_>, unit: &str) -> String {
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

fn holds(p: &Predicates, v: &View<'_>) -> bool {
    if let Some(e) = p.equals
        && !equals(v.seen, e, p.tolerance)
    {
        return false;
    }
    let number = as_f64(v.seen);
    if let Some(min) = p.min
        && number.is_none_or(|x| x < min)
    {
        return false;
    }
    if let Some(max) = p.max
        && number.is_none_or(|x| x > max)
    {
        return false;
    }
    if let Some(q) = &p.quality {
        let quality = v.sample.quality();
        if quality.code() as u8 != q.code || q.reason.is_some_and(|r| r != quality.reason().to_u8())
        {
            return false;
        }
    }
    if let Some(o) = &p.outcome {
        let Some(out) = v.output else {
            return false;
        };
        let actual = outcome_text(out.result.outcome);
        let matches = match &o.detail {
            Some(_) => actual == o.text(),
            None => actual.split('(').next() == Some(o.name),
        };
        if !matches {
            return false;
        }
    }
    if let Some(level) = p.level {
        let Some(out) = v.output else {
            return false;
        };
        if out.result.level.map(|l| l.get()) != level {
            return false;
        }
    }
    true
}

fn ticks(when: When) -> (u64, u64) {
    match when {
        When::At(t) => (t, t),
        When::During(a, b) => (a, b),
    }
}

fn check(
    suite: &Suite,
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
            let (first, last) = ticks(*when);
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
                let Some(v) = view(suite, subject, record) else {
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
            .find_map(|e| check(suite, timeline, e, index, source)),
        Expect::AnyOf(list) => {
            let failures: Vec<Failure> = list
                .iter()
                .filter_map(|e| check(suite, timeline, e, index, source))
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
    case.expects
        .iter()
        .enumerate()
        .filter_map(|(i, e)| check(suite, timeline, &e.expect, i, &e.source))
        .collect()
}

/// A digest of the full per-tick result sequence of the case, the bytes the
/// determinism gate compares between hosts and runs (16 §7, ref/03 §2.6.4):
/// FNV-1a 64 over, per tick and per output, the tick index, the output
/// index, the result code and detail, the applied value, the level and the
/// trip flag, all little-endian.
pub fn sequence_digest(timeline: &Timeline) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for (t, record) in timeline.ticks.iter().enumerate() {
        for (o, out) in record.outputs.iter().enumerate() {
            let r = &out.result;
            feed(&(t as u64).to_le_bytes());
            feed(&(o as u32).to_le_bytes());
            feed(&[r.outcome.code(), r.outcome.detail()]);
            match r.applied {
                Applied::Value(bits) => {
                    feed(&[0]);
                    feed(&bits.to_le_bytes());
                }
                Applied::Relinquish => feed(&[1]),
            }
            feed(&[r.level.map_or(0, |l| l.get()), u8::from(r.tripped)]);
        }
    }
    hash
}
