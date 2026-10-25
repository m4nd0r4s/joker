//! What the harness records for an output, tick by tick, and the
//! assertions on it (ref/04 §6.4).

use core::fmt;
use core::time::Duration;

use joker::{Level, WriteOutcome};

use crate::value::Value;

/// One tick of an output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    /// Virtual time since the start.
    pub(crate) at: Duration,
    /// The applied value after the commit; `None` when control was handed
    /// back to the device (`safe = "relinquish"`).
    pub(crate) applied: Option<Value>,
    /// What the app proposed in this tick, if it did.
    pub(crate) proposal: Option<Value>,
    /// The outcome of the app's proposal in this tick.
    pub(crate) outcome: Option<WriteOutcome<Value>>,
    /// The level that won the output; `None` is the safe state.
    pub(crate) level: Option<Level>,
}

/// The trace of one output, or a window of it.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputTrace {
    points: Vec<Point>,
}

/// An observed value, compared with an `Option`:
/// `assert_eq!(h.output("led").last(), Some(false))`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observed(pub Option<Value>);

impl<T: Into<Value> + Copy> PartialEq<Option<T>> for Observed {
    fn eq(&self, other: &Option<T>) -> bool {
        match (self.0, other) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                let b: Value = (*b).into();
                match (a, b) {
                    (Value::Bool(x), Value::Bool(y)) => x == y,
                    (Value::Bool(_), _) | (_, Value::Bool(_)) => false,
                    (x, y) => x.as_f64() == y.as_f64(),
                }
            }
            _ => false,
        }
    }
}

impl fmt::Display for Observed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(v) => write!(f, "{v:?}"),
            None => f.write_str("none"),
        }
    }
}

impl OutputTrace {
    pub(crate) fn new(points: Vec<Point>) -> Self {
        Self { points }
    }

    fn window(&self, keep: impl Fn(&Point) -> bool) -> Self {
        Self {
            points: self.points.iter().copied().filter(|p| keep(p)).collect(),
        }
    }

    /// The ticks at or after `t`.
    pub fn after(&self, t: Duration) -> Self {
        self.window(|p| p.at >= t)
    }

    /// The ticks before `t`.
    pub fn before(&self, t: Duration) -> Self {
        self.window(|p| p.at < t)
    }

    /// The ticks in `[a, b)`.
    pub fn between(&self, a: Duration, b: Duration) -> Self {
        self.window(|p| p.at >= a && p.at < b)
    }

    /// The applied value of every tick.
    pub fn values(&self) -> impl Iterator<Item = Value> + '_ {
        self.points.iter().filter_map(|p| p.applied)
    }

    /// Whether every applied value satisfies `f`.
    pub fn all(&self, f: impl FnMut(Value) -> bool) -> bool {
        self.values().all(f)
    }

    /// Whether some applied value satisfies `f`.
    pub fn any(&self, mut f: impl FnMut(Value) -> bool) -> bool {
        self.values().any(&mut f)
    }

    /// The applied value at the last tick at or before `t`.
    pub fn at(&self, t: Duration) -> Observed {
        Observed(
            self.points
                .iter()
                .take_while(|p| p.at <= t)
                .last()
                .and_then(|p| p.applied),
        )
    }

    /// The applied value of the last tick.
    pub fn last(&self) -> Observed {
        Observed(self.points.last().and_then(|p| p.applied))
    }

    fn changes(&self) -> Vec<Duration> {
        self.points
            .windows(2)
            .filter(|w| w[0].applied != w[1].applied)
            .map(|w| w[1].at)
            .collect()
    }

    /// How many times the applied value changed.
    pub fn toggles(&self) -> usize {
        self.changes().len()
    }

    /// The time between consecutive changes.
    pub fn intervals(&self) -> impl Iterator<Item = Duration> + '_ {
        let changes = self.changes();
        (1..changes.len())
            .map(move |i| changes[i] - changes[i - 1])
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Whether the value changed at least twice and every interval between
    /// changes is exactly `d` (the clock is virtual).
    pub fn toggles_every(&self, d: Duration) -> bool {
        self.toggles() >= 2 && self.intervals().all(|i| i == d)
    }

    /// The app's proposals, with their instants.
    pub fn proposals(&self) -> impl Iterator<Item = (Duration, Value)> + '_ {
        self.points
            .iter()
            .filter_map(|p| p.proposal.map(|v| (p.at, v)))
    }

    /// Whether the app proposed nothing in the window.
    pub fn no_new_writes(&self) -> bool {
        self.points.iter().all(|p| p.proposal.is_none())
    }

    /// The outcome of every proposal, with its instant (docs/16 §6).
    pub fn outcomes(&self) -> impl Iterator<Item = (Duration, WriteOutcome<Value>)> + '_ {
        self.points
            .iter()
            .filter_map(|p| p.outcome.map(|o| (p.at, o)))
    }

    /// The winning level of every tick; `None` is the safe state (ADR
    /// 0083).
    pub fn levels(&self) -> impl Iterator<Item = (Duration, Option<Level>)> + '_ {
        self.points.iter().map(|p| (p.at, p.level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ms: u64, v: bool) -> Point {
        Point {
            at: Duration::from_millis(ms),
            applied: Some(Value::Bool(v)),
            proposal: Some(Value::Bool(v)),
            outcome: Some(WriteOutcome::Accepted),
            level: None,
        }
    }

    #[test]
    fn toggles_and_intervals_follow_the_applied_value() {
        let t = OutputTrace::new(vec![
            point(0, false),
            point(20, true),
            point(40, true),
            point(60, false),
            point(80, true),
        ]);
        assert_eq!(t.toggles(), 3);
        assert!(!t.toggles_every(Duration::from_millis(20)));
        assert_eq!(
            t.intervals().collect::<Vec<_>>(),
            vec![Duration::from_millis(40), Duration::from_millis(20)]
        );
        assert_eq!(t.last(), Some(true));
        assert_eq!(t.at(Duration::from_millis(59)), Some(true));
        assert_eq!(t.after(Duration::from_millis(60)).toggles(), 1);
    }
}
