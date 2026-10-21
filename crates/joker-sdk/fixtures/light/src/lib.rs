use joker::prelude::*;

/// Blinks an LED. Version 0.1 of the track.
#[joker::app(name = "light", version = "0.1.0")]
pub struct Light {
    /// The LED. The app only knows it is an output bool.
    #[output]
    led: Output<bool>,

    /// Half period of the blink: 500 ms on, 500 ms off.
    #[param(unit = "ms", range = "20..5000", default = 500.0)]
    half_period: Param<f64>,
}

/// Local state of the cycle task: lives between ticks, in the app's memory.
#[derive(Default)]
struct Blink {
    elapsed_ms: f64,
    on: bool,
}

#[joker::cycle(period = "20ms", wcet = "5us")]
fn blink(io: &mut CycleIo<Light>, st: &mut Blink) -> Result<()> {
    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;

    if st.elapsed_ms >= io.half_period.get() {
        st.elapsed_ms -= io.half_period.get();
        st.on = !st.on;
    }

    io.led.set(st.on);
    Ok(())
}
