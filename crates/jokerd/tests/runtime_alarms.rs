//! The runtime alarms of ref/01 §5.15 (ADR 0085): each condition from
//! what the generation publishes, and their lifecycle in the alarm book.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::sync::Arc;

use joker_core::alarm::{AlarmState, Severity};
use joker_core::outcome::{Outcome, RejectReason};
use jokerd::alarms::{AlarmBook, Annotation, RuntimeCondition};
use jokerd::load::{Loaded, load};
use jokerd::manual::{ManualTable, Occupant};
use jokerd::native::NativeTasks;
use jokerd::runtime_alarms::{MANUAL_EXPIRING_NS, WatchSources, WatchState, conditions};
use jokerd::shared::{OutputMeta, Shared};

const MANIFEST: &str = r#"
[project]
name  = "runtime-alarms"
joker = "1.0"
mode  = "dev"

[cycles.fast]
period      = "10ms"
alarm_after = 3

[outputs."pi.p_set"]
safe           = "0 kW"
max_actuations = "2/h"
verify         = { readback = "pi.p_rb", tolerance = "1 kW", within = "1s" }

[devices.pi]
driver = "sim"

[devices.pi.signals]
p_set = { type = "f64", direction = "output", unit = "kW" }
p_rb  = { type = "f64", direction = "input", unit = "kW" }

[devices.host]
driver = "sysfs"

[devices.host.signals]
cpu_temp = { type = "f64", direction = "input", thermal = "thermal_zone0", poll = "20ms" }
"#;

fn loaded() -> Loaded {
    let fs = common::fs(&[("joker.toml", MANIFEST)]);
    match load(&fs, Path::new("joker.toml"), &NativeTasks) {
        Ok(l) => l,
        Err(e) => panic!("{e}"),
    }
}

fn active(list: &[RuntimeCondition], id: &str) -> bool {
    list.iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("no condition {id} in {list:#?}"))
        .active
}

fn occupant(since: i64, until_mono: u64) -> Occupant {
    Occupant {
        principal: "user:ana".into(),
        value: serde_json::json!(10),
        ttl_ns: until_mono,
        since_utc_ns: since,
        until_utc_ns: since + until_mono as i64,
        until_mono_ns: until_mono,
        reason: "test".into(),
    }
}

#[test]
fn each_condition_follows_what_the_generation_publishes() {
    let l = loaded();
    let plan = &l.plan;
    let shared = Shared::new(plan);
    let manual = Arc::new(ManualTable::default());
    let sources = WatchSources {
        manual: Some(Arc::clone(&manual)),
        restored: vec![("pi.p_set".into(), 4)],
        historian: None,
    };
    let mut state = WatchState::default();
    let mut eval = |now| conditions(plan, &shared, &sources, &mut state, now);

    let quiet = eval(0);
    assert!(quiet.iter().all(|c| !c.active), "{quiet:#?}");

    let p_set = plan.id("pi.p_set").unwrap();
    let rejected = Outcome::Rejected(RejectReason::Budget);
    shared.output(p_set).unwrap().publish(
        0,
        OutputMeta {
            level: 0,
            outcome: rejected.code(),
            detail: rejected.detail(),
            tripped: false,
            relinquish: false,
        },
        1,
        0,
    );
    shared.set_unconfirmed(p_set, true);
    let fast = plan.groups.iter().position(|g| g.name == "fast").unwrap();
    shared.groups[fast].consecutive_overruns.set(3);
    let host = plan.devices.iter().position(|d| d.name == "host").unwrap();
    shared.devices[host].link.set(3);
    manual
        .set("pi.p_set", 4, occupant(100, MANUAL_EXPIRING_NS / 2), 0)
        .unwrap();
    let raised = eval(0);
    for id in [
        "joker.output.pi.p_set.budget",
        "joker.output.pi.p_set.unconfirmed",
        "joker.output.pi.p_set.manual_expiring",
        "joker.output.pi.p_set.manual_restored",
        "joker.group.fast.overrun",
        "joker.device.host.offline",
    ] {
        assert!(active(&raised, id), "{id} should be active");
    }
    assert!(!active(&raised, "joker.device.host.degraded"));
    assert_eq!(plan.groups[fast].alarm_after, 3);

    shared.devices[host].link.set(2);
    shared.groups[fast].consecutive_overruns.set(2);
    shared.set_unconfirmed(p_set, false);
    manual
        .set("pi.p_set", 4, occupant(200, 3 * MANUAL_EXPIRING_NS), 0)
        .unwrap();
    let later = eval(0);
    assert!(active(&later, "joker.device.host.degraded"));
    assert!(!active(&later, "joker.device.host.offline"));
    assert!(!active(&later, "joker.group.fast.overrun"));
    assert!(!active(&later, "joker.output.pi.p_set.unconfirmed"));
    assert!(!active(&later, "joker.output.pi.p_set.manual_expiring"));
    assert!(
        !active(&later, "joker.output.pi.p_set.manual_restored"),
        "a renewal confirms the restored level"
    );
}

#[test]
fn a_critical_historian_disk_raises_joker_store_disk_full() {
    let dir = tempfile::tempdir().unwrap();
    let probe: joker_store::DiskProbe = Arc::new(|_| {
        Some(joker_store::DiskSpace {
            free: 1,
            total: 1000,
        })
    });
    let historian = Arc::new(
        jokerd::historian::Historian::open_with(&dir.path().join("store"), Some(probe)).unwrap(),
    );
    historian.store().enforce_retention().unwrap();
    let l = loaded();
    let shared = Shared::new(&l.plan);
    let sources = WatchSources {
        manual: None,
        restored: Vec::new(),
        historian: Some(historian),
    };
    let list = conditions(&l.plan, &shared, &sources, &mut WatchState::default(), 0);
    assert!(active(&list, "joker.store.disk_full"));
}

#[test]
fn runtime_alarms_follow_the_lifecycle_without_delays() {
    let dir = tempfile::tempdir().unwrap();
    let book = AlarmBook::open(dir.path()).unwrap();
    let c = |active| RuntimeCondition {
        id: "joker.device.pi.offline".into(),
        severity: Severity::High,
        message: "device pi is offline".into(),
        active,
    };
    assert!(
        book.runtime(&c(false), 1).is_none(),
        "an alarm that never held stays out"
    );
    assert!(book.get("joker.device.pi.offline").is_none());
    let raised = book.runtime(&c(true), 2).unwrap();
    assert_eq!(
        (raised["kind"].as_str(), raised["to"].as_str()),
        (Some("transition"), Some("active_unacked"))
    );
    assert!(book.runtime(&c(true), 3).is_none());
    let cleared = book.runtime(&c(false), 4).unwrap();
    assert_eq!(cleared["to"], "rtn_unacked");
    let who = Annotation {
        by: "user:ana".into(),
        reason: Some("seen".into()),
        until_utc_ns: None,
    };
    let acked = book
        .runtime_command(
            "joker.device.pi.offline",
            jokerd::shared::AlarmOp::Ack,
            who.clone(),
            5,
        )
        .unwrap()
        .unwrap();
    assert_eq!(acked["to"], "normal");
    assert_eq!(acked["by"], "user:ana");
    book.runtime_command(
        "joker.device.pi.offline",
        jokerd::shared::AlarmOp::Shelve(10),
        who.clone(),
        6,
    )
    .unwrap();
    assert!(
        book.get("joker.device.pi.offline")
            .unwrap()
            .shelved
            .is_some()
    );
    assert!(book.expire_runtime_shelves(10).is_empty());
    assert_eq!(book.expire_runtime_shelves(20)[0]["kind"], "shelve_expired");
    assert!(
        book.runtime_command(
            "joker.device.pi.offline",
            jokerd::shared::AlarmOp::Unshelve,
            who,
            21
        )
        .is_err()
    );
    book.persist().unwrap();
    let again = AlarmBook::open(dir.path()).unwrap();
    let e = again.get("joker.device.pi.offline").unwrap();
    assert_eq!(
        (e.source, e.state, e.severity),
        ("runtime", AlarmState::Normal, Severity::High)
    );
}

#[test]
fn an_exhausted_budget_stays_raised_until_a_new_result_or_the_level_ends() {
    let l = loaded();
    let plan = &l.plan;
    let shared = Shared::new(plan);
    let sources = WatchSources {
        manual: None,
        restored: Vec::new(),
        historian: None,
    };
    let id = "joker.output.pi.p_set.budget";
    let p_set = plan.id("pi.p_set").unwrap();
    let status = shared.output(p_set).unwrap();
    let publish = |outcome: Outcome, level: u8, tick: u64| {
        status.publish(
            0,
            OutputMeta {
                level,
                outcome: outcome.code(),
                detail: outcome.detail(),
                tripped: false,
                relinquish: false,
            },
            tick,
            0,
        );
    };
    let eval = || conditions(plan, &shared, &sources, &mut WatchState::default(), 0);

    publish(Outcome::Rejected(RejectReason::Budget), 4, 1);
    assert!(active(&eval(), id));
    publish(Outcome::None, 4, 2);
    publish(Outcome::None, 4, 3);
    assert!(
        active(&eval(), id),
        "ticks with no new proposal keep the refused level's alarm"
    );
    publish(Outcome::None, 0, 4);
    assert!(!active(&eval(), id), "a released level clears it");

    publish(Outcome::Rejected(RejectReason::Budget), 4, 5);
    publish(Outcome::Accepted, 4, 6);
    assert!(!active(&eval(), id), "an accepted proposal clears it");
}

#[test]
fn a_group_silent_for_three_periods_raises_joker_group_stalled() {
    let l = loaded();
    let plan = &l.plan;
    let shared = Shared::new(plan);
    let sources = WatchSources {
        manual: None,
        restored: Vec::new(),
        historian: None,
    };
    let mut state = WatchState::default();
    let id = "joker.group.fast.stalled";
    let ms = 1_000_000;

    let idle = conditions(plan, &shared, &sources, &mut state, 0);
    assert!(
        !active(&idle, id),
        "a cycle loop that is not running is not stalled"
    );
    let stalled = idle.iter().find(|c| c.id == id).unwrap();
    assert_eq!(stalled.severity, Severity::Critical);

    shared
        .running
        .store(true, std::sync::atomic::Ordering::Release);
    let armed = conditions(plan, &shared, &sources, &mut state, 1_000 * ms);
    assert!(!active(&armed, id));
    let on_time = conditions(plan, &shared, &sources, &mut state, 1_030 * ms);
    assert!(!active(&on_time, id), "three periods is not yet stale");
    let late = conditions(plan, &shared, &sources, &mut state, 1_031 * ms);
    assert!(active(&late, id), "{late:#?}");

    let fast = plan.groups.iter().position(|g| g.name == "fast").unwrap();
    shared.groups[fast].last_tick_mono_ns.set(1_040 * ms);
    let ticking = conditions(plan, &shared, &sources, &mut state, 1_045 * ms);
    assert!(!active(&ticking, id), "a group that ticks again clears");
}
