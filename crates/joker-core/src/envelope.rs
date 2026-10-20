//! The envelope of one output: the steps of docs/01 §7 that M0 needs (clock,
//! range, rate, safe state and trips), applied to the effective value on
//! every tick (docs/16 A7, E1–E2, E5, E7; ADR 0033).
//!
//! Steps 2–3 (permissive and quality) and 6–7 (deadband, `min_interval`,
//! actuation budget, atomic groups) need expressions and atomic groups and
//! come later; the step order leaves their place.

use crate::image::WriterId;
use crate::outcome::{Outcome, RejectReason};
use crate::output::{ClockRequirement, ConfigError, OnRange, OutputConfig, SECOND_NS, SafeValue};
use crate::time::ClockQuality;
use crate::value::ValueType;

/// The value an output applies.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    /// Value bits in the output's type and unit.
    Value(u64),
    /// Control handed back to the device (`safe = "relinquish"`).
    Relinquish,
}

impl Applied {
    /// The applied state of `safe`.
    pub const fn from_safe(safe: SafeValue) -> Self {
        match safe {
            SafeValue::Value(bits) => Self::Value(bits),
            SafeValue::Relinquish => Self::Relinquish,
        }
    }

    /// The value bits, or 0 for `Relinquish`, as the trace stores them.
    pub const fn bits(self) -> u64 {
        match self {
            Self::Value(bits) => bits,
            Self::Relinquish => 0,
        }
    }
}

/// What the arbiter hands to the envelope.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Effective {
    /// No active level: the output goes to `safe` (docs/16 A6).
    Safe,
    /// The value of the active level and the clock its writer requires.
    Writer {
        /// Value bits.
        value: u64,
        /// The writer's `requires_clock` (docs/16 E1).
        requires_clock: ClockRequirement,
    },
}

/// Result of one envelope pass.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// The value to apply in this tick.
    pub applied: Applied,
    /// The envelope result for the active writer; `None` when no writer is
    /// active.
    pub outcome: Option<Outcome>,
    /// The output entered a trip in this tick.
    pub tripped: bool,
}

/// True when `clock` meets `requirement` (docs/16 E1, ADR 0033).
pub const fn clock_meets(requirement: ClockRequirement, clock: ClockQuality) -> bool {
    match requirement {
        ClockRequirement::Any => true,
        ClockRequirement::Holdover => !matches!(clock, ClockQuality::Unsynced),
        ClockRequirement::Synced => matches!(clock, ClockQuality::Synced),
    }
}

/// Runs the envelope once.
///
/// `last` is the last applied value (`safe` on the first tick of a
/// generation, docs/16 A8), `in_trip` whether the output is in trip, and
/// `dt_ns` the step length of ADR 0032.
pub fn evaluate(
    output: &OutputConfig<'_>,
    last: Applied,
    in_trip: bool,
    effective: Effective,
    clock: ClockQuality,
    dt_ns: u64,
) -> Verdict {
    let (value, requires_clock) = match effective {
        Effective::Safe => {
            return Verdict {
                applied: to_safe(output, last, dt_ns),
                outcome: None,
                tripped: false,
            };
        }
        Effective::Writer {
            value,
            requires_clock,
        } => (value, requires_clock),
    };
    let safe_with = |outcome, tripped| Verdict {
        applied: to_safe(output, last, dt_ns),
        outcome: Some(outcome),
        tripped,
    };
    if in_trip {
        return safe_with(Outcome::Tripped, false);
    }
    if !clock_meets(requires_clock, clock) {
        return safe_with(Outcome::Rejected(RejectReason::ClockUnsynced), false);
    }
    let Some(mut x) = output.ty.bits_to_f64(value) else {
        return Verdict {
            applied: Applied::Value(value),
            outcome: Some(Outcome::Accepted),
            tripped: false,
        };
    };
    let mut outcome = None;
    let mut bits = value;
    if let Some((low, high)) = output.range
        && !(low..=high).contains(&x)
    {
        match output.on_range {
            OnRange::Clamp => {
                x = x.clamp(low, high);
                bits = encode(output.ty, x).unwrap_or(value);
                outcome = Some(Outcome::ClampedRange);
            }
            OnRange::Reject => {
                return Verdict {
                    applied: last,
                    outcome: Some(Outcome::Rejected(RejectReason::Range)),
                    tripped: false,
                };
            }
            OnRange::Trip => return safe_with(Outcome::Tripped, true),
        }
    }
    if let Some(rate) = output.rate
        && let Some(limited) = limit_rate(output.ty, last, bits, rate, dt_ns)
    {
        bits = limited;
        outcome = outcome.or(Some(Outcome::ClampedRate));
    }
    Verdict {
        applied: Applied::Value(bits),
        outcome: Some(outcome.unwrap_or(Outcome::Accepted)),
        tripped: false,
    }
}

fn encode(ty: ValueType, x: f64) -> Option<u64> {
    ty.f64_to_bits(x)
}

fn to_safe(output: &OutputConfig<'_>, last: Applied, dt_ns: u64) -> Applied {
    match (output.safe, output.safe_ramp) {
        (SafeValue::Value(target), Some(ramp)) => {
            Applied::Value(limit_rate(output.ty, last, target, ramp, dt_ns).unwrap_or(target))
        }
        (safe, _) => Applied::from_safe(safe),
    }
}

/// Limits a move from `last` to `target` to `rate × dt` (docs/01 §7 step 5).
/// Returns the limited bits, or `None` when no limit applies.
fn limit_rate(ty: ValueType, last: Applied, target: u64, rate: f64, dt_ns: u64) -> Option<u64> {
    let Applied::Value(last_bits) = last else {
        return None;
    };
    let from = ty.bits_to_f64(last_bits)?;
    let to = ty.bits_to_f64(target)?;
    let step = rate.abs() * dt_ns as f64 / SECOND_NS as f64;
    if (to - from).abs() <= step {
        return None;
    }
    let limited = if to > from { from + step } else { from - step };
    encode(ty, limited)
}

/// Checks the envelope of one output (ref/01 §5.10, docs/16 E5): a safe
/// value of the right type (`J0231`), ranges and rates only on numeric
/// outputs (`J0233`), a range with minimum below maximum (`J0230`), a safe
/// value inside a constant range (`J0203`), and non-negative finite rates
/// (`J0202`).
pub fn check_output(output: &OutputConfig<'_>) -> Result<(), ConfigError> {
    let err = |code| ConfigError {
        code,
        output: Some(output.signal),
        writer: None::<WriterId>,
    };
    let numeric = output.ty.is_numeric();
    if (output.range.is_some() || output.rate.is_some() || output.safe_ramp.is_some()) && !numeric {
        return Err(err("J0233"));
    }
    for rate in [output.rate, output.safe_ramp].into_iter().flatten() {
        if !rate.is_finite() || rate < 0.0 {
            return Err(err("J0202"));
        }
    }
    if let Some((low, high)) = output.range
        && !(low.is_finite() && high.is_finite() && low < high)
    {
        return Err(err("J0230"));
    }
    if let SafeValue::Value(bits) = output.safe {
        if !output.accepts(bits) {
            return Err(err("J0231"));
        }
        if let (Some((low, high)), Some(x)) = (output.range, output.ty.bits_to_f64(bits))
            && !(low..=high).contains(&x)
        {
            return Err(err("J0203"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{GroupId, SignalId};

    const MS: u64 = 1_000_000;

    fn out() -> OutputConfig<'static> {
        let mut o = OutputConfig::new(
            SignalId(0),
            ValueType::F64,
            GroupId(0),
            SafeValue::Value(0f64.to_bits()),
        );
        o.range = Some((-100.0, 100.0));
        o.rate = Some(10.0);
        o
    }

    fn w(x: f64) -> Effective {
        Effective::Writer {
            value: x.to_bits(),
            requires_clock: ClockRequirement::Any,
        }
    }

    fn v(x: f64) -> Applied {
        Applied::Value(x.to_bits())
    }

    fn run(o: &OutputConfig<'_>, last: f64, e: Effective) -> (Applied, Option<Outcome>, bool) {
        let r = evaluate(o, v(last), false, e, ClockQuality::Synced, 100 * MS);
        (r.applied, r.outcome, r.tripped)
    }

    #[test]
    fn rate_limits_against_last_applied() {
        assert_eq!(
            run(&out(), 0.0, w(50.0)),
            (v(1.0), Some(Outcome::ClampedRate), false)
        );
        assert_eq!(
            run(&out(), 49.5, w(50.0)),
            (v(50.0), Some(Outcome::Accepted), false)
        );
        assert_eq!(
            run(&out(), 5.0, w(-50.0)),
            (v(4.0), Some(Outcome::ClampedRate), false)
        );
    }

    #[test]
    fn range_clamp_comes_first_and_decides_the_outcome() {
        assert_eq!(
            run(&out(), 99.5, w(150.0)),
            (v(100.0), Some(Outcome::ClampedRange), false)
        );
        assert_eq!(
            run(&out(), 0.0, w(150.0)),
            (v(1.0), Some(Outcome::ClampedRange), false)
        );
    }

    #[test]
    fn range_reject_holds_and_trip_goes_safe() {
        let mut o = out();
        o.on_range = OnRange::Reject;
        assert_eq!(
            run(&o, 7.0, w(150.0)),
            (v(7.0), Some(Outcome::Rejected(RejectReason::Range)), false)
        );
        o.on_range = OnRange::Trip;
        assert_eq!(
            run(&o, 7.0, w(150.0)),
            (v(0.0), Some(Outcome::Tripped), true)
        );
        let r = evaluate(&o, v(0.0), true, w(5.0), ClockQuality::Synced, 100 * MS);
        assert_eq!(
            (r.applied, r.outcome, r.tripped),
            (v(0.0), Some(Outcome::Tripped), false)
        );
    }

    #[test]
    fn no_writer_goes_safe_at_once_or_by_safe_ramp() {
        let mut o = out();
        assert_eq!(run(&o, 30.0, Effective::Safe), (v(0.0), None, false));
        o.safe_ramp = Some(50.0);
        assert_eq!(run(&o, 30.0, Effective::Safe), (v(25.0), None, false));
        assert_eq!(run(&o, 3.0, Effective::Safe), (v(0.0), None, false));
        o.safe = SafeValue::Relinquish;
        assert_eq!(
            run(&o, 3.0, Effective::Safe),
            (Applied::Relinquish, None, false)
        );
    }

    #[test]
    fn clock_step_comes_before_everything() {
        let e = Effective::Writer {
            value: 150.0f64.to_bits(),
            requires_clock: ClockRequirement::Synced,
        };
        let r = evaluate(&out(), v(30.0), false, e, ClockQuality::Holdover, 100 * MS);
        assert_eq!(r.applied, v(0.0));
        assert_eq!(
            r.outcome,
            Some(Outcome::Rejected(RejectReason::ClockUnsynced))
        );
        assert!(clock_meets(
            ClockRequirement::Holdover,
            ClockQuality::Holdover
        ));
        assert!(!clock_meets(
            ClockRequirement::Holdover,
            ClockQuality::Unsynced
        ));
    }

    #[test]
    fn output_checks() {
        assert_eq!(check_output(&out()), Ok(()));
        let mut o = out();
        o.safe = SafeValue::Value(200f64.to_bits());
        assert_eq!(check_output(&o).unwrap_err().code, "J0203");
        let mut o = out();
        o.safe = SafeValue::Value(f64::NAN.to_bits());
        assert_eq!(check_output(&o).unwrap_err().code, "J0231");
        let mut o = out();
        o.range = Some((5.0, 5.0));
        assert_eq!(check_output(&o).unwrap_err().code, "J0230");
        let mut o = out();
        o.rate = Some(-1.0);
        assert_eq!(check_output(&o).unwrap_err().code, "J0202");
        let mut o = out();
        o.ty = ValueType::Bool;
        assert_eq!(check_output(&o).unwrap_err().code, "J0233");
    }

    #[test]
    fn dt_scales_the_rate_step() {
        let r = evaluate(
            &out(),
            v(0.0),
            false,
            w(50.0),
            ClockQuality::Synced,
            200 * MS,
        );
        assert_eq!(r.applied, v(2.0));
    }

    #[test]
    fn integers_and_non_numeric_types() {
        let mut o = out();
        o.ty = ValueType::I64;
        o.rate = Some(15.0);
        let r = evaluate(
            &o,
            Applied::Value(0),
            false,
            Effective::Writer {
                value: 50u64,
                requires_clock: ClockRequirement::Any,
            },
            ClockQuality::Synced,
            100 * MS,
        );
        assert_eq!(r.applied, Applied::Value(2));
        let mut o = out();
        o.ty = ValueType::Bool;
        o.range = None;
        o.rate = None;
        let r = evaluate(
            &o,
            Applied::Value(0),
            false,
            Effective::Writer {
                value: 1,
                requires_clock: ClockRequirement::Any,
            },
            ClockQuality::Synced,
            100 * MS,
        );
        assert_eq!(
            (r.applied, r.outcome),
            (Applied::Value(1), Some(Outcome::Accepted))
        );
    }
}
