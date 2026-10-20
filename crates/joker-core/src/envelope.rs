//! The envelope of one output: the steps of docs/01 §7 applied to the
//! effective value on every tick (docs/16 A7, E1–E10; ADR 0033, ADR 0038).
//!
//! Expressions (permissive, expression range limits, `reset_when`) and the
//! `requires_good` timer are evaluated by the commit stage, which hands the
//! results in a [`Context`]. Step 7 (atomic groups) runs across outputs, in
//! the commit stage.

use crate::image::WriterId;
use crate::outcome::{Outcome, RejectReason};
use crate::output::{
    Bound, ClockRequirement, ConfigError, OnInterlock, OnRange, OutputConfig, SECOND_NS, SafeValue,
};
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

/// The facts of one tick that the envelope needs besides the output's
/// configuration (ADR 0038).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Context {
    /// Quality of the node's clock (step 0).
    pub clock: ClockQuality,
    /// Step length of ADR 0032, for the rate step.
    pub dt_ns: u64,
    /// Latch instant, monotonic ns.
    pub now_ns: u64,
    /// The permissive and the expression range limits pass (step 2).
    pub permit: bool,
    /// `requires_good` holds, within its grace (step 3).
    pub quality: bool,
    /// The range limits of this tick (step 4).
    pub range: Option<(f64, f64)>,
    /// Latch instant of the last actuation, if any (step 6).
    pub last_actuation: Option<u64>,
    /// The actuation budget is spent (step 6).
    pub budget_exhausted: bool,
}

impl Context {
    /// A context with no expressions, no `requires_good`, the constant range
    /// and no actuation history.
    pub fn simple(output: &OutputConfig<'_>, clock: ClockQuality, dt_ns: u64) -> Self {
        Self {
            clock,
            dt_ns,
            now_ns: 0,
            permit: true,
            quality: true,
            range: output.range,
            last_actuation: None,
            budget_exhausted: false,
        }
    }
}

/// True when `clock` meets `requirement` (docs/16 E1, ADR 0033).
pub const fn clock_meets(requirement: ClockRequirement, clock: ClockQuality) -> bool {
    match requirement {
        ClockRequirement::Any => true,
        ClockRequirement::Holdover => !matches!(clock, ClockQuality::Unsynced),
        ClockRequirement::Synced => matches!(clock, ClockQuality::Synced),
    }
}

/// Runs the envelope once with [`Context::simple`].
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
    evaluate_in(
        output,
        last,
        in_trip,
        effective,
        &Context::simple(output, clock, dt_ns),
    )
}

/// Runs the envelope once: steps 0 and 2–6 of docs/01 §7 (step 1 is the
/// arbiter's intake, step 7 the commit stage's).
pub fn evaluate_in(
    output: &OutputConfig<'_>,
    last: Applied,
    in_trip: bool,
    effective: Effective,
    ctx: &Context,
) -> Verdict {
    let dt_ns = ctx.dt_ns;
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
    let refuse = |reason| match output.on_interlock {
        OnInterlock::Safe => safe_with(Outcome::Rejected(reason), false),
        OnInterlock::Hold => Verdict {
            applied: last,
            outcome: Some(Outcome::Rejected(reason)),
            tripped: false,
        },
    };
    let hold = |outcome| Verdict {
        applied: last,
        outcome: Some(outcome),
        tripped: false,
    };
    if in_trip {
        return safe_with(Outcome::Tripped, false);
    }
    if !clock_meets(requires_clock, ctx.clock) {
        return safe_with(Outcome::Rejected(RejectReason::ClockUnsynced), false);
    }
    if !ctx.permit {
        return refuse(RejectReason::Interlock);
    }
    if !ctx.quality {
        return refuse(RejectReason::Quality);
    }
    let mut outcome = None;
    let mut bits = value;
    let number = output.ty.bits_to_f64(value);
    if let (Some(x), Some((low, high))) = (number, ctx.range)
        && !(low..=high).contains(&x)
    {
        match output.on_range {
            OnRange::Clamp => {
                bits = encode(output.ty, x.clamp(low, high)).unwrap_or(value);
                outcome = Some(Outcome::ClampedRange);
            }
            OnRange::Reject => return hold(Outcome::Rejected(RejectReason::Range)),
            OnRange::Trip => return safe_with(Outcome::Tripped, true),
        }
    }
    if let Some(rate) = output.rate
        && let Some(limited) = limit_rate(output.ty, last, bits, rate, dt_ns)
    {
        bits = limited;
        outcome = outcome.or(Some(Outcome::ClampedRate));
    }
    if let (Some(deadband), Applied::Value(last_bits)) = (output.deadband, last)
        && let (Some(to), Some(from)) = (
            output.ty.bits_to_f64(bits),
            output.ty.bits_to_f64(last_bits),
        )
        && (to - from).abs() < deadband
    {
        return hold(Outcome::Suppressed);
    }
    if Applied::Value(bits) != last {
        if ctx
            .last_actuation
            .is_some_and(|t| ctx.now_ns.saturating_sub(t) < output.min_interval_ns)
        {
            return hold(Outcome::Suppressed);
        }
        if ctx.budget_exhausted {
            return hold(Outcome::Rejected(RejectReason::Budget));
        }
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
    if (output.range.is_some()
        || output.range_exprs.is_some()
        || output.rate.is_some()
        || output.safe_ramp.is_some()
        || output.deadband.is_some())
        && !numeric
    {
        return Err(err("J0233"));
    }
    if output.range.is_some() && output.range_exprs.is_some() {
        return Err(err("J0230"));
    }
    if let Some((Bound::Const(low), Bound::Const(high))) = output.range_exprs
        && !(low.is_finite() && high.is_finite() && low < high)
    {
        return Err(err("J0230"));
    }
    if output
        .max_actuations
        .is_some_and(|b| b.count == 0 || b.window_ns == 0)
    {
        return Err(err("J0159"));
    }
    for rate in [output.rate, output.safe_ramp, output.deadband]
        .into_iter()
        .flatten()
    {
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
