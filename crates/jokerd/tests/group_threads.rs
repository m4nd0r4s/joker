//! Per-group state and the delivery of an alarm's trip to the group that
//! owns the output (ADR 0089 decision 3).

#![allow(clippy::unwrap_used, clippy::panic)]

use joker_core::time::{ClockQuality, Instant};
use joker_core::{Quality, SignalId};
use joker_manifest::OsFileSystem;

use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::plan::Plan;
use jokerd::shared::{Shared, TRIP_CAPACITY, TripRequest};

/// An alarm of `fast` (10 ms) trips `pi.led`, an output of `slow` (100 ms).
const JOKER_TOML: &str = r#"
[project]
name  = "group-threads"
joker = "1.0"
mode  = "dev"

[cycles.fast]
period = "10ms"

[cycles.slow]
period = "100ms"

[devices.pi]
driver = "sim"

[devices.pi.signals]
temp = { type = "f64", direction = "input", unit = "°C" }
led  = { type = "bool", direction = "output" }

[outputs."pi.led"]
safe  = false
group = "slow"

[alarms.hot]
when     = "pi.temp > 5 °C"
severity = "high"
group    = "fast"
actions  = { trip = ["pi.led"] }
"#;

fn plan() -> Plan {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    loaded.plan
}

fn hot(plan: &Plan, shared: &Shared) {
    let temp = plan.id("pi.temp").unwrap();
    shared.slots[temp.index()].write(300f64.to_bits(), Quality::GOOD, i64::MIN, 999_000_000);
}

fn run(plan: &Plan, shared: &Shared, ticks: u64) {
    jokerd::cycle::run(
        plan,
        shared,
        plan.groups.iter().map(|_| Vec::new()).collect(),
        &mut jokerd::pacer::VirtualPacer::new(Instant {
            mono_ns: 1_000_000_000,
            utc_ns: 1_760_000_000_000_000_000,
            clock: ClockQuality::Synced,
        }),
        jokerd::cycle::RunOptions {
            max_ticks: Some(ticks),
            ..jokerd::cycle::RunOptions::default()
        },
    )
    .unwrap();
}

fn tripped(plan: &Plan, shared: &Shared) -> bool {
    let (_, meta, _) = shared
        .output(plan.id("pi.led").unwrap())
        .unwrap()
        .read()
        .unwrap();
    meta.tripped
}

#[test]
fn an_alarm_trips_an_output_of_another_group() {
    let plan = plan();
    let shared = Shared::new(&plan);
    hot(&plan, &shared);
    run(&plan, &shared, 25);
    assert!(tripped(&plan, &shared));
    assert_eq!(shared.trip_requests_dropped.get(), 0);
}

#[test]
fn a_dropped_trip_request_is_counted_and_the_active_alarm_still_trips() {
    let plan = plan();
    let shared = Shared::new(&plan);
    let slow = plan.groups.iter().position(|g| g.name == "slow").unwrap();
    let not_an_output: SignalId = plan.id("pi.temp").unwrap();
    for _ in 0..TRIP_CAPACITY {
        shared.trips[slow]
            .push(TripRequest {
                signal: not_an_output,
            })
            .unwrap();
    }
    hot(&plan, &shared);
    run(&plan, &shared, 25);
    assert_eq!(shared.trip_requests_dropped.get(), 1);
    assert!(tripped(&plan, &shared));
}

#[test]
fn a_cold_alarm_trips_nothing() {
    let plan = plan();
    let shared = Shared::new(&plan);
    let temp = plan.id("pi.temp").unwrap();
    shared.slots[temp.index()].write(1f64.to_bits(), Quality::GOOD, i64::MIN, 999_000_000);
    run(&plan, &shared, 25);
    assert!(!tripped(&plan, &shared));
}

#[test]
fn rate_monotonic_priorities_follow_the_periods() {
    assert_eq!(
        jokerd::cycle::rt_priorities(&[100, 10, 100, 1_000]),
        [89, 90, 89, 88]
    );
    let many: Vec<u64> = (1..=100).collect();
    assert_eq!(*jokerd::cycle::rt_priorities(&many).last().unwrap(), 2);
}
