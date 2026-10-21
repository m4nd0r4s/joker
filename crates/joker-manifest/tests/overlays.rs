//! Envelopes only tighten (ref/01 §7.4), and the approvals of the lock
//! (ref/02 §2.1.14).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::path::Path;

use joker_manifest::context::{Direction, DriverInfo, ProfileInfo, SignalInfo, SignalType};
use joker_manifest::lock::{self, Approval, Lock};
use joker_manifest::model::BusKind;
use joker_manifest::{Context, MemoryFileSystem, Report, check};
use joker_units::Unit;

fn context() -> Context {
    let mut c = Context::default();
    c.drivers.insert(
        "modbus-tcp".into(),
        DriverInfo {
            buses: vec![BusKind::Ethernet],
            profile_required: true,
            config: ["address".to_owned()].into_iter().collect(),
            ..DriverInfo::default()
        },
    );
    c.drivers.insert("joker-link".into(), DriverInfo::default());
    let p_set =
        SignalInfo::new(SignalType::Float, Direction::Output).with_unit(Unit::parse("kW").unwrap());
    let p_ac =
        SignalInfo::new(SignalType::Float, Direction::Input).with_unit(Unit::parse("kW").unwrap());
    let led = SignalInfo::new(SignalType::Bool, Direction::Output);
    c.profiles.insert(
        "acme/pcs@1".into(),
        ProfileInfo {
            signals: [
                ("p_set".to_owned(), p_set),
                ("p_ac".to_owned(), p_ac),
                (
                    "rb".to_owned(),
                    SignalInfo::new(SignalType::Float, Direction::Input)
                        .with_unit(Unit::parse("kW").unwrap()),
                ),
            ]
            .into_iter()
            .collect(),
            fallback: true,
            ..ProfileInfo::default()
        },
    );
    c.profiles.insert(
        "acme/led@1".into(),
        ProfileInfo {
            signals: [("led".to_owned(), led)].into_iter().collect(),
            fallback: true,
            ..ProfileInfo::default()
        },
    );
    c
}

const BASE: &str = "[project]\nname = \"s\"\nmode = \"dev\"\n[devices.pcs]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.1:502\"\nprofile = \"acme/pcs@1\"\n[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nrange = [\"-125 kW\", \"125 kW\"]\nrate = \"25 kW/s\"\nmin_interval = \"200ms\"\nmax_actuations = \"120/h\"\ninterlock = \"pcs.p_ac < 200 kW\"\nrequires_good = [\"pcs.p_ac\"]\nmax_lease = \"10m\"\non_range = \"reject\"\nverify = { readback = \"pcs.rb\", tolerance = \"1 kW\", within = \"2s\" }\n";

fn overlay(body: &str) -> Report {
    let mut fs = MemoryFileSystem::default();
    fs.file("joker.base.toml", BASE);
    fs.file(
        "joker.toml",
        format!("extends = [\"joker.base.toml\"]\n[outputs.\"pcs.p_set\"]\n{body}"),
    );
    check(&fs, Path::new("joker.toml"), &context())
}

const WIDENING: [(&str, &str); 17] = [
    ("range", "range = [\"-150 kW\", \"150 kW\"]"),
    ("range", "range = { unset = true }"),
    ("range", "range = [\"-pcs.p_ac\", \"100 kW\"]"),
    ("rate", "rate = \"30 kW/s\""),
    ("rate", "rate = { unset = true }"),
    ("min_interval", "min_interval = \"100ms\""),
    ("max_actuations", "max_actuations = \"3/min\""),
    ("interlock", "interlock = \"pcs.p_ac < 300 kW\""),
    ("permit_uncertain", "permit_uncertain = true"),
    ("requires_good", "requires_good = []"),
    ("grace", "grace = \"2s\""),
    ("max_lease", "max_lease = \"12m\""),
    ("on_range", "on_range = \"clamp\""),
    ("on_interlock", "on_interlock = \"hold\""),
    (
        "verify",
        "verify = { readback = \"pcs.rb\", tolerance = \"5 kW\", within = \"2s\" }",
    ),
    ("reset_when", "reset_when = \"pcs.p_ac < 1 kW\""),
    ("safe", "safe = \"1 kW\""),
];

const TIGHTENING: [&str; 13] = [
    "range = [\"-100000 W\", \"125 kW\"]",
    "rate = \"20 kW/s\"",
    "min_interval = \"1s\"",
    "max_actuations = \"1/min\"",
    "interlock = \"(pcs.p_ac < 200 kW) && pcs.p_ac > -200 kW\"",
    "requires_good = { append = [\"pcs.rb\"] }",
    "max_lease = \"5m\"",
    "max_manual = \"4h\"",
    "on_range = \"trip\"",
    "verify = { readback = \"pcs.rb\", tolerance = \"500 W\", within = \"1s\" }",
    "deadband = \"2 kW\"",
    "group = \"default\"",
    "safe_ramp = \"50 kW/s\"",
];

#[test]
fn each_widening_needs_widen() {
    for (key, body) in WIDENING {
        let report = overlay(body);
        assert!(
            report.codes().contains(&"J0220"),
            "{key}: {body} did not widen\n{}",
            report.render()
        );
        let justified = overlay(&format!(
            "{body}\nwiden = {{ {key} = \"approved by the site owner\" }}"
        ));
        assert!(
            !justified.codes().contains(&"J0220"),
            "{key}: widen ignored\n{}",
            justified.render()
        );
        assert_eq!(
            justified.widenings.len(),
            1,
            "{key}: {:?}",
            justified.widenings
        );
        assert_eq!(justified.widenings[0].key, key);
    }
}

#[test]
fn tightening_needs_nothing() {
    for body in TIGHTENING {
        let report = overlay(body);
        assert!(
            !report.codes().contains(&"J0220"),
            "{body} widened\n{}",
            report.render()
        );
        assert!(report.widenings.is_empty());
    }
}

#[test]
fn widen_rules() {
    let short = overlay("rate = \"30 kW/s\"\nwiden = \"bigger inverter\"");
    assert_eq!(short.widenings.len(), 1);
    let useless = overlay("rate = \"20 kW/s\"\nwiden = \"nothing to widen here\"");
    assert!(useless.codes().contains(&"J0167"), "{}", useless.render());
    let named = overlay("widen = { accept_missing_fallback = \"never a widening\" }");
    assert!(named.codes().contains(&"J0167"));
    let wrong = overlay("widen = { colour = \"not an envelope key\" }");
    assert!(wrong.codes().contains(&"J0166"));
    let removed = {
        let mut fs = MemoryFileSystem::default();
        fs.file("joker.base.toml", BASE);
        fs.file(
            "joker.toml",
            "extends = [\"joker.base.toml\"]\n[outputs]\n\"pcs.p_set\" = { unset = true }\n",
        );
        check(&fs, Path::new("joker.toml"), &context())
    };
    assert!(removed.codes().contains(&"J0220"));
}

#[test]
fn an_envelope_that_moves_to_the_mcu_widens_nothing() {
    let base = "[project]\nname = \"s\"\nmode = \"dev\"\n[devices.esp]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.2:502\"\nprofile = \"acme/led@1\"\n[outputs.\"esp.led\"]\nsafe = false\n";
    let link = "extends = [\"joker.base.toml\"]\n[devices.esp]\ndriver = \"joker-link\"\naddress = { unset = true }\nprofile = { unset = true }\n[outputs]\n\"esp.led\" = { unset = true }\n";
    let mut fs = MemoryFileSystem::default();
    fs.file("joker.base.toml", base);
    fs.file("joker.toml", link);
    let report = check(&fs, Path::new("joker.toml"), &context());
    assert!(!report.codes().contains(&"J0220"), "{}", report.render());
}

#[test]
fn widenings_are_approved_in_the_lock() {
    let report = overlay(
        "range = [\"-150 kW\", \"150 kW\"]\nwiden = { range = \"new 150 kW PCS, approved\" }\naccept_missing_fallback = \"bench only, no load\"",
    );
    assert_eq!(report.widenings.len(), 1);
    assert_eq!(report.acceptances.len(), 1);
    let codes = |lock: &Lock| -> Vec<&'static str> {
        lock::check_accepted(lock, &report.widenings, &report.acceptances)
            .iter()
            .map(|d| d.code)
            .collect()
    };
    let mut lock = Lock::new();
    assert_eq!(codes(&lock), ["J0168", "J0440"]);
    let approval = Approval {
        by: "user:dl".into(),
        at: "2026-09-18T09:02:11Z".into(),
    };
    lock.set_widen(&report.widenings[0], &approval);
    lock.set_acceptance(&report.acceptances[0], &approval);
    assert!(codes(&lock).is_empty(), "{:?}", codes(&lock));
    let text = lock.canonical();
    assert!(text.contains("widen.\"pcs.p_set.range\" = { reason = \"new 150 kW PCS, approved\", value = [\"-150 kW\", \"150 kW\"], by = \"user:dl\", at = \"2026-09-18T09:02:11Z\" }"), "{text}");
    let reread = lock::read(Path::new("joker.lock"), &text);
    assert!(reread.diagnostics.is_empty(), "{:?}", reread.diagnostics);
    assert!(codes(&reread.lock.unwrap()).is_empty());
    let bigger = overlay(
        "range = [\"-200 kW\", \"200 kW\"]\nwiden = { range = \"new 150 kW PCS, approved\" }\naccept_missing_fallback = \"bench only, no load\"",
    );
    let found: Vec<&str> = lock::check_accepted(&lock, &bigger.widenings, &bigger.acceptances)
        .iter()
        .map(|d| d.code)
        .collect();
    assert_eq!(
        found,
        ["J0168"],
        "an approval for 150 kW does not cover 200 kW"
    );
    let none = overlay("rate = \"20 kW/s\"");
    let orphan: Vec<&str> = lock::check_accepted(&lock, &none.widenings, &none.acceptances)
        .iter()
        .map(|d| d.code)
        .collect();
    assert_eq!(orphan, ["J0441", "J0441"]);
}

#[test]
fn every_overlay_code_has_a_case() {
    let cases: [(&str, &str); 3] = [
        ("J0220", "rate = \"30 kW/s\""),
        (
            "J0167",
            "rate = \"20 kW/s\"\nwiden = \"nothing to widen here\"",
        ),
        ("J0166", "widen = { colour = \"not an envelope key\" }"),
    ];
    for (code, body) in cases {
        let report = overlay(body);
        assert!(
            report.codes().contains(&code),
            "{code}: {}",
            report.render()
        );
    }
}
