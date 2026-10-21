//! End to end: apps written with the SDK compile to components, load in
//! `joker-host` from their own `joker:meta` fragments, and run cycles against
//! a joker-core process image, arbiter and envelope.
//!
//! The fixtures are workspace members; each test builds them for
//! `wasm32-wasip2` with the workspace lock, as this does by hand:
//!
//! ```sh
//! cargo build --locked --release --target wasm32-wasip2 -p joker-sdk-fixture-light
//! ```
//!
//! `fixtures/light` is the app of track part 1 §5, unchanged except that its
//! `//` comments are dropped (AGENTS.md: code has no comments).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Bench, PERIOD_MS, guest};
use joker_core::registry::{SignalMeta, UnitRef};
use joker_core::{Outcome, Quality, QualityCode, QualityReason, ValueType};
use joker_host::meta::{PortKind, TaskKind};
use joker_host::{
    AppComponent, CycleStatus, HostEngine, LogLevel, PortBinding, PortSource, RuntimeMode,
};

#[test]
fn the_track_light_app_blinks_at_1_hz() {
    let engine = HostEngine::native().unwrap();
    let app = AppComponent::compile(
        &engine,
        &guest("joker-sdk-fixture-light"),
        RuntimeMode::Prod,
    )
    .unwrap();
    let meta = app.meta();
    assert_eq!(
        (meta.name.as_str(), meta.version.as_str()),
        ("light", "0.1.0")
    );
    assert_eq!(meta.world, "joker:runtime@0.1.0");
    assert_eq!(meta.ports.len(), 2);
    assert_eq!(meta.ports[0].kind, PortKind::Output);
    assert_eq!(meta.ports[1].unit.as_deref(), Some("ms"));
    assert_eq!(meta.ports[1].default, Some(500.0));
    assert_eq!(meta.tasks.len(), 1);
    assert_eq!(meta.tasks[0].name, "blink");
    assert_eq!(meta.tasks[0].kind, TaskKind::Cycle);
    assert_eq!(
        (meta.tasks[0].period_ns, meta.tasks[0].wcet_ns),
        (Some(20_000_000), Some(5_000))
    );

    let mut bench = Bench::new(
        PERIOD_MS,
        &[SignalMeta::output(
            "pi.act_led",
            ValueType::Bool,
            UnitRef("1"),
            "main",
        )],
        None,
    );
    let led = bench.id("pi.act_led");
    let ports = [
        PortBinding {
            source: PortSource::Signal(led),
            name: "pi.act_led",
        },
        PortBinding {
            source: PortSource::Param(500f64.to_bits()),
            name: "app.light.half_period",
        },
    ];
    let mut instance = bench.instance(&app, &engine, &ports, &[0]);
    let mut toggles = Vec::new();
    let mut last = 0;
    for k in 0..100 {
        let (reports, log) = bench.tick(&mut instance, k, &[0]);
        assert_eq!(reports[0].status, CycleStatus::Ok);
        assert_eq!(reports[0].staged, 1);
        assert_eq!(log.writes.len(), 1);
        assert_eq!(log.writes[0].outcome, Outcome::Accepted);
        let now = bench.applied("pi.act_led");
        if now != last {
            toggles.push(k);
            last = now;
        }
    }
    assert_eq!(toggles, vec![24, 49, 74, 99]);
}

#[test]
fn reads_return_results_and_substitutions_are_noted() {
    let engine = HostEngine::native().unwrap();
    let app = AppComponent::compile(
        &engine,
        &guest("joker-sdk-fixture-probe"),
        RuntimeMode::Prod,
    )
    .unwrap();
    let meta = app.meta();
    assert_eq!(meta.proposal_capacity, Some(1024));
    let names: Vec<&str> = meta.tasks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["regulate", "watch"]);
    let mut bench = Bench::new(
        100,
        &[
            SignalMeta::input("room.temp", ValueType::F64, UnitRef("°C")),
            SignalMeta::output("room.alarm", ValueType::Bool, UnitRef("1"), "main"),
            SignalMeta::output("room.heater", ValueType::F64, UnitRef("%"), "main"),
        ],
        Some(("room.heater", 0.0, 50.0)),
    );
    let ports = [
        PortBinding {
            source: PortSource::Signal(bench.id("room.temp")),
            name: "room.temp",
        },
        PortBinding {
            source: PortSource::Param(30f64.to_bits()),
            name: "app.probe.limit",
        },
        PortBinding {
            source: PortSource::Signal(bench.id("room.alarm")),
            name: "room.alarm",
        },
        PortBinding {
            source: PortSource::Signal(bench.id("room.heater")),
            name: "room.heater",
        },
    ];
    let mut instance = bench.instance(&app, &engine, &ports, &[0, 1]);
    let good = Quality::GOOD;

    bench.input("room.temp", 20.0, good, 0);
    let (r, _) = bench.tick(&mut instance, 0, &[0]);
    assert_eq!((r[0].status, r[0].staged), (CycleStatus::Ok, 2));
    assert_eq!(f64::from_bits(bench.applied("room.heater")), 10.0);
    assert_eq!(bench.applied("room.alarm"), 0);

    bench.input("room.temp", 35.0, good, 1);
    bench.tick(&mut instance, 1, &[0, 1]);
    assert_eq!(bench.applied("room.alarm"), 1);
    assert_eq!(f64::from_bits(bench.applied("room.heater")), 0.0);

    bench.input(
        "room.temp",
        35.0,
        Quality::new(QualityCode::Bad, QualityReason::CommFailure),
        2,
    );
    let (r, _) = bench.tick(&mut instance, 2, &[0]);
    assert_eq!((r[0].status, r[0].staged), (CycleStatus::AppError, 1));
    assert_eq!(
        instance.last_error(),
        Some(("J-E0102", "port `temp` is bad(comm_failure)"))
    );
    assert_eq!(instance.notes(), &[0]);
    assert_eq!(bench.applied("room.alarm"), 0);

    bench.input("room.temp", -30.0, good, 3);
    let (_, log) = bench.tick(&mut instance, 3, &[0, 1]);
    assert!(
        log.writes
            .iter()
            .any(|w| matches!(w.outcome, Outcome::Rejected(_)))
    );
    let (r, _) = bench.tick(&mut instance, 4, &[0, 1]);
    assert_eq!(r[1].status, CycleStatus::AppError);
    assert_eq!(
        instance.last_error(),
        Some(("J-E0050", "the heater proposal was not applied"))
    );
}

#[test]
fn allocating_in_a_cycle_task_traps_with_j_e0903() {
    let engine = HostEngine::native().unwrap();
    let app = AppComponent::compile(
        &engine,
        &guest("joker-sdk-fixture-greedy"),
        RuntimeMode::Prod,
    )
    .unwrap();
    let mut bench = Bench::new(
        PERIOD_MS,
        &[SignalMeta::output(
            "greedy.kept",
            ValueType::U64,
            UnitRef("1"),
            "main",
        )],
        None,
    );
    let ports = [PortBinding {
        source: PortSource::Signal(bench.id("greedy.kept")),
        name: "greedy.kept",
    }];
    let mut instance = bench.instance(&app, &engine, &ports, &[0]);
    for k in 0..4 {
        let (r, _) = bench.tick(&mut instance, k, &[0]);
        assert_eq!((r[0].status, r[0].staged), (CycleStatus::Ok, 1));
        assert_eq!(bench.applied("greedy.kept"), k + 1);
    }
    let (r, log) = bench.tick(&mut instance, 4, &[0]);
    assert_eq!(r[0].status, CycleStatus::Trap("J-E0903"));
    assert_eq!(r[0].staged, 0);
    assert!(log.writes.is_empty());
    assert!(instance.is_poisoned());
    let mut records = Vec::new();
    instance.drain_log(|r| records.push((r.level, r.message.to_owned())));
    assert_eq!(
        records,
        [(
            LogLevel::Error,
            "J-E0903: heap allocation during a cycle task".to_owned()
        )]
    );
}
