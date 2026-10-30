//! A cycle app that drives its output on for a few ticks and then never
//! returns from its cycle, for the test of the epoch deadline that cuts it
//! without stopping its group (docs/01 §4, ADR 0098).

use joker::prelude::*;

/// Drives `led` on, then loops forever from its eleventh tick.
#[joker::app(name = "spin", version = "0.1.0")]
pub struct Spin {
    /// The output it drives.
    #[output]
    led: Output<bool>,
}

/// The ticks run so far.
#[derive(Default)]
struct Count {
    ticks: u32,
}

#[joker::cycle(period = "20ms", wcet = "50us")]
fn run(io: &mut CycleIo<Spin>, st: &mut Count) -> Result<()> {
    st.ticks += 1;
    if st.ticks > 10 {
        let mut n = 0u64;
        loop {
            n = core::hint::black_box(n.wrapping_add(1));
        }
    }
    io.led.set(true);
    Ok(())
}
