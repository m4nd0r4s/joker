//! Generates the items of every class of `classes/std/` with `jkc`
//! (docs/17 §6) into `$OUT_DIR/classes.rs`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    println!("cargo:rerun-if-changed=../../classes/std");
    let compilation = joker_jkc::stdlib::compile();
    if compilation.has_errors() {
        eprintln!("{}", compilation.render());
        return ExitCode::FAILURE;
    }
    let code = joker_jkc::codegen::rust_package(compilation.classes.values());
    let Some(out) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        eprintln!("OUT_DIR is not set");
        return ExitCode::FAILURE;
    };
    match std::fs::write(out.join("classes.rs"), code) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("writing classes.rs: {e}");
            ExitCode::FAILURE
        }
    }
}
