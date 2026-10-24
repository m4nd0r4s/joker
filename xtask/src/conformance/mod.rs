//! `cargo xtask conformance`: runs the core suites of `conformance/`
//! against `joker-core` with a virtual clock and compares result by result
//! (docs/13 §3, docs/16 §8, ref/03 §2.6), and the `action` suites of
//! `conformance/actions/` against the daemon with a register double (ADR
//! 0066).

pub mod actions;
pub mod eval;
pub mod exec;
pub mod expr;
pub mod literal;
pub mod load;
pub mod model;
pub mod report;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use exec::TickGuard;
use model::{Case, Diagnostic, Suite};
use report::{CaseReport, Report, Status, SuiteReport};

/// Options of one run.
#[derive(Debug, Default)]
pub struct Options {
    /// Files or directories to run; `conformance/` when empty.
    pub paths: Vec<PathBuf>,
    /// Where to write the JSON report.
    pub json: Option<PathBuf>,
    /// Where to write the Markdown report.
    pub markdown: Option<PathBuf>,
    /// The known-failures file: listed failures do not fail the run.
    pub known_failures: Option<PathBuf>,
    /// Only cases whose full id starts with one of these prefixes.
    pub filters: Vec<String>,
    /// Print every case, not only the failed ones.
    pub verbose: bool,
}

/// Collects the suite files under `path`, sorted.
pub fn discover(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    walk(path, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let entries = fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", path.display()))?;
        let p = entry.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(load::SUFFIX))
        {
            out.push(p);
        }
    }
    Ok(())
}

/// Runs one suite file.
pub fn run_file(
    path: &Path,
    text: &str,
    filters: &[String],
    guard: &impl TickGuard,
) -> SuiteReport {
    if actions::is_action_suite(text) {
        return actions::run_file(path, text, filters);
    }
    let file = path.display().to_string();
    match load::load(path, text) {
        Err(diagnostics) => SuiteReport {
            id: None,
            file,
            diagnostics,
            cases: Vec::new(),
        },
        Ok(suite) => SuiteReport {
            id: Some(suite.id.clone()),
            file,
            diagnostics: Vec::new(),
            cases: run_suite(&suite, filters, guard),
        },
    }
}

/// Whether the case id passes the `--filter` prefixes.
pub fn selected(id: &str, filters: &[String]) -> bool {
    filters.is_empty() || filters.iter().any(|f| id.starts_with(f.as_str()))
}

/// The report of a case before it runs: `skip` when it needs a feature the
/// core lacks or this runner does not drive, `error` when it does not resolve, and `pass` when it is
/// ready to run.
pub fn prepare(suite: &Suite, case: &Case) -> CaseReport {
    let mut spec = case.spec.clone();
    if spec.is_empty() {
        spec.clone_from(&suite.spec);
    }
    let mut report = CaseReport {
        id: format!("{}.{}", suite.id, case.id),
        name: case.name.clone(),
        spec,
        status: Status::Pass,
        known: None,
        missing: case.missing.clone(),
        diagnostics: Vec::new(),
        failures: Vec::new(),
        ticks: case.ticks,
        digest: None,
    };
    if !case.missing.is_empty() {
        report.status = Status::Skip;
        report.diagnostics.clone_from(&case.errors);
    } else if !case.errors.is_empty() {
        report.status = Status::Error;
        report.diagnostics.clone_from(&case.errors);
    }
    report
}

/// Whether a case is ready to run: it needs nothing the core lacks or this runner does not drive and
/// it resolves.
pub fn runnable(case: &Case) -> bool {
    case.missing.is_empty() && case.errors.is_empty()
}

/// Runs every case of a loaded suite.
pub fn run_suite(suite: &Suite, filters: &[String], guard: &impl TickGuard) -> Vec<CaseReport> {
    let mut out = Vec::new();
    for case in &suite.cases {
        let mut report = prepare(suite, case);
        if !selected(&report.id, filters) {
            continue;
        }
        if runnable(case) {
            let before = guard.violations();
            let result = exec::run(suite, case, guard);
            let allocations = guard.violations().saturating_sub(before);
            if allocations > 0 {
                report.diagnostics.push(Diagnostic {
                    code: "R7",
                    location: "tick".to_string(),
                    message: format!(
                        "the tick path made {allocations} allocator calls (docs/10 R7, AGENTS.md §2 rule 6)"
                    ),
                });
            }
            match result {
                Err(e) => {
                    report.status = Status::Error;
                    report.diagnostics.push(Diagnostic {
                        code: e.code,
                        location: "fixture".to_string(),
                        message: e.message.to_string(),
                    });
                }
                Ok(timeline) => {
                    report.digest = Some(eval::sequence_digest(&timeline));
                    report.failures = eval::evaluate(suite, case, &timeline);
                    if allocations > 0 {
                        report.status = Status::Error;
                    } else if !report.failures.is_empty() {
                        report.status = Status::Fail;
                    }
                }
            }
        }
        out.push(report);
    }
    out
}

/// A known failure: a case whose failure is triaged and tracked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    /// Why it fails, with the paragraph of the specification.
    pub reason: String,
    /// The issue that tracks it, if any.
    pub issue: Option<String>,
}

/// Reads the known-failures file: `schema = 1` and `[[known]]` entries
/// with `case` (the full case id), `reason` (the triage, with the paragraph
/// of the specification) and an optional `issue`.
pub fn read_known(path: &Path) -> Result<BTreeMap<String, Known>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("{}: {}", path.display(), e.message()))?;
    if root.get("schema").and_then(toml::Value::as_integer) != Some(1) {
        return Err(format!("{}: `schema = 1` is required", path.display()));
    }
    let mut out = BTreeMap::new();
    let entries = match root.get("known") {
        None => return Ok(out),
        Some(toml::Value::Array(a)) => a,
        Some(_) => return Err(format!("{}: `known` is an array of tables", path.display())),
    };
    for entry in entries {
        let toml::Value::Table(t) = entry else {
            return Err(format!("{}: `known` is an array of tables", path.display()));
        };
        let get = |k: &str| t.get(k).and_then(toml::Value::as_str).map(str::to_string);
        let (Some(case), Some(reason)) = (get("case"), get("reason")) else {
            return Err(format!(
                "{}: each entry has `case` and `reason`",
                path.display()
            ));
        };
        if out
            .insert(
                case.clone(),
                Known {
                    reason,
                    issue: get("issue"),
                },
            )
            .is_some()
        {
            return Err(format!("{}: `{case}` is listed twice", path.display()));
        }
    }
    Ok(out)
}

/// Runs the suites and writes the reports. Returns whether the run passed:
/// no suite or case errors, no failures outside the known list, and no
/// stale entry in that list.
pub fn run(options: &Options, guard: &impl TickGuard) -> Result<bool, String> {
    let paths = if options.paths.is_empty() {
        vec![PathBuf::from("conformance")]
    } else {
        options.paths.clone()
    };
    let mut files = Vec::new();
    for p in &paths {
        files.extend(discover(p)?);
    }
    let known = match &options.known_failures {
        Some(p) => read_known(p)?,
        None => BTreeMap::new(),
    };
    let mut suites = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        suites.push(run_file(&file, &text, &options.filters, guard));
    }
    let report = Report::new(suites, &known, options.filters.is_empty());
    print!("{}", report.text(options.verbose));
    if let Some(p) = &options.json {
        write(p, &report.json())?;
    }
    if let Some(p) = &options.markdown {
        write(p, &report.markdown())?;
    }
    Ok(report.passed())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}
