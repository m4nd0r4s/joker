#![doc = include_str!("../README.md")]

pub mod plan;

pub use plan::{DESCRIPTOR, I2cPlanner};

use std::vec::Vec;

use joker_hal::{
    BatchFailure, CommittedWrite, ConnectOut, Cx, Driver, DriverError, HealthOut, I2cBus, IoError,
    Link, OpenArgs, PointId, RawSample, RawSink, RawStatus, RawValue, ReadBatch, Reason, WriteAcks,
    WriteStatus, code, types::MAX_RAW_BYTES,
};

use crate::plan::{check_point, device_address};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Point {
    id: PointId,
    reg: u8,
    len: u8,
}

/// The runtime half of `i2c`.
#[derive(Debug)]
pub struct I2cDriver {
    bus: I2cBus,
    addr: u8,
    points: Vec<Point>,
    errors: u64,
    dropped_samples: u64,
}

fn io_status(e: IoError) -> RawStatus {
    match e {
        IoError::Denied => RawStatus::bad(Reason::CONFIG_ERROR),
        IoError::Disconnected => RawStatus::bad(Reason::COMM_FAILURE),
        _ => RawStatus::bad(Reason::DEVICE_FAULT),
    }
}

impl I2cDriver {
    fn point(&self, id: PointId) -> Option<Point> {
        self.points.iter().copied().find(|p| p.id == id)
    }
}

impl Driver for I2cDriver {
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError> {
        let Link::I2c(bus) = link else {
            return Err(DriverError::new(
                code!("J0873"),
                "i2c needs the bus granted by the manifest",
            ));
        };
        let device = args
            .device(0)
            .ok_or_else(|| DriverError::new(code!("J0852"), "an i2c instance has one device"))?;
        let addr =
            device_address(device.config).map_err(|m| DriverError::new(code!("J0852"), m))?;
        let mut points = Vec::new();
        for p in args.request().points {
            let a = check_point(p).map_err(|d| DriverError::new(d.code, d.message))?;
            points.push(Point {
                id: p.id,
                reg: a.reg,
                len: a.len,
            });
        }
        Ok(Self {
            bus,
            addr,
            points,
            errors: 0,
            dropped_samples: 0,
        })
    }

    async fn connect(
        &mut self,
        _cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        Ok(())
    }

    async fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let now = cx.now().mono_ns;
        for &id in batch.points {
            let Some(p) = self.point(id) else {
                continue;
            };
            let mut buf = [0u8; MAX_RAW_BYTES];
            let data = &mut buf[..usize::from(p.len)];
            let sample = match self.bus.transfer(self.addr, &[p.reg], data) {
                Ok(()) => RawSample {
                    ts_hw: Some(now),
                    ..RawSample::good(id, RawValue::Bytes(data))
                },
                Err(e) => {
                    self.errors += 1;
                    RawSample::failed(id, io_status(e))
                }
            };
            if out.push(sample).is_err() {
                self.dropped_samples += 1;
            }
        }
        Ok(())
    }

    /// Read-only in this version: `check_point` already refuses any
    /// `output`/non-`param` point, so no write ever reaches here.
    async fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) {
        let now = cx.now().mono_ns;
        for (k, _) in writes.iter().enumerate() {
            acks.set(k, WriteStatus::Unsupported, Some(now));
        }
    }

    fn health(&self, out: &mut HealthOut<'_>) {
        out.counter("errors", self.errors);
        out.counter("dropped_samples", self.dropped_samples);
    }
}
