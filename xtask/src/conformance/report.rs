//! Reports of a run: text for people, JSON for CI (the shape of a
//! `joker.conformance-report.v1+json`, ref/00 §7.1, ref/06 §6.5), and a
//! Markdown summary for pull requests.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Value, json};

use super::Known;
use super::eval::Failure;
use super::model::{Diagnostic, Feature};

/// The result of one case.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    /// Every expectation held.
    Pass,
    /// At least one expectation failed (`J0842`).
    Fail,
    /// The case needs a feature the M0 core lacks.
    Skip,
    /// The case is malformed or its fixture does not build.
    Error,
}

impl Status {
    /// The name used in reports.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Error => "error",
        }
    }
}

/// The report of one case.
#[derive(Clone, Debug)]
pub struct CaseReport {
    /// `<suite id>.<case id>`.
    pub id: String,
    /// Free text of the case.
    pub name: Option<String>,
    /// Paragraphs of the case (those of the suite when it has none).
    pub spec: Vec<String>,
    /// Result.
    pub status: Status,
    /// The triage of a known failure.
    pub known: Option<Known>,
    /// Features the M0 core lacks.
    pub missing: Vec<Feature>,
    /// Problems in the case.
    pub diagnostics: Vec<Diagnostic>,
    /// Failed expectations.
    pub failures: Vec<Failure>,
    /// Ticks the case covered.
    pub ticks: u64,
    /// Digest of the full result sequence, when the case ran.
    pub digest: Option<u64>,
}

/// The report of one suite file.
#[derive(Clone, Debug)]
pub struct SuiteReport {
    /// Suite id, when the file loaded.
    pub id: Option<String>,
    /// The file.
    pub file: String,
    /// Problems that keep the whole suite from running.
    pub diagnostics: Vec<Diagnostic>,
    /// The cases.
    pub cases: Vec<CaseReport>,
}

/// Counts per status.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Passed cases.
    pub pass: usize,
    /// Failed cases.
    pub fail: usize,
    /// Failed cases that are in the known list.
    pub known: usize,
    /// Skipped cases.
    pub skip: usize,
    /// Cases with errors.
    pub error: usize,
}

impl Counts {
    fn add(&mut self, case: &CaseReport) {
        match case.status {
            Status::Pass => self.pass += 1,
            Status::Fail => {
                self.fail += 1;
                if case.known.is_some() {
                    self.known += 1;
                }
            }
            Status::Skip => self.skip += 1,
            Status::Error => self.error += 1,
        }
    }

    fn total(&self) -> usize {
        self.pass + self.fail + self.skip + self.error
    }

    fn json(&self) -> Value {
        json!({
            "total": self.total(),
            "pass": self.pass,
            "fail": self.fail,
            "known_fail": self.known,
            "skip": self.skip,
            "error": self.error,
        })
    }
}

/// The report of a run.
#[derive(Clone, Debug)]
pub struct Report {
    /// The suites, in file order.
    pub suites: Vec<SuiteReport>,
    /// Known-failure entries that did not fail in this run.
    pub stale: Vec<String>,
    /// The MCU target the cases ran on in QEMU (`cargo xtask qemu`), or
    /// `None` on the host.
    pub target: Option<String>,
}

fn suite_counts(s: &SuiteReport) -> Counts {
    let mut c = Counts::default();
    s.cases.iter().for_each(|case| c.add(case));
    c
}

fn features(list: &[Feature]) -> String {
    list.iter()
        .map(|f| format!("{} ({})", f.name, f.spec))
        .collect::<Vec<_>>()
        .join(", ")
}

fn md(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

impl Report {
    /// Applies the known-failures list. `complete` says whether every case
    /// ran, so that a listed case that does not exist is stale.
    pub fn new(
        mut suites: Vec<SuiteReport>,
        known: &BTreeMap<String, Known>,
        complete: bool,
    ) -> Self {
        let mut seen = Vec::new();
        for case in suites.iter_mut().flat_map(|s| s.cases.iter_mut()) {
            if let Some(k) = known.get(&case.id) {
                seen.push(case.id.clone());
                if case.status == Status::Fail {
                    case.known = Some(k.clone());
                }
            }
        }
        let stale = known
            .keys()
            .filter(|id| {
                let ran = seen.contains(id);
                (complete && !ran)
                    || suites
                        .iter()
                        .flat_map(|s| &s.cases)
                        .any(|c| &c.id == *id && c.status != Status::Fail)
            })
            .cloned()
            .collect();
        Self {
            suites,
            stale,
            target: None,
        }
    }

    /// Counts over all suites.
    pub fn totals(&self) -> Counts {
        let mut c = Counts::default();
        self.suites
            .iter()
            .flat_map(|s| &s.cases)
            .for_each(|case| c.add(case));
        c
    }

    /// Whether the run passes the gate.
    pub fn passed(&self) -> bool {
        let t = self.totals();
        self.suites.iter().all(|s| s.diagnostics.is_empty())
            && t.error == 0
            && t.fail == t.known
            && self.stale.is_empty()
    }

    /// The text report.
    pub fn text(&self, verbose: bool) -> String {
        let mut out = String::new();
        for s in &self.suites {
            let id = s.id.as_deref().unwrap_or("?");
            if s.cases.is_empty() && s.diagnostics.is_empty() {
                continue;
            }
            if !s.diagnostics.is_empty() {
                let _ = writeln!(out, "{id:<24} ERROR  {}", s.file);
                for d in &s.diagnostics {
                    let _ = writeln!(out, "    {} {}: {}", d.code, d.location, d.message);
                }
                continue;
            }
            let c = suite_counts(s);
            let _ = writeln!(
                out,
                "{id:<24} pass {:>3}  fail {:>3} (known {:>3})  skip {:>3}  error {:>3}",
                c.pass, c.fail, c.known, c.skip, c.error
            );
            for case in &s.cases {
                let show = verbose
                    || matches!(case.status, Status::Error)
                    || (case.status == Status::Fail && case.known.is_none());
                if !show {
                    continue;
                }
                let mark = match (case.status, &case.known) {
                    (Status::Fail, Some(_)) => "known",
                    (status, _) => status.name(),
                };
                let _ = writeln!(out, "  {mark:<5} {}", case.id);
                if case.status == Status::Skip {
                    let _ = writeln!(
                        out,
                        "        skipped (not in M0): {}",
                        features(&case.missing)
                    );
                }
                for d in &case.diagnostics {
                    let _ = writeln!(out, "        {} {}: {}", d.code, d.location, d.message);
                }
                for f in &case.failures {
                    let _ = writeln!(
                        out,
                        "        J0842 expect[{}] {} at {}\n          expected: {}\n          actual:   {}",
                        f.index, f.subject, f.at, f.expected, f.actual
                    );
                }
            }
        }
        for id in &self.stale {
            let _ = writeln!(out, "stale known failure: {id} (it did not fail)");
        }
        let t = self.totals();
        let _ = writeln!(
            out,
            "total: {} cases, pass {}, fail {} (known {}), skip {}, error {}; suites with errors {}",
            t.total(),
            t.pass,
            t.fail,
            t.known,
            t.skip,
            t.error,
            self.suites
                .iter()
                .filter(|s| !s.diagnostics.is_empty())
                .count()
        );
        out
    }

    /// The JSON report.
    pub fn json(&self) -> String {
        let diag =
            |d: &Diagnostic| json!({"code": d.code, "location": d.location, "message": d.message});
        let suites: Vec<Value> = self
            .suites
            .iter()
            .map(|s| {
                let cases: Vec<Value> = s
                    .cases
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "name": c.name,
                            "status": c.status.name(),
                            "spec": c.spec,
                            "known": c.known.as_ref().map(|k| json!({"reason": k.reason, "issue": k.issue})),
                            "skipped": if c.status == Status::Skip {
                                Some(json!({
                                    "reason": "not in M0",
                                    "features": c.missing.iter().map(|f| json!({"name": f.name, "spec": f.spec})).collect::<Vec<_>>(),
                                }))
                            } else {
                                None
                            },
                            "diagnostics": c.diagnostics.iter().map(diag).collect::<Vec<_>>(),
                            "failures": c.failures.iter().map(|f| json!({
                                "code": "J0842",
                                "expect": f.index,
                                "source": f.source,
                                "tick": f.tick,
                                "at": f.at,
                                "subject": f.subject,
                                "expected": f.expected,
                                "actual": f.actual,
                            })).collect::<Vec<_>>(),
                            "ticks": c.ticks,
                            "sequence_digest": c.digest.map(|d| format!("fnv1a64:{d:016x}")),
                        })
                    })
                    .collect();
                json!({
                    "id": s.id,
                    "file": s.file,
                    "status": if s.diagnostics.is_empty() { "ok" } else { "error" },
                    "diagnostics": s.diagnostics.iter().map(diag).collect::<Vec<_>>(),
                    "counts": suite_counts(s).json(),
                    "cases": cases,
                })
            })
            .collect();
        let report = json!({
            "schema": 1,
            "kind": "core",
            "runner": if self.target.is_some() { "cargo xtask qemu" } else { "cargo xtask conformance" },
            "host": {"arch": std::env::consts::ARCH, "os": std::env::consts::OS},
            "target": self.target,
            "passed": self.passed(),
            "totals": self.totals().json(),
            "stale_known_failures": self.stale,
            "suites": suites,
        });
        let mut text = serde_json::to_string_pretty(&report).unwrap_or_default();
        text.push('\n');
        text
    }

    /// The Markdown summary: counts per suite and a table of failures.
    pub fn markdown(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "| Suite | Pass | Fail | Skip (not in M0) | Error |");
        let _ = writeln!(out, "|---|---:|---:|---:|---:|");
        for s in &self.suites {
            let c = suite_counts(s);
            let id = s.id.as_deref().unwrap_or(&s.file);
            if s.diagnostics.is_empty() {
                let _ = writeln!(
                    out,
                    "| `{id}` | {} | {} | {} | {} |",
                    c.pass, c.fail, c.skip, c.error
                );
            } else {
                let _ = writeln!(out, "| `{id}` | suite error | | | |");
            }
        }
        let t = self.totals();
        let _ = writeln!(
            out,
            "| **total ({})** | **{}** | **{}** | **{}** | **{}** |\n",
            t.total(),
            t.pass,
            t.fail,
            t.skip,
            t.error
        );
        if t.fail == 0 {
            let _ = writeln!(out, "No failed expectation.");
        } else {
            let _ = writeln!(out, "| Case | Expected | Actual | Spec | Triage |");
            let _ = writeln!(out, "|---|---|---|---|---|");
        }
        for case in self.suites.iter().flat_map(|s| &s.cases) {
            for f in &case.failures {
                let triage = case.known.as_ref().map_or(String::new(), |k| md(&k.reason));
                let _ = writeln!(
                    out,
                    "| `{}` | `{}` at {}: {} | {} | {} | {} |",
                    case.id,
                    f.subject,
                    f.at,
                    md(&f.expected),
                    md(&f.actual),
                    md(&case.spec.join(", ")),
                    triage
                );
            }
        }
        let skipped: Vec<&CaseReport> = self
            .suites
            .iter()
            .flat_map(|s| &s.cases)
            .filter(|c| c.status == Status::Skip)
            .collect();
        let mut by_feature: BTreeMap<String, usize> = BTreeMap::new();
        for c in &skipped {
            *by_feature.entry(features(&c.missing)).or_default() += 1;
        }
        let _ = writeln!(out, "\n| Skipped (not in M0): needs | Cases |");
        let _ = writeln!(out, "|---|---:|");
        for (f, n) in by_feature {
            let _ = writeln!(out, "| {} | {n} |", md(&f));
        }
        out
    }
}
