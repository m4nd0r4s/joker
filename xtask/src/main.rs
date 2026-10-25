//! Repository tasks, run with `cargo xtask <task>` (docs/13 §1).
//!
//! - `conformance`: runs the core suites of `conformance/` against
//!   `joker-core` with a virtual clock (docs/13 §3, ref/03 §2.6);
//! - `rt-bench`: tick jitter and envelope + arbiter cost per output, with
//!   the no-alloc guard on the tick path (docs/13 §4 task 20, AGENTS.md §5);
//! - `qemu`: the same suites on Cortex-M33 and RISC-V in QEMU, with no
//!   allocator linked (docs/13 §5, task 22);
//! - `msrv`: the MSRV of the core crates against the Xtensa toolchain
//!   (docs/13 §5.1);
//! - `determinism`: the core suites run many times, with the same bytes
//!   every time (docs/11 Phase 0 gates 2 and 5, docs/16 §7);
//! - `size`: the binary size gate of `jokerd` (docs/01 §21, docs/13 §5).

extern crate alloc;

mod conformance;
mod determinism;
mod guard;
mod msrv;
mod qemu;
mod rt_bench;
mod size;

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

  qemu --target TRIPLE [PATH...] [--json FILE] [--markdown FILE]
       [--known-failures FILE] [--filter PREFIX]... [--verbose]
       [--qemu BINARY] [--timeout SECONDS]
      Runs the same suites in QEMU: thumbv8m.main-none-eabihf on mps2-an505
      (qemu-system-arm) or riscv32imac-unknown-none-elf on virt
      (qemu-system-riscv32). Embeds the cases in joker-qemu-conformance,
      checks that no allocator is linked, and reports status and sequence
      digest per case, with the exit codes of `conformance` (default
      timeout: 600 s).

  rt-bench [--outputs N] [--period DURATION] [--ticks N] [--iterations N]
           [--json FILE] [--compare BASELINE.json] [--threshold PERCENT]
           [--max-output-ns NS]
      Measures the tick jitter and the envelope + arbiter cost per output
      (defaults: 64 outputs, 1ms, 10000 paced ticks, 20000 cost ticks).
      With --compare, exits 1 when a p99.9 regresses by more than the
      threshold (default 5%); with --max-output-ns, when the median cost
      per output is above NS (Phase 0 gate 4: 300).

  msrv --max VERSION
      Checks that the rust-version of the core crates is at most VERSION,
      the Rust version of the esp toolchain (`rustc +esp --version`).

  determinism [PATH...] [--runs N] [--json FILE]
      Runs the core suites (default: conformance/) N times (default 1000)
      and exits 1 unless every run gives the same result sequence, byte for
      byte. Prints the SHA-256 of the canonical sequence, which CI compares
      between targets.

  size --target TRIPLE [--limits FILE] [--build NAME]... [--json FILE]
       [--compare BASELINE.json] [--top N]
      Measures each build of the limits file (default:
      xtask/size-limits.toml) with `cargo bloat --release --crates`, strips
      the binary and exits 1 when one is above its limit, listing the
      largest crates and, with --compare, those that grew the most
      (default top: 15).
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
            "--max-output-ns" => {
                options.max_output_ns = Some(parse("--max-output-ns", &value("--max-output-ns")?)?);
            }
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    let result = rt_bench::bench(&options)?;
    print!("{}", rt_bench::summary(&result));
    let text = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
    if let Some(path) = &options.json {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, format!("{text}\n"))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Some(max) = options.max_output_ns
        && let Err(e) = rt_bench::check_output_cost(&result, max)
    {
        println!("  gate: {e}");
        return Ok(false);
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

fn qemu(args: &[String]) -> Result<bool, String> {
    let mut options = qemu::Options {
        timeout_s: 600,
        ..qemu::Options::default()
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--target" => options.target = value("--target")?,
            "--json" => options.json = Some(PathBuf::from(value("--json")?)),
            "--markdown" => options.markdown = Some(PathBuf::from(value("--markdown")?)),
            "--known-failures" => {
                options.known_failures = Some(PathBuf::from(value("--known-failures")?));
            }
            "--filter" => options.filters.push(value("--filter")?),
            "--qemu" => options.qemu = Some(value("--qemu")?),
            "--timeout" => options.timeout_s = parse("--timeout", &value("--timeout")?)?,
            "--verbose" | "-v" => options.verbose = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            path => options.paths.push(PathBuf::from(path)),
        }
    }
    if options.target.is_empty() {
        return Err(format!("qemu needs --target TRIPLE\n{USAGE}"));
    }
    qemu::run(&options)
}

fn determinism(args: &[String]) -> Result<bool, String> {
    let mut options = determinism::Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--runs" => options.runs = parse("--runs", &value("--runs")?)?,
            "--json" => options.json = Some(PathBuf::from(value("--json")?)),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            path => options.paths.push(PathBuf::from(path)),
        }
    }
    determinism::run(&options, &NoAlloc)
}

fn size(args: &[String]) -> Result<bool, String> {
    let mut options = size::Options {
        limits: PathBuf::from("xtask/size-limits.toml"),
        top: 15,
        ..size::Options::default()
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--target" => options.target = value("--target")?,
            "--limits" => options.limits = PathBuf::from(value("--limits")?),
            "--build" => options.builds.push(value("--build")?),
            "--json" => options.json = Some(PathBuf::from(value("--json")?)),
            "--compare" => options.compare = Some(PathBuf::from(value("--compare")?)),
            "--top" => options.top = parse("--top", &value("--top")?)?,
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    size::run(&options)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("conformance") => conformance(&args[1..]),
        Some("qemu") => qemu(&args[1..]),
        Some("rt-bench") => rt_bench(&args[1..]),
        Some("determinism") => determinism(&args[1..]),
        Some("size") => size(&args[1..]),
        Some("msrv") => match &args[1..] {
            [flag, max] if flag == "--max" => msrv::run(max),
            _ => Err(format!("msrv needs --max VERSION\n{USAGE}")),
        },
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
