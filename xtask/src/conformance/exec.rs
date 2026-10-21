//! Runs one case with the executor of `joker-case-runner`, the same one the
//! QEMU firmware runs (docs/13 §5), and keeps the whole timeline for the
//! reports: for every tick, the result of every output and the latched
//! sample of every input (ref/03 §2.6.4).

use alloc::vec::Vec;

use joker_case_runner::{Observer, Script, TickView};
use joker_core::Sample;

pub use joker_case_runner::{OutputTick, SetupError, TickGuard};

use super::model::{Case, Suite};

/// Signals (inputs and outputs) of one suite on the host.
pub const SIGNALS: usize = 256;
/// Outputs of one suite on the host.
pub const OUTPUTS: usize = 128;
/// Writers of one suite on the host.
pub const WRITERS: usize = 64;
/// Capacity of the staging buffer of one tick, in proposals.
pub const STAGING: usize = 256;

/// What the case saw in one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickRecord {
    /// Per output, in the order of [`Suite::outputs`].
    pub outputs: Vec<OutputTick>,
    /// Per input, the latched sample, in the order of [`Suite::signals`].
    pub inputs: Vec<Sample>,
}

impl TickRecord {
    /// The record as the view the expectations take.
    pub fn view(&self) -> TickView<'_> {
        TickView {
            outputs: &self.outputs,
            inputs: &self.inputs,
        }
    }
}

/// The full timeline of a case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    /// One record per tick, from tick 0.
    pub ticks: Vec<TickRecord>,
}

impl Observer for Timeline {
    fn tick(&mut self, _tick: u64, view: &TickView<'_>) {
        self.ticks.push(TickRecord {
            outputs: view.outputs.to_vec(),
            inputs: view.inputs.to_vec(),
        });
    }
}

/// Runs a case and returns its timeline.
pub fn run(suite: &Suite, case: &Case, guard: &impl TickGuard) -> Result<Timeline, SetupError> {
    let view = suite.view();
    let nodes = case.nodes();
    let script = Script {
        ticks: case.ticks,
        actions: &case.actions,
        expects: &nodes,
    };
    let mut timeline = Timeline {
        ticks: Vec::with_capacity(usize::try_from(case.ticks).unwrap_or(0)),
    };
    joker_case_runner::run::<SIGNALS, OUTPUTS, WRITERS, STAGING>(
        &view.fixture(),
        &script,
        guard,
        &mut timeline,
    )?;
    Ok(timeline)
}
