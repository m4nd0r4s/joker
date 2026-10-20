//! Repository tasks, run with `cargo xtask <task>` (docs/13 §1).
//!
//! - `conformance`: runs the core suites of `conformance/` against
//!   `joker-core` with a virtual clock (docs/13 §3, ref/03 §2.6).

extern crate alloc;

mod conformance;
mod guard;

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
";

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
