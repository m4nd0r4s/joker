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

/// The repo's real `boards/*.board.toml` files (AUR-412) parse cleanly.
#[test]
fn the_real_project_board_files_parse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../boards");
    let entries = std::fs::read_dir(&root).unwrap();
    let mut checked = 0;
    for entry in entries {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let report = parse(&path, &text);
        assert!(
            report.board.is_some(),
            "{}: {}",
            path.display(),
            report.render()
        );
        checked += 1;
    }
    assert!(
        checked >= 2,
        "expected at least the rpi5 and revpi-connect-5 boards, found {checked}"
    );
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
fn new_from_renames_and_marks_tier_review() {
    let source = "schema = 1\n\n[board]\nname    = \"rpi5\"\nversion = \"1.0.0\"\narch    = \"aarch64\"\ntier    = \"1\"\ncpus    = 4\nmemory  = \"8 GiB\"\n\n[gpio]\nchip = \"pinctrl-rp1\"\n";
    let out = joker_manifest::board::new_from(source, "rpi-zero-w");
    assert!(out.contains("name    = \"rpi-zero-w\""));
    assert!(out.contains("version = \"1.0.0\""));
    assert!(out.contains("tier    = \"dev\""), "{out}");
    assert!(out.contains("# REVIEW"), "{out}");
    assert!(out.contains("arch    = \"aarch64\""), "untouched keys stay");
    assert!(
        out.contains("[gpio]\nchip = \"pinctrl-rp1\""),
        "other sections stay"
    );
}

#[test]
fn new_from_adds_a_tier_key_when_the_source_has_none() {
    let source = "[board]\nname = \"x\"\nversion = \"2.0.0\"\n";
    let out = joker_manifest::board::new_from(source, "y");
    assert!(out.contains("name    = \"y\""), "{out}");
    assert!(out.contains("tier"));
    assert!(out.contains("# REVIEW"));
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

/// The real `rpi5.board.toml` (ref/03 §2.3.4) exercises `storage`,
/// `[watchdog]`, `[gpio]`, `[i2c]` and `[serial]` (`jk board show`,
/// AUR-279).
#[test]
fn the_real_rpi5_board_s_peripherals_parse() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../boards/rpi5.board.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let report = parse(&path, &text);
    let p = report.peripherals;
    assert_eq!(p.storage, Some("ssd".to_owned()));
    let w = p.watchdog.expect("watchdog");
    assert_eq!(w.device, Some("/dev/watchdog0".to_owned()));
    assert_eq!(w.timeout, Some("15s".to_owned()));
    let g = p.gpio.expect("gpio");
    assert_eq!(g.chip, Some("pinctrl-rp1".to_owned()));
    assert_eq!(p.i2c.get("i2c1"), Some(&"/dev/i2c-1".to_owned()));
    let serial = p.serial.get("uart0").expect("uart0");
    assert_eq!(serial.path, "/dev/ttyAMA0");
    assert!(!serial.rs485);
}

/// `[io.<name>]` (ref/03 §2.3.2): `driver`/`direction`/`fallback` are
/// pulled out, everything else (the driver family's own address) is
/// kept in `extra`.
#[test]
fn an_io_point_carries_its_family_address_in_extra() {
    let text = concat!(
        "[board]\n",
        "name    = \"x\"\n",
        "version = \"1.0.0\"\n",
        "arch    = \"aarch64\"\n",
        "\n",
        "[io.act_led]\n",
        "driver    = \"gpio\"\n",
        "direction = \"output\"\n",
        "fallback  = \"false\"\n",
        "line      = 17\n",
    );
    let report = parse(Path::new("x.board.toml"), text);
    let led = report.peripherals.io.get("act_led").expect("act_led");
    assert_eq!(led.driver, Some("gpio".to_owned()));
    assert_eq!(led.direction, Some("output".to_owned()));
    assert_eq!(led.fallback, Some("false".to_owned()));
    assert_eq!(led.extra.get("line"), Some(&"17".to_owned()));
    assert!(!led.extra.contains_key("driver"));
    assert_eq!(led.extra.len(), 1);
}

/// `[serial]` also accepts the `{ path, rs485, max_baud }` table form
/// (ref/03 §2.3.2), not just a bare path.
#[test]
fn a_serial_port_may_be_a_table_with_rs485_and_max_baud() {
    let text = concat!(
        "[board]\n",
        "name    = \"x\"\n",
        "version = \"1.0.0\"\n",
        "arch    = \"aarch64\"\n",
        "\n",
        "[serial]\n",
        "rs485-0 = { path = \"/dev/ttyRS485-0\", rs485 = true, max_baud = 115200 }\n",
    );
    let report = parse(Path::new("x.board.toml"), text);
    let port = report.peripherals.serial.get("rs485-0").expect("rs485-0");
    assert_eq!(port.path, "/dev/ttyRS485-0");
    assert!(port.rs485);
    assert_eq!(port.max_baud, Some(115_200));
}

/// `[gpio] lines` names offsets explicitly (ref/03 §2.3.2), instead of
/// falling back to the kernel's own names.
#[test]
fn gpio_lines_name_offsets() {
    let text = concat!(
        "[board]\n",
        "name    = \"x\"\n",
        "version = \"1.0.0\"\n",
        "arch    = \"aarch64\"\n",
        "\n",
        "[gpio]\n",
        "chip = \"pinctrl-rp1\"\n",
        "\n",
        "[gpio.lines]\n",
        "led_a1 = 17\n",
    );
    let report = parse(Path::new("x.board.toml"), text);
    let gpio = report.peripherals.gpio.expect("gpio");
    assert_eq!(gpio.lines.get("led_a1"), Some(&17));
}
