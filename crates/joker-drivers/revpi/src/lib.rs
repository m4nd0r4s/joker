#![doc = include_str!("../README.md")]

pub mod image;
pub mod plan;

pub use plan::{DESCRIPTOR, RevPiPlanner};

use std::boxed::Box;
use std::string::String;
use std::vec::Vec;

use joker_hal::{
    BatchFailure, CommittedWrite, ConnectOut, Cx, Direction, Driver, DriverError, HealthOut, Link,
    LogLevel, OpenArgs, PointId, RawSample, RawSink, RawStatus, RawValue, ReadBatch, Reason,
    WriteAcks, WriteKind, WriteStatus, code,
};

use crate::image::{Device, PiControl, PiImage, Variable};
use crate::plan::{Kind, check_point};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Point {
    id: PointId,
    var: String,
    kind: Kind,
    output: bool,
}

/// A variable resolved at `connect`, and the module that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Resolved {
    var: Variable,
    module: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counters {
    reads: u64,
    writes: u64,
    unresolved: u64,
    absent: u64,
    resets: u64,
    dropped_samples: u64,
}

/// The runtime half of `revpi`.
#[derive(Debug)]
pub struct RevPiDriver {
    image: Box<dyn PiImage>,
    points: Vec<Point>,
    resolved: Vec<Option<Resolved>>,
    modules: Vec<Device>,
    counters: Counters,
}

/// The modules without their presence: what changes only when piControl
/// loads another configuration.
fn layout(modules: &[Device]) -> Vec<Device> {
    modules
        .iter()
        .map(|m| Device {
            state: 0,
            active: false,
            ..*m
        })
        .collect()
}

fn bytes_of(length: u16) -> usize {
    usize::from(length.div_ceil(8))
}

impl RevPiDriver {
    /// The driver over `image`; [`Driver::open`] uses [`PiControl`] on the
    /// granted paths, and tests a [`image::FakeImage`].
    pub fn with_image(args: &OpenArgs<'_>, image: Box<dyn PiImage>) -> Result<Self, DriverError> {
        let mut points = Vec::new();
        for p in args.request().points {
            let (var, kind) = check_point(p).map_err(|d| DriverError::new(d.code, d.message))?;
            points.push(Point {
                id: p.id,
                var,
                kind,
                output: p.direction == Direction::Output,
            });
        }
        let resolved = std::vec![None; points.len()];
        Ok(Self {
            image,
            points,
            resolved,
            modules: Vec::new(),
            counters: Counters::default(),
        })
    }

    fn index(&self, id: PointId) -> Option<usize> {
        self.points.iter().position(|p| p.id == id)
    }

    fn present(&self, r: &Resolved) -> bool {
        match r.module {
            Some(address) => self
                .modules
                .iter()
                .any(|m| m.address == address && m.active),
            None => true,
        }
    }

    fn value(&mut self, kind: Kind, var: Variable) -> std::io::Result<RawValue<'static>> {
        let mut buf = [0u8; 4];
        let n = bytes_of(var.length).clamp(1, 4);
        self.image.read(var.address, &mut buf[..n])?;
        let bits = u32::from_le_bytes(buf);
        Ok(match kind {
            Kind::Bool => RawValue::Bool(buf[0] >> (var.bit & 7) & 1 == 1),
            Kind::Unsigned => RawValue::Uint(u64::from(bits)),
            Kind::Signed => {
                let shift = 32 - u32::from(var.length);
                RawValue::Int(i64::from(((bits << shift) as i32) >> shift))
            }
        })
    }

    fn put(&mut self, var: Variable, value: RawValue<'_>) -> Result<(), WriteStatus> {
        let width = u32::from(var.length);
        let raw: u32 = match value {
            RawValue::Bool(on) if var.length == 1 => {
                return self
                    .image
                    .set_bit(var.address, var.bit, on)
                    .map_err(|_| WriteStatus::NotConnected);
            }
            RawValue::Uint(u) if u < 1u64 << width => u as u32,
            RawValue::Int(i) if var.length > 1 => {
                let half = 1i64 << (width - 1);
                if i < -half || i >= half {
                    return Err(WriteStatus::DeviceRejected);
                }
                i as u32
            }
            _ => return Err(WriteStatus::DeviceRejected),
        };
        let bytes = raw.to_le_bytes();
        self.image
            .write(var.address, &bytes[..bytes_of(var.length)])
            .map_err(|_| WriteStatus::NotConnected)
    }
}

impl Driver for RevPiDriver {
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError> {
        let Link::Paths(paths) = link else {
            return Err(DriverError::new(
                code!("J0873"),
                "revpi needs /dev/piControl0 granted as device paths",
            ));
        };
        Self::with_image(args, Box::new(PiControl::new(paths)))
    }

    async fn connect(
        &mut self,
        cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        let fail = |m: &str| DriverError::new(code!("J0876"), m);
        self.image
            .open()
            .map_err(|_| fail("piControl cannot be opened"))?;
        self.modules = self
            .image
            .modules()
            .map_err(|_| fail("piControl does not list its modules"))?;
        for i in 0..self.points.len() {
            let p = &self.points[i];
            let found = self.image.find(&p.var).ok();
            let fits = found.filter(|v| p.kind.fits(v.length));
            if fits.is_none() {
                self.counters.unresolved += 1;
                let why = if found.is_some() {
                    "has a length its type cannot carry"
                } else {
                    "is not in the piControl configuration"
                };
                cx.log(
                    LogLevel::Warn,
                    &std::format!("revpi: variable `{}` {why}", p.var),
                );
            }
            self.resolved[i] = fits.map(|var| Resolved {
                var,
                module: self
                    .modules
                    .iter()
                    .find(|m| m.holds(var.address))
                    .map(|m| m.address),
            });
        }
        Ok(())
    }

    async fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let now = self
            .image
            .modules()
            .map_err(|_| BatchFailure::NotConnected)?;
        if layout(&now) != layout(&self.modules) {
            self.counters.resets += 1;
            return Err(BatchFailure::NotConnected);
        }
        self.modules = now;
        let ts = cx.now().mono_ns;
        for &id in batch.points {
            let Some(i) = self.index(id) else {
                continue;
            };
            let kind = self.points[i].kind;
            let sample = match self.resolved[i] {
                None => RawSample::failed(id, RawStatus::bad(Reason::CONFIG_ERROR)),
                Some(r) if !self.present(&r) => {
                    self.counters.absent += 1;
                    RawSample::failed(id, RawStatus::bad(Reason::NOT_CONNECTED))
                }
                Some(r) => {
                    let value = self
                        .value(kind, r.var)
                        .map_err(|_| BatchFailure::NotConnected)?;
                    self.counters.reads += 1;
                    RawSample {
                        ts_hw: Some(ts),
                        ..RawSample::good(id, value)
                    }
                }
            };
            if out.push(sample).is_err() {
                self.counters.dropped_samples += 1;
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
        if let Ok(now) = self.image.modules() {
            self.modules = now;
        }
        let mut failed_group: Option<u32> = None;
        for (k, w) in writes.iter().enumerate() {
            if let (Some(g), Some(a)) = (failed_group, w.atomic)
                && a.group == g
            {
                acks.set(k, WriteStatus::Skipped, None);
                continue;
            }
            let target = self
                .index(w.point)
                .filter(|i| self.points[*i].output)
                .map(|i| self.resolved[i]);
            let status = match (target, w.kind) {
                (_, WriteKind::Relinquish) | (None, _) | (Some(None), _) => {
                    WriteStatus::Unsupported
                }
                (Some(Some(r)), _) if !self.present(&r) => WriteStatus::DeviceRejected,
                (Some(Some(r)), _) => match self.put(r.var, w.value) {
                    Ok(()) => {
                        self.counters.writes += 1;
                        WriteStatus::Ok
                    }
                    Err(s) => s,
                },
            };
            if status != WriteStatus::Ok
                && let Some(a) = w.atomic
            {
                failed_group = Some(a.group);
            }
            acks.set(k, status, Some(cx.now().mono_ns));
        }
    }

    fn health(&self, out: &mut HealthOut<'_>) {
        let c = self.counters;
        out.counter("reads", c.reads);
        out.counter("writes", c.writes);
        out.counter("unresolved_variables", c.unresolved);
        out.counter("absent_module_reads", c.absent);
        out.counter("resets", c.resets);
        out.counter("dropped_samples", c.dropped_samples);
    }
}
