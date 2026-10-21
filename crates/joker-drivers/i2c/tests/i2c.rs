//! `i2c` against an in-memory bus.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use joker_driver_i2c::{I2cDriver, I2cPlanner};
use joker_hal::acquire::OwnedRaw;
use joker_hal::cbor::Value;
use joker_hal::spec::owned::{OwnedBus, OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, MemoryI2c, VirtualHost};
use joker_hal::{BusKind, I2cBus, Link, Planner, RawKind, RawStatus};

const MS: u64 = 1_000_000;

fn addr(reg: u8, len: u8) -> Vec<u8> {
    Value::map([
        ("reg", Value::Uint(u64::from(reg))),
        ("len", Value::Uint(u64::from(len))),
    ])
    .encode()
}

fn request() -> OwnedRequest {
    OwnedRequest {
        bus: Some(OwnedBus {
            name: "i2c1".into(),
            kind: BusKind::I2c,
            params: Value::map([]).encode(),
        }),
        devices: vec![OwnedDevice::new(
            "sensor",
            Value::map([("address", Value::Uint(0x40))]).encode(),
            2_000 * MS,
        )],
        points: vec![OwnedPoint::input(
            0,
            "sensor.temp",
            RawKind::Bytes,
            2,
            "u16",
            addr(0, 2),
            100 * MS,
        )],
    }
}

#[test]
fn reads_two_bytes_from_a_register() {
    let (chip, handle) = MemoryI2c::new();
    handle.preload(0x40, &[0xAB, 0xCD]);
    let req = request();
    let views = req.views();
    let plan = I2cPlanner::plan(&views.request()).unwrap();
    let link = Link::I2c(I2cBus::new(chip));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, I2cDriver> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();

    let settled = bench.read(0).unwrap();
    assert_eq!(settled[0].status, RawStatus::OK);
    assert_eq!(settled[0].value, OwnedRaw::Bytes(vec![0xAB, 0xCD]));
    assert_eq!(handle.register(0x40, 0), 0xAB);
}

#[test]
fn a_denied_address_surfaces_as_a_bad_read() {
    let (chip, handle) = MemoryI2c::new();
    handle.deny(0x40);
    let req = request();
    let views = req.views();
    let plan = I2cPlanner::plan(&views.request()).unwrap();
    let link = Link::I2c(I2cBus::new(chip));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, I2cDriver> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();
    let settled = bench.read(0).unwrap();
    assert_ne!(settled[0].status, RawStatus::OK);
}
