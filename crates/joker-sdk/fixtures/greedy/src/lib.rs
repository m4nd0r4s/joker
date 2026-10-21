//! Test app of the SDK: a cycle task whose state grows a `Vec`. The capacity
//! reserved in `Default`, at `init`, can be used; growing past it in a cycle
//! task traps with `J-E0903` (ref/04 §4.13).

use joker::prelude::*;

/// Counts the ticks it has kept.
#[joker::app(name = "greedy", version = "0.1.0")]
pub struct Greedy {
    /// How many ticks are kept.
    #[output(unit = "1")]
    kept: Output<u64>,
}

/// The ticks seen so far, in a buffer of 4 reserved at `init`.
struct Hoard {
    ticks: Vec<u64>,
}

impl Default for Hoard {
    fn default() -> Self {
        Self {
            ticks: Vec::with_capacity(4),
        }
    }
}

/// Keeps every tick: no allocation for the first 4, a trap on the 5th.
#[joker::cycle(period = "20ms")]
fn hoard(io: &mut CycleIo<Greedy>, st: &mut Hoard) -> Result<()> {
    st.ticks.push(io.tick());
    io.kept.set(st.ticks.len() as u64);
    Ok(())
}
