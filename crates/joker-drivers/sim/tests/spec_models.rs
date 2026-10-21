//! Every simulation model of the specification (ref/03 §2.4, document 04
//! §6, example 00 and the learning track) loads and runs, and the complete
//! model language gives the same bits on every run.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;

use joker_driver_sim::model::{Effect, Gen, SimDirection, SimType};
use joker_driver_sim::plant::{Reading, fnv1a};
use joker_driver_sim::{Inputs, Model, Plant, parse, parse_with};
use joker_hal::{RawStatus, Reason};

const MS: u64 = 1_000_000;
const S: u64 = 1_000 * MS;

const TANK_SDK: &str = include_str!("spec/tank-sdk.sim.toml");
const TANK_FIRST: &str = include_str!("spec/tank-first.sim.toml");
const TEMP_SENSOR: &str = include_str!("spec/temp_sensor.sim.toml");
const ESP_REF03: &str = include_str!("spec/esp-ref03.sim.toml");
const PI_PART1: &str = include_str!("spec/pi-part1.sim.toml");
const PI_PART3: &str = include_str!("spec/pi-part3.sim.toml");
const ESP_PART4: &str = include_str!("spec/esp-part4.sim.toml");
const ESP_PART7: &str = include_str!("spec/esp-part7.sim.toml");
const ESP_PROFILE: &str = include_str!("spec/esp-profile.sim.toml");
const BENCH_PROFILE: &str = include_str!("spec/bench-esp32@1.profile.toml");
const EVERYTHING: &str = include_str!("spec/everything.sim.toml");

fn model(src: &str) -> Model {
    parse(src, &BTreeMap::new()).unwrap_or_else(|e| panic!("{e}"))
}

fn with_profile(src: &str) -> Model {
    let inputs = Inputs {
        profile: Some(String::from(BENCH_PROFILE)),
        ..Inputs::default()
    };
    parse_with(src, &inputs).unwrap_or_else(|e| panic!("{e}"))
}

fn num(p: &mut Plant, t: u64, name: &str) -> f64 {
    let i = p.model().signal(name).unwrap();
    p.advance_to(t);
    match p.read(t, i) {
        Reading::Value(Some(v), _) => v,
        other => panic!("{name} at {t}: {other:?}"),
    }
}

fn trace(m: Model, seconds: u64, step_ms: u64) -> Vec<u64> {
    let names: Vec<String> = m
        .signals
        .iter()
        .filter(|s| s.direction != SimDirection::Output && !s.ty.is_text())
        .map(|s| s.name.clone())
        .collect();
    let ids: Vec<usize> = names.iter().map(|n| m.signal(n).unwrap()).collect();
    let mut p = Plant::new(m, fnv1a("dev"));
    p.start(0);
    let mut out = Vec::new();
    for k in 0..=(seconds * 1000 / step_ms) {
        let now = k * step_ms * MS;
        p.advance_to(now);
        for &i in &ids {
            match p.read(now, i) {
                Reading::Down => out.push(u64::MAX),
                Reading::Value(Some(v), s) => {
                    out.push(v.to_bits());
                    out.push(u64::from(s.reason.0));
                }
                Reading::Value(None, s) => out.push(u64::from(s.reason.0) << 32),
            }
        }
    }
    out
}

#[test]
fn every_model_of_the_specification_loads_and_runs() {
    let models = [
        ("docs/04 §6 tank", model(TANK_SDK)),
        ("example 00 tank", model(TANK_FIRST)),
        ("ref/03 §2.4.11 temp_sensor", model(TEMP_SENSOR)),
        ("ref/03 §2.4.12 esp", model(ESP_REF03)),
        ("track 1 pi", model(PI_PART1)),
        ("track 3 pi", model(PI_PART3)),
        ("track 4 esp", model(ESP_PART4)),
        ("track 7 esp with classes", model(ESP_PART7)),
        ("esp from its profile", with_profile(ESP_PROFILE)),
    ];
    for (name, m) in models {
        let a = trace(m.clone(), 600, 1000);
        assert_eq!(a, trace(m, 600, 1000), "{name} is not deterministic");
        assert!(
            !a.is_empty() || name == "track 1 pi",
            "{name} reads nothing"
        );
    }
}

#[test]
fn the_tank_fills_drains_and_faults_on_schedule() {
    let m = model(TANK_SDK);
    let pump = m.signal("pump").unwrap();
    let level = m.signal("level").unwrap();
    let mut p = Plant::new(m, 1);
    p.start(0);
    let start = num(&mut p, 0, "level");
    assert_eq!(start, 1.0);
    let filled = num(&mut p, 150 * S, "level");
    let inflow_avg = 2.0
        + 1.0 * (1.0 - (2.0 * std::f64::consts::PI * 150.0 / 600.0).cos()) * 600.0
            / (2.0 * std::f64::consts::PI * 150.0);
    let expected = 1.0 + inflow_avg * 150.0 / 3600.0 / 3.2;
    assert!((filled - expected).abs() < 1e-3, "{filled} vs {expected}");
    p.apply(pump, 100.0);
    let draining = num(&mut p, 250 * S, "level");
    let at_300 = num(&mut p, 299 * S, "level");
    assert!(at_300 < draining, "with the pump at 100 % the level drops");
    p.advance_to(300 * S + 10 * S);
    assert_eq!(
        p.read(310 * S, level),
        Reading::Value(None, RawStatus::bad(Reason::COMM_FAILURE))
    );
    p.advance_to(321 * S);
    assert!(matches!(p.read(321 * S, level), Reading::Value(Some(_), s) if s == RawStatus::OK));
    let empty = num(&mut p, 4 * 3_600 * S, "level");
    assert_eq!(empty, 0.0, "integrate stops at min = 0 m");
}

#[test]
fn the_first_project_tank_uses_its_own_coefficients() {
    let m = model(TANK_FIRST);
    let Some(Gen::Sine { amp, period, .. }) = &m.signals[m.signal("inflow").unwrap()].generator
    else {
        panic!("inflow is a sine");
    };
    assert_eq!((*amp, *period), (1.5, 1200.0));
    assert!(m.faults.is_empty());
}

#[test]
fn the_class_simulator_takes_parameters_and_its_fault_key() {
    let mut params = BTreeMap::new();
    params.insert(
        String::from("mean"),
        joker_driver_sim::literal::Literal::Text(String::from("300 K")),
    );
    let m = parse(TEMP_SENSOR, &params).unwrap();
    let Some(Gen::Sine { mean, amp, .. }) = &m.signals[0].generator else {
        panic!("temp is a sine");
    };
    assert!((mean - 26.85).abs() < 1e-9 && *amp == 3.0);
    assert_eq!(m.implements[0].0, "std/temp_sensor@1");
    let temp = m.signal("temp").unwrap();
    let mut p = Plant::new(m, 3);
    p.start(0);
    let before = num(&mut p, 0, "temp");
    assert!(p.press("up", 0));
    let after = num(&mut p, 10 * MS, "temp");
    assert!((after - before - 0.5).abs() < 0.5, "{before} → {after}");
    assert!(p.press("f", 10 * MS));
    p.advance_to(20 * MS);
    assert_eq!(
        p.read(20 * MS, temp),
        Reading::Value(None, RawStatus::bad(Reason::COMM_FAILURE))
    );
}

#[test]
fn the_track_esp_models_agree() {
    let a = model(ESP_REF03);
    let b = model(ESP_PART4);
    let c = model(ESP_PART7);
    let d = with_profile(ESP_PROFILE);
    for m in [&b, &c, &d] {
        let names: Vec<&str> = m.signals.iter().map(|s| s.name.as_str()).collect();
        let want: Vec<&str> = a.signals.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, want);
        for (x, y) in a.signals.iter().zip(&m.signals) {
            assert_eq!((x.ty, x.direction), (y.ty, y.direction), "{}", x.name);
        }
        assert_eq!(m.panel_faults[0].effect, Effect::Disconnect);
    }
    let classes: Vec<&str> = c.implements.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(classes, ["std/led@1", "std/button@1", "std/analog_in@1"]);
    let mut pa = Plant::new(a, fnv1a("esp"));
    let mut pd = Plant::new(d, fnv1a("esp"));
    for t in [0, S, 10 * S, 60 * S] {
        assert_eq!(num(&mut pa, t, "uptime"), num(&mut pd, t, "uptime"));
        assert_eq!(num(&mut pa, t, "rssi"), num(&mut pd, t, "rssi"));
    }
}

#[test]
fn the_pi_of_part_3_heats_up_under_load() {
    let m = model(PI_PART3);
    let load = m.signal("load").unwrap();
    let mut p = Plant::new(m, 0);
    p.start(0);
    assert_eq!(num(&mut p, 0, "cpu_temp"), 45.0);
    assert!(p.press("right", 0));
    assert!(p.press("right", 0));
    assert_eq!(p.value(load), Some(0.0));
    let t = num(&mut p, 600 * S, "cpu_temp");
    assert!((t - 51.0).abs() < 0.01, "{t}");
}

#[test]
fn everything_runs_with_its_faults() {
    let m = model(EVERYTHING);
    assert_eq!(m.dynamics.mems.len(), 7);
    assert_eq!(m.signals[m.signal("serial").unwrap()].ty, SimType::String);
    let heater = m.signal("heater").unwrap();
    let room = m.signal("room").unwrap();
    let n = m.signal("n").unwrap();
    let mut p = Plant::new(m, 9);
    p.start(0);
    p.apply(heater, 100.0);
    p.advance_to(20 * S + S);
    assert_eq!(
        p.read(21 * S, n),
        Reading::Value(None, RawStatus::bad(Reason::COMM_FAILURE))
    );
    p.advance_to(40 * S + S / 2);
    assert_eq!(
        p.read(40 * S + S / 2, room),
        Reading::Value(None, RawStatus::bad(Reason::OUT_OF_RANGE))
    );
    p.advance_to(55 * S);
    assert!(p.disconnected(55 * S));
    let energy = num(&mut p, 59 * S, "energy");
    assert!((energy - 4.5 * 59.0 / 3600.0).abs() < 1e-3, "{energy}");
    let pulses = num(&mut p, 59 * S, "pulses");
    assert_eq!(pulses, 20.0);
    assert_eq!(num(&mut p, 59 * S, "late"), 1.0);
    assert_eq!(num(&mut p, 59 * S, "status"), 1.0);
}

#[test]
fn the_complete_language_gives_the_same_bits_on_every_architecture() {
    let bits = trace(model(EVERYTHING), 70, 50);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in &bits {
        for byte in b.to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    assert_eq!(bits.len(), 47_234);
    assert_eq!(
        h, 0x14e6_6ba8_4412_b8b8,
        "golden hash of the reference run: {h:#018x}"
    );
}
