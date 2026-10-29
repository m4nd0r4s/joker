//! The clock quality through `jokerd` (M14, docs/16 E1, ADR 0069): an
//! app that requires a synced clock has its writes rejected at step 0 of
//! the envelope while the node's clock is not synced, and accepted once a
//! re-evaluation finds it synced, without restarting the generation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use jokerd::clock::{ClockMonitor, SyncSource};
use jokerd::control::{Config, Controller};
use jokerd::decisions::DecisionLog;
use jokerd::devices::NoDevices;
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, SystemPacer};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
struct FakeSync(Arc<Mutex<Option<i64>>>);

impl FakeSync {
    fn synchronise_now(&self) {
        *self.0.lock().unwrap() = Some(jokerd::wire::utc_now_ns());
    }
}

impl SyncSource for FakeSync {
    fn last_sync_utc_ns(&self) -> Option<i64> {
        *self.0.lock().unwrap()
    }
}

fn manifest() -> String {
    common::JOKER_TOML.replace(
        "cycles = { blink = \"main\" }",
        "cycles = { blink = \"main\" }\nrequires_clock = \"synced\"",
    ) + "\n[devices.pi]\ndriver = \"sim\"\n\n[devices.pi.signals]\nact_led = { type = \"bool\", direction = \"output\" }\n"
}

fn controller(dir: &std::path::Path, source: FakeSync) -> Controller {
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.join("decisions.jsonl")).unwrap(),
    ));
    let clock = Monotonic::start();
    let quality = Arc::new(ClockMonitor::new(Box::new(source)));
    let cell = quality.cell();
    Controller::new(
        dir,
        log,
        Arc::new(NativeTasks),
        Arc::new(NoDevices),
        Box::new(move || Box::new(SystemPacer::tracking(clock, Arc::clone(&cell)))),
        Config {
            clock: quality,
            ..Config::default()
        },
    )
    .unwrap()
}

fn led_writes(log: &[Value]) -> Vec<(String, Option<String>)> {
    log.iter()
        .filter(|e| e["record"] == "write" && e["output"] == "pi.act_led")
        .map(|e| {
            (
                e["outcome"].as_str().unwrap_or("").to_owned(),
                e["reason"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

#[test]
fn an_unsynced_clock_blocks_the_writes_of_an_app_that_requires_synced() {
    let dir = tempfile::tempdir().unwrap();
    let source = FakeSync::default();
    let mut c = controller(dir.path(), source.clone());
    c.propose(&manifest(), "", Some("clock".into()), false)
        .unwrap();
    c.activate(1, None, None).unwrap();
    assert_eq!(c.clock().quality(), joker_core::ClockQuality::Unsynced);
    let refused = || {
        DecisionLog::read_all(&dir.path().join("decisions.jsonl"))
            .map(|log| led_writes(&log).iter().any(|w| w.0 == "rejected"))
            .unwrap_or(false)
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !refused() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }

    source.synchronise_now();
    c.refresh_clock();
    assert_eq!(c.clock().quality(), joker_core::ClockQuality::Synced);
    std::thread::sleep(Duration::from_millis(200));
    c.shutdown();

    let log = DecisionLog::read_all(&dir.path().join("decisions.jsonl")).unwrap();
    let writes = led_writes(&log);
    let rejected = ("rejected".to_owned(), Some("clock_unsynced".to_owned()));
    let first_accepted = writes
        .iter()
        .position(|w| w.0 == "accepted")
        .unwrap_or_else(|| panic!("no accepted write after the clock synced: {writes:?}"));
    assert!(first_accepted > 0, "{writes:?}");
    assert!(
        writes[..first_accepted].iter().all(|w| *w == rejected),
        "{writes:?}"
    );
    let clock: Vec<(&str, &str)> = log
        .iter()
        .filter(|e| e["record"] == "clock")
        .map(|e| {
            (
                e["from"].as_str().unwrap_or(""),
                e["quality"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(clock, vec![("unsynced", "synced")]);
}
