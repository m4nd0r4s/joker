#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use joker_jkc::stdlib;
use joker_jkc::verify::{Status, verify};

fn std_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../classes/std")
}

#[test]
fn std_compiles_without_any_finding() {
    let c = stdlib::compile();
    assert!(c.diagnostics.is_empty(), "{}", c.render());
    assert_eq!(c.classes.len(), stdlib::CLASSES.len());
    for class in c.classes.values() {
        let text = class.to_json();
        let back = joker_jkc::contract::ContractFile::from_json(&text).unwrap();
        assert_eq!(&back, class);
    }
}

#[test]
fn every_file_of_classes_std_is_embedded() {
    let mut on_disk = BTreeSet::new();
    for entry in fs::read_dir(std_dir()).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if let Some(stem) = name.strip_suffix(".jkc") {
            on_disk.insert(stem.to_owned());
            assert!(
                std_dir().join(format!("{stem}.conformance.toml")).exists(),
                "{stem}.jkc has no conformance suite"
            );
        }
    }
    let embedded: BTreeSet<String> = stdlib::CLASSES.iter().map(|c| c.name.to_owned()).collect();
    assert_eq!(on_disk, embedded);
    assert!(stdlib::PACKAGE.contains("name        = \"std\""));
}

#[test]
fn the_track_classes_of_document_14_are_there() {
    let contracts = stdlib::contracts();
    for class in [
        "led",
        "lamp",
        "relay",
        "button",
        "switch",
        "analog_in",
        "analog_out",
        "temp_sensor",
        "humidity_sensor",
        "pressure_sensor",
    ] {
        assert!(contracts.contains_key(&format!("std/{class}@1")), "{class}");
    }
    let led = &contracts["std/led@1"].contract;
    assert!(led.member("on").is_some() && led.method("on").is_some());
    assert_eq!(led.capabilities, ["dimmable"]);
    let analog_in = &contracts["std/analog_in@1"].contract;
    assert_eq!(analog_in.member("reading").unwrap().unit, None);
    assert_eq!(
        contracts["std/button@1"]
            .contract
            .query("clicked")
            .unwrap()
            .expr,
        "rose(pressed)"
    );
}

#[test]
fn every_suite_passes_against_a_simulated_instance() {
    let contracts = stdlib::contracts();
    for class in stdlib::CLASSES {
        let contract = &contracts[&format!("std/{}@1", class.name)];
        let all = contract.contract.capabilities.clone();
        let file = format!("{}.conformance.toml", class.name);
        let report = verify(&file, class.conformance, contract, &all).unwrap();
        assert!(
            report.failed() == 0 && report.skipped() == 0 && report.passed() > 0,
            "{}",
            report.render()
        );
        let bare = verify(&file, class.conformance, contract, &[]).unwrap();
        assert_eq!(bare.failed(), 0, "{}", bare.render());
        for case in &bare.cases {
            if let Status::Skipped(code, _) = &case.status {
                assert_eq!(*code, "J0846");
            }
        }
        if !all.is_empty() {
            assert!(bare.skipped() > 0, "{}", bare.render());
        }
    }
}

#[test]
fn a_wrong_expectation_fails_with_j0842() {
    let contracts = stdlib::contracts();
    let led = &contracts["std/led@1"];
    let suite = r#"schema = 1

[suite]
kind  = "class"
class = "std/led@1"

[[case]]
id     = "on_is_off"
do     = [{ at = "100ms", method = "on" }]
expect = [{ member = "on", equals = false, at = "100ms" }]
"#;
    let report = verify("led.conformance.toml", suite, led, &[]).unwrap();
    assert_eq!(report.failed(), 1);
    let Status::Failed(why) = &report.cases[0].status else {
        panic!()
    };
    assert!(why[0].starts_with("J0842"), "{why:?}");
    assert!(report.render().contains("on_is_off"));
}

#[test]
fn a_suite_that_does_not_match_its_class_is_j0845() {
    let contracts = stdlib::contracts();
    let led = &contracts["std/led@1"];
    let suite = stdlib::CLASSES
        .iter()
        .find(|c| c.name == "button")
        .unwrap()
        .conformance;
    let e = verify("led.conformance.toml", suite, led, &[]).unwrap_err();
    assert_eq!(e.code, "J0845");
    let led_suite = stdlib::CLASSES
        .iter()
        .find(|c| c.name == "led")
        .unwrap()
        .conformance;
    let e = verify("lamp.conformance.toml", led_suite, led, &[]).unwrap_err();
    assert_eq!(e.code, "J0845");
}

#[test]
fn the_runner_holds_its_writes_until_lease_expire() {
    let contracts = stdlib::contracts();
    let led = &contracts["std/led@1"];
    let suite = r#"schema = 1

[suite]
kind  = "class"
class = "std/led@1"

[[case]]
id     = "held"
do     = [{ at = "100ms", method = "on" }]
expect = [{ member = "on", equals = true, level = "control:12", during = "100ms .. 3s" }]

[[case]]
id     = "toggle_is_not_a_no_op"
do     = [{ at = "100ms", method = "on" }, { at = "300ms", method = "toggle" }]
expect = [{ member = "on", equals = true, at = "300ms" }]

[[case]]
id     = "expired"
do     = [{ at = "100ms", method = "on" }, { at = "1s", lease_expire = "on" }]
expect = [{ member = "on", equals = true, at = "1.5s" }]
"#;
    let report = verify("led.conformance.toml", suite, led, &[]).unwrap();
    let status: Vec<bool> = report
        .cases
        .iter()
        .map(|c| c.status == Status::Passed)
        .collect();
    assert_eq!(status, [true, false, false], "{}", report.render());
}
