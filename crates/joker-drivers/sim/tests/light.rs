//! The `sim` driver end to end: plan, open, connect, committed writes from a
//! real envelope, polled reads on a virtual clock.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use joker_core::ValueType;
use joker_driver_sim::{SimDriver, SimPlanner};
use joker_hal::acquire::{OwnedRaw, Settled};
use joker_hal::cbor::Value;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, Event, OutputHarness, VirtualHost};
use joker_hal::{
    CommitBox, CommitRx, CommitTx, Link, Planner, RawEncoding, RawKind, RawStatus, Reason, RxState,
    WriteStatus,
};

const PI: &str = include_str!("models/pi.sim.toml");
const ESP: &str = include_str!("models/esp.sim.toml");
const MS: u64 = 1_000_000;

fn signal(name: &str) -> Vec<u8> {
    Value::map([("signal", Value::text(name))]).encode()
}

fn device(name: &str, model: &str) -> OwnedDevice {
    let config = Value::map([
        ("model", Value::text(model)),
        ("params", Value::Map(vec![])),
    ])
    .encode();
    OwnedDevice::new(name, config, 1_000 * MS)
}

fn pi_request() -> OwnedRequest {
    OwnedRequest {
        bus: None,
        devices: vec![device("pi", PI)],
        points: vec![
            OwnedPoint::output(0, "act_led", RawKind::Bool, 0, "bool", signal("act_led")),
            OwnedPoint::input(
                1,
                "cpu_temp",
                RawKind::Float,
                0,
                "f64",
                signal("cpu_temp"),
                1_000 * MS,
            ),
        ],
    }
}

fn floats(events: &[Event]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Read { points, .. } => Some(points.clone()),
            _ => None,
        })
        .flatten()
        .filter_map(|s: Settled| match s.value {
            OwnedRaw::Float(f) => Some(f.to_bits()),
            _ => None,
        })
        .collect()
}

fn run_light() -> (Vec<u64>, Vec<WriteStatus>) {
    let req = pi_request();
    let views = req.views();
    let plan = SimPlanner::plan(&views.request()).unwrap();
    assert_eq!(
        joker_hal::plan::validate(&views.request(), &plan.view()),
        Ok(())
    );
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, SimDriver> =
        Bench::open(host, views.request(), plan.view(), Link::None).unwrap();
    bench.connect().unwrap();
    let handle = bench.driver.handle();
    let load = handle.plant().model().signal("load").unwrap();
    handle.plant().set(load, 1.0);

    let mut outputs = OutputHarness::new(&[("pi.act_led", ValueType::Bool, 0, 0)]).unwrap();
    let bindings = outputs.bindings(&[RawEncoding::Bool]);
    let boxes = [CommitBox::new()];
    let mut last = [None];
    let mut state = [RxState::default()];
    let mut tx = CommitTx::new(outputs.outputs(), &bindings, &boxes, &mut last).unwrap();
    let mut rx = CommitRx::new(&bindings, &boxes, &mut state).unwrap();
    let mut acks = Vec::new();
    let mut temps = Vec::new();
    for second in 0..20u64 {
        outputs.manual("pi.act_led", second % 2);
        tx.publish(outputs.outputs());
        let mut writes = Vec::new();
        rx.drain_into(&mut writes);
        let events = bench
            .run_until((second + 1) * 1_000 * MS, |_| std::mem::take(&mut writes))
            .unwrap();
        for e in &events {
            if let Event::Write { acks: a, .. } = e {
                acks.extend(a.iter().map(|a| a.status));
            }
        }
        temps.extend(floats(&events));
    }
    (temps, acks)
}

#[test]
fn light_runs_against_pi_sim_deterministically() {
    let (a, acks) = run_light();
    let (b, _) = run_light();
    assert_eq!(a, b);
    assert_eq!(a.len(), 21);
    assert!(acks.iter().all(|s| *s == WriteStatus::Ok));
    assert_eq!(acks.len(), 20);
    let temps: Vec<f64> = a.iter().map(|b| f64::from_bits(*b)).collect();
    assert_eq!(temps[0], 45.0);
    assert!(temps.windows(2).all(|w| w[1] > w[0]));
    assert!(
        (temps[20] - (45.0 + 30.0 * (1.0 - (-20.0f64 / 60.0).exp()))).abs() < 0.05,
        "{temps:?}"
    );
}

#[test]
fn the_pi_model_has_the_sysfs_names_and_types() {
    let model = joker_driver_sim::parse(PI, &Default::default()).unwrap();
    let led = &model.signals[model.signal("act_led").unwrap()];
    let temp = &model.signals[model.signal("cpu_temp").unwrap()];
    assert_eq!(
        (led.ty, led.direction),
        (
            joker_driver_sim::model::SimType::Bool,
            joker_driver_sim::model::SimDirection::Output
        )
    );
    assert_eq!(
        (temp.ty, temp.unit.as_str()),
        (joker_driver_sim::model::SimType::F64, "°C")
    );
}

#[test]
fn the_esp_disconnect_key_gives_comm_failure() {
    let req = OwnedRequest {
        bus: None,
        devices: vec![device("esp", ESP)],
        points: vec![
            OwnedPoint::input(
                0,
                "button",
                RawKind::Bool,
                0,
                "bool",
                signal("button"),
                100 * MS,
            ),
            OwnedPoint::input(
                1,
                "touch",
                RawKind::Uint,
                0,
                "u64",
                signal("touch"),
                100 * MS,
            ),
            OwnedPoint::input(
                2,
                "uptime",
                RawKind::Float,
                0,
                "f64",
                signal("uptime"),
                100 * MS,
            ),
        ],
    };
    let views = req.views();
    let plan = SimPlanner::plan(&views.request()).unwrap();
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, SimDriver> =
        Bench::open(host, views.request(), plan.view(), Link::None).unwrap();
    bench.connect().unwrap();
    let handle = bench.driver.handle();
    let before = bench.read(0).unwrap();
    assert!(before.iter().all(|s| s.status == RawStatus::OK));
    assert!(handle.plant().press("d", 0));
    let after = bench.read(0).unwrap();
    assert!(
        after
            .iter()
            .all(|s| s.status == RawStatus::bad(Reason::COMM_FAILURE) && s.value == OwnedRaw::None)
    );
}

#[test]
fn plan_refuses_points_the_model_does_not_have() {
    let mut req = pi_request();
    req.points.push(OwnedPoint::input(
        2,
        "fan",
        RawKind::Float,
        0,
        "f64",
        signal("fan"),
        MS,
    ));
    req.points.push(OwnedPoint::input(
        3,
        "act_led",
        RawKind::Float,
        0,
        "f64",
        signal("cpu_temp"),
        MS,
    ));
    let views = req.views();
    let errors = SimPlanner::plan(&views.request()).unwrap_err();
    let codes: Vec<&str> = errors.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["J0853"]);
    req.points.remove(2);
    req.points[2].expect = RawKind::Int;
    let views = req.views();
    let errors = SimPlanner::plan(&views.request()).unwrap_err();
    assert_eq!(errors[0].code.as_str(), "J0809");
}
