//! `jokerd`: the Joker daemon.

use std::process::ExitCode;

use jokerd::daemon::{Options, run};
use jokerd::native::NativeTasks;

fn main() -> ExitCode {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match run(options, std::sync::Arc::new(NativeTasks)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("jokerd: {e}");
            ExitCode::FAILURE
        }
    }
}
