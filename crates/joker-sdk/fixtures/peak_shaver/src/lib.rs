//! `peak_shaver`, the parity app of ref/05 §20.1 (Rust column), as written
//! there: a cycle task that shaves the grid import with the battery, an
//! async task that plans from the historian and one that watches the state
//! of charge.

#![allow(
    missing_debug_implementations,
    reason = "the parity app is kept as ref/05 §20.1 writes it"
)]

use joker::prelude::*;

#[derive(Record, Clone, Copy, Default)]
pub struct Plan {
    target_soc: f64,
    limit_kw: f64,
    valid_until: UtcSeconds,
}

impl Plan {
    fn from_peak(peak_kw: f64, limit_kw: f64, now: Instant) -> Self {
        let excess = (peak_kw - limit_kw).max(0.0);
        let target = (0.3 + excess / 500.0).clamp(0.3, 0.95);
        Plan {
            target_soc: target,
            limit_kw,
            valid_until: now.utc_seconds() + 30 * 60,
        }
    }
}

#[derive(joker::Event)]
pub struct LowSoc {
    pub soc: f64,
}

#[joker::app(name = "peak_shaver", version = "1.4.2")]
pub struct PeakShaver {
    #[input(unit = "kW", quantity = "active_power")]
    grid_power: Input<f64>,
    #[input(unit = "1")]
    soc: Input<f64>,
    #[output(unit = "kW", quantity = "active_power")]
    battery: Output<f64>,
    #[param(unit = "kW", range = "0..5000", default = 400.0)]
    limit: Param<f64>,
    #[param(unit = "%", range = "0..50", default = 10.0)]
    reserve: Param<f64>,
    #[internal]
    plan: Internal<Plan>,
    #[alarm(severity = "medium", message = "plan expired")]
    plan_stale: Alarm,
    #[output(event)]
    low_soc: EventOutput<LowSoc>,
}

#[derive(Default)]
struct Ctl {
    integral: f64,
}

#[joker::cycle(period = "200ms", wcet = "40us")]
fn control(io: &mut CycleIo<PeakShaver>, st: &mut Ctl) -> Result<()> {
    let grid = io.grid_power.get()?;
    let soc = io.soc.get()?;
    let plan = io.plan.get_or_default();
    let stale = plan.valid_until < io.now().utc_seconds();
    io.plan_stale.set(stale);
    let limit = if stale {
        io.limit.get()
    } else {
        plan.limit_kw.min(io.limit.get())
    };
    let excess = grid - limit;
    st.integral = (st.integral + excess * io.dt().as_secs_f64()).clamp(-50.0, 50.0);
    let floor = io.reserve.get() / 100.0;
    io.battery.set(if soc <= floor {
        0.0
    } else {
        (excess + 0.1 * st.integral).max(0.0)
    });
    Ok(())
}

#[joker::task(every = "15m", jitter = "30s")]
async fn replan(io: &TaskIo<PeakShaver>) -> Result<()> {
    let now = io.now();
    let hist = io.grid_power.history(now - 24.h(), now, 15.mins()).await?;
    let peak = hist
        .iter()
        .filter_map(|s| s.good().copied())
        .fold(0.0_f64, f64::max);
    io.plan.set(Plan::from_peak(peak, io.limit.get(), now));
    Ok(())
}

#[joker::task(on = "soc")]
async fn watch_soc(io: &TaskIo<PeakShaver>, s: Sample<f64>) -> Result<()> {
    if let Some(&v) = s.good()
        && v < 0.05
    {
        io.low_soc.emit(LowSoc { soc: v });
    }
    Ok(())
}
