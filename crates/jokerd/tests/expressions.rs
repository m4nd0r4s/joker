//! AUR-417: the manifest's expressions reach live signals in jokerd. An
//! `interlock` gates an output, `requires_good` rejects on quality, an
//! expression `range` limit in another unit clamps, a `calc` feeds an
//! alarm that trips an output, and `reset_when` ends the trip, all
//! through the real cycle loop (docs/16 E3–E7, ADR 0068).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::sync::Arc;

use joker_core::image::{Proposal, Source};
use joker_core::registry::Direction;
use joker_core::time::{ClockQuality, Instant};
use joker_core::value::NO_TS_SRC;
use joker_core::{Outcome, Quality, RejectReason, Sample, SignalId};
use joker_units::Duration;
use jokerd::cycle::{RunOptions, run};
use jokerd::expr_plan::{CALC_GROUP, Expect, compile_expr, permit_period};
use jokerd::load::{Loaded, instantiate, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::{Pacer, VirtualPacer};
use jokerd::plan::Plan;
use jokerd::shared::Shared;

const MS: u64 = 1_000_000;

const JOKER_TOML: &str = r#"
[project]
name  = "expressions"
joker = "1.0"
mode  = "dev"

[devices.pi]
driver = "sim"

[devices.pi.signals]
act_led = { type = "bool", direction = "output" }
enable  = { type = "bool", direction = "input" }
load    = { type = "f64", direction = "input", unit = "kW" }
p_set   = { type = "f64", direction = "output", unit = "kW" }
p_max   = { type = "f64", direction = "input", unit = "W" }

[outputs."pi.act_led"]
safe      = false
interlock = "pi.enable"

[outputs."pi.p_set"]
safe          = "0 kW"
group         = "main"
requires_good = ["pi.load"]
range         = ["0 kW", "pi.p_max"]
reset_when    = "!alarm.overheat.active"

[cycles.main]
period = "20ms"

[apps.light]
path   = "apps/light"
bind   = { led = "pi.act_led" }
params = { half_period = "500 ms" }
cycles = { blink = "main" }

[calc]
hot = "pi.load > 50 kW"

[alarms.overheat]
when     = "calc.hot"
severity = "high"
actions  = { trip = ["pi.p_set"] }
"#;

fn loaded(text: &str) -> Loaded {
    let fs = common::fs(&[("joker.toml", text)]);
    match load(&fs, Path::new("joker.toml"), &NativeTasks) {
        Ok(l) => l,
        Err(e) => panic!("{e}"),
    }
}

fn start() -> Instant {
    Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }
}

/// A virtual pacer that calls `hook(k, shared)` right before tick `k` of
/// the plan's single group: it can publish inputs for tick `k` and read
/// what tick `k - 1` applied.
struct Scripted<F> {
    inner: VirtualPacer,
    shared: Arc<Shared>,
    tick: u64,
    hook: F,
}

impl<F: FnMut(u64, &Shared) + Send> Pacer for Scripted<F> {
    fn now(&self) -> Instant {
        self.inner.now()
    }

    fn wait_until(&mut self, mono_ns: u64) {
        self.inner.wait_until(mono_ns);
        (self.hook)(self.tick, &self.shared);
        self.tick += 1;
    }
}

fn publish(shared: &Shared, id: SignalId, bits: u64) {
    shared.slots[id.index()].write(bits, Quality::GOOD, NO_TS_SRC, 0);
}

fn sample(shared: &Shared, id: SignalId) -> Sample {
    shared.slots[id.index()].read(8).unwrap()
}

/// (outcome, applied bits, tripped) of an output after its last tick.
fn status(shared: &Shared, id: SignalId) -> (Outcome, u64, bool) {
    let (applied, meta, _) = shared.output(id).unwrap().read().unwrap();
    (
        Outcome::from_code(meta.outcome, meta.detail).unwrap(),
        applied,
        meta.tripped,
    )
}

#[derive(Debug, Default, Clone)]
struct Seen {
    led: Vec<(u64, Outcome, u64)>,
    p_set: Vec<(u64, Outcome, f64, bool)>,
    hot: Vec<(u64, u64)>,
    active: Vec<(u64, u64)>,
}

#[test]
fn interlock_quality_range_calc_alarm_and_reset_run_in_the_cycle() {
    let l = loaded(JOKER_TOML);
    let plan = &l.plan;
    let id = |p: &str| plan.id(p).unwrap();
    let (led, enable, load, p_set, p_max) = (
        id("pi.act_led"),
        id("pi.enable"),
        id("pi.load"),
        id("pi.p_set"),
        id("pi.p_max"),
    );
    let (hot, active) = (id("calc.hot"), id("alarm.overheat.active"));
    let manual = plan.writer("user:manual:4").unwrap();
    let shared = Arc::new(Shared::new(plan));
    let write = Proposal {
        writer: manual,
        signal: p_set,
        value: 5.0f64.to_bits(),
        source: Source::Async,
        ttl_ns: 3_600_000 * MS,
        ..Proposal::EMPTY
    };
    publish(&shared, enable, 0);
    publish(&shared, p_max, 3_000.0f64.to_bits());

    let seen = Arc::new(std::sync::Mutex::new(Seen::default()));
    let record = Arc::clone(&seen);
    let mut pacer = Scripted {
        inner: VirtualPacer::new(start()),
        shared: Arc::clone(&shared),
        tick: 0,
        hook: move |k: u64, s: &Shared| {
            if k > 0 {
                let done = k - 1;
                let mut seen = record.lock().unwrap();
                let (o, bits, _) = status(s, led);
                seen.led.push((done, o, bits));
                let (o, bits, tripped) = status(s, p_set);
                seen.p_set.push((done, o, f64::from_bits(bits), tripped));
                seen.hot.push((done, sample(s, hot).transport_bits()));
                seen.active.push((done, sample(s, active).transport_bits()));
            }
            s.inboxes[0].push(write).unwrap();
            match k {
                5 => {
                    publish(s, enable, 1);
                    publish(s, load, 20.0f64.to_bits());
                }
                10 => publish(s, load, 80.0f64.to_bits()),
                15 => publish(s, load, 20.0f64.to_bits()),
                _ => {}
            }
        },
    };
    let tasks = instantiate(plan, &l.manifest, &NativeTasks).unwrap();
    run(
        plan,
        &shared,
        tasks,
        &mut pacer,
        RunOptions {
            max_ticks: Some(20),
            ..RunOptions::default()
        },
    )
    .unwrap();

    let seen = seen.lock().unwrap().clone();
    let at = |list: &[(u64, Outcome, u64)], k: u64| *list.iter().find(|e| e.0 == k).unwrap();
    let p_at = |k: u64| *seen.p_set.iter().find(|e| e.0 == k).unwrap();
    let flag_at = |list: &[(u64, u64)], k: u64| list.iter().find(|e| e.0 == k).unwrap().1;

    assert_eq!(
        at(&seen.led, 2),
        (2, Outcome::Rejected(RejectReason::Interlock), 0),
        "an interlock that reads false keeps the output at its safe state"
    );
    assert_eq!(
        at(&seen.led, 6),
        (6, Outcome::Accepted, 1),
        "once the interlock reads true the app's write applies"
    );

    assert_eq!(
        p_at(2),
        (2, Outcome::Rejected(RejectReason::Quality), 0.0, false),
        "a requires_good signal that was never good rejects with Quality"
    );
    assert_eq!(
        p_at(6),
        (6, Outcome::ClampedRange, 3.0, false),
        "the 3000 W limit clamps the 5 kW manual write to 3 kW"
    );
    assert_eq!(flag_at(&seen.hot, 6), 0);
    assert_eq!(flag_at(&seen.active, 6), 0);

    assert_eq!(flag_at(&seen.hot, 10), 1, "the calc follows its input");
    assert_eq!(flag_at(&seen.active, 10), 1, "the calc raises the alarm");
    assert_eq!(
        p_at(10),
        (10, Outcome::Tripped, 0.0, true),
        "the alarm trips the output in the same tick"
    );
    assert_eq!(p_at(14), (14, Outcome::Tripped, 0.0, true));

    assert_eq!(flag_at(&seen.active, 15), 0);
    assert_eq!(
        p_at(15),
        (15, Outcome::ClampedRange, 3.0, false),
        "reset_when ends the trip once the alarm is no longer active"
    );
}

#[test]
fn derived_signals_take_the_group_that_consumes_them() {
    let l = loaded(JOKER_TOML);
    let plan = &l.plan;
    assert_eq!(plan.calcs.len(), 1);
    assert_eq!(plan.calcs[0].group, "main");
    assert_eq!(plan.alarms.len(), 1);
    assert_eq!(plan.alarms[0].group, "main");
    assert_eq!(plan.alarms[0].trip, vec![plan.id("pi.p_set").unwrap()]);
    let hot = plan.signal("calc.hot").unwrap();
    assert_eq!(hot.direction, Direction::Derived);
    assert_eq!(hot.group.as_deref(), Some("main"));
    assert!(!plan.groups.iter().any(|g| g.name == CALC_GROUP));
    let ports = &plan.groups[0].ports;
    for read in ["pi.enable", "pi.load", "pi.p_max"] {
        assert!(ports.contains(&read.to_owned()), "{read} is latched");
    }
    assert!(
        !ports.contains(&"calc.hot".to_owned()),
        "a derived signal of the group is read from its slot"
    );
}

fn with(extra: &str) -> Plan {
    loaded(&format!("{JOKER_TOML}\n{extra}")).plan
}

#[test]
fn a_calc_nothing_reads_runs_in_the_implicit_calc_group() {
    let plan = with("[calc.idle]\nexpr = \"pi.load * 2\"\n");
    let idle = plan.calcs.iter().find(|c| c.path == "calc.idle").unwrap();
    assert_eq!(idle.group, CALC_GROUP);
    let group = plan.groups.iter().find(|g| g.name == CALC_GROUP).unwrap();
    assert_eq!(group.period_ns, 100 * MS);
    assert!(group.ports.contains(&"pi.load".to_owned()));
}

#[test]
fn windows_are_sized_with_the_evaluating_groups_period() {
    let plan = with("[alarms.warm]\nwhen = \"avg(pi.load, 1s) > 30 kW\"\nseverity = \"low\"\n");
    let warm = plan.alarms.iter().find(|a| a.name == "warm").unwrap();
    assert_eq!(warm.group, CALC_GROUP);
    let at = |ms| {
        compile_expr(
            &warm.when.source,
            &plan,
            Duration::from_millis(ms),
            Expect::Bool,
        )
        .unwrap()
        .program
        .state_words()
    };
    assert_eq!(warm.when.program.state_words(), at(100));
    assert_ne!(at(100), at(20));
}

#[test]
fn a_permit_is_sized_with_its_devices_fastest_output_group() {
    let plan = with("");
    assert_eq!(permit_period(&plan, "pi"), Duration::from_millis(20));
    assert_eq!(permit_period(&plan, "nobody"), Duration::from_millis(100));
}

#[test]
fn an_interlock_that_does_not_compile_stops_the_load() {
    let text = JOKER_TOML.replace("interlock = \"pi.enable\"", "interlock = \"pi.load\"");
    let fs = common::fs(&[("joker.toml", &text)]);
    let err = load(&fs, Path::new("joker.toml"), &NativeTasks).unwrap_err();
    assert!(err.to_string().contains("J0304"), "{err}");
}
