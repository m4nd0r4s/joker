//! Repository tasks, run with `cargo xtask <task>` (docs/13 §1).
//!
//! - `conformance`: runs the core suites of `conformance/` against
//!   `joker-core` with a virtual clock (docs/13 §3, ref/03 §2.6);
//! - `rt-bench`: tick jitter and envelope + arbiter cost per output, with
//!   the no-alloc guard on the tick path (docs/13 §4 task 20, AGENTS.md §5).

extern crate alloc;

mod conformance;
mod guard;
mod rt_bench;

use std::path::PathBuf;
use std::process::ExitCode;

use conformance::Options;
use guard::NoAlloc;

const USAGE: &str = "usage: cargo xtask <task>

tasks:
  conformance [PATH...] [--json FILE] [--markdown FILE] [--known-failures FILE]
              [--filter PREFIX]... [--verbose]
      Runs the core conformance suites (default: conformance/) against
      joker-core with a virtual clock. Exits 1 on a suite or case error, on a
      failure that is not in the known-failures file, or on a stale entry.

  rt-bench [--outputs N] [--period DURATION] [--ticks N] [--iterations N]
           [--json FILE] [--compare BASELINE.json] [--threshold PERCENT]
      Measures the tick jitter and the envelope + arbiter cost per output
      (defaults: 64 outputs, 1ms, 10000 paced ticks, 20000 cost ticks).
      With --compare, exits 1 when a p99.9 regresses by more than the
      threshold (default 5%).
";

fn parse<T: std::str::FromStr>(name: &str, text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("{name}: invalid value {text:?}"))
}

fn rt_bench(args: &[String]) -> Result<bool, String> {
    let mut options = rt_bench::Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--outputs" => options.outputs = parse("--outputs", &value("--outputs")?)?,
            "--period" => {
                let text = value("--period")?;
                options.period_ns = conformance::literal::duration_ns(&text)
                    .map_err(|e| format!("--period: {}", e.message))?;
            }
            "--ticks" => options.ticks = parse("--ticks", &value("--ticks")?)?,
            "--iterations" => options.iterations = parse("--iterations", &value("--iterations")?)?,
            "--json" => options.json = Some(PathBuf::from(value("--json")?)),
            "--compare" => options.compare = Some(PathBuf::from(value("--compare")?)),
            "--threshold" => options.threshold_pct = parse("--threshold", &value("--threshold")?)?,
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    let result = rt_bench::bench(&options)?;
    print!("{}", rt_bench::summary(&result));
    let text = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
    if let Some(path) = &options.json {
        std::fs::write(path, format!("{text}\n"))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let Some(baseline) = &options.compare else {
        return Ok(true);
    };
    let base =
        std::fs::read_to_string(baseline).map_err(|e| format!("{}: {e}", baseline.display()))?;
    let base: serde_json::Value =
        serde_json::from_str(&base).map_err(|e| format!("{}: {e}", baseline.display()))?;
    let regressions = rt_bench::compare(&result, &base, options.threshold_pct)?;
    for r in &regressions {
        println!("  regression: {r}");
    }
    Ok(regressions.is_empty())
}

fn conformance(args: &[String]) -> Result<bool, String> {
    let mut options = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--json" => options.json = Some(PathBuf::from(value("--json")?)),
            "--markdown" => options.markdown = Some(PathBuf::from(value("--markdown")?)),
            "--known-failures" => {
                options.known_failures = Some(PathBuf::from(value("--known-failures")?));
            }
            "--filter" => options.filters.push(value("--filter")?),
            "--verbose" | "-v" => options.verbose = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            path => options.paths.push(PathBuf::from(path)),
        }
    }
    conformance::run(&options, &NoAlloc)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("conformance") => conformance(&args[1..]),
        Some("rt-bench") => rt_bench(&args[1..]),
        Some("help" | "--help" | "-h") | None => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown task `{other}`\n{USAGE}")),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}
