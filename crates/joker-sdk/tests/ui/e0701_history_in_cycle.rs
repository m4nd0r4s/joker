use joker::prelude::*;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[input(unit = "kW")]
    grid: Input<f64>,
}

#[joker::cycle(period = "200ms")]
fn control(io: &mut CycleIo<Probe>) -> Result<()> {
    let now = io.now();
    let _ = io.grid.history(now - 1.h(), now, 15.mins());
    Ok(())
}

fn main() {}
