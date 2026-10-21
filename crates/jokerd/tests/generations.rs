#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jokerd::control::{Config, Controller};
use jokerd::decisions::DecisionLog;
use jokerd::devices::NoDevices;
use jokerd::generations::{GenState, Origin};
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, SystemPacer};

const WRONG_PORT: &str = r#"
[buses.rs485]
kind = "serial"
port = "/dev/ttyNOPE"
baud = 115200

[devices.esp]
driver  = "modbus-rtu"
bus     = "rs485"
profile = "local/bench-esp32@1"
unit_id = 1

[outputs."esp.led"]
safe = false
"#;

fn effective(extra: &str) -> String {
    format!(
        "{}\n[devices.pi]\ndriver = \"sim\"\n\n[devices.pi.signals]\nact_led = {{ type = \"bool\", direction = \"output\" }}\n{extra}",
        common::JOKER_TOML
    )
}

fn controller(dir: &std::path::Path) -> Controller {
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.join("decisions.jsonl")).unwrap(),
    ));
    let clock = Monotonic::start();
    Controller::new(
        dir,
        log,
        Arc::new(NativeTasks),
        Arc::new(NoDevices),
        Box::new(move || Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced))),
        Config {
            min_window: Duration::from_millis(100),
            ..Config::default()
        },
    )
    .unwrap()
}

fn wait_for_rollback(c: &mut Controller, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        if c.poll().unwrap().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn an_unconfirmed_generation_is_rolled_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = controller(dir.path());

    let g1 = c
        .propose(&effective(""), "", Some("bench".into()), false)
        .unwrap();
    assert_eq!(g1.number, 1);
    assert_eq!(g1.state, GenState::Pending);
    assert!(g1.hash.starts_with("sha256:"));
    c.activate(1, None, Some("unix:dl".into())).unwrap();
    assert_eq!(c.store().get(1).unwrap().state, GenState::Confirmed);
    std::thread::sleep(Duration::from_millis(100));

    let g2 = c
        .propose(&effective(WRONG_PORT), "", Some("wrong port".into()), false)
        .unwrap();
    assert_ne!(g2.hash, g1.hash);
    let switched = c
        .activate(2, Some(Duration::from_millis(300)), None)
        .unwrap();
    assert_eq!(switched.from, Some(1));
    assert_eq!(switched.generation.state, GenState::Confirming);
    assert_eq!(c.current().unwrap().0, 2);
    assert!(c.confirming().is_some());

    assert!(wait_for_rollback(&mut c, Duration::from_secs(3)));
    assert_eq!(c.current().unwrap().0, 1);
    assert!(c.confirming().is_none());
    assert_eq!(c.store().get(2).unwrap().state, GenState::RolledBack);
    assert_eq!(
        c.store().get(2).unwrap().failure.as_deref(),
        Some("confirmation window expired")
    );
    assert_eq!(c.store().get(1).unwrap().state, GenState::Confirmed);
    let origins: Vec<(u64, Origin)> = c
        .store()
        .activations()
        .unwrap()
        .iter()
        .map(|a| (a.number, a.origin))
        .collect();
    assert_eq!(
        origins,
        vec![
            (1, Origin::Local),
            (2, Origin::Local),
            (1, Origin::Rollback)
        ]
    );
    assert_eq!(c.confirm(Some(2)).unwrap_err().code, "J-E1611");

    c.shutdown();
    let log = DecisionLog::read_all(&dir.path().join("decisions.jsonl")).unwrap();
    let events: Vec<(u64, String)> = log
        .iter()
        .filter(|e| e["record"] == "generation")
        .map(|e| {
            (
                e["gen"].as_u64().unwrap(),
                e["event"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        events,
        vec![
            (1, "confirmed".into()),
            (2, "confirming".into()),
            (2, "rolled_back".into()),
            (1, "confirmed".into()),
        ]
    );
    assert!(log.iter().any(|e| e["record"] == "output" && e["gen"] == 2));
}

#[test]
fn a_confirmed_generation_stays_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut c = controller(dir.path());
        c.propose(&effective(""), "", None, false).unwrap();
        c.activate(1, None, None).unwrap();
        c.propose(&effective(WRONG_PORT), "", None, false).unwrap();
        c.activate(2, Some(Duration::from_millis(200)), None)
            .unwrap();
        assert_eq!(c.activate(1, None, None).unwrap_err().code, "J-E1608");
        c.confirm(None).unwrap();
        assert!(!wait_for_rollback(&mut c, Duration::from_millis(400)));
        assert_eq!(c.current().unwrap().0, 2);
        assert_eq!(c.store().get(2).unwrap().state, GenState::Confirmed);
    }
    let mut c = controller(dir.path());
    let booted = c.boot().unwrap().unwrap();
    assert_eq!(booted.number, 2);
    assert_eq!(c.current().unwrap().0, 2);
    assert_eq!(
        c.store().activations().unwrap().last().unwrap().origin,
        Origin::Boot
    );
}

#[test]
fn a_crash_while_confirming_boots_the_previous_generation() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut c = controller(dir.path());
        c.propose(&effective(""), "", None, false).unwrap();
        c.activate(1, None, None).unwrap();
        c.propose(&effective(WRONG_PORT), "", None, false).unwrap();
        c.activate(2, Some(Duration::from_secs(60)), None).unwrap();
    }
    let mut c = controller(dir.path());
    assert_eq!(c.boot().unwrap().unwrap().number, 1);
    assert_eq!(c.store().get(2).unwrap().state, GenState::RolledBack);
}

#[test]
fn invalid_proposals_and_windows_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = controller(dir.path());
    let bad = effective("[outputs.\"pi.nope\"]\nsafe = false\n");
    let e = c.propose(&bad, "", None, false).unwrap_err();
    assert_eq!(e.code, "J-E1601");
    assert!(!e.errors.is_empty());
    assert!(c.store().list().unwrap().is_empty());
    let dry = c.propose(&effective(""), "", None, true).unwrap();
    assert_eq!(dry.number, 0);
    assert!(c.store().list().unwrap().is_empty());
    c.propose(&effective(""), "", None, false).unwrap();
    assert_eq!(
        c.activate(1, Some(Duration::from_millis(10)), None)
            .unwrap_err()
            .code,
        "J-E1623"
    );
    c.propose(&effective(""), "", None, false).unwrap();
    assert_eq!(c.store().get(1).unwrap().state, GenState::Discarded);
    assert_eq!(c.activate(1, None, None).unwrap_err().code, "J-E1612");
    assert_eq!(c.confirm(None).unwrap_err().code, "J-E1611");
}

#[test]
fn outputs_kept_across_a_switch_inherit_their_applied_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = controller(dir.path());
    c.propose(&effective(""), "", None, false).unwrap();
    c.activate(1, None, None).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    c.propose(&effective(WRONG_PORT), "", None, false).unwrap();
    c.activate(2, None, None).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    c.shutdown();
    let log = DecisionLog::read_all(&dir.path().join("decisions.jsonl")).unwrap();
    let first_of_gen2 = log
        .iter()
        .find(|e| e["record"] == "output" && e["gen"] == 2 && e["output"] == "pi.act_led")
        .unwrap();
    assert!(first_of_gen2["previous"].is_null());
    assert_eq!(first_of_gen2["applied"], true);
}
