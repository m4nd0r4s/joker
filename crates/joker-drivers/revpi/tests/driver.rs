//! `revpi` (ADR 0078) against a fake piControl process image laid out as
//! a RevPi Connect with a DIO and an AIO module: variables resolved by
//! name at `connect`, bits and words read from the image, outputs written
//! through the envelope (a bit in the kernel, a word as bytes), a module
//! that leaves the bus, a variable the configuration does not have, and a
//! piControl reset that moves the modules.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use joker_core::ValueType;
use joker_driver_revpi::image::{Device, FakeImage};
use joker_driver_revpi::{RevPiDriver, RevPiPlanner};
use joker_hal::acquire::OwnedRaw;
use joker_hal::cbor::Value;
use joker_hal::link::DevicePaths;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, BenchError, OutputHarness, VirtualHost};
use joker_hal::{
    CommitBox, CommitRx, CommitTx, CommittedWrite, Driver, Link, OpenArgs, Planner, RawEncoding,
    RawKind, RawStatus, Reason, RxState, WriteStatus,
};

const MS: u64 = 1_000_000;

const DIO: u8 = 32;
const AIO: u8 = 33;

fn image() -> FakeImage {
    let image = FakeImage::new();
    image
        .module(Device {
            address: 0,
            module_type: 136,
            input: (0, 6),
            output: (6, 5),
            active: true,
            ..Device::default()
        })
        .module(Device {
            address: DIO,
            module_type: 96,
            input: (11, 70),
            output: (81, 18),
            active: true,
            ..Device::default()
        })
        .module(Device {
            address: AIO,
            module_type: 103,
            input: (99, 20),
            output: (119, 8),
            active: true,
            ..Device::default()
        });
    image
        .variable("I_1", 11, 0, 1)
        .variable("I_2", 11, 1, 1)
        .variable("O_1", 81, 0, 1)
        .variable("O_2", 81, 1, 1)
        .variable("InputValue_1", 99, 0, 16)
        .variable("InputStatus_1", 103, 0, 8)
        .variable("OutputValue_1", 119, 0, 16)
        .variable("Core_Temperature", 2, 0, 8);
    image
}

fn var(name: &str) -> Vec<u8> {
    Value::map([("var", Value::text(name))]).encode()
}

const I1: u32 = 0;
const I2: u32 = 1;
const AIN: u32 = 2;
const STATUS: u32 = 3;
const O1: u32 = 4;
const AOUT: u32 = 5;
const MISSING: u32 = 6;
const WRONG: u32 = 7;

fn request() -> OwnedRequest {
    let input = |id, path: &str, kind, name: &str| {
        OwnedPoint::input(id, path, kind, 0, "", var(name), 100 * MS)
    };
    OwnedRequest {
        bus: None,
        devices: vec![OwnedDevice::new("io", Value::map([]).encode(), 200 * MS)],
        points: vec![
            input(I1, "io.i_1", RawKind::Bool, "I_1"),
            input(I2, "io.i_2", RawKind::Bool, "I_2"),
            input(AIN, "io.ain", RawKind::Int, "InputValue_1"),
            input(STATUS, "io.status", RawKind::Uint, "InputStatus_1"),
            OwnedPoint::output(O1, "io.o_1", RawKind::Bool, 0, "", var("O_1")),
            OwnedPoint::output(AOUT, "io.aout", RawKind::Int, 0, "", var("OutputValue_1")),
            input(MISSING, "io.gone", RawKind::Bool, "I_99"),
            input(WRONG, "io.wrong", RawKind::Bool, "InputValue_1"),
        ],
    }
}

fn bench<'r>(
    views: &'r joker_hal::spec::owned::RequestViews<'r>,
    plan: &'r joker_hal::plan::Plan,
    image: &FakeImage,
) -> Bench<'r, RevPiDriver> {
    let driver = RevPiDriver::with_image(
        &OpenArgs::new(views.request(), plan.view()),
        Box::new(image.clone()),
    )
    .unwrap();
    Bench::with_driver(
        Arc::new(VirtualHost::new(0)),
        views.request(),
        plan.view(),
        driver,
    )
}

fn read(bench: &mut Bench<'_, RevPiDriver>) -> Vec<(u32, RawStatus, OwnedRaw)> {
    bench
        .read(0)
        .unwrap()
        .into_iter()
        .map(|s| (s.point, s.status, s.value))
        .collect()
}

fn sample(samples: &[(u32, RawStatus, OwnedRaw)], id: u32) -> (RawStatus, OwnedRaw) {
    samples
        .iter()
        .find(|s| s.0 == id)
        .map(|s| (s.1, s.2.clone()))
        .unwrap_or_else(|| panic!("no sample of {id}: {samples:?}"))
}

fn committed(ty: ValueType, point: u32, bits: u64) -> CommittedWrite<'static> {
    let mut harness = OutputHarness::new(&[("io.out", ty, 0, point)]).unwrap();
    let encoding = match ty {
        ValueType::Bool => RawEncoding::Bool,
        ValueType::I64 => RawEncoding::Int,
        _ => RawEncoding::Uint,
    };
    let bindings: &'static [_] = Box::leak(harness.bindings(&[encoding]).into_boxed_slice());
    let boxes: &'static [CommitBox] = Box::leak(Box::new([CommitBox::new()]));
    let last = Box::leak(Box::new([None]));
    let state = Box::leak(Box::new([RxState::default()]));
    let mut tx = CommitTx::new(harness.outputs(), bindings, boxes, last).unwrap();
    let mut rx = CommitRx::new(bindings, boxes, state).unwrap();
    harness.manual("io.out", bits);
    tx.publish(harness.outputs());
    let mut out = Vec::new();
    rx.drain_into(&mut out);
    out.pop().unwrap()
}

#[test]
fn inputs_are_read_from_the_process_image_by_variable_name() {
    let image = image();
    image.set_bytes(11, &[0b10]);
    image.set_bytes(99, &(-1234i16).to_le_bytes());
    image.set_bytes(103, &[0x80]);
    let req = request();
    let views = req.views();
    let plan = RevPiPlanner::plan(&views.request()).unwrap();
    let mut bench = bench(&views, &plan, &image);
    bench.connect().unwrap();
    let s = read(&mut bench);
    assert_eq!(sample(&s, I1), (RawStatus::OK, OwnedRaw::Bool(false)));
    assert_eq!(sample(&s, I2), (RawStatus::OK, OwnedRaw::Bool(true)));
    assert_eq!(sample(&s, AIN), (RawStatus::OK, OwnedRaw::Int(-1234)));
    assert_eq!(sample(&s, STATUS), (RawStatus::OK, OwnedRaw::Uint(0x80)));
    assert_eq!(
        sample(&s, MISSING).0,
        RawStatus::bad(Reason::CONFIG_ERROR),
        "a variable the configuration does not have"
    );
    assert_eq!(
        sample(&s, WRONG).0,
        RawStatus::bad(Reason::CONFIG_ERROR),
        "a 16-bit variable is not a bool"
    );
    let counters = bench.counters();
    assert!(
        counters.contains(&("unresolved_variables", 2)),
        "{counters:?}"
    );
}

#[test]
fn outputs_set_one_bit_in_the_kernel_and_write_words_as_bytes() {
    let image = image();
    image.set_bytes(81, &[0b1000_0010]);
    let req = request();
    let views = req.views();
    let plan = RevPiPlanner::plan(&views.request()).unwrap();
    let mut bench = bench(&views, &plan, &image);
    bench.connect().unwrap();
    let writes = [
        committed(ValueType::Bool, O1, 1),
        committed(ValueType::I64, AOUT, (-300i64) as u64),
    ];
    let acks = bench.write(&writes).unwrap();
    assert!(acks.iter().all(|a| a.status == WriteStatus::Ok), "{acks:?}");
    assert_eq!(image.byte(81), 0b1000_0011, "bit 0 set, bits 1 and 7 kept");
    assert_eq!(i16::from_le_bytes([image.byte(119), image.byte(120)]), -300);
    assert_eq!(
        image.writes(),
        vec![(81, vec![0x80]), (119, (-300i16).to_le_bytes().to_vec())],
        "a bit set in the kernel, then the two bytes of the word"
    );
    let too_wide = [committed(ValueType::I64, AOUT, 40_000)];
    assert_eq!(
        bench.write(&too_wide).unwrap()[0].status,
        WriteStatus::DeviceRejected,
        "40000 does not fit a 16-bit variable"
    );
}

#[test]
fn a_module_that_leaves_the_bus_reads_not_connected_and_refuses_writes() {
    let image = image();
    let req = request();
    let views = req.views();
    let plan = RevPiPlanner::plan(&views.request()).unwrap();
    let mut bench = bench(&views, &plan, &image);
    bench.connect().unwrap();
    image.set_active(DIO, false);
    let s = read(&mut bench);
    assert_eq!(sample(&s, I1).0, RawStatus::bad(Reason::NOT_CONNECTED));
    assert_eq!(sample(&s, AIN).0, RawStatus::OK, "the AIO is still there");
    let acks = bench.write(&[committed(ValueType::Bool, O1, 1)]).unwrap();
    assert_eq!(acks[0].status, WriteStatus::DeviceRejected);
    assert_eq!(image.byte(81), 0, "nothing written");
    image.set_active(DIO, true);
    let s = read(&mut bench);
    assert_eq!(sample(&s, I1).0, RawStatus::OK, "back on the bus");
}

#[test]
fn a_picontrol_reset_that_moves_the_modules_is_resolved_again_at_connect() {
    let image = image();
    let req = request();
    let views = req.views();
    let plan = RevPiPlanner::plan(&views.request()).unwrap();
    let mut bench = bench(&views, &plan, &image);
    bench.connect().unwrap();
    image.relocate(AIO, (200, 20), (220, 8));
    image.variable("InputValue_1", 200, 0, 16);
    image.set_bytes(200, &77i16.to_le_bytes());
    match bench.read(0) {
        Err(BenchError::Driver(_)) | Err(BenchError::Stalled) | Err(BenchError::NoBatch) => {
            panic!("a reset is a batch failure, not a bench error")
        }
        Ok(settled) => assert!(
            settled
                .iter()
                .all(|s| s.status == RawStatus::bad(Reason::COMM_FAILURE)),
            "the batch is not-connected: {settled:?}"
        ),
    }
    bench.connect().unwrap();
    let s = read(&mut bench);
    assert_eq!(sample(&s, AIN), (RawStatus::OK, OwnedRaw::Int(77)));
    assert!(bench.counters().contains(&("resets", 1)));
}

#[test]
fn picontrol_that_does_not_answer_fails_connect_and_reads() {
    let image = image();
    let req = request();
    let views = req.views();
    let plan = RevPiPlanner::plan(&views.request()).unwrap();
    let mut bench = bench(&views, &plan, &image);
    bench.connect().unwrap();
    image.set_down(true);
    let settled = bench.read(0).unwrap();
    assert!(
        settled
            .iter()
            .all(|s| s.status == RawStatus::bad(Reason::COMM_FAILURE))
    );
    match bench.connect() {
        Err(BenchError::Driver(e)) => assert_eq!(e.code.as_str(), "J0876"),
        other => panic!("{other:?}"),
    }
    image.set_down(false);
    bench.connect().unwrap();
    assert_eq!(read(&mut bench).len(), 6);
}

#[test]
fn open_needs_the_granted_device_paths_and_plans_check_the_types() {
    let req = request();
    let views = req.views();
    let plan = RevPiPlanner::plan(&views.request()).unwrap();
    let args = OpenArgs::new(views.request(), plan.view());
    match RevPiDriver::open(&args, Link::None) {
        Err(e) => assert_eq!(e.code.as_str(), "J0873"),
        Ok(_) => panic!("Link::None"),
    }
    let dir = std::env::temp_dir();
    assert!(RevPiDriver::open(&args, Link::Paths(DevicePaths::new(dir, &["piControl0"]))).is_ok());
    let mut bad = request();
    bad.points[0] = OwnedPoint::input(0, "io.x", RawKind::Regs, 1, "", var("I_1"), 100 * MS);
    let errors = RevPiPlanner::plan(&bad.views().request()).unwrap_err();
    assert_eq!(errors[0].code.as_str(), "J0809");
}
