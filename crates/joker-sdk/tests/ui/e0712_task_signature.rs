use joker::prelude::*;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[output]
    led: Output<bool>,
}

#[joker::task(every = "1s")]
fn blink(io: &TaskIo<Probe>) -> Result<()> {
    io.led.set(true);
    Ok(())
}

fn main() {}
