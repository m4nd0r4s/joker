//! `jk profile import csv` (ref/03 §2.2.19).

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

fn jk(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jk"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn imports_a_csv_register_map_into_a_profile_draft() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("map.csv"),
        "signal,fc,at,type\nled,5,0,bool\n",
    )
    .unwrap();
    let out = jk(
        dir.path(),
        &[
            "profile",
            "import",
            "csv",
            "map.csv",
            "--name",
            "local/bench",
            "--driver",
            "modbus",
        ],
    );
    assert_eq!(code(&out), 0, "{}", text(&out));
    let written = dir.path().join("profiles/local/bench@1.profile.toml");
    assert!(written.exists(), "{}", text(&out));
    let content = std::fs::read_to_string(&written).unwrap();
    assert!(content.contains("schema = 1"));
    assert!(content.contains("\"led\""));
}

#[test]
fn refuses_to_overwrite_an_existing_draft() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("map.csv"), "signal,fc,at\nled,5,0\n").unwrap();
    std::fs::create_dir_all(dir.path().join("profiles/local")).unwrap();
    std::fs::write(dir.path().join("profiles/local/bench@1.profile.toml"), "x").unwrap();
    let out = jk(
        dir.path(),
        &[
            "profile",
            "import",
            "csv",
            "map.csv",
            "--name",
            "local/bench",
            "--driver",
            "modbus",
        ],
    );
    assert_eq!(code(&out), 1, "{}", text(&out));
    assert!(text(&out).contains("already exists"));
}

#[test]
fn a_bad_register_map_exits_2_with_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("map.csv"), "not,a,map\n1,2,3\n").unwrap();
    let out = jk(
        dir.path(),
        &[
            "profile",
            "import",
            "csv",
            "map.csv",
            "--name",
            "local/bench",
            "--driver",
            "modbus",
        ],
    );
    assert_eq!(code(&out), 2, "{}", text(&out));
    assert!(text(&out).contains("J0803"));
    assert!(!dir.path().join("profiles").exists());
}

#[test]
fn a_driver_other_than_modbus_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("map.csv"), "signal,fc,at\nx,3,0\n").unwrap();
    let out = jk(
        dir.path(),
        &[
            "profile",
            "import",
            "csv",
            "map.csv",
            "--name",
            "local/bench",
            "--driver",
            "opcua",
        ],
    );
    assert_eq!(code(&out), 1, "{}", text(&out));
}
