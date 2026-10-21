//! The firmware proper: runs the embedded cases and reports them. The
//! critical section of the core comes from `joker-qemu-sys`.

use joker_qemu_sys as _;

use joker_case_runner::{
    Fixture, Observer, Program, STAGING, Script, SequenceDigest, TickView, Unguarded, Verdict,
};

/// A case of the embedded corpus, over the fixture of its suite.
#[derive(Debug)]
pub struct Case {
    /// The fixture of the suite.
    pub fixture: &'static Fixture<'static>,
    /// What the case does and expects.
    pub script: Script<'static>,
}

mod cases {
    //! The cases written by `cargo xtask qemu`: `CASES`, in the order of the
    //! host report, and the capacities `SIGNALS`, `OUTPUTS`, `WRITERS`,
    //! `NODES`, `PROGRAMS` (compiled expressions) and `ARENA` (words of
    //! expression state), the largest of the corpus.

    use super::Case;

    include!(env!("JOKER_CONFORMANCE_CASES"));
}

use cases::{ARENA, CASES, NODES, OUTPUTS, PROGRAMS, SIGNALS, WRITERS};

struct Run<'a> {
    fixture: &'a Fixture<'a>,
    digest: SequenceDigest,
    verdict: Verdict<'a>,
}

impl Observer for Run<'_> {
    fn tick(&mut self, tick: u64, view: &TickView<'_>) {
        self.digest.tick(tick, view.outputs);
        self.verdict.tick(self.fixture, tick, view);
    }
}

fn run(index: usize, case: &Case) {
    let mut failed = [false; NODES];
    let Some(verdict) = Verdict::new(case.script.expects, &mut failed) else {
        semihosting::println!("case {index} error - J0844");
        return;
    };
    let mut run = Run {
        fixture: case.fixture,
        digest: SequenceDigest::new(),
        verdict,
    };
    let mut programs: [Option<Program>; PROGRAMS] = core::array::from_fn(|_| None);
    let mut arena = [0u64; ARENA];
    let result = joker_case_runner::run::<SIGNALS, OUTPUTS, WRITERS, STAGING>(
        case.fixture,
        &case.script,
        &mut programs,
        &mut arena,
        &Unguarded,
        &mut run,
    );
    match result {
        Err(e) => semihosting::println!("case {index} error - {}", e.code),
        Ok(()) => {
            let status = if run.verdict.failures() == 0 {
                "pass"
            } else {
                "fail"
            };
            semihosting::println!("case {index} {status} {:016x}", run.digest.value());
        }
    }
}

#[cfg_attr(target_arch = "arm", cortex_m_rt::entry)]
#[cfg_attr(target_arch = "riscv32", riscv_rt::entry)]
fn main() -> ! {
    semihosting::println!("joker-conformance {} cases", CASES.len());
    for (index, case) in CASES.iter().enumerate() {
        run(index, case);
    }
    semihosting::println!("joker-conformance end");
    semihosting::process::exit(0)
}
