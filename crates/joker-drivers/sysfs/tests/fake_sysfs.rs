//! The `sysfs` driver against a fake `/sys` in a temporary directory.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use joker_core::ValueType;
use joker_driver_sysfs::{SysfsDriver, SysfsPlanner, grants};
use joker_hal::acquire::OwnedRaw;
use joker_hal::cbor::Value;
use joker_hal::link::DevicePaths;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, OutputHarness, VirtualHost};
use joker_hal::{
    CommitBox, CommitRx, CommitTx, CommittedWrite, Link, Planner, RawEncoding, RawKind, RawStatus,
    Reason, RxState, WriteStatus,
};

const S: u64 = 1_000_000_000;

struct FakeSys(PathBuf);

impl FakeSys {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "joker-sysfs-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let led = dir.join("class/leds/ACT");
        let zone = dir.join("class/thermal/thermal_zone0");
        fs::create_dir_all(&led).unwrap();
        fs::create_dir_all(&zone).unwrap();
        fs::write(led.join("brightness"), "0\n").unwrap();
        fs::write(led.join("max_brightness"), "255\n").unwrap();
        fs::write(led.join("trigger"), "none [mmc0] timer heartbeat\n").unwrap();
        fs::write(zone.join("temp"), "48312\n").unwrap();
        Self(dir)
    }

    fn read(&self, file: &str) -> String {
        fs::read_to_string(self.0.join(file)).unwrap()
    }

    fn write(&self, file: &str, text: &str) {
        fs::write(self.0.join(file), text).unwrap();
    }
}

impl Drop for FakeSys {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn addr(key: &str, name: &str) -> Vec<u8> {
    Value::map([(key, Value::text(name))]).encode()
}

fn request() -> OwnedRequest {
    OwnedRequest {
        bus: None,
        devices: vec![OwnedDevice::new("pi", Value::Map(vec![]).encode(), S)],
        points: vec![
            OwnedPoint::output(0, "act_led", RawKind::Bool, 0, "bool", addr("led", "ACT")),
            OwnedPoint::input(
                1,
                "cpu_temp",
                RawKind::Float,
                0,
                "f64",
                addr("thermal", "thermal_zone0"),
                S,
            ),
        ],
    }
}

struct Writes {
    harness: OutputHarness,
    tx: CommitTx<'static>,
    rx: CommitRx<'static>,
}

impl Writes {
    fn new() -> Self {
        let harness = OutputHarness::new(&[("pi.act_led", ValueType::Bool, 0, 0)]).unwrap();
        let bindings: &'static [_] =
            Box::leak(harness.bindings(&[RawEncoding::Bool]).into_boxed_slice());
        let boxes: &'static [CommitBox] = Box::leak(Box::new([CommitBox::new()]));
        let last = Box::leak(Box::new([None]));
        let state = Box::leak(Box::new([RxState::default()]));
        let tx = CommitTx::new(harness.outputs(), bindings, boxes, last).unwrap();
        let rx = CommitRx::new(bindings, boxes, state).unwrap();
        Self { harness, tx, rx }
    }

    fn set(&mut self, on: bool) -> Vec<CommittedWrite<'static>> {
        self.harness.manual("pi.act_led", u64::from(on));
        self.tx.publish(self.harness.outputs());
        let mut out = Vec::new();
        self.rx.drain_into(&mut out);
        out
    }
}

#[test]
fn led_toggles_with_readback_and_temperature_reads_in_celsius() {
    let sys = FakeSys::new();
    let req = request();
    let views = req.views();
    let plan = SysfsPlanner::plan(&views.request()).unwrap();
    assert_eq!(
        joker_hal::plan::validate(&views.request(), &plan.view()),
        Ok(())
    );
    let dirs = grants(&views.request());
    assert_eq!(dirs, ["class/leds/ACT", "class/thermal/thermal_zone0"]);
    let grants: Vec<&str> = dirs.iter().map(String::as_str).collect();
    let link = Link::Paths(DevicePaths::new(&sys.0, &grants));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, SysfsDriver> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();
    assert_eq!(sys.read("class/leds/ACT/trigger"), "none");

    let mut writes = Writes::new();
    let w = writes.set(true);
    assert_eq!(w.len(), 1);
    assert_eq!(bench.write(&w).unwrap()[0].status, WriteStatus::Ok);
    assert_eq!(sys.read("class/leds/ACT/brightness"), "255");
    let w = writes.set(false);
    assert_eq!(bench.write(&w).unwrap()[0].status, WriteStatus::Ok);
    assert_eq!(sys.read("class/leds/ACT/brightness"), "0");

    let settled = bench.read(0).unwrap();
    assert_eq!(settled[0].status, RawStatus::OK);
    assert_eq!(settled[0].value, OwnedRaw::Float(48.312));

    sys.write("class/thermal/thermal_zone0/temp", "garbage\n");
    assert_eq!(
        bench.read(0).unwrap()[0].status,
        RawStatus::bad(Reason::DEVICE_FAULT)
    );
    fs::remove_file(sys.0.join("class/thermal/thermal_zone0/temp")).unwrap();
    assert_eq!(
        bench.read(0).unwrap()[0].status,
        RawStatus::bad(Reason::CONFIG_ERROR)
    );

    bench.disconnect().unwrap();
    assert_eq!(sys.read("class/leds/ACT/trigger"), "mmc0");
}

#[test]
fn an_abrupt_stop_leaves_the_trigger_at_none() {
    let sys = FakeSys::new();
    let req = request();
    let views = req.views();
    let plan = SysfsPlanner::plan(&views.request()).unwrap();
    let link = Link::Paths(DevicePaths::new(
        &sys.0,
        &["class/leds/ACT", "class/thermal/thermal_zone0"],
    ));
    let mut bench: Bench<'_, SysfsDriver> = Bench::open(
        Arc::new(VirtualHost::new(0)),
        views.request(),
        plan.view(),
        link,
    )
    .unwrap();
    bench.connect().unwrap();
    let mut writes = Writes::new();
    let w = writes.set(true);
    bench.write(&w).unwrap();
    drop(bench);
    assert_eq!(sys.read("class/leds/ACT/trigger"), "none");
    assert_eq!(sys.read("class/leds/ACT/brightness"), "255");
}

#[test]
fn a_led_that_cannot_be_opened_is_config_error() {
    let sys = FakeSys::new();
    fs::remove_file(sys.0.join("class/leds/ACT/max_brightness")).unwrap();
    let mut req = request();
    req.points[0] = OwnedPoint::input(
        0,
        "act_led",
        RawKind::Bool,
        0,
        "bool",
        addr("led", "ACT"),
        S,
    );
    req.points.push(OwnedPoint::output(
        2,
        "pwr",
        RawKind::Bool,
        0,
        "bool",
        addr("led", "PWR"),
    ));
    let views = req.views();
    let plan = SysfsPlanner::plan(&views.request()).unwrap();
    let link = Link::Paths(DevicePaths::new(
        &sys.0,
        &["class/leds/ACT", "class/thermal/thermal_zone0"],
    ));
    let mut bench: Bench<'_, SysfsDriver> = Bench::open(
        Arc::new(VirtualHost::new(0)),
        views.request(),
        plan.view(),
        link,
    )
    .unwrap();
    bench.connect().unwrap();
    let settled = bench.read(0).unwrap();
    assert_eq!(settled[0].status, RawStatus::bad(Reason::CONFIG_ERROR));
    assert_eq!(settled[0].value, OwnedRaw::None);
    assert_eq!(settled[1].value, OwnedRaw::Float(48.312));
}

#[test]
fn plan_checks_types_and_addresses() {
    let mut req = request();
    req.points.push(OwnedPoint::input(
        2,
        "x",
        RawKind::Int,
        0,
        "i64",
        addr("thermal", "thermal_zone1"),
        S,
    ));
    req.points.push(OwnedPoint::input(
        3,
        "y",
        RawKind::Bool,
        0,
        "bool",
        addr("gpio", "GPIO17"),
        S,
    ));
    req.points.push(OwnedPoint::input(
        4,
        "z",
        RawKind::Bool,
        0,
        "bool",
        addr("led", "ACT"),
        S,
    ));
    let views = req.views();
    let codes: Vec<&str> = SysfsPlanner::plan(&views.request())
        .unwrap_err()
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(codes, ["J0809", "J0853", "J0178"]);
    let req = request();
    let views = req.views();
    let plan = SysfsPlanner::plan(&views.request()).unwrap();
    let r = Bench::<SysfsDriver>::open(
        Arc::new(VirtualHost::new(0)),
        views.request(),
        plan.view(),
        Link::None,
    );
    match r {
        Err(joker_hal::testing::BenchError::Driver(e)) => assert_eq!(e.code.as_str(), "J0873"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_full_sample_ring_counts_every_dropped_sample() {
    let sys = FakeSys::new();
    let req = request();
    let views = req.views();
    let plan = SysfsPlanner::plan(&views.request()).unwrap();
    let dirs = grants(&views.request());
    let grants: Vec<&str> = dirs.iter().map(String::as_str).collect();
    let link = Link::Paths(DevicePaths::new(&sys.0, &grants));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, SysfsDriver> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();
    let points = plan.view().batch(0).unwrap().points.len() as u64;
    assert_eq!(u64::from(bench.read_into_full_ring(0).unwrap()), points);
    assert!(bench.counters().contains(&("dropped_samples", points)));
}
