//! Test app of the SDK: quality-aware reads, a stateful and a stateless
//! task, and the results of the previous commit.

use joker::prelude::*;

/// Regulates a heater from a temperature and raises an alarm above a limit.
#[joker::app(name = "probe", version = "0.2.0", proposal_buffer = "1 KiB")]
pub struct Probe {
    /// The measured temperature.
    #[input(unit = "°C", quantity = "temperature")]
    temp: Input<f64>,

    /// The alarm limit.
    #[param(unit = "°C", range = "0..90", default = 30.0)]
    limit: Param<f64>,

    /// Raised above the limit.
    #[output]
    alarm: Output<bool>,

    /// Heater power.
    #[output(unit = "%")]
    heater: Output<f64>,
}

/// Ticks seen by `regulate`.
#[derive(Default)]
struct Count {
    ticks: u64,
}

/// Proposes the alarm from a substituted read (never on the first tick),
/// then the heater from a strict one: a temperature that is not good ends the task with `Err`, after the
/// alarm proposal.
#[joker::cycle(period = "100ms", wcet = "50us")]
fn regulate(io: &mut CycleIo<Probe>, st: &mut Count) -> Result<()> {
    st.ticks += 1;
    let t = io.temp.get_or(0.0);
    io.alarm.set(st.ticks > 1 && t > io.limit.get());
    let strict = io.temp.get()?;
    io.heater.set((io.limit.get() - strict).clamp(0.0, 100.0));
    Ok(())
}

/// Fails when the heater proposal of the previous commit was not applied.
#[joker::cycle(period = "100ms")]
fn watch(io: &mut CycleIo<Probe>) -> Result<()> {
    match io.heater.last_outcome() {
        Some(o) if !o.is_applied() => Err(Error::app("the heater proposal was not applied")),
        _ => Ok(()),
    }
}
