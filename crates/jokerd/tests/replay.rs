//! Record on a running generation and replay from the file (docs/16 §7,
//! gate 5 of the M0 slice).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use joker_core::ClockQuality;
use joker_core::trace::{Header, Item, Items};
use jokerd::decisions::DecisionLog;
use jokerd::load::{instantiate, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, Pacer, SystemPacer};
use jokerd::replay::replay;
use jokerd::runner::{Running, Start};
use jokerd::traces::Source;

fn record(half_period: &str) -> (Vec<u8>, jokerd::load::Loaded) {
    let toml = common::JOKER_TOML.replace("500 ms", half_period);
    let fs = common::fs(&[("joker.toml", &toml), ("joker.sim.toml", common::SIM_TOML)]);
    let loaded = load(&fs, std::path::Path::new("joker.sim.toml"), &NativeTasks).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("d.jsonl")).unwrap(),
    ));
    let clock = Monotonic::start();
    let tasks = instantiate(&loaded.plan, &loaded.manifest, &NativeTasks).unwrap();
    let pacer = SystemPacer::new(clock, ClockQuality::Unsynced);
    let running = Running::start(
        Arc::new(loaded.plan.clone()),
        tasks,
        Box::new(pacer),
        log,
        Start {
            generation: 1,
            inherit: Default::default(),
            retention: Duration::from_secs(60),
            max_ticks: None,
            utc_origin_ns: 0,
            cpus: Vec::new(),
        },
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(330));
    let source = Source {
        plan: Arc::clone(&running.plan),
        shared: Arc::clone(&running.shared),
        ring: Arc::clone(&running.ring),
        generation: (1, loaded.plan.manifest_hash.clone()),
        utc_origin_ns: 0,
        clock: 2,
    };
    let state = source.state_segment(Duration::from_millis(500)).unwrap();
    let from = SystemPacer::new(clock, ClockQuality::Unsynced)
        .now()
        .mono_ns;
    std::thread::sleep(Duration::from_millis(1300));
    let to = from + 1_200_000_000;
    let bytes = source.cut(from, to, Some(&state)).unwrap();
    running.stop().unwrap();
    (bytes, loaded)
}

#[test]
fn a_recording_replays_with_zero_divergences() {
    let (file, loaded) = record("500 ms");
    let (header, rest) = Header::decode(&file).unwrap();
    assert!(header.flags & joker_core::trace::FLAG_STATE != 0);
    let items: Vec<Item<'_>> = Items::new(rest).map(Result::unwrap).collect();
    assert!(matches!(items[0], Item::State { n_apps: 1, .. }));
    let report = replay(&loaded.plan, &loaded.manifest, &NativeTasks, &file).unwrap();
    assert!(report.ticks >= 50, "{report:?}");
    assert_eq!(report.restored, vec!["light".to_owned()]);
    assert_eq!(report.before_state, 0);
    assert!(report.identical(), "{:?}", report.divergences.first());
}

#[test]
fn a_changed_app_diverges() {
    let (file, _) = record("500 ms");
    let toml = common::JOKER_TOML.replace("500 ms", "100 ms");
    let fs = common::fs(&[("joker.toml", &toml), ("joker.sim.toml", common::SIM_TOML)]);
    let other = load(&fs, std::path::Path::new("joker.sim.toml"), &NativeTasks).unwrap();
    let report = replay(&other.plan, &other.manifest, &NativeTasks, &file).unwrap();
    assert!(!report.identical());
    let d = &report.divergences[0];
    assert_eq!(d.group, "main");
    assert_ne!(d.recorded, d.replayed);
}

#[test]
fn another_registry_is_refused() {
    let (file, _) = record("500 ms");
    let fs = common::fs(&[
        (
            "joker.toml",
            &common::JOKER_TOML.replace(
                "[apps.light]",
                "[outputs.\"pi.other\"]\nsafe = false\n\n[apps.light]",
            ),
        ),
        (
            "joker.sim.toml",
            &format!(
                "{}other = {{ type = \"bool\", direction = \"output\" }}\n",
                common::SIM_TOML
            ),
        ),
    ]);
    let other = load(&fs, std::path::Path::new("joker.sim.toml"), &NativeTasks).unwrap();
    assert!(replay(&other.plan, &other.manifest, &NativeTasks, &file).is_err());
}
