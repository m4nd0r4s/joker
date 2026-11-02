//! The parity app `peak_shaver` of ref/05 §20.1 (Rust column) runs in the
//! harness: the cycle task shaves the import, `replan` plans from the
//! historian every 15 minutes with its jitter, and `watch_soc` emits
//! `low_soc` on every change of a low state of charge.

use joker::prelude::*;
use joker::{Record, UtcSeconds};
use joker_sdk_fixture_peak_shaver::{LowSoc, PeakShaver};
use joker_test::prelude::*;

/// The schema of `Plan`, read back from the internal.
#[derive(Record, Clone, Copy, Debug, PartialEq)]
struct PlanView {
    target_soc: f64,
    limit_kw: f64,
    valid_until: UtcSeconds,
}

fn mins(n: u64) -> Duration {
    Duration::from_secs(60 * n)
}

const START_S: i64 = 1_767_225_600;

#[test]
fn the_battery_shaves_the_import_above_the_limit() {
    let mut h = Harness::<PeakShaver>::new()
        .input("grid_power", 600.0)
        .input("soc", 0.5)
        .history("grid_power", 900.0);
    h.run_for(Duration::from_secs(10));
    let out = h.output("battery");
    assert!(out.all(|v| v > 199.9), "{out:?}");
    assert!(
        out.values()
            .zip(out.values().skip(1))
            .all(|(a, b)| b.as_f64() >= a.as_f64())
    );
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    assert!(h.alarm("plan_stale").active_at(Duration::ZERO));
}

#[test]
fn replan_runs_every_15_minutes_and_the_plan_clears_the_alarm() {
    let mut h = Harness::<PeakShaver>::new()
        .input("grid_power", 300.0)
        .input("soc", 0.5)
        .history("grid_power", 900.0)
        .seed(7);
    h.run_until(mins(14));
    assert!(h.internal_value::<PlanView>("plan").is_none());
    assert!(h.alarm("plan_stale").active());
    h.run_until(mins(16));
    let plan = h
        .internal_value::<PlanView>("plan")
        .expect("replan ran once");
    assert_eq!(plan.limit_kw, 400.0);
    assert_eq!(plan.target_soc, 0.95);
    let replanned = plan.valid_until.0 - 30 * 60 - START_S;
    assert!((900..=930).contains(&replanned), "{replanned}");
    let alarm = h.alarm("plan_stale");
    assert_eq!(alarm.raised(), [Duration::ZERO]);
    let cleared = alarm.cleared();
    assert_eq!(cleared.len(), 1);
    assert!(cleared[0] > mins(15) && cleared[0] <= mins(15) + Duration::from_secs(31));
    assert_eq!(h.output("battery").last(), Some(0.0));
    assert!(h.errors().is_empty(), "{:?}", h.errors());
}

#[test]
fn a_low_state_of_charge_emits_low_soc_and_stops_the_battery() {
    let mut h = Harness::<PeakShaver>::new()
        .input("grid_power", 600.0)
        .input("soc", Series::step(0.5, 0.04, Duration::from_secs(2)))
        .history("grid_power", 900.0);
    h.run_for(Duration::from_secs(3));
    let events = h.events("low_soc").decode::<LowSoc>();
    assert_eq!(events.len(), 5);
    assert_eq!(events[0].0, Duration::from_secs(2));
    assert!(events.iter().all(|(_, e)| e.soc == 0.04));
    let out = h.output("battery");
    assert!(out.before(Duration::from_secs(2)).all(|v| v > 0.0));
    assert!(out.after(Duration::from_secs(2)).all(|v| v == 0.0));
}

#[test]
fn a_bad_grid_power_ends_the_tick_without_a_proposal() {
    let mut h = Harness::<PeakShaver>::new()
        .input(
            "grid_power",
            Series::constant(600.0)
                .with_quality_at(Duration::from_secs(1), Quality::bad(CommFailure)),
        )
        .input("soc", 0.5)
        .safe("battery", 0.0);
    h.run_for(Duration::from_secs(2));
    let out = h.output("battery");
    assert!(
        out.between(Duration::from_secs(1), Duration::from_secs(2))
            .no_new_writes()
    );
    assert_eq!(out.last(), Some(0.0));
    let first = &h.errors()[0];
    assert_eq!(
        (first.at, first.task.as_str(), first.code.as_str()),
        (Duration::from_secs(1), "control", "J-E0102")
    );
}

#[test]
fn the_same_run_gives_the_same_trace() {
    let run = || {
        let mut h = Harness::<PeakShaver>::new()
            .input("grid_power", Series::sine(500.0, 200.0, mins(7)))
            .input("soc", Series::ramp(0.6, 0.02, mins(20)))
            .history("grid_power", Series::sine(500.0, 300.0, mins(60)))
            .seed(3);
        h.run_until(mins(31));
        (
            h.output("battery"),
            h.events("low_soc"),
            h.alarm("plan_stale"),
            h.internal_value::<PlanView>("plan"),
        )
    };
    let first = run();
    assert!(!first.1.is_empty());
    assert!(first.3.is_some());
    assert_eq!(first, run());
}
