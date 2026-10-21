//! `joker_manifest::profile::parse`: loading a `.profile.toml` (ref/03
//! §2.2) into a `ProfileInfo` (AUR-414).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::path::Path;

use joker_manifest::context::{Direction, SignalType};
use joker_manifest::profile::parse;

fn ok(text: &str) -> joker_manifest::context::ProfileInfo {
    let report = parse(Path::new("acme/pcs.profile.toml"), text);
    assert!(
        report.profile.is_some(),
        "expected a profile, got errors:\n{}",
        report.render()
    );
    report.profile.unwrap()
}

const HEADER: &str = r#"
[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"
"#;

#[test]
fn a_minimal_profile_with_one_output_signal() {
    let p = ok(&format!(
        "{HEADER}[signals.p_set]\naddress = {{ at = 0, fc = 6 }}\ntype = \"u16\"\ndirection = \"output\"\n"
    ));
    let s = p.signals.get("p_set").expect("p_set");
    assert_eq!(s.ty, SignalType::Integer);
    assert_eq!(s.direction, Direction::Output);
}

/// A case: the code a malformed profile reports, the file and what it
/// shows.
type Case = (&'static str, String, &'static str);

fn cases() -> Vec<Case> {
    vec![
        ("J0142", "[signals]\n".to_owned(), "no `[profile]` section"),
        (
            "J0142",
            format!("{HEADER}[signals.p_set]\naddress = {{ at = 0, fc = 6 }}\n"),
            "signal without `type`",
        ),
        (
            "J0809",
            format!("{HEADER}[signals.p_set]\naddress = {{ at = 0 }}\ntype = \"nope\"\n"),
            "unknown raw type",
        ),
        (
            "J0815",
            format!(
                "{HEADER}[signals.p_ac]\naddress = {{ at = 0 }}\ntype = \"f32\"\nunit = \"bogus\"\n"
            ),
            "invalid unit",
        ),
        (
            "J0820",
            format!("{HEADER}[fallback]\ntimeout = \"soon\"\n"),
            "fallback timeout is not a duration",
        ),
        (
            "J0821",
            format!("{HEADER}[actions.reset]\nsteps = []\ntimeout = \"soon\"\n"),
            "action timeout is not a duration",
        ),
    ]
}

#[test]
fn every_case_reports_its_code() {
    for (code, text, what) in cases() {
        let report = parse(Path::new("x.profile.toml"), &text);
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

#[test]
fn unit_on_a_signal() {
    let p = ok(&format!(
        "{HEADER}[signals.p_ac]\naddress = {{ at = 0, fc = 4 }}\ntype = \"f32\"\nunit = \"kW\"\n"
    ));
    let s = p.signals.get("p_ac").unwrap();
    assert_eq!(s.ty, SignalType::Float);
    assert!(s.unit.is_some());
}

#[test]
fn an_enum_signal_resolves_its_variants() {
    let p = ok(&format!(
        "{HEADER}[enums]\nPcsMode = {{ off = 0, standby = 1 }}\n[signals.mode]\naddress = {{ at = 0, fc = 3 }}\ntype = \"u16\"\nenum = \"PcsMode\"\n"
    ));
    let s = p.signals.get("mode").unwrap();
    match &s.ty {
        SignalType::Enum { name, variants } => {
            assert_eq!(name, "PcsMode");
            assert_eq!(variants, &vec!["off".to_owned(), "standby".to_owned()]);
        }
        other => panic!("expected an enum, got {other:?}"),
    }
}

#[test]
fn defaults_poll_and_timeout() {
    let p = ok(&format!(
        "{HEADER}[defaults]\npoll = \"200ms\"\ntimeout = \"1s\"\n"
    ));
    assert_eq!(p.poll, Some(joker_units::Duration::from_millis(200)));
    assert_eq!(p.timeout, Some(joker_units::Duration::from_secs(1)));
}

#[test]
fn fallback_presence_and_fixed_timeout() {
    let p = ok(&format!("{HEADER}[fallback]\ntimeout = \"2s\"\n"));
    assert!(p.fallback);
    assert!(p.configurable);
    assert_eq!(
        p.fallback_timeout,
        Some(joker_units::Duration::from_secs(2))
    );
}

#[test]
fn fallback_not_configurable() {
    let p = ok(&format!(
        "{HEADER}[fallback]\ntimeout = \"2s\"\nconfigurable = false\n"
    ));
    assert!(!p.configurable);
}

#[test]
fn no_fallback_section_means_no_fallback() {
    let p = ok(HEADER);
    assert!(!p.fallback);
}

#[test]
fn an_action_with_an_explicit_timeout() {
    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ wait = \"500ms\" }}]\ntimeout = \"9s\"\n"
    ));
    assert_eq!(
        p.actions.get("reset"),
        Some(&Some(joker_units::Duration::from_secs(9)))
    );
}

#[test]
fn an_action_default_timeout_sums_the_waits_plus_a_second_per_step() {
    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ write = {{ at = 1, value = 1 }} }}, {{ wait = \"500ms\" }}, {{ write = {{ at = 1, value = 0 }} }}]\n"
    ));
    // 3 steps x 1s + the 500ms wait.
    assert_eq!(
        p.actions.get("reset"),
        Some(&Some(joker_units::Duration::from_millis(3_500)))
    );
}

#[test]
fn quirks_latency() {
    let p = ok(&format!("{HEADER}\n[profile.quirks]\nlatency = \"15ms\"\n"));
    assert_eq!(p.latency, Some(joker_units::Duration::from_millis(15)));
}
