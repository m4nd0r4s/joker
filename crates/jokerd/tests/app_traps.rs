//! A Wasm app of a node's generation that traps: the trap is reported in
//! the decision log (`app_trap`), and its output goes to `safe` when its
//! lease ends (docs/01 §9, ADR 0067).

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
name  = "traps"
joker = "1.0"
mode  = "dev"

[outputs."pi.act_led"]
safe = false

[cycles.main]
period = "20ms"

[apps.crash]
path   = "apps/crash"
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
            "jokerd-fixture-crash",
        ])
        .arg("--target-dir")
        .arg(&target_dir)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .status()
        .unwrap();
    assert!(status.success(), "building jokerd-fixture-crash failed");
    std::fs::read(target_dir.join("wasm32-wasip2/release/jokerd_fixture_crash.wasm")).unwrap()
}

#[test]
fn a_trapping_app_is_reported_and_its_output_goes_safe() {
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
        name: "apps.crash".into(),
        kind: APP_WASM.into(),
        digest,
    }];
    let g = c
        .propose_with(MANIFEST, "", &artifacts, None, false, None)
        .unwrap();
    c.activate(g.number, None, None).unwrap();

    let logs = c.logs();
    let path = dir.path().join("decisions.jsonl");
    let start = Instant::now();
    let mut entries = Vec::new();
    while start.elapsed() < Duration::from_secs(5) {
        c.poll().unwrap();
        entries = DecisionLog::read_all(&path).unwrap();
        let trapped = entries.iter().any(|e| e["record"] == "app_trap");
        let safe = entries
            .iter()
            .rfind(|e| e["record"] == "output" && e["output"] == "pi.act_led")
            .is_some_and(|e| e["applied"] == false);
        let logged = !logs
            .query(&jokerd::logs::LogFilter {
                source: Some("app:crash".into()),
                ..jokerd::logs::LogFilter::default()
            })
            .0
            .is_empty();
        if trapped && safe && logged {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    c.shutdown();

    let trap = entries
        .iter()
        .find(|e| e["record"] == "app_trap")
        .unwrap_or_else(|| panic!("no app_trap in {entries:?}"));
    assert_eq!(trap["app"], "crash");
    assert_eq!(trap["task"], "run");
    assert_eq!(trap["gen"], g.number);
    assert!(
        trap["code"]
            .as_str()
            .is_some_and(|c| c.starts_with("J-E09")),
        "{trap}"
    );
    let led: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e["record"] == "output" && e["output"] == "pi.act_led")
        .map(|e| &e["applied"])
        .collect();
    assert!(led.contains(&&serde_json::Value::Bool(true)), "{led:?}");
    assert_eq!(
        led.last(),
        Some(&&serde_json::Value::Bool(false)),
        "{led:?}"
    );

    let (lines, _) = logs.query(&jokerd::logs::LogFilter {
        source: Some("app:crash".into()),
        ..jokerd::logs::LogFilter::default()
    });
    let line = lines
        .iter()
        .find(|e| e.msg == "about to trap")
        .unwrap_or_else(|| panic!("{lines:?}"));
    assert_eq!(line.level, jokerd::logs::LogLevel::Warn);
    assert_eq!(
        line.fields,
        vec![("ticks".to_owned(), "11".to_owned())],
        "{line:?}"
    );
    let panic = lines
        .iter()
        .find(|e| e.msg == "crashed at tick 11")
        .unwrap_or_else(|| panic!("no panic line in {lines:?}"));
    assert_eq!(panic.level, jokerd::logs::LogLevel::Error);
    assert!(
        panic
            .fields
            .iter()
            .any(|(k, v)| k == "at" && v.contains("lib.rs")),
        "{panic:?}"
    );
}
