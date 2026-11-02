use joker::prelude::*;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[output]
    led: Output<bool>,
}

#[joker::cycle(period = "200ms")]
fn blink(io: &CycleIo<Probe>) -> Result<()> {
    io.led.set(true);
    Ok(())
}

fn main() {}
