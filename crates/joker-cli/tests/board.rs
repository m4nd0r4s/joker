//! `jk board check` / `jk board new` (docs/07, ref/03 §2.3).

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

const RPI5: &str = "schema = 1\n\n[board]\nname    = \"rpi5\"\nversion = \"1.0.0\"\narch    = \"aarch64\"\ntier    = \"1\"\ncpus    = 4\nmemory  = \"8 GiB\"\n\n[gpio]\nchip = \"pinctrl-rp1\"\n";

#[test]
fn checks_a_valid_board_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("rpi5.board.toml"), RPI5).unwrap();
    let out = jk(dir.path(), &["board", "check", "rpi5.board.toml"]);
    assert_eq!(code(&out), 0, "{}", text(&out));
}

#[test]
fn reports_errors_in_a_broken_board_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.board.toml"), "[board]\nname = \"x\"\n").unwrap();
    let out = jk(dir.path(), &["board", "check", "bad.board.toml"]);
    assert_eq!(code(&out), 2, "{}", text(&out));
    assert!(text(&out).contains("J0142"));
}

#[test]
fn board_new_copies_and_renames() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("boards")).unwrap();
    std::fs::write(dir.path().join("boards/rpi5.board.toml"), RPI5).unwrap();
    let out = jk(dir.path(), &["board", "new", "rpi5-lab", "--from", "rpi5"]);
    assert_eq!(code(&out), 0, "{}", text(&out));
    let written = dir.path().join("boards/rpi5-lab.board.toml");
    let content = std::fs::read_to_string(&written).unwrap();
    assert!(content.contains("name    = \"rpi5-lab\""));
    assert!(content.contains("# REVIEW"));

    let check = jk(
        dir.path(),
        &["board", "check", "boards/rpi5-lab.board.toml"],
    );
    assert_eq!(code(&check), 0, "{}", text(&check));
}

#[test]
fn board_new_refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("boards")).unwrap();
    std::fs::write(dir.path().join("boards/rpi5.board.toml"), RPI5).unwrap();
    std::fs::write(dir.path().join("boards/dup.board.toml"), "x").unwrap();
    let out = jk(dir.path(), &["board", "new", "dup", "--from", "rpi5"]);
    assert_eq!(code(&out), 1, "{}", text(&out));
    assert!(text(&out).contains("already exists"));
}
