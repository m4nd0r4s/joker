//! `jk check` accepts the bench example of the learning track
//! (`examples/bench`, AUR-185) for the simulator and for the Pi 3, with the
//! `light` app built to a Wasm component by `jk` itself.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/bench")
        .canonicalize()
        .unwrap()
}

fn jk(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jk"))
        .current_dir(bench())
        .args(args)
        .env(
            "CARGO_TARGET_DIR",
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bench-example-cli"),
        )
        .env_remove("JK_NODE")
        .env_remove("JK_ENV")
        .env_remove("JK_MANIFEST")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn the_bench_example_checks_for_the_simulator_and_the_pi() {
    for env in ["sim", "lab"] {
        let check = jk(&["--json", "check", "--env", env]);
        assert_eq!(check.status.code(), Some(0), "{env}: {}", text(&check));
        let v: serde_json::Value = serde_json::from_slice(&check.stdout).unwrap();
        assert_eq!(v["ok"], true, "{env}: {v}");
    }
}
