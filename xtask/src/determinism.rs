//! `cargo xtask determinism`: the determinism gate of Phase 0 (docs/11
//! Phase 0 gates 2 and 5, docs/16 §7): the core suites of `conformance/`
//! run many times in one process, and every run must give the same result
//! sequence, byte for byte, as the first. The run's digest, the SHA-256
//! of the canonical list of `<case> <status> <sequence digest>`, is
//! published per target so CI compares targets with one value.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::conformance::exec::TickGuard;
use crate::conformance::model::Suite;
use crate::conformance::{actions, discover, load, run_suite};

/// Options of the gate.
#[derive(Debug)]
pub struct Options {
    /// Files or directories to run; `conformance/` when empty.
    pub paths: Vec<PathBuf>,
    /// How many times every case runs.
    pub runs: usize,
    /// Where to write the JSON result.
    pub json: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            runs: 1000,
            json: None,
        }
    }
}

/// The result of the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Runs made.
    pub runs: usize,
    /// Cases with a sequence digest in every run.
    pub cases: usize,
    /// The canonical list of the first run, one case per line.
    pub canonical: String,
    /// The SHA-256 of the canonical list, in hex.
    pub digest: String,
    /// The first run that differed from the first one, and the first line
    /// where it did, if any.
    pub divergence: Option<(usize, String, String)>,
}

impl Outcome {
    /// Whether every run gave the same bytes.
    pub fn passed(&self) -> bool {
        self.divergence.is_none() && self.cases > 0
    }

    /// The JSON form: runs, cases, the digest and the per-case lines.
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "runs": self.runs,
            "cases": self.cases,
            "digest": format!("sha256:{}", self.digest),
            "deterministic": self.divergence.is_none(),
            "divergence": self.divergence.as_ref().map(|(run, expected, got)| serde_json::json!({
                "run": run,
                "expected": expected,
                "got": got,
            })),
            "sequence": self.canonical.lines().collect::<Vec<_>>(),
        })
    }
}

fn suites(paths: &[PathBuf]) -> Result<Vec<Suite>, String> {
    let mut out = Vec::new();
    for root in paths {
        for file in discover(root)? {
            let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            if actions::is_action_suite(&text) {
                continue;
            }
            let suite = load::load(&file, &text).map_err(|d| {
                let first = d.first().map(|d| d.message.clone()).unwrap_or_default();
                format!("{}: {first}", file.display())
            })?;
            out.push(suite);
        }
    }
    Ok(out)
}

/// One run of every suite, as its canonical list and the number of cases
/// that produced a digest.
pub fn canonical(suites: &[Suite], guard: &impl TickGuard) -> (String, usize) {
    let mut text = String::new();
    let mut cases = 0;
    for suite in suites {
        for report in run_suite(suite, &[], guard) {
            let digest = match report.digest {
                Some(d) => {
                    cases += 1;
                    format!("fnv1a64:{d:016x}")
                }
                None => "-".to_owned(),
            };
            let _ = writeln!(text, "{} {} {digest}", report.id, report.status.name());
        }
    }
    (text, cases)
}

/// Runs the gate.
pub fn gate(options: &Options, guard: &impl TickGuard) -> Result<Outcome, String> {
    if options.runs == 0 {
        return Err("--runs must be at least 1".to_owned());
    }
    let paths = if options.paths.is_empty() {
        vec![PathBuf::from("conformance")]
    } else {
        options.paths.clone()
    };
    let suites = suites(&paths)?;
    let (first, cases) = canonical(&suites, guard);
    let mut divergence = None;
    for run in 2..=options.runs {
        let (text, _) = canonical(&suites, guard);
        if text != first {
            let (expected, got) = first
                .lines()
                .zip(text.lines())
                .find(|(a, b)| a != b)
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
                .unwrap_or_else(|| ("(length)".to_owned(), "(length)".to_owned()));
            divergence = Some((run, expected, got));
            break;
        }
    }
    let digest = Sha256::digest(first.as_bytes())
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
    Ok(Outcome {
        runs: options.runs,
        cases,
        canonical: first,
        digest,
        divergence,
    })
}

/// Runs the gate, prints the summary and writes the JSON. Returns whether
/// it passed.
pub fn run(options: &Options, guard: &impl TickGuard) -> Result<bool, String> {
    let outcome = gate(options, guard)?;
    println!(
        "determinism: {} runs, {} cases, sha256:{}",
        outcome.runs, outcome.cases, outcome.digest
    );
    if let Some((run, expected, got)) = &outcome.divergence {
        println!("  run {run} differs from run 1:\n    expected {expected}\n    got      {got}");
    }
    if outcome.cases == 0 {
        println!("  no case produced a sequence digest");
    }
    if let Some(path) = &options.json {
        write(path, &format!("{:#}\n", outcome.json()))?;
    }
    Ok(outcome.passed())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::NoAlloc;

    #[test]
    fn the_core_suites_give_the_same_bytes_in_every_run() {
        let options = Options {
            paths: vec![PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../conformance"
            ))],
            runs: 3,
            json: None,
        };
        let outcome = gate(&options, &NoAlloc).unwrap_or_else(|e| panic!("{e}"));
        assert!(outcome.passed(), "{:?}", outcome.divergence);
        assert!(outcome.cases >= 150, "only {} cases ran", outcome.cases);
        assert_eq!(outcome.digest.len(), 64);
    }
}
