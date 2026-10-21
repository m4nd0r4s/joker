//! The expectations of a case (ref/03 §2.5.4, §2.6.4): what a predicate
//! sees in a tick, whether it holds, and a verdict fed one tick at a time,
//! so a target needs no storage for the timeline.

use joker_core::{Applied, Sample, ValueType};

use crate::exec::{AlarmTick, OutputTick, TickView};
use crate::model::{Expected, Fixture, Node, Predicates, Subject};

/// A decoded value of a signal.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Seen {
    /// A number in the unit of the signal.
    Number(f64),
    /// An integer.
    Integer(i128),
    /// A boolean.
    Bool(bool),
    /// The output relinquished.
    Relinquish,
    /// The sample has no usable value (`Bad`).
    NoValue,
}

impl Seen {
    /// The value as a number, if it is one.
    pub fn as_f64(self) -> Option<f64> {
        match self {
            Self::Number(x) => Some(x),
            Self::Integer(i) => Some(i as f64),
            _ => None,
        }
    }
}

/// Decodes the bits of a value of type `ty`.
pub fn decode(bits: u64, ty: ValueType) -> Seen {
    match ty {
        ValueType::Bool if bits <= 1 => Seen::Bool(bits == 1),
        ValueType::F64 | ValueType::F32 => Seen::Number(f64::from_bits(bits)),
        ValueType::I64 => Seen::Integer(i128::from(bits as i64)),
        _ => Seen::Integer(i128::from(bits)),
    }
}

fn sample_value(sample: &Sample, ty: ValueType) -> Seen {
    match sample.get_uncertain() {
        Ok(bits) => decode(bits, ty),
        Err(_) => Seen::NoValue,
    }
}

/// What an expectation sees of its subject in one tick.
#[derive(Copy, Clone, Debug)]
pub struct View<'a> {
    /// The value: the applied value of an output, or the value of a sample.
    pub seen: Seen,
    /// The sample of the subject.
    pub sample: &'a Sample,
    /// The result of the output, when the subject is an output.
    pub output: Option<&'a OutputTick>,
    /// The state of the alarm, when the subject is an alarm.
    pub alarm: Option<&'a AlarmTick>,
}

/// The sample an alarm subject carries: alarms are checked by state.
static NO_SAMPLE: Sample = Sample::INITIAL;

/// The view of `subject` in a tick, or `None` when the tick has no record
/// for it.
pub fn view<'a>(fixture: &Fixture<'_>, subject: Subject, tick: &TickView<'a>) -> Option<View<'a>> {
    match subject {
        Subject::Output(i) => {
            let o = tick.outputs.get(i)?;
            let seen = match o.result.applied {
                Applied::Relinquish => Seen::Relinquish,
                Applied::Value(bits) => decode(bits, fixture.outputs.get(i)?.ty),
            };
            Some(View {
                seen,
                sample: &o.sample,
                output: Some(o),
                alarm: None,
            })
        }
        Subject::OutputSignal(i) => {
            let o = tick.outputs.get(i)?;
            Some(signal_view(&o.sample, fixture.outputs.get(i)?.ty))
        }
        Subject::Input(i) => Some(signal_view(tick.inputs.get(i)?, fixture.signals.get(i)?.ty)),
        Subject::Calc(i) => Some(signal_view(tick.calcs.get(i)?, fixture.calcs.get(i)?.ty)),
        Subject::Commit(i) => Some(signal_view(
            tick.commits.get(i)?,
            fixture.atomic.get(i)?.commit?.ty,
        )),
        Subject::Alarm(i) => Some(View {
            seen: Seen::NoValue,
            sample: &NO_SAMPLE,
            output: None,
            alarm: Some(tick.alarms.get(i)?),
        }),
    }
}

fn signal_view(sample: &Sample, ty: ValueType) -> View<'_> {
    View {
        seen: sample_value(sample, ty),
        sample,
        output: None,
        alarm: None,
    }
}

/// Whether a value equals the expected one, within the tolerance.
pub fn equals(seen: Seen, expected: Expected, tolerance: Option<f64>) -> bool {
    match (seen, expected) {
        (Seen::Bool(a), Expected::Bool(b)) => a == b,
        (Seen::Relinquish, Expected::Relinquish) => true,
        (Seen::Integer(a), Expected::Integer(b)) if tolerance.is_none() => a == b,
        (seen, Expected::Number(_) | Expected::Integer(_)) => {
            let Some(a) = seen.as_f64() else {
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

/// Whether every predicate holds in the view.
pub fn holds(p: &Predicates, v: &View<'_>) -> bool {
    if let Some(e) = p.equals
        && !equals(v.seen, e, p.tolerance)
    {
        return false;
    }
    let number = v.seen.as_f64();
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
    if let Some(q) = p.quality {
        let quality = v.sample.quality();
        if quality.code() as u8 != q.code || q.reason.is_some_and(|r| r != quality.reason().to_u8())
        {
            return false;
        }
    }
    if let Some(o) = p.outcome {
        let Some(out) = v.output else {
            return false;
        };
        let outcome = out.result.outcome;
        if outcome.code() != o.code || o.detail.is_some_and(|d| d != outcome.detail()) {
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
    if p.state.is_some() || p.active.is_some() {
        let Some(alarm) = v.alarm else {
            return false;
        };
        if p.state.is_some_and(|s| s != alarm.state) || p.active.is_some_and(|a| a != alarm.active)
        {
            return false;
        }
    }
    true
}

/// The end of the tree that starts at `nodes[i]`.
fn subtree_end(nodes: &[Node], i: usize) -> usize {
    let mut next = i + 1;
    let n = match nodes.get(i) {
        Some(Node::AnyOf(n) | Node::AllOf(n)) => *n,
        _ => 0,
    };
    for _ in 0..n {
        next = subtree_end(nodes, next);
    }
    next
}

/// The verdict of a case, fed one tick at a time: one flag per node of
/// [`Script::expects`](crate::model::Script::expects), set when a simple
/// expectation fails at a tick of its window.
#[derive(Debug)]
pub struct Verdict<'a> {
    nodes: &'a [Node],
    failed: &'a mut [bool],
    reached: u64,
}

impl<'a> Verdict<'a> {
    /// A verdict over `nodes`, with `failed` as storage (at least as long as
    /// `nodes`; extra entries are ignored). Returns `None` when it is
    /// shorter.
    pub fn new(nodes: &'a [Node], failed: &'a mut [bool]) -> Option<Self> {
        let failed = failed.get_mut(..nodes.len())?;
        failed.fill(false);
        Some(Self {
            nodes,
            failed,
            reached: 0,
        })
    }

    /// Checks every simple expectation whose window covers `tick`.
    pub fn tick(&mut self, fixture: &Fixture<'_>, tick: u64, view: &TickView<'_>) {
        for (node, failed) in self.nodes.iter().zip(self.failed.iter_mut()) {
            let Node::Check {
                when,
                subject,
                predicates,
            } = node
            else {
                continue;
            };
            let (first, last) = when.bounds();
            if *failed || tick < first || tick > last {
                continue;
            }
            let holds = self::view(fixture, *subject, view).is_some_and(|v| holds(predicates, &v));
            if !holds {
                *failed = true;
            }
        }
        self.reached = self.reached.max(tick.saturating_add(1));
    }

    fn passes(&self, i: usize) -> bool {
        match self.nodes.get(i) {
            Some(Node::Check { when, .. }) => {
                let (_, last) = when.bounds();
                !self.failed.get(i).copied().unwrap_or(true) && last < self.reached
            }
            Some(Node::AllOf(n)) => {
                let mut child = i + 1;
                for _ in 0..*n {
                    if !self.passes(child) {
                        return false;
                    }
                    child = subtree_end(self.nodes, child);
                }
                true
            }
            Some(Node::AnyOf(n)) => {
                let mut child = i + 1;
                for _ in 0..*n {
                    if self.passes(child) {
                        return true;
                    }
                    child = subtree_end(self.nodes, child);
                }
                *n == 0
            }
            None => false,
        }
    }

    /// The number of top-level expectations that fail, once every tick of
    /// the case has been fed.
    pub fn failures(&self) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < self.nodes.len() {
            count += usize::from(!self.passes(i));
            i = subtree_end(self.nodes, i);
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::When;

    fn check(at: u64) -> Node {
        Node::Check {
            when: When::At(at),
            subject: Subject::Output(0),
            predicates: Predicates::default(),
        }
    }

    #[test]
    fn subtrees_skip_their_children() {
        let nodes = [Node::AnyOf(2), check(0), Node::AllOf(1), check(1), check(2)];
        assert_eq!(subtree_end(&nodes, 0), 4);
        assert_eq!(subtree_end(&nodes, 2), 4);
        assert_eq!(subtree_end(&nodes, 4), 5);
    }

    #[test]
    fn an_expectation_past_the_last_tick_fails() {
        let nodes = [check(0), check(5)];
        let mut failed = [false; 2];
        let verdict = Verdict::new(&nodes, &mut failed);
        assert!(verdict.is_some_and(|mut v| {
            v.reached = 3;
            v.passes(0) && !v.passes(1) && v.failures() == 1
        }));
    }

    #[test]
    fn any_of_passes_with_one_branch() {
        let nodes = [Node::AnyOf(2), check(9), check(0)];
        let mut failed = [false; 3];
        let verdict = Verdict::new(&nodes, &mut failed);
        assert!(verdict.is_some_and(|mut v| {
            v.reached = 1;
            v.failures() == 0
        }));
    }
}
