//! Input series over virtual time (ref/04 §6.5).

use core::time::Duration;

use joker::Quality;

use crate::value::Value;

#[derive(Clone, Debug, PartialEq)]
enum Shape {
    Constant(Value),
    Step(Value, Value, u64),
    Ramp(f64, f64, u64),
    Linear(f64, f64, u64),
    Sine(f64, f64, u64),
    Points(Vec<(u64, f64)>),
}

/// The value of an input over time, sampled at every tick unless
/// [`Series::every`] says otherwise, with its quality.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    shape: Shape,
    every_ns: Option<u64>,
    qualities: Vec<(u64, u64, Quality)>,
    frozen_after_ns: Option<u64>,
}

fn ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

impl Series {
    fn of(shape: Shape) -> Self {
        Self {
            shape,
            every_ns: None,
            qualities: Vec::new(),
            frozen_after_ns: None,
        }
    }

    /// The same value all the time.
    pub fn constant(v: impl Into<Value>) -> Self {
        Self::of(Shape::Constant(v.into()))
    }

    /// `before` until `at`, then `after`.
    pub fn step(before: impl Into<Value>, after: impl Into<Value>, at: Duration) -> Self {
        Self::of(Shape::Step(before.into(), after.into(), ns(at)))
    }

    /// Linear from `from` to `to` over `over`, then `to`.
    pub fn ramp(from: f64, to: f64, over: Duration) -> Self {
        Self::of(Shape::Ramp(from, to, ns(over).max(1)))
    }

    /// The straight line through `at_0` at 0 and `at_over` at `over`, which
    /// goes on after `over`.
    pub fn linear(at_0: f64, at_over: f64, over: Duration) -> Self {
        Self::of(Shape::Linear(at_0, at_over, ns(over).max(1)))
    }

    /// `mean + amp × sin(2π t / period)`.
    pub fn sine(mean: f64, amp: f64, period: Duration) -> Self {
        Self::of(Shape::Sine(mean, amp, ns(period).max(1)))
    }

    /// Steps at the given instants; before the first, its value.
    pub fn points(p: &[(Duration, f64)]) -> Self {
        let mut points: Vec<(u64, f64)> = p.iter().map(|&(t, v)| (ns(t), v)).collect();
        points.sort_by_key(|&(t, _)| t);
        Self::of(Shape::Points(points))
    }

    /// A new sample every `d` instead of every tick.
    pub fn every(mut self, d: Duration) -> Self {
        self.every_ns = Some(ns(d).max(1));
        self
    }

    /// Quality `q` from `t` on.
    pub fn with_quality_at(self, t: Duration, q: Quality) -> Self {
        self.with_quality(t, Duration::MAX, q)
    }

    /// Quality `q` from `from` until `to`.
    pub fn with_quality(mut self, from: Duration, to: Duration, q: Quality) -> Self {
        self.qualities.push((ns(from), ns(to), q));
        self
    }

    /// The value stops changing after `t`, bit for bit.
    pub fn frozen_after(mut self, t: Duration) -> Self {
        self.frozen_after_ns = Some(ns(t));
        self
    }

    /// The instant whose sample is current at `t_ns`.
    pub(crate) fn sample_instant(&self, t_ns: u64) -> u64 {
        let t = self.frozen_after_ns.map_or(t_ns, |f| t_ns.min(f));
        match self.every_ns {
            Some(e) => t - t % e,
            None => t,
        }
    }

    /// The value at `t_ns`.
    pub(crate) fn value_at(&self, t_ns: u64) -> Value {
        let t = self.sample_instant(t_ns);
        let tf = t as f64;
        match &self.shape {
            Shape::Constant(v) => *v,
            Shape::Step(before, after, at) => {
                if t < *at {
                    *before
                } else {
                    *after
                }
            }
            Shape::Ramp(from, to, over) => {
                let k = (tf / *over as f64).min(1.0);
                Value::F64(from + (to - from) * k)
            }
            Shape::Linear(a, b, over) => Value::F64(a + (b - a) * tf / *over as f64),
            Shape::Sine(mean, amp, period) => {
                Value::F64(mean + amp * (core::f64::consts::TAU * tf / *period as f64).sin())
            }
            Shape::Points(points) => {
                let v = points
                    .iter()
                    .take_while(|&&(at, _)| at <= t)
                    .last()
                    .or(points.first())
                    .map_or(0.0, |&(_, v)| v);
                Value::F64(v)
            }
        }
    }

    /// The quality at `t_ns`: the last matching window wins.
    pub(crate) fn quality_at(&self, t_ns: u64) -> Quality {
        self.qualities
            .iter()
            .rev()
            .find(|&&(from, to, _)| from <= t_ns && t_ns < to)
            .map_or(Quality::GOOD, |&(_, _, q)| q)
    }
}

impl From<f64> for Series {
    fn from(v: f64) -> Self {
        Self::constant(v)
    }
}

impl From<bool> for Series {
    fn from(v: bool) -> Self {
        Self::constant(v)
    }
}

impl From<i64> for Series {
    fn from(v: i64) -> Self {
        Self::constant(v)
    }
}

impl From<u64> for Series {
    fn from(v: u64) -> Self {
        Self::constant(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_sample_where_they_should() {
        let s = Series::step(1.0, 2.0, Duration::from_millis(100));
        assert_eq!(s.value_at(99_999_999), Value::F64(1.0));
        assert_eq!(s.value_at(100_000_000), Value::F64(2.0));
        let r = Series::ramp(0.0, 10.0, Duration::from_secs(1));
        assert_eq!(r.value_at(500_000_000), Value::F64(5.0));
        assert_eq!(r.value_at(5_000_000_000), Value::F64(10.0));
        let p = Series::points(&[(Duration::ZERO, 1.0), (Duration::from_secs(1), 3.0)]);
        assert_eq!(p.value_at(999_999_999), Value::F64(1.0));
        assert_eq!(p.value_at(1_000_000_000), Value::F64(3.0));
        let e = Series::linear(0.0, 1.0, Duration::from_secs(1)).every(Duration::from_millis(100));
        assert_eq!(e.value_at(150_000_000), Value::F64(0.1));
    }

    #[test]
    fn quality_windows_apply_in_order() {
        let bad = Quality::bad(joker::QualityReason::CommFailure);
        let s =
            Series::constant(1.0).with_quality(Duration::from_secs(1), Duration::from_secs(2), bad);
        assert_eq!(s.quality_at(0), Quality::GOOD);
        assert_eq!(s.quality_at(1_500_000_000), bad);
        assert_eq!(s.quality_at(2_000_000_000), Quality::GOOD);
    }
}
