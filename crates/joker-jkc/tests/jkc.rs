#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use joker_jkc::compile;
use joker_jkc::contract::ContractFile;

fn here() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn read_dir(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "jkc"))
        .map(|p| {
            let text = fs::read_to_string(&p).unwrap();
            let rel = p
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap()
                .to_path_buf();
            (rel, text)
        })
        .collect();
    out.sort();
    out
}

fn base() -> Vec<(PathBuf, String)> {
    read_dir(&here().join("fixtures/base"))
}

#[test]
fn the_base_fixtures_are_clean() {
    let c = compile(base());
    assert!(c.diagnostics.is_empty(), "{}", c.render());
    assert_eq!(c.classes.len(), 4);
}

#[test]
fn every_code_has_a_failing_fixture() {
    let fixtures = read_dir(&here().join("fixtures"));
    assert!(fixtures.len() >= 24);
    for (path, text) in fixtures {
        let stem = path.file_stem().unwrap().to_str().unwrap().to_uppercase();
        let mut files = base();
        files.push((path.clone(), text));
        let c = compile(files);
        let codes: Vec<&str> = c.diagnostics.iter().map(|d| d.code).collect();
        assert!(
            codes.contains(&stem.as_str()),
            "{}: expected {stem}, got {codes:?}\n{}",
            path.display(),
            c.render()
        );
        let own = stem.to_lowercase();
        if !stem.starts_with("JW") && stem != "J0804" {
            assert!(
                !c.classes
                    .keys()
                    .any(|k| k.starts_with(&format!("fx/{own}@"))),
                "{stem}: a class with errors has no contract"
            );
        }
    }
}

#[test]
fn every_code_of_section_7_is_covered() {
    let doc = fs::read_to_string(here().join("../../../docs/17-jkc.md")).unwrap();
    let section = doc
        .split("## 7. `jkc check` rules")
        .nth(1)
        .unwrap()
        .split("\n## ")
        .next()
        .unwrap();
    let fixtures: Vec<String> = read_dir(&here().join("fixtures"))
        .into_iter()
        .map(|(p, _)| p.file_stem().unwrap().to_str().unwrap().to_uppercase())
        .collect();
    let mut seen = 0;
    for line in section.lines() {
        let Some(code) = line.strip_prefix("| `").and_then(|l| l.split('`').next()) else {
            continue;
        };
        if code.starts_with("JC") || code.starts_with("JW") {
            seen += 1;
            if code == "JC0601" {
                continue;
            }
            assert!(fixtures.iter().any(|f| f == code), "no fixture for {code}");
        }
    }
    assert!(seen >= 23, "found {seen} codes in docs/17 §7");
}

#[test]
fn diagnostics_render_as_jk_check() {
    let mut files = base();
    files.push((
        "tests/fixtures/jc0107.jkc".into(),
        fs::read_to_string(here().join("fixtures/jc0107.jkc")).unwrap(),
    ));
    let c = compile(files);
    assert_eq!(
        c.render(),
        "error[JC0107]: `out x` is a physical quantity without a unit; write `1` if it is dimensionless\n \
         --> tests/fixtures/jc0107.jkc:4:12\n  |\n4 |     out x: f64\n  |            ^^^\n"
    );
    let mut files = base();
    files.push((
        "tests/fixtures/jw0230.jkc".into(),
        fs::read_to_string(here().join("fixtures/jw0230.jkc")).unwrap(),
    ));
    let c = compile(files);
    assert!(!c.has_errors());
    assert!(
        c.render()
            .starts_with("warning[JW0230]: method `m` is conditioned on `ok`")
    );
    assert!(c.classes.contains_key("fx/jw0230@1"));
}

const PUMP: &str = r#"package std

/// Pump with start/stop and status feedback.
@aligns("CiA 402 when the actuator is a drive")
class pump@1.0.0 {

    /// Run command.
    out run_cmd: bool            @safe_hint(false)

    /// Run feedback (contactor or drive).
    in  running: bool
    in  fault:   bool
    in  fault_code: u64          @optional

    param rated_flow: f64 m3/h   @optional

    event tripped { code: u64 }

    /// Resets after a fault.
    action reset() -> ()         @permit_hint(fault && !running) @timeout(5 s)

    method start()               { run_cmd = true }
    method stop()                { run_cmd = false }

    query failed_to_start: bool = held(run_cmd && !running, 10 s)
    query failed_to_stop:  bool = held(!run_cmd && running, 30 s)

    capability variable_speed {
        out speed_set: f64 %     @safe_hint(0 %) @range_hint(0 %, 100 %)
        in  speed:     f64 %
        atomic method run_at(pct: f64 %) order(speed_set, run_cmd) {
            speed_set = clamp(pct, 0 %, 100 %)
            run_cmd   = pct > 0 %
        }
    }

    capability pressure {
        in pressure_out: f64 bar
        query dry_running: bool = running && held(pressure_out < 0.2 bar, 5 s)
    }

    capability hour_counter {
        in hours: f64 h
    }

    health required(running, fault)
}
"#;

const STATION: &str = r#"package local

class station@1.0.0 {
    member pumps: std/pump@1[1..8]
    member lead:  pump@1 @requires(variable_speed)
    in flow: f64 m3/h

    query any_pump_failed: bool = any(pumps.failed_to_start)
    query pumps_running:   u64  = count(pumps.running)

    atomic method boost(pct: f64 %) order(lead) { lead.run_at(pct) }
    method stop() { lead.stop() }
}
"#;

fn pump_files(pump: &str) -> Vec<(PathBuf, String)> {
    vec![
        ("classes/std/pump.jkc".into(), pump.to_owned()),
        ("classes/local/station.jkc".into(), STATION.to_owned()),
    ]
}

#[test]
fn the_pump_of_document_17_compiles_to_the_golden_contract() {
    let c = compile(pump_files(PUMP));
    assert!(c.diagnostics.is_empty(), "{}", c.render());
    let pump = &c.classes["std/pump@1"];
    assert_eq!(pump.file_name(), "pump@1.0.0.contract.json");
    let golden = here().join("golden/pump@1.0.0.contract.json");
    if std::env::var_os("JKC_BLESS").is_some() {
        fs::write(&golden, pump.to_json()).unwrap();
    }
    assert_eq!(pump.to_json(), fs::read_to_string(&golden).unwrap());
    let again = compile(pump_files(PUMP));
    assert_eq!(again.classes["std/pump@1"].to_json(), pump.to_json());
    let back = ContractFile::from_json(&pump.to_json()).unwrap();
    assert_eq!(&back, pump);
    let station = &c.classes["local/station@1"];
    let back = ContractFile::from_json(&station.to_json()).unwrap();
    assert_eq!(&back, station);
}

#[test]
fn layout_and_comments_do_not_change_the_hashes() {
    let a = compile(pump_files(PUMP));
    let reformatted = PUMP
        .replace(
            "held(run_cmd && !running, 10 s)",
            "held( run_cmd&&! running ,10s )",
        )
        .replace(
            "    in  fault:   bool\n",
            "    in fault: bool // the fault input\n",
        );
    let b = compile(pump_files(&reformatted));
    assert!(b.diagnostics.is_empty(), "{}", b.render());
    let (a, b) = (&a.classes["std/pump@1"], &b.classes["std/pump@1"]);
    assert_eq!(a.contract_hash(), b.contract_hash());
    assert_eq!(a.doc_hash(), b.doc_hash());
    let documented = compile(pump_files(
        &PUMP.replace("/// Run command.", "/// The run command."),
    ));
    let d = &documented.classes["std/pump@1"];
    assert_eq!(a.contract_hash(), d.contract_hash());
    assert_ne!(a.doc_hash(), d.doc_hash());
}

fn version(pump: &str, v: &str) -> BTreeMap<String, ContractFile> {
    compile(pump_files(
        &pump.replace("pump@1.0.0", &format!("pump@{v}")),
    ))
    .classes
}

#[test]
fn jc0601_requires_the_bump_of_section_9() {
    let old = version(PUMP, "1.0.0");
    let codes = |next: &BTreeMap<String, ContractFile>| -> Vec<&'static str> {
        joker_jkc::compatibility(&old, next)
            .into_iter()
            .map(|d| d.code)
            .collect()
    };
    assert!(codes(&version(PUMP, "1.0.0")).is_empty());
    let doc = PUMP.replace("/// Run command.", "/// The run command.");
    assert_eq!(codes(&version(&doc, "1.0.0")), ["JC0601"]);
    assert!(codes(&version(&doc, "1.0.1")).is_empty());
    let added = PUMP.replace(
        "    health required",
        "    capability noise {\n        in level: f64 dB\n    }\n\n    health required",
    );
    let added = added.replace("in level: f64 dB", "in level: f64 1");
    assert_eq!(codes(&version(&added, "1.0.1")), ["JC0601"]);
    assert!(codes(&version(&added, "1.1.0")).is_empty());
    let body = PUMP.replace(
        "held(run_cmd && !running, 10 s)",
        "held(run_cmd && !running, 12 s)",
    );
    assert_eq!(codes(&version(&body, "1.9.0")), ["JC0601"]);
    assert!(codes(&version(&body, "2.0.0")).is_empty());
    let unit = PUMP.replace("rated_flow: f64 m3/h", "rated_flow: f64 l/s");
    assert!(codes(&version(&unit, "1.1.0")).is_empty());
    let removed = PUMP.replace("    in  fault_code: u64          @optional\n", "");
    assert_eq!(codes(&version(&removed, "1.1.0")), ["JC0601"]);
    let event = PUMP.replace(
        "event tripped { code: u64 }",
        "event tripped { code: u64, at: u64 }",
    );
    assert!(codes(&version(&event, "1.1.0")).is_empty());
    let event = PUMP.replace(
        "event tripped { code: u64 }",
        "event tripped { at: u64, code: u64 }",
    );
    assert_eq!(codes(&version(&event, "1.1.0")), ["JC0601"]);
}
