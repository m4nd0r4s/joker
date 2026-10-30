//! A cycle app that drives its output on for a few ticks and then traps,
//! for the tests of the trap report and of the lease that takes its output
//! to `safe` (docs/01 §9, ADR 0067), writing a `warn` log line first with
//! `joker::log::warn!`, for the test of app lines in the node's log ring.

use joker::prelude::*;

/// Drives `led` on, then traps on its eleventh tick.
#[joker::app(name = "crash", version = "0.1.0")]
pub struct Crash {
    /// The output it drives.
    #[output]
    led: Output<bool>,
}

/// The ticks run so far.
#[derive(Default)]
struct Count {
    ticks: u32,
}

#[joker::cycle(period = "20ms", wcet = "2ms")]
fn run(io: &mut CycleIo<Crash>, st: &mut Count) -> Result<()> {
    st.ticks += 1;
    if st.ticks > 10 {
        log::warn!("about to trap"; ticks = st.ticks);
        std::process::abort();
    }
    io.led.set(true);
    Ok(())
}
