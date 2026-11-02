use joker::prelude::*;

#[derive(Record, Clone, Copy, Default)]
pub struct Plan {
    limit: f64,
}

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[internal]
    plan: Internal<Plan>,
}

#[joker::cycle(period = "200ms")]
fn control(io: &mut CycleIo<Probe>) -> Result<()> {
    io.plan.set(Plan { limit: 1.0 });
    Ok(())
}

fn main() {}
