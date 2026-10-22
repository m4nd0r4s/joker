//! The cycle thread's `CommitTx` reaches a real driver's `Driver::write`
//! input: a `CommitRx` on the same boxes sees a real `CommittedWrite` after
//! a commit (ref/06 §2.9, docs/01 §3.4).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use joker_core::time::{ClockQuality, Instant};

use jokerd::decisions::DecisionLog;
use jokerd::load::{instantiate, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::VirtualPacer;
use jokerd::runner::{Running, Start};

const MS: u64 = 1_000_000;

const GPIO_JOKER_TOML: &str = r#"
[project]
name  = "commit-channel-test"
joker = "1.0"
mode  = "dev"

[outputs."io.relay"]
safe = false

[cycles.main]
period = "20ms"

[apps.light]
path   = "apps/light"
bind   = { led = "io.relay" }
params = { half_period = "500 ms" }
cycles = { blink = "main" }

[devices.io]
driver = "gpio"
chip   = "gpiochip0"

[devices.io.signals]
relay = { type = "bool", direction = "output", line = 22 }
"#;

#[test]
fn a_real_devices_output_commit_reaches_a_commit_rx() {
    let fs = common::fs(&[("joker.toml", GPIO_JOKER_TOML)]);
    let loaded = load(&fs, std::path::Path::new("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    assert_eq!(loaded.plan.devices.len(), 1);
    assert_eq!(loaded.plan.devices[0].points.len(), 1);

    let tasks = instantiate(&loaded.plan, &loaded.manifest, &NativeTasks).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let pacer = Box::new(VirtualPacer::new(Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }));
    let running = Running::start(
        Arc::new(loaded.plan),
        tasks,
        pacer,
        log,
        Start {
            generation: 1,
            inherit: Default::default(),
            retention: Duration::from_secs(600),
            max_ticks: Some(30),
            utc_origin_ns: 0,
            cpus: Vec::new(),
        },
    )
    .unwrap();
    let shared = Arc::clone(&running.shared);
    running.join().unwrap();

    assert_eq!(shared.commit_bindings.len(), 1);
    let bindings = &shared.commit_bindings[0];
    let boxes = &shared.commit_boxes[0];
    assert_eq!(bindings.len(), 1, "one output point on the gpio device");
    let mut state = vec![joker_hal::RxState::default(); bindings.len()];
    let mut rx = joker_hal::CommitRx::new(bindings, boxes, &mut state).unwrap();
    let mut writes = Vec::new();
    rx.drain_into(&mut writes);
    assert!(
        !writes.is_empty(),
        "the app's writes to io.relay must have reached the commit box"
    );
    assert_eq!(writes[0].point, 0);
}
