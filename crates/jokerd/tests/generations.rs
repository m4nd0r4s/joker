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

const PCS: &str = r#"
[devices.pcs]
driver  = "modbus-tcp"
address = "127.0.0.1:1502"
profile = "acme/pcs@1"
"#;

const PCS_PATH: &str = "profiles/acme/pcs@1.profile.toml";

fn pcs_profile(description: &str) -> String {
    format!(
        "schema = 1\n\n[profile]\nname        = \"acme/pcs\"\nversion     = \"1.0.0\"\ndriver      = \"modbus\"\ndescription = \"{description}\"\n\n[signals.p_ac]\nat   = 0\nfc   = 4\ntype = \"f32\"\nunit = \"kW\"\n"
    )
}

fn pcs_files(description: &str) -> jokerd::catalog::CatalogFiles {
    [(PCS_PATH.to_owned(), pcs_profile(description))]
        .into_iter()
        .collect()
}

fn stored_blob(dir: &std::path::Path, digest: &str) -> std::path::PathBuf {
    dir.join("artifacts/sha256")
        .join(digest.strip_prefix("sha256:").unwrap())
}

#[test]
fn a_generation_carries_its_profiles_as_artifacts_and_runs_with_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = controller(dir.path());
    let g = c
        .propose_files(&effective(PCS), "", &pcs_files("a"), None, false, None)
        .unwrap();
    let artifacts = c.store().artifacts(g.number).unwrap();
    let profile = artifacts
        .iter()
        .find(|a| a.name == PCS_PATH)
        .expect("the profile is an artifact of the generation");
    assert_eq!(profile.kind, jokerd::catalog::PROFILE_KIND);
    assert_eq!(
        std::fs::read_to_string(stored_blob(dir.path(), &profile.digest)).unwrap(),
        pcs_profile("a")
    );
    c.activate(g.number, None, None).unwrap();
    assert!(
        c.current().is_some_and(
            |(n, _, running)| n == g.number && running.plan.signal("pcs.p_ac").is_some()
        ),
        "the activated generation planned the profile's signal"
    );

    let refused = c
        .propose(&effective(PCS), "", None, false)
        .expect_err("without its profile the manifest does not check");
    assert_eq!(refused.code, "J-E1601");
}

#[test]
fn the_bytes_of_a_profile_are_part_of_the_generation_hash() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = controller(dir.path());
    let hash = |c: &mut Controller, description: &str| {
        c.propose_files(
            &effective(PCS),
            "",
            &pcs_files(description),
            None,
            true,
            None,
        )
        .unwrap()
        .hash
    };
    let a = hash(&mut c, "a");
    assert_eq!(hash(&mut c, "a"), a);
    assert_ne!(
        hash(&mut c, "b"),
        a,
        "a changed local profile changes the hash"
    );
}

#[test]
fn a_stored_profile_that_no_longer_matches_its_digest_stops_the_generation() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = controller(dir.path());
    let g = c
        .propose_files(&effective(PCS), "", &pcs_files("a"), None, false, None)
        .unwrap();
    let artifacts = c.store().artifacts(g.number).unwrap();
    let digest = &artifacts
        .iter()
        .find(|a| a.name == PCS_PATH)
        .unwrap()
        .digest;
    std::fs::write(stored_blob(dir.path(), digest), pcs_profile("tampered")).unwrap();
    let refused = c.activate(g.number, None, None).unwrap_err();
    assert_eq!(refused.code, "J-E1602");
}

#[test]
fn a_file_that_is_not_a_profile_or_board_path_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = controller(dir.path());
    for name in [
        "../escape.profile.toml",
        "profiles/../../x@1.profile.toml",
        "/etc/passwd",
        "profiles/acme/pcs@1.toml",
        "boards/nested/rpi5.board.toml",
        "apps/light.wasm",
    ] {
        let files: jokerd::catalog::CatalogFiles =
            [(name.to_owned(), String::new())].into_iter().collect();
        let refused = c
            .propose_files(&effective(""), "", &files, None, false, None)
            .expect_err(name);
        assert_eq!(refused.code, "J-E1601", "{name}");
    }
    assert!(jokerd::catalog::is_catalog_path("boards/rpi5.board.toml"));
}

#[test]
fn a_project_root_lends_its_files_to_the_generation_once() {
    let dir = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("profiles/acme")).unwrap();
    std::fs::write(project.path().join(PCS_PATH), pcs_profile("a")).unwrap();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let clock = Monotonic::start();
    let mut c = Controller::new(
        dir.path(),
        log,
        Arc::new(NativeTasks),
        Arc::new(NoDevices),
        Box::new(move || Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced))),
        Config {
            project_root: project.path().to_path_buf(),
            ..Config::default()
        },
    )
    .unwrap();
    let g = c.propose(&effective(PCS), "", None, false).unwrap();
    assert!(
        c.store()
            .artifacts(g.number)
            .unwrap()
            .iter()
            .any(|a| a.name == PCS_PATH)
    );
    std::fs::write(project.path().join(PCS_PATH), "not a profile").unwrap();
    c.activate(g.number, None, None)
        .expect("the generation runs with the profile it was proposed with");
}
