//! `cargo xtask qemu`: the core suites on Cortex-M33 (`mps2-an505`) and
//! RISC-V (`virt`) in QEMU, the blocking MCU gate of docs/13 §5 (task 22)
//! and docs/11 Phase 0 gates 2 and 3.
//!
//! The run reads the suites as `cargo xtask conformance` does, writes the
//! cases that run as static Rust data, builds `joker-qemu-conformance` for
//! the target with that file, checks that no allocator is linked, runs the
//! firmware in QEMU and turns the lines it prints (status and sequence
//! digest per case) into the same reports as the host. The determinism job
//! of the CI compares those digests with x86_64 and aarch64.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use joker_core::ValueType;
use joker_core::output::{ClockRequirement, OnRange, WriterKind};

use joker_case_runner::Needs;

use crate::conformance::model::{
    Action, ActionKind, Case, Diagnostic, Expected, Item, LimitSpec, Link, Node, Predicates,
    SafeDef, Subject, Suite, When,
};
use crate::conformance::report::{CaseReport, Report, Status, SuiteReport};
use crate::conformance::{self, load};

/// The package of the firmware.
const FIRMWARE: &str = "joker-qemu-conformance";

/// The environment variable that points the firmware build at the cases.
const CASES_ENV: &str = "JOKER_CONFORMANCE_CASES";

/// Symbols that only exist when an allocator is linked (docs/11 Phase 0
/// gate 3).
const ALLOCATOR_SYMBOLS: [&str; 4] = [
    "__rust_alloc",
    "__rg_alloc",
    "__rdl_alloc",
    "__rust_alloc_error_handler",
];

/// Options of one run.
#[derive(Debug, Default)]
pub struct Options {
    /// The target triple: `thumbv8m.main-none-eabihf` or
    /// `riscv32imac-unknown-none-elf`.
    pub target: String,
    /// Files or directories to run; `conformance/` when empty.
    pub paths: Vec<PathBuf>,
    /// Where to write the JSON report.
    pub json: Option<PathBuf>,
    /// Where to write the Markdown report.
    pub markdown: Option<PathBuf>,
    /// The known-failures file.
    pub known_failures: Option<PathBuf>,
    /// Only cases whose full id starts with one of these prefixes.
    pub filters: Vec<String>,
    /// Print every case, not only the failed ones.
    pub verbose: bool,
    /// The QEMU binary, instead of the one of the machine.
    pub qemu: Option<String>,
    /// How long QEMU may run, in seconds.
    pub timeout_s: u64,
}

/// A QEMU machine for a target.
struct Machine {
    qemu: &'static str,
    args: &'static [&'static str],
}

fn machine(target: &str) -> Result<Machine, String> {
    match target {
        "thumbv8m.main-none-eabihf" => Ok(Machine {
            qemu: "qemu-system-arm",
            args: &["-machine", "mps2-an505", "-cpu", "cortex-m33"],
        }),
        "riscv32imac-unknown-none-elf" => Ok(Machine {
            qemu: "qemu-system-riscv32",
            args: &["-machine", "virt", "-bios", "none"],
        }),
        other => Err(format!(
            "no QEMU machine for {other}: use thumbv8m.main-none-eabihf or riscv32imac-unknown-none-elf"
        )),
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), PathBuf::from)
}

fn f64_lit(x: f64) -> String {
    format!("f64::from_bits(0x{:016x})", x.to_bits())
}

fn opt<T>(value: Option<T>, f: impl FnOnce(T) -> String) -> String {
    value.map_or_else(|| "None".to_string(), |v| format!("Some({})", f(v)))
}

fn ty_lit(ty: ValueType) -> String {
    format!("c::ValueType::{ty:?}")
}

fn on_range_lit(on_range: OnRange) -> String {
    format!("c::output::OnRange::{on_range:?}")
}

fn kind_lit(kind: WriterKind) -> String {
    format!("c::WriterKind::{kind:?}")
}

fn clock_requirement_lit(requirement: ClockRequirement) -> String {
    format!("c::output::ClockRequirement::{requirement:?}")
}

fn expected_lit(e: Expected) -> String {
    match e {
        Expected::Number(x) => format!("m::Expected::Number({})", f64_lit(x)),
        Expected::Integer(i) => format!("m::Expected::Integer({i}i128)"),
        Expected::Bool(b) => format!("m::Expected::Bool({b})"),
        Expected::Relinquish => "m::Expected::Relinquish".to_string(),
    }
}

fn quality_lit(code: u8, reason: Option<u8>) -> String {
    format!(
        "m::QualityLit {{ code: {code}, reason: {} }}",
        opt(reason, |r| r.to_string())
    )
}

fn subject_lit(subject: Subject) -> String {
    match subject {
        Subject::Output(i) => format!("m::Subject::Output({i})"),
        Subject::Input(i) => format!("m::Subject::Input({i})"),
        Subject::OutputSignal(i) => format!("m::Subject::OutputSignal({i})"),
        Subject::Calc(i) => format!("m::Subject::Calc({i})"),
        Subject::Alarm(i) => format!("m::Subject::Alarm({i})"),
        Subject::Commit(i) => format!("m::Subject::Commit({i})"),
    }
}

fn when_lit(when: When) -> String {
    match when {
        When::At(t) => format!("m::When::At({t})"),
        When::During(a, b) => format!("m::When::During({a}, {b})"),
    }
}

fn predicates_lit(p: &Predicates) -> String {
    format!(
        "m::Predicates {{ equals: {}, min: {}, max: {}, tolerance: {}, quality: {}, outcome: {}, level: {}, state: {}, active: {} }}",
        opt(p.equals, expected_lit),
        opt(p.min, f64_lit),
        opt(p.max, f64_lit),
        opt(p.tolerance, f64_lit),
        opt(p.quality, |q| quality_lit(q.code, q.reason)),
        opt(p.outcome, |o| format!(
            "m::OutcomeLit {{ code: {}, detail: {} }}",
            o.code,
            opt(o.detail, |d| d.to_string())
        )),
        opt(p.level, |l| opt(l, |n| n.to_string())),
        opt(p.state, |s| s.to_string()),
        opt(p.active, |a| a.to_string()),
    )
}

fn node_lit(node: &Node) -> String {
    match node {
        Node::Check {
            when,
            subject,
            predicates,
        } => format!(
            "m::Node::Check {{ when: {}, subject: {}, predicates: {} }}",
            when_lit(*when),
            subject_lit(*subject),
            predicates_lit(predicates)
        ),
        Node::AnyOf(n) => format!("m::Node::AnyOf({n})"),
        Node::AllOf(n) => format!("m::Node::AllOf({n})"),
    }
}

fn action_lit(action: &Action) -> String {
    let kind = match action.kind {
        ActionKind::Input {
            signal,
            bits,
            quality,
        } => format!(
            "m::ActionKind::Input {{ signal: {signal}, bits: 0x{bits:016x}, quality: {} }}",
            quality_lit(quality.code, quality.reason)
        ),
        ActionKind::Propose {
            output,
            writer,
            bits,
        } => format!(
            "m::ActionKind::Propose {{ output: {output}, writer: {writer}, bits: 0x{bits:016x} }}"
        ),
        ActionKind::Release { output, writer } => {
            format!("m::ActionKind::Release {{ output: {output}, writer: {writer} }}")
        }
        ActionKind::Reset { output } => format!("m::ActionKind::Reset {{ output: {output} }}"),
        ActionKind::Clock(q) => format!("m::ActionKind::Clock(c::ClockQuality::{q:?})"),
        ActionKind::Generation => "m::ActionKind::Generation".to_string(),
    };
    format!("m::Action {{ tick: {}, kind: {kind} }}", action.tick)
}

fn limit_lit(limit: LimitSpec<'_>) -> String {
    match limit {
        LimitSpec::Const(x) => format!("m::LimitSpec::Const({})", f64_lit(x)),
        LimitSpec::Expr(src) => format!("m::LimitSpec::Expr({src:?})"),
    }
}

fn links_lit(links: &[Link<'_>]) -> String {
    let items: Vec<String> = links
        .iter()
        .map(|l| {
            let item = match l.item {
                Item::Path(p) => format!("m::Item::Path({p:?})"),
                Item::Output(o) => format!("m::Item::Output({o})"),
            };
            format!("m::Link {{ owner: {}, item: {item} }}", l.owner)
        })
        .collect();
    format!("&[{}]", items.join(", "))
}

fn fixture_lit(suite: &Suite) -> String {
    let view = suite.view();
    let fixture = view.fixture();
    let mut out = String::new();
    let _ = write!(
        out,
        "m::Fixture {{\n    period_ns: {},\n    signals: &[\n",
        fixture.period_ns
    );
    for s in fixture.signals {
        let _ = writeln!(
            out,
            "        m::SignalSpec {{ path: {:?}, ty: {}, unit: {:?}, max_age_ns: {} }},",
            s.path,
            ty_lit(s.ty),
            s.unit,
            opt(s.max_age_ns, |n| n.to_string())
        );
    }
    out.push_str("    ],\n    outputs: &[\n");
    for o in fixture.outputs {
        let safe = match o.safe {
            SafeDef::Value(bits) => format!("m::SafeDef::Value(0x{bits:016x})"),
            SafeDef::Relinquish => "m::SafeDef::Relinquish".to_string(),
        };
        let _ = writeln!(
            out,
            "        m::OutputSpec {{ path: {:?}, ty: {}, unit: {:?}, safe: {safe}, safe_ramp: {}, range: {}, on_range: {}, rate: {}, max_lease_ns: {}, max_manual_ns: {}, interlock: {}, permit_uncertain: {}, on_interlock: c::output::OnInterlock::{:?}, grace_ns: {}, reset_when: {}, deadband: {}, min_interval_ns: {}, max_actuations: {} }},",
            o.path,
            ty_lit(o.ty),
            o.unit,
            opt(o.safe_ramp, f64_lit),
            opt(o.range, |(a, b)| format!(
                "({}, {})",
                limit_lit(a),
                limit_lit(b)
            )),
            on_range_lit(o.on_range),
            opt(o.rate, f64_lit),
            opt(o.max_lease_ns, |n| n.to_string()),
            opt(o.max_manual_ns, |n| n.to_string()),
            opt(o.interlock, |s| format!("{s:?}")),
            o.permit_uncertain,
            o.on_interlock,
            o.grace_ns,
            opt(o.reset_when, |s| format!("{s:?}")),
            opt(o.deadband, f64_lit),
            o.min_interval_ns,
            opt(o.max_actuations, |(n, w)| format!("({n}, {w})")),
        );
    }
    out.push_str("    ],\n    writers: &[\n");
    for w in fixture.writers {
        let _ = writeln!(
            out,
            "        m::WriterSpec {{ level: {}, kind: {}, lease_ns: {}, requires_clock: {} }},",
            w.level,
            kind_lit(w.kind),
            opt(w.lease_ns, |n| n.to_string()),
            clock_requirement_lit(w.requires_clock),
        );
    }
    out.push_str("    ],\n    calcs: &[\n");
    for c in fixture.calcs {
        let _ = writeln!(
            out,
            "        m::CalcSpec {{ path: {:?}, expr: {:?}, ty: {}, unit: {:?} }},",
            c.path,
            c.expr,
            ty_lit(c.ty),
            c.unit
        );
    }
    out.push_str("    ],\n    alarms: &[\n");
    for a in fixture.alarms {
        let _ = writeln!(
            out,
            "        m::AlarmSpec {{ state_path: {:?}, active_path: {:?}, when: {:?}, suppress_when: {}, severity: c::alarm::Severity::{:?}, delay_on_ns: {}, delay_off_ns: {}, max_shelve_ns: {} }},",
            a.state_path,
            a.active_path,
            a.when,
            opt(a.suppress_when, |s| format!("{s:?}")),
            a.severity,
            a.delay_on_ns,
            a.delay_off_ns,
            opt(a.max_shelve_ns, |n| n.to_string()),
        );
    }
    out.push_str("    ],\n    atomic: &[\n");
    for g in fixture.atomic {
        let _ = writeln!(
            out,
            "        m::AtomicSpec {{ on_rate: c::output::OnRate::{:?}, commit: {} }},",
            g.on_rate,
            opt(g.commit, |c| format!(
                "m::CommitSpec {{ path: {:?}, ty: {}, value: 0x{:016x} }}",
                c.path,
                ty_lit(c.ty),
                c.value
            )),
        );
    }
    let _ = write!(
        out,
        "    ],\n    requires_good: {},\n    trips: {},\n    members: {},\n    order: {},\n}}",
        links_lit(fixture.requires_good),
        links_lit(fixture.trips),
        links_lit(fixture.members),
        links_lit(fixture.order),
    );
    out
}

/// Writes the cases to run as the Rust source the firmware includes: one
/// `static` fixture per suite, `CASES` in the order given, and the
/// capacities the firmware needs (the largest of the corpus, at least 1).
pub fn emit(cases: &[(usize, &Suite, &Case)], suites: &[&Suite]) -> String {
    let mut out = String::from(
        "// @generated by `cargo xtask qemu` from conformance/: do not edit.\n\n\
         use joker_case_runner::model as m;\nuse joker_core as c;\n\n",
    );
    let used: Vec<usize> = {
        let mut used: Vec<usize> = cases.iter().map(|(i, _, _)| *i).collect();
        used.sort_unstable();
        used.dedup();
        used
    };
    let signals = used
        .iter()
        .filter_map(|i| suites.get(*i))
        .map(|s| {
            s.signals.len()
                + s.outputs.len()
                + s.calcs.len()
                + 2 * s.alarms.len()
                + s.atomic.iter().filter(|g| g.commit.is_some()).count()
        })
        .max();
    let needs: Vec<Needs> = used
        .iter()
        .filter_map(|i| suites.get(*i))
        .filter_map(|s| joker_case_runner::needs(&s.view().fixture()).ok())
        .collect();
    let programs = needs.iter().map(|n| n.programs).max();
    let arena = needs.iter().map(|n| n.arena).max();
    let outputs = used
        .iter()
        .filter_map(|i| suites.get(*i))
        .map(|s| s.outputs.len())
        .max();
    let writers = used
        .iter()
        .filter_map(|i| suites.get(*i))
        .map(|s| s.writers.len())
        .max();
    let all_nodes: Vec<Vec<Node>> = cases.iter().map(|(_, _, c)| c.nodes()).collect();
    let nodes = all_nodes.iter().map(Vec::len).max();
    for (name, value) in [
        ("SIGNALS", signals),
        ("OUTPUTS", outputs),
        ("WRITERS", writers),
        ("NODES", nodes),
        ("PROGRAMS", programs),
        ("ARENA", arena),
    ] {
        let _ = writeln!(
            out,
            "pub const {name}: usize = {};",
            value.unwrap_or(0).max(1)
        );
    }
    out.push('\n');
    for (i, suite) in used.iter().filter_map(|i| Some((*i, suites.get(*i)?))) {
        let _ = writeln!(
            out,
            "static FIXTURE_{i}: m::Fixture<'static> = {};\n",
            fixture_lit(suite)
        );
    }
    out.push_str("pub static CASES: &[Case] = &[\n");
    for ((fixture, _, case), nodes) in cases.iter().zip(&all_nodes) {
        let _ = writeln!(
            out,
            "    Case {{\n        fixture: &FIXTURE_{fixture},\n        script: m::Script {{\n            ticks: {},\n            actions: &[",
            case.ticks
        );
        for action in &case.actions {
            let _ = writeln!(out, "                {},", action_lit(action));
        }
        out.push_str("            ],\n            expects: &[\n");
        for node in nodes {
            let _ = writeln!(out, "                {},", node_lit(node));
        }
        out.push_str("            ],\n        },\n    },\n");
    }
    out.push_str("];\n");
    out
}

/// What the firmware reported for one case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// Status.
    pub status: Status,
    /// The sequence digest, when the case ran.
    pub digest: Option<u64>,
    /// The code of a setup error.
    pub code: Option<String>,
}

/// Parses the output of the firmware: `case <index> <status> <digest>
/// [code]` per case, between the `joker-conformance <n> cases` and
/// `joker-conformance end` lines.
pub fn parse(output: &str, expected: usize) -> Result<BTreeMap<usize, Line>, String> {
    let mut lines = BTreeMap::new();
    let mut header = None;
    let mut ended = false;
    for line in output.lines().map(str::trim) {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["joker-conformance", n, "cases"] => header = n.parse::<usize>().ok(),
            ["joker-conformance", "end"] => ended = true,
            ["case", index, status, digest, rest @ ..] => {
                let index: usize = index
                    .parse()
                    .map_err(|_| format!("bad case line `{line}`"))?;
                let status = match *status {
                    "pass" => Status::Pass,
                    "fail" => Status::Fail,
                    "error" => Status::Error,
                    _ => return Err(format!("bad status in `{line}`")),
                };
                let digest = match *digest {
                    "-" => None,
                    hex => Some(
                        u64::from_str_radix(hex, 16)
                            .map_err(|_| format!("bad digest in `{line}`"))?,
                    ),
                };
                let code = rest.first().map(|c| (*c).to_string());
                lines.insert(
                    index,
                    Line {
                        status,
                        digest,
                        code,
                    },
                );
            }
            _ => {}
        }
    }
    if header != Some(expected) {
        return Err(format!(
            "the firmware announced {header:?} cases, {expected} were embedded"
        ));
    }
    if !ended {
        return Err("the firmware stopped before the end of the corpus".to_string());
    }
    Ok(lines)
}

/// Whether the ELF file links an allocator (docs/11 Phase 0 gate 3).
pub fn allocator_symbols(elf: &[u8]) -> Vec<&'static str> {
    ALLOCATOR_SYMBOLS
        .iter()
        .copied()
        .filter(|s| elf.windows(s.len()).any(|w| w == s.as_bytes()))
        .collect()
}

fn build(target: &str, cases: &Path) -> Result<PathBuf, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .current_dir(root())
        .args(["build", "--release", "--locked", "--package", FIRMWARE])
        .args(["--target", target])
        .env(CASES_ENV, cases)
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err(format!("the firmware does not build for {target}"));
    }
    Ok(target_dir().join(target).join("release").join(FIRMWARE))
}

fn run_qemu(
    machine: &Machine,
    qemu: &str,
    elf: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let mut child = Command::new(qemu)
        .args(machine.args)
        .args(["-display", "none", "-serial", "none", "-monitor", "none"])
        .args(["-semihosting-config", "enable=on,target=native"])
        .arg("-kernel")
        .arg(elf)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("{qemu}: {e}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{qemu}: no stdout"))?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).map(|_| text)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("{qemu}: {e}"))? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{qemu} did not finish in {}s", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let text = reader
        .join()
        .map_err(|_| format!("{qemu}: the output reader failed"))?
        .map_err(|e| format!("{qemu}: {e}"))?;
    if !status.success() {
        return Err(format!(
            "{qemu} exited with {status} (a panic in the firmware exits with 1)\n{text}"
        ));
    }
    Ok(text)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs the suites in QEMU and writes the reports. Returns whether the run
/// passed, with the same rules as `cargo xtask conformance`.
pub fn run(options: &Options) -> Result<bool, String> {
    let machine = machine(&options.target)?;
    let paths = if options.paths.is_empty() {
        vec![root().join("conformance")]
    } else {
        options.paths.clone()
    };
    let mut files = Vec::new();
    for p in &paths {
        files.extend(conformance::discover(p)?);
    }
    let known = match &options.known_failures {
        Some(p) => conformance::read_known(p)?,
        None => BTreeMap::new(),
    };
    let mut loaded = Vec::new();
    for file in files {
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let display = file
            .strip_prefix(root())
            .unwrap_or(&file)
            .display()
            .to_string();
        loaded.push((display, load::load(&file, &text)));
    }
    let suites: Vec<&Suite> = loaded.iter().filter_map(|(_, s)| s.as_ref().ok()).collect();
    let mut embedded = Vec::new();
    for (i, suite) in suites.iter().enumerate() {
        for case in &suite.cases {
            let id = format!("{}.{}", suite.id, case.id);
            if conformance::selected(&id, &options.filters) && conformance::runnable(case) {
                embedded.push((i, *suite, case));
            }
        }
    }
    let dir = target_dir().join("qemu-conformance");
    let cases = dir.join("cases.rs");
    write(&cases, &emit(&embedded, &suites))?;
    let elf = build(&options.target, &cases)?;
    let bytes = std::fs::read(&elf).map_err(|e| format!("{}: {e}", elf.display()))?;
    let linked = allocator_symbols(&bytes);
    if !linked.is_empty() {
        return Err(format!(
            "{}: an allocator is linked ({}), docs/11 Phase 0 gate 3",
            elf.display(),
            linked.join(", ")
        ));
    }
    println!(
        "{}: {} cases, {} bytes, no allocator linked",
        options.target,
        embedded.len(),
        bytes.len()
    );
    let qemu = options.qemu.as_deref().unwrap_or(machine.qemu);
    let output = run_qemu(&machine, qemu, &elf, Duration::from_secs(options.timeout_s))?;
    let lines = parse(&output, embedded.len())?;
    let mut index = 0;
    let mut reports = Vec::new();
    for (file, suite) in &loaded {
        let suite = match suite {
            Err(diagnostics) => {
                reports.push(SuiteReport {
                    id: None,
                    file: file.clone(),
                    diagnostics: diagnostics.clone(),
                    cases: Vec::new(),
                });
                continue;
            }
            Ok(suite) => suite,
        };
        let mut cases = Vec::new();
        for case in &suite.cases {
            let mut report: CaseReport = conformance::prepare(suite, case);
            if !conformance::selected(&report.id, &options.filters) {
                continue;
            }
            if conformance::runnable(case) {
                match lines.get(&index) {
                    None => {
                        report.status = Status::Error;
                        report.diagnostics.push(Diagnostic {
                            code: "J0844",
                            location: "firmware".to_string(),
                            message: "the firmware did not report the case".to_string(),
                        });
                    }
                    Some(line) => {
                        report.status = line.status;
                        report.digest = line.digest;
                        if let Some(code) = &line.code {
                            report.diagnostics.push(Diagnostic {
                                code: "J0844",
                                location: "fixture".to_string(),
                                message: format!("the core refused the fixture with {code}"),
                            });
                        }
                    }
                }
                index += 1;
            }
            cases.push(report);
        }
        reports.push(SuiteReport {
            id: Some(suite.id.clone()),
            file: file.clone(),
            diagnostics: Vec::new(),
            cases,
        });
    }
    let mut report = Report::new(reports, &known, options.filters.is_empty());
    report.target = Some(options.target.clone());
    print!("{}", report.text(options.verbose));
    if let Some(p) = &options.json {
        write(p, &report.json())?;
    }
    if let Some(p) = &options.markdown {
        write(p, &report.markdown())?;
    }
    Ok(report.passed())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_firmware_output_is_parsed_case_by_case() {
        let text = "joker-conformance 3 cases\ncase 0 pass 00000000000000ff\n\
                    case 1 fail 0000000000000001\ncase 2 error - J0203\njoker-conformance end\n";
        let lines = parse(text, 3).unwrap();
        assert_eq!(lines[&0].digest, Some(0xff));
        assert_eq!(lines[&1].status, Status::Fail);
        assert_eq!(
            (lines[&2].status, lines[&2].code.as_deref()),
            (Status::Error, Some("J0203"))
        );
        assert!(parse(text, 4).is_err());
        assert!(parse("joker-conformance 1 cases\ncase 0 pass 01\n", 1).is_err());
    }

    #[test]
    fn allocator_symbols_are_found_in_the_image() {
        assert_eq!(
            allocator_symbols(b"\0main\0__rust_alloc\0"),
            ["__rust_alloc"]
        );
        assert!(allocator_symbols(b"\0main\0").is_empty());
    }

    #[test]
    fn every_machine_has_a_qemu() {
        assert!(machine("thumbv8m.main-none-eabihf").is_ok());
        assert!(machine("riscv32imac-unknown-none-elf").is_ok());
        assert!(machine("x86_64-unknown-linux-gnu").is_err());
    }
}
