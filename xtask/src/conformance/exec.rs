//! Runs one case with the executor of `joker-case-runner`, the same one the
//! QEMU firmware runs (docs/13 §5), and keeps the whole timeline for the
//! reports: for every tick, the result of every output and the latched
//! sample of every input (ref/03 §2.6.4).

use alloc::vec::Vec;

use alloc::vec;

use joker_case_runner::{AlarmTick, Observer, Program, Script, TickView};
use joker_core::Sample;

pub use joker_case_runner::{OutputTick, STAGING, SetupError, TickGuard};

use super::model::{Case, Suite};

/// Signals (inputs and outputs) of one suite on the host.
pub const SIGNALS: usize = 256;
/// Outputs of one suite on the host.
pub const OUTPUTS: usize = 128;
/// Writers of one suite on the host.
pub const WRITERS: usize = 64;

/// What the case saw in one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickRecord {
    /// Per output, in the order of [`Suite::outputs`].
    pub outputs: Vec<OutputTick>,
    /// Per input, the latched sample, in the order of [`Suite::signals`].
    pub inputs: Vec<Sample>,
    /// Per derived signal, its sample, in the order of [`Suite::calcs`].
    pub calcs: Vec<Sample>,
    /// Per alarm, its state, in the order of [`Suite::alarms`].
    pub alarms: Vec<AlarmTick>,
    /// Per atomic group, its commit register, in the order of
    /// [`Suite::atomic`].
    pub commits: Vec<Sample>,
}

impl TickRecord {
    /// The record as the view the expectations take.
    pub fn view(&self) -> TickView<'_> {
        TickView {
            outputs: &self.outputs,
            inputs: &self.inputs,
            calcs: &self.calcs,
            alarms: &self.alarms,
            commits: &self.commits,
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
            calcs: view.calcs.to_vec(),
            alarms: view.alarms.to_vec(),
            commits: view.commits.to_vec(),
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
    let fixture = view.fixture();
    let needs = joker_case_runner::needs(&fixture)?;
    let mut programs: Vec<Option<Program>> = vec![None; needs.programs];
    let mut arena = vec![0u64; needs.arena];
    joker_case_runner::run::<SIGNALS, OUTPUTS, WRITERS, STAGING>(
        &fixture,
        &script,
        &mut programs,
        &mut arena,
        guard,
        &mut timeline,
    )?;
    Ok(timeline)
}
