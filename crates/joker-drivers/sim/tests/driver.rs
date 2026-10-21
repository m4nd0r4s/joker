//! The complete `sim` driver through the driver contract: what the core
//! passes in `config` (profile, files, seed), simulated actions as native
//! actions, text signals, per-signal latency and the `[plant]` engine.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use joker_driver_sim::{PlantEngine, PlantInstance, SimDriver, SimPlanner};
use joker_hal::acquire::{OwnedRaw, Settled};
use joker_hal::cbor::Value;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, VirtualHost};
use joker_hal::{
    ActionCall, ActionFailure, ActionOut, ActionSpec, Cbor, Cx, Driver, Link, OpenArgs,
    PlanRequest, Planner, RawKind, RawStatus, Reason,
};

const MS: u64 = 1_000_000;
const S: u64 = 1_000 * MS;
const BENCH_PROFILE: &str = include_str!("spec/bench-esp32@1.profile.toml");
const ESP_PROFILE: &str = include_str!("spec/esp-profile.sim.toml");

fn signal(name: &str) -> Vec<u8> {
    Value::map([("signal", Value::text(name))]).encode()
}

fn config(model: &str, extra: Vec<(String, Value)>) -> Vec<u8> {
    let mut entries = vec![(String::from("model"), Value::text(model))];
    entries.extend(extra);
    Value::Map(entries).encode()
}

fn request(config: Vec<u8>, points: Vec<OwnedPoint>) -> OwnedRequest {
    OwnedRequest {
        bus: None,
        devices: vec![OwnedDevice::new("dev", config, S)],
        points,
    }
}

fn input(id: u32, name: &str, kind: RawKind, ty: &str) -> OwnedPoint {
    OwnedPoint::input(id, name, kind, 0, ty, signal(name), 100 * MS)
}

fn value(s: &Settled) -> f64 {
    match &s.value {
        OwnedRaw::Float(f) => *f,
        OwnedRaw::Uint(u) => *u as f64,
        OwnedRaw::Int(i) => *i as f64,
        OwnedRaw::Bool(b) => f64::from(u8::from(*b)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_model_with_a_profile_reads_through_the_driver() {
    let cfg = config(
        ESP_PROFILE,
        vec![(String::from("profile"), Value::text(BENCH_PROFILE))],
    );
    let req = request(
        cfg,
        vec![
            input(0, "rssi", RawKind::Int, "i16"),
            input(1, "uptime", RawKind::Float, "u32"),
            input(2, "touch", RawKind::Uint, "u16"),
        ],
    );
    let views = req.views();
    let plan = SimPlanner::plan(&views.request()).unwrap();
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, SimDriver> =
        Bench::open(host.clone(), views.request(), plan.view(), Link::None).unwrap();
    bench.connect().unwrap();
    host.advance_to(2 * S);
    let got = bench.read(0).unwrap();
    assert!(got.iter().all(|s| s.status == RawStatus::OK));
    assert!((value(&got[1]) - 2.0).abs() < 1e-9);
    assert!((55.0..=65.0).contains(&value(&got[2])));
}

#[test]
fn replay_files_and_the_seed_come_in_the_config() {
    let model = "schema = 1\n[signals]\np = { type = \"f64\", gen = { kind = \"replay\", file = \"rec/p.csv\", column = \"p\" } }\nn = { type = \"f64\", gen = { kind = \"noise\", mean = 0, std = 1 } }\n";
    let read_at = |seed: Option<u64>, t: u64| {
        let mut extra = vec![(
            String::from("files"),
            Value::Map(vec![(
                String::from("rec/p.csv"),
                Value::Bytes(b"t,p\n0,1\n1s,5\n".to_vec()),
            )]),
        )];
        if let Some(s) = seed {
            extra.push((String::from("seed"), Value::Uint(s)));
        }
        let req = request(
            config(model, extra),
            vec![
                input(0, "n", RawKind::Float, "f64"),
                input(1, "p", RawKind::Float, "f64"),
            ],
        );
        let views = req.views();
        let plan = SimPlanner::plan(&views.request()).unwrap();
        let host = Arc::new(VirtualHost::new(0));
        let mut bench: Bench<'_, SimDriver> =
            Bench::open(host.clone(), views.request(), plan.view(), Link::None).unwrap();
        bench.connect().unwrap();
        host.advance_to(t);
        let got = bench.read(0).unwrap();
        (value(&got[0]), value(&got[1]))
    };
    assert_eq!(read_at(None, 0).1, 1.0);
    assert_eq!(read_at(None, 2 * S).1, 5.0);
    assert_eq!(read_at(Some(7), S), read_at(Some(7), S));
    assert_ne!(read_at(Some(7), S).0, read_at(Some(8), S).0);
    assert_ne!(read_at(None, S).0, read_at(Some(8), S).0);
}

#[test]
fn text_signals_latency_and_limits() {
    let model = r#"schema = 1
[model]
latency = "1ms"
[signals]
serial = { type = "string", init = "SN-7" }
blob = { type = "bytes", init = [1, 2] }
slow = { type = "f64", init = 3, latency = "30ms" }
high = { type = "f64", init = 9, limits = [0, 5] }
"#;
    let req = request(
        config(model, vec![]),
        vec![
            input(0, "serial", RawKind::Text, "string"),
            input(1, "blob", RawKind::Bytes, "bytes"),
            input(2, "slow", RawKind::Float, "f64"),
            input(3, "high", RawKind::Float, "f64"),
        ],
    );
    let views = req.views();
    let plan = SimPlanner::plan(&views.request()).unwrap();
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, SimDriver> =
        Bench::open(host.clone(), views.request(), plan.view(), Link::None).unwrap();
    bench.connect().unwrap();
    let got = bench.read(0).unwrap();
    assert_eq!(got[0].value, OwnedRaw::Text(String::from("SN-7")));
    assert_eq!(got[1].value, OwnedRaw::Bytes(vec![1, 2]));
    assert_eq!(value(&got[2]), 3.0);
    assert_eq!(got[3].status, RawStatus::bad(Reason::OUT_OF_RANGE));
    assert_eq!(host.mono(), 30 * MS);
}

#[test]
fn simulated_actions_are_native_actions() {
    let model = r#"schema = 1
[enums]
Mode = { auto = 0, manual = 1 }
[signals]
fault = { type = "bool", init = true }
mode = { enum = "Mode", direction = "param" }
temp = { type = "f64", unit = "°C", gen = { kind = "constant", value = "20 °C" } }
[actions.reset]
args = { to = "Mode", t = "f64 K" }
steps = [{ wait = "200ms" }, { set = { mode = "$to", temp = "$t", fault = false } }]
[actions.check]
steps = [{ fail_if = { signal = "fault", equals = true, message = "still faulted" } }]
[actions.release]
steps = [{ set = { temp = true, release = true } }]
[actions.slow]
timeout = "100ms"
steps = [{ wait = "50ms" }, { wait = "40ms" }]
"#;
    let req = request(
        config(model, vec![]),
        vec![
            input(0, "fault", RawKind::Bool, "bool"),
            input(1, "temp", RawKind::Float, "f64"),
        ],
    );
    let views = req.views();
    let spec = |id: u32, name: &'static str| ActionSpec {
        id,
        device: 0,
        name,
        native: Cbor::EMPTY_MAP,
        timeout_ns: if name == "slow" { 60 * MS } else { 0 },
        long: false,
    };
    let actions = [
        spec(0, "reset"),
        spec(1, "check"),
        spec(2, "release"),
        spec(3, "slow"),
    ];
    let request = PlanRequest {
        actions: &actions,
        ..views.request()
    };
    let plan = SimPlanner::plan(&request).unwrap();
    let host = Arc::new(VirtualHost::new(0));
    let mut driver: SimDriver =
        SimDriver::open(&OpenArgs::new(request, plan.view()), Link::None).unwrap();
    let handle = driver.handle();
    handle.plant().start(0);
    let mut call = |action: u32, args: Vec<u8>| {
        let mut cx = Cx::new(&*host, host.mono() + 10 * S);
        let mut buf = [0u8; 16];
        let mut out = ActionOut::new(&mut buf);
        let c = ActionCall {
            call: 1,
            action,
            args: Cbor::new(&args),
        };
        host.run(driver.invoke(&mut cx, &c, &mut out)).unwrap()
    };
    let empty = Value::Map(vec![]).encode();
    assert!(matches!(
        call(1, empty.clone()),
        Err(ActionFailure::Device(e)) if e.detail.as_str() == "still faulted"
    ));
    let args = Value::Map(vec![
        (String::from("to"), Value::text("manual")),
        (String::from("t"), Value::Float(300.0)),
    ])
    .encode();
    assert_eq!(call(0, args), Ok(()));
    assert_eq!(host.mono(), 200 * MS);
    let model = handle.plant().model().clone();
    let (mode, temp) = (model.signal("mode").unwrap(), model.signal("temp").unwrap());
    handle.plant().advance_to(210 * MS);
    assert_eq!(handle.plant().value(mode), Some(1.0));
    assert!((handle.plant().value(temp).unwrap() - 26.85).abs() < 1e-9);
    assert_eq!(call(1, empty.clone()), Ok(()));
    assert_eq!(call(2, empty.clone()), Ok(()));
    handle.plant().advance_to(host.mono() + 20 * MS);
    assert_eq!(handle.plant().value(temp), Some(20.0));
    assert_eq!(call(3, empty.clone()), Err(ActionFailure::Timeout));
    let positional = Value::Map(vec![]).encode();
    assert!(matches!(
        call(0, positional),
        Err(ActionFailure::Invalid(_))
    ));
    assert_eq!(call(9, empty), Err(ActionFailure::Unsupported));
    let bad = [spec(0, "nope")];
    let bad_req = PlanRequest {
        actions: &bad,
        ..views.request()
    };
    assert!(SimPlanner::plan(&bad_req).is_err());
}

#[derive(Debug)]
struct Valve {
    cv: f64,
    flow: f64,
}

impl PlantInstance for Valve {
    fn step(&mut self, dt_ns: u64, inputs: &[f64]) -> Result<Vec<f64>, String> {
        let [opening] = inputs else {
            return Err(String::from("one input"));
        };
        if *opening > 100.0 {
            return Err(String::from("opening above 100 %"));
        }
        self.flow += (self.cv * opening / 100.0 - self.flow) * (dt_ns as f64 / 1e9);
        Ok(vec![self.flow])
    }
}

struct TestEngine;

impl PlantEngine for TestEngine {
    fn instantiate(
        component: &[u8],
        config: &[u8],
        seed: u64,
    ) -> Result<Box<dyn PlantInstance>, String> {
        if component != b"\0asm valve" || seed != 5 {
            return Err(String::from("unexpected component or seed"));
        }
        let cv = Cbor::new(config)
            .get("cv")
            .ok()
            .flatten()
            .and_then(|v| v.as_f64().ok())
            .ok_or("no cv")?;
        Ok(Box::new(Valve { cv, flow: 0.0 }))
    }
}

#[test]
fn a_wasm_plant_steps_through_its_engine() {
    let model = r#"schema = 1
[model]
step = "100ms"
seed = 5
[signals]
opening = { type = "f64", unit = "%", direction = "output" }
flow = { type = "f64", unit = "m3/h" }
[plant]
component = "plants/valve.wasm"
inputs = ["opening"]
outputs = ["flow"]
config = { cv = 10.0 }
"#;
    let files = |bytes: &[u8]| {
        vec![(
            String::from("files"),
            Value::Map(vec![(
                String::from("plants/valve.wasm"),
                Value::Bytes(bytes.to_vec()),
            )]),
        )]
    };
    let req = request(
        config(model, files(b"\0asm valve")),
        vec![
            input(0, "flow", RawKind::Float, "f64"),
            OwnedPoint::output(1, "opening", RawKind::Float, 0, "f64", signal("opening")),
        ],
    );
    let views = req.views();
    let plan = SimPlanner::plan(&views.request()).unwrap();
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, SimDriver<TestEngine>> =
        Bench::open(host.clone(), views.request(), plan.view(), Link::None).unwrap();
    bench.connect().unwrap();
    let handle = bench.driver.handle();
    let opening = handle.plant().model().signal("opening").unwrap();
    handle.plant().apply(opening, 50.0);
    host.advance_to(60 * S);
    let got = bench.read(0).unwrap();
    assert!((value(&got[0]) - 5.0).abs() < 1e-3, "{:?}", got[0]);
    handle.plant().apply(opening, 150.0);
    host.advance_to(61 * S);
    let got = bench.read(0).unwrap();
    assert_eq!(got[0].status, RawStatus::bad(Reason::DEVICE_FAULT));
    assert!(handle.plant().plant_failed());

    let no_engine = request(config(model, files(b"\0asm valve")), vec![]);
    let views = no_engine.views();
    let plan = SimPlanner::plan(&views.request()).unwrap();
    let e = SimDriver::<joker_driver_sim::NoEngine>::open(
        &OpenArgs::new(views.request(), plan.view()),
        Link::None,
    )
    .unwrap_err();
    assert_eq!(e.code.as_str(), "J0837");
    let missing = request(config(model, vec![]), vec![]);
    let views = missing.views();
    let plan = SimPlanner::plan(&views.request()).unwrap();
    let e = SimDriver::<TestEngine>::open(&OpenArgs::new(views.request(), plan.view()), Link::None)
        .unwrap_err();
    assert_eq!(e.code.as_str(), "J0837");
}
