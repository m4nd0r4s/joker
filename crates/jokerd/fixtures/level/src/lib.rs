//! A cycle app that writes its parameter to its output on every tick, for
//! the test of a hot swap: the instance of the old generation and the one
//! of the new give different values, and no tick goes without one (docs/01
//! §9, ADR 0107).

use joker::prelude::*;

/// Writes `level` to `out`.
#[joker::app(name = "level", version = "0.1.0")]
pub struct Steady {
    /// The output it drives.
    #[output(unit = "1")]
    out: Output<f64>,

    /// The value it writes.
    #[param(unit = "1", default = 1.0)]
    level: Param<f64>,
}

#[joker::cycle(period = "10ms", wcet = "1ms")]
fn run(io: &mut CycleIo<Steady>) -> Result<()> {
    io.out.set(io.level.get());
    Ok(())
}
