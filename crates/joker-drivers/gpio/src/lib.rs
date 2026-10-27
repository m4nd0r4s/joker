#![doc = include_str!("../README.md")]

pub mod plan;

pub use plan::{DESCRIPTOR, GpioPlanner};

use std::vec::Vec;

use joker_hal::{
    AddrRead, AddrWrite, BatchFailure, CommittedWrite, ConnectOut, Cx, Direction, Driver,
    DriverError, GpioChip, HealthOut, Link, OpenArgs, PointId, RawKind, RawSample, RawSink,
    RawStatus, RawValue, ReadBatch, Reason, WriteAcks, WriteKind, WriteStatus, code,
};

use crate::plan::{Address, Line, check_point};

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
    dropped_samples: u64,
}

fn io_status(e: joker_hal::IoError) -> RawStatus {
    match e {
        joker_hal::IoError::Denied => RawStatus::bad(Reason::CONFIG_ERROR),
        joker_hal::IoError::Disconnected => RawStatus::bad(Reason::COMM_FAILURE),
        _ => RawStatus::bad(Reason::DEVICE_FAULT),
    }
}

fn io_failure(e: joker_hal::IoError) -> BatchFailure {
    match e {
        joker_hal::IoError::Denied => BatchFailure::ConfigError("the line is not granted"),
        joker_hal::IoError::Disconnected => BatchFailure::CommFailure("the chip is gone"),
        _ => BatchFailure::DeviceFault("the line failed"),
    }
}

impl GpioDriver {
    fn point(&self, id: PointId) -> Option<Point> {
        self.points.iter().copied().find(|p| p.id == id)
    }

    fn step_line(
        &self,
        addr: joker_hal::Cbor<'_>,
        write: bool,
    ) -> Result<(u32, bool), BatchFailure> {
        let a = Address::parse(addr).map_err(BatchFailure::ConfigError)?;
        let Line::Offset(line) = a.line else {
            return Err(BatchFailure::ConfigError(
                "a step reaches a line by its offset in this version",
            ));
        };
        let requested = self
            .points
            .iter()
            .any(|p| p.line == line && (p.output || !write));
        if requested {
            Ok((line, a.active_low))
        } else if write {
            Err(BatchFailure::ConfigError(
                "a step writes a line one of the device's outputs requests",
            ))
        } else {
            Err(BatchFailure::ConfigError(
                "a step reads a line one of the device's points requests",
            ))
        }
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
            let Line::Offset(line) = addr.line else {
                return Err(DriverError::new(
                    code!("J0828"),
                    "a named gpio line reaches the driver unresolved; the host maps it to an offset first",
                ));
            };
            points.push(Point {
                id: p.id,
                line,
                active_low: addr.active_low,
                output: p.direction == Direction::Output,
            });
        }
        Ok(Self {
            chip,
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
            if out.push(sample).is_err() {
                self.dropped_samples += 1;
            }
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

    async fn read_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrRead<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let (line, active_low) = self.step_line(req.addr, false)?;
        let level = self.chip.get(line).map_err(|e| {
            self.errors += 1;
            io_failure(e)
        })?;
        let on = level != active_low;
        let value = match req.expect {
            RawKind::Int => RawValue::Int(i64::from(on)),
            RawKind::Uint => RawValue::Uint(u64::from(on)),
            _ => RawValue::Bool(on),
        };
        let sample = RawSample {
            ts_hw: Some(cx.now().mono_ns),
            ..RawSample::good(0, value)
        };
        if out.push(sample).is_err() {
            self.dropped_samples += 1;
        }
        Ok(())
    }

    async fn write_addr(
        &mut self,
        _cx: &mut Cx<'_>,
        req: &AddrWrite<'_>,
    ) -> Result<(), BatchFailure> {
        let (line, active_low) = self.step_line(req.addr, true)?;
        let on = match req.value {
            RawValue::Bool(b) => b,
            RawValue::Uint(u) if u <= 1 => u == 1,
            RawValue::Int(i) if (0..=1).contains(&i) => i == 1,
            _ => return Err(BatchFailure::ConfigError("a line takes a bool")),
        };
        self.chip.set(line, on != active_low).map_err(|e| {
            self.errors += 1;
            io_failure(e)
        })
    }

    fn health(&self, out: &mut HealthOut<'_>) {
        out.counter("errors", self.errors);
        out.counter("dropped_samples", self.dropped_samples);
    }
}
