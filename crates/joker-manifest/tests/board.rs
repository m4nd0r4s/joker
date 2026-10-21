//! `joker_manifest::board::parse`: loading a `.board.toml` (ref/03 §2.3)
//! into a `BoardInfo` (AUR-414).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::path::Path;

use joker_manifest::board::parse;
use joker_manifest::context::BoardInfo;

fn ok(text: &str) -> BoardInfo {
    let report = parse(Path::new("boards/rpi5.board.toml"), text);
    assert!(
        report.board.is_some(),
        "expected a board, got errors:\n{}",
        report.render()
    );
    report.board.unwrap()
}

#[test]
fn a_linux_board() {
    let b = ok(concat!(
        "[board]\n",
        "name    = \"rpi5\"\n",
        "version = \"1.0.0\"\n",
        "arch    = \"aarch64\"\n",
        "cpus    = 4\n",
        "memory  = \"8 GiB\"\n",
    ));
    assert_eq!(b.majors, vec![1]);
    assert_eq!(b.arch, "aarch64");
    assert!(!b.micro);
    assert_eq!(b.cpus, Some(4));
    assert_eq!(b.memory, Some(8 * (1u64 << 30)));
    assert!(!b.no_wasm);
}

#[test]
fn a_microcontroller_board() {
    let b = ok(concat!(
        "[board]\n",
        "name    = \"rp2350-pico2\"\n",
        "version = \"1.0.0\"\n",
        "os      = \"none\"\n",
        "arch    = \"thumbv8m.main-none-eabihf\"\n",
        "memory  = \"520 KiB\"\n",
    ));
    assert!(b.micro);
    assert_eq!(b.memory, Some(520 * (1u64 << 10)));
    assert_eq!(b.cpus, None);
}

#[test]
fn armv6_has_no_wasm_engine() {
    let b = ok(concat!(
        "[board]\n",
        "name    = \"rpi-zero-w\"\n",
        "version = \"1.0.0\"\n",
        "arch    = \"armv6\"\n",
        "cpus    = 1\n",
        "memory  = \"512 MiB\"\n",
    ));
    assert!(b.no_wasm);
}

/// A case: the code a malformed board file reports, the file and what it
/// shows.
type Case = (&'static str, &'static str, &'static str);

fn cases() -> Vec<Case> {
    vec![
        ("J0142", "", "no `[board]` section"),
        (
            "J0142",
            "[board]\nname = \"x\"\nversion = \"1.0.0\"\n",
            "missing `arch`",
        ),
        (
            "J0142",
            "[board]\nname = \"x\"\narch = \"aarch64\"\n",
            "missing `version`",
        ),
    ]
}

#[test]
fn every_case_reports_its_code() {
    for (code, text, what) in cases() {
        let report = parse(Path::new("x.board.toml"), text);
        assert!(
            report
                .diagnostics
                .sorted(&report.sources)
                .iter()
                .any(|d| d.code == code),
            "{code} ({what}) not reported; got {:?}\n{}",
            report.diagnostics.sorted(&report.sources),
            report.render()
        );
    }
}
