//! A Wasm app whose cycle never returns is cut by its epoch deadline
//! (`J-E0904`), and its group keeps ticking (docs/01 §4, ADR 0098).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jokerd::apps::{APP_WASM, NodeApps};
use jokerd::blobs::{Blobs, digest_of, parse_digest};
use jokerd::control::{Config, Controller};
use jokerd::decisions::DecisionLog;
use jokerd::devices::NoDevices;
use jokerd::generations::GenArtifact;
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, SystemPacer};

const MANIFEST: &str = r#"
[project]
name  = "deadline"
joker = "1.0"
mode  = "dev"

[outputs."pi.act_led"]
safe = false

[cycles.main]
period = "20ms"

[apps.spin]
path   = "apps/spin"
bind   = { led = "pi.act_led" }
cycles = { run = "main" }

[devices.pi]
driver = "sim"

[devices.pi.signals]
act_led = { type = "bool", direction = "output" }
"#;

fn component() -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fixtures");
    let status = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-wasip2",
            "-p",
            "jokerd-fixture-spin",
        ])
        .arg("--target-dir")
        .arg(&target_dir)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .status()
        .unwrap();
    assert!(status.success(), "building jokerd-fixture-spin failed");
    std::fs::read(target_dir.join("wasm32-wasip2/release/jokerd_fixture_spin.wasm")).unwrap()
}

#[test]
fn a_cycle_that_never_returns_is_cut_and_its_group_keeps_ticking() {
    let wasm = component();
    let dir = tempfile::tempdir().unwrap();
    let digest = digest_of(&wasm);
    Blobs::new(dir.path())
        .put(&parse_digest(&digest).unwrap(), &wasm)
        .unwrap();
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
        Config::default(),
    )
    .unwrap()
    .with_apps(Arc::new(NodeApps::open(dir.path()).unwrap()));
    let artifacts = [GenArtifact {
        name: "apps.spin".into(),
        kind: APP_WASM.into(),
        digest,
    }];
    let g = c
        .propose_with(MANIFEST, "", &artifacts, None, false, None)
        .unwrap();
    c.activate(g.number, None, None).unwrap();

    let path = dir.path().join("decisions.jsonl");
    let start = Instant::now();
    let mut trap = None;
    while start.elapsed() < Duration::from_secs(10) && trap.is_none() {
        c.poll().unwrap();
        trap = DecisionLog::read_all(&path)
            .unwrap()
            .into_iter()
            .find(|e| e["record"] == "app_trap");
        std::thread::sleep(Duration::from_millis(20));
    }
    let trap = trap.unwrap_or_else(|| panic!("the spinning cycle was never cut"));
    assert_eq!(trap["app"], "spin");
    assert_eq!(trap["code"], "J-E0904", "{trap}");

    let ticks = |c: &Controller| {
        c.current()
            .map(|(_, _, r)| r.shared.groups.iter().map(|g| g.ticks.get()).sum::<u64>())
            .unwrap_or(0)
    };
    let before = ticks(&c);
    std::thread::sleep(Duration::from_millis(400));
    c.poll().unwrap();
    let after = ticks(&c);
    assert!(
        after >= before + 10,
        "the group stalled: {before} -> {after}"
    );
    c.shutdown();
}
