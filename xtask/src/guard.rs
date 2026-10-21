//! The no-alloc guard of the tick path (docs/09 §5, docs/10 R7, AGENTS.md
//! §2 rule 6): a global allocator that counts every allocation and
//! deallocation made inside a guarded closure on the same thread.
//!
//! It counts instead of aborting, so that a violation becomes a failed case
//! or test with its name, and not a dead process. The guard is installed in
//! every build of `xtask`, debug and release.

use assert_no_alloc::{AllocDisabler, assert_no_alloc, violation_count};

use crate::conformance::exec::TickGuard;

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

/// Runs code that must not allocate, and counts the allocator calls it
/// makes on this thread.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoAlloc;

impl TickGuard for NoAlloc {
    fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        assert_no_alloc(f)
    }

    fn violations(&self) -> u32 {
        violation_count()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::conformance::report::Status;
    use crate::conformance::{discover, run_file};

    #[test]
    fn no_tick_of_the_conformance_corpus_allocates() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance");
        let files = discover(&root).unwrap_or_default();
        assert!(!files.is_empty(), "no suites under {}", root.display());
        let guard = NoAlloc;
        let mut ran = 0;
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let report = run_file(&file, &text, &[], &guard);
            for case in &report.cases {
                assert!(
                    case.diagnostics.iter().all(|d| d.code != "R7"),
                    "{}: {:?}",
                    case.id,
                    case.diagnostics
                );
                ran += usize::from(matches!(case.status, Status::Pass | Status::Fail));
            }
        }
        assert!(ran > 0);
    }

    #[test]
    fn the_guard_counts_allocations_inside_it_only() {
        let guard = NoAlloc;
        let before = guard.violations();
        let outside: Vec<u8> = (0..64).collect();
        assert_eq!(guard.violations(), before);
        let inside = guard.run(|| vec![2u8; 64]);
        assert!(guard.violations() > before);
        assert_eq!(outside.len() + inside.len(), 128);
    }
}
