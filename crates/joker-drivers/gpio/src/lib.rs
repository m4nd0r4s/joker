#![doc = include_str!("../README.md")]

pub mod plan;

pub use plan::{DESCRIPTOR, GpioPlanner};

use std::vec::Vec;

use joker_hal::{
    BatchFailure, CommittedWrite, ConnectOut, Cx, Direction, Driver, DriverError, GpioChip,
    HealthOut, Link, OpenArgs, PointId, RawSample, RawSink, RawStatus, RawValue, ReadBatch, Reason,
    WriteAcks, WriteKind, WriteStatus, code,
};

use crate::plan::check_point;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Point {
    id: PointId,
    line: u32,
    active_low: bool,
    output: bool,
}

/// The runtime half of `gpio`.
#[derive(Debug)]
pub struct GpioDriver {
    chip: GpioChip,
    points: Vec<Point>,
    errors: u64,
}

fn io_status(e: joker_hal::IoError) -> RawStatus {
    match e {
        joker_hal::IoError::Denied => RawStatus::bad(Reason::CONFIG_ERROR),
        joker_hal::IoError::Disconnected => RawStatus::bad(Reason::COMM_FAILURE),
        _ => RawStatus::bad(Reason::DEVICE_FAULT),
    }
}

impl GpioDriver {
    fn point(&self, id: PointId) -> Option<Point> {
        self.points.iter().copied().find(|p| p.id == id)
    }
}

impl Driver for GpioDriver {
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError> {
        let Link::Gpio(chip) = link else {
            return Err(DriverError::new(
                code!("J0873"),
                "gpio needs the digital lines granted by the manifest",
            ));
        };
        let mut points = Vec::new();
        for p in args.request().points {
            let addr = check_point(p).map_err(|d| DriverError::new(d.code, d.message))?;
            points.push(Point {
                id: p.id,
                line: addr.line,
                active_low: addr.active_low,
                output: p.direction == Direction::Output,
            });
        }
        Ok(Self {
            chip,
            points,
            errors: 0,
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
            let sample = match self.chip.get(p.line) {
                Ok(level) => RawSample {
                    ts_hw: Some(now),
                    ..RawSample::good(id, RawValue::Bool(level != p.active_low))
                },
                Err(e) => {
                    self.errors += 1;
                    RawSample::failed(id, io_status(e))
                }
            };
            let _ = out.push(sample);
        }
        Ok(())
    }

    async fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) {
        let now = cx.now().mono_ns;
        for (k, w) in writes.iter().enumerate() {
            let status = match (self.point(w.point), w.value, w.kind) {
                (_, _, WriteKind::Relinquish) => WriteStatus::Unsupported,
                (Some(p), RawValue::Bool(on), _) if p.output => {
                    match self.chip.set(p.line, on != p.active_low) {
                        Ok(()) => WriteStatus::Ok,
                        Err(_) => {
                            self.errors += 1;
                            WriteStatus::CommFailure
                        }
                    }
                }
                _ => WriteStatus::Unsupported,
            };
            acks.set(k, status, Some(now));
        }
    }

    fn health(&self, out: &mut HealthOut<'_>) {
        out.counter("errors", self.errors);
    }
}
