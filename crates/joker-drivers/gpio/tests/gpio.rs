//! `gpio` against an in-memory set of lines.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use joker_driver_gpio::{GpioDriver, GpioPlanner};
use joker_hal::acquire::OwnedRaw;
use joker_hal::cbor::Value;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, MemoryGpio, OutputHarness, VirtualHost};
use joker_hal::{
    CommitBox, CommitRx, CommitTx, CommittedWrite, GpioChip, Link, Planner, RawEncoding, RawKind,
    RawStatus, RxState, WriteStatus,
};

const MS: u64 = 1_000_000;

fn addr(line: u32, active_low: bool) -> Vec<u8> {
    Value::map([
        ("line", Value::Uint(u64::from(line))),
        ("active_low", Value::Bool(active_low)),
    ])
    .encode()
}

fn request() -> OwnedRequest {
    OwnedRequest {
        bus: None,
        devices: vec![OwnedDevice::new(
            "gpio0",
            Value::map([("chip", Value::text("gpiochip0"))]).encode(),
            2_000 * MS,
        )],
        points: vec![
            OwnedPoint::input(
                0,
                "gpio.button",
                RawKind::Bool,
                0,
                "bool",
                addr(17, true),
                100 * MS,
            ),
            OwnedPoint::output(1, "gpio.relay", RawKind::Bool, 0, "bool", addr(22, false)),
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
        let harness =
            OutputHarness::new(&[("gpio.relay", joker_core::ValueType::Bool, 0, 1)]).unwrap();
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
        self.harness.manual("gpio.relay", u64::from(on));
        self.tx.publish(self.harness.outputs());
        let mut out = Vec::new();
        self.rx.drain_into(&mut out);
        out
    }
}

#[test]
fn reads_a_button_inverted_and_writes_a_relay() {
    let (chip, lines) = MemoryGpio::new();
    let req = request();
    let views = req.views();
    let plan = GpioPlanner::plan(&views.request()).unwrap();
    let link = Link::Gpio(GpioChip::new(chip));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, GpioDriver> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();

    // Button is active_low: a physical `false` (pressed, pulled to ground)
    // reads as the logical `true`.
    lines.drive(17, false);
    let settled = bench.read(0).unwrap();
    assert_eq!(settled[0].status, RawStatus::OK);
    assert_eq!(settled[0].value, OwnedRaw::Bool(true));

    let mut writes = Writes::new();
    let w = writes.set(true);
    assert_eq!(bench.write(&w).unwrap()[0].status, WriteStatus::Ok);
    assert!(lines.level(22));
}

#[test]
fn a_denied_line_surfaces_as_a_bad_read() {
    let (chip, lines) = MemoryGpio::new();
    lines.deny(17);
    let req = request();
    let views = req.views();
    let plan = GpioPlanner::plan(&views.request()).unwrap();
    let link = Link::Gpio(GpioChip::new(chip));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, GpioDriver> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();
    let settled = bench.read(0).unwrap();
    assert_ne!(settled[0].status, RawStatus::OK);
}
