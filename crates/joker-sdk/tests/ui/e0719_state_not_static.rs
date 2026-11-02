use joker::prelude::*;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[output]
    led: Output<bool>,
}

#[derive(Default)]
struct Window<'a> {
    last: Option<&'a f64>,
}

#[joker::cycle(period = "200ms")]
fn blink(io: &mut CycleIo<Probe>, st: &mut Window<'_>) -> Result<()> {
    io.led.set(st.last.is_some());
    Ok(())
}

fn main() {}
