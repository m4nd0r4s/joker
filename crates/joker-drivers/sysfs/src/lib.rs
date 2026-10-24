#![doc = include_str!("../README.md")]

use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;

use joker_hal::descriptor::INTERFACE;
use joker_hal::link::DevicePaths;
use joker_hal::plan::Plan;
use joker_hal::types::{Diagnostic, J0809, J0853};
use joker_hal::{
    BatchFailure, BatchKind, BusKind, Capabilities, Capability, Cbor, CommittedWrite, ConnectOut,
    Cx, Descriptor, Direction, Driver, DriverError, HealthOut, InstanceMode, Link, LinkState,
    OpenArgs, PlanRequest, Planner, PointId, PointSpec, Poll, RawKind, RawSample, RawSink,
    RawStatus, RawValue, ReadBatch, Reason, Tier, WriteAcks, WriteKind, WriteStatus,
};

/// The descriptor of `sysfs`.
pub const DESCRIPTOR: Descriptor = Descriptor {
    name: "sysfs",
    version: "0.1.0",
    tier: Tier::N,
    protocols: &["sysfs"],
    buses: &[BusKind::None],
    instance: InstanceMode::Device,
    capabilities: Capabilities::of(&[Capability::Write]),
    interface: INTERFACE,
    micro: false,
    license: "Apache-2.0 OR MIT",
    description: "LEDs and thermal zones of any Linux board, through /sys/class (docs/05 §10)",
};

/// The root the host grants on a node.
pub const SYSFS_ROOT: &str = "/sys";

/// What a point addresses (ref/01 appendix A).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    /// `led = "<name>"`: `/sys/class/leds/<name>`.
    Led(String),
    /// `thermal = "<zone>"`: `/sys/class/thermal/<zone>`.
    Thermal(String),
}

impl Address {
    /// The directory under the root, which the host grants.
    pub fn dir(&self) -> String {
        match self {
            Self::Led(n) => std::format!("class/leds/{n}"),
            Self::Thermal(z) => std::format!("class/thermal/{z}"),
        }
    }

    /// Reads `{ led = … }` or `{ thermal = … }`.
    pub fn parse(addr: Cbor<'_>) -> Result<Self, &'static str> {
        let text = |key| -> Result<Option<String>, &'static str> {
            match addr.get(key).map_err(|_| "the address is not a table")? {
                None => Ok(None),
                Some(v) => {
                    let s = v.as_str().map_err(|_| "expected a name")?;
                    let ok = !s.is_empty()
                        && s.len() <= 64
                        && s != "."
                        && s != ".."
                        && !s.contains('/')
                        && !s.contains('\0');
                    if ok {
                        Ok(Some(String::from(s)))
                    } else {
                        Err("invalid name")
                    }
                }
            }
        };
        let entries = addr.entries().map_err(|_| "the address is not a table")?;
        for entry in entries {
            let (k, _) = entry.map_err(|_| "the address is not a table")?;
            if k != "led" && k != "thermal" && k != "direction" {
                return Err("unknown address key");
            }
        }
        match (text("led")?, text("thermal")?) {
            (Some(l), None) => Ok(Self::Led(l)),
            (None, Some(t)) => Ok(Self::Thermal(t)),
            _ => Err("exactly one of `led` and `thermal`"),
        }
    }
}

/// The directories to grant for `request`, for the host's [`DevicePaths`].
pub fn grants(request: &PlanRequest<'_>) -> Vec<String> {
    let mut dirs: Vec<String> = request
        .points
        .iter()
        .filter_map(|p| Address::parse(p.addr).ok().map(|a| a.dir()))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn check(p: &PointSpec<'_>) -> Result<Address, Diagnostic<'static>> {
    let addr = Address::parse(p.addr).map_err(|m| Diagnostic::at(J0853, p.id, m))?;
    match (&addr, p.direction, p.expect) {
        (Address::Led(_), Direction::Input | Direction::Output, RawKind::Bool) => Ok(addr),
        (Address::Thermal(_), Direction::Input, RawKind::Float) => Ok(addr),
        (Address::Thermal(_), Direction::Output | Direction::Param, _) => {
            Err(Diagnostic::at(J0853, p.id, "a thermal zone is an input"))
        }
        _ => Err(Diagnostic::at(
            J0809,
            p.id,
            "a LED is bool and a thermal zone f64 in °C",
        )),
    }
}

/// The plan half of `sysfs`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SysfsPlanner;

impl Planner for SysfsPlanner {
    const DESCRIPTOR: &'static Descriptor = &DESCRIPTOR;

    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>> {
        let mut errors = Vec::new();
        let mut groups: BTreeMap<(u16, u64, bool), Vec<PointId>> = BTreeMap::new();
        let mut leds: BTreeMap<String, PointId> = BTreeMap::new();
        for p in req.points {
            let addr = match check(p) {
                Ok(a) => a,
                Err(d) => {
                    errors.push(d);
                    continue;
                }
            };
            if let Address::Led(name) = &addr
                && leds.insert(name.clone(), p.id).is_some()
            {
                errors.push(Diagnostic::at(
                    joker_hal::code!("J0178"),
                    p.id,
                    "the same LED twice in one device",
                ));
            }
            match (p.direction, p.poll) {
                (Direction::Output, _) => {}
                (_, Poll::Period(ns)) => {
                    groups.entry((p.device, ns, false)).or_default().push(p.id)
                }
                (_, Poll::OnDemand) => groups.entry((p.device, 0, true)).or_default().push(p.id),
                (_, Poll::Push) => {
                    errors.push(Diagnostic::at(J0809, p.id, "sysfs points are polled"))
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut plan = Plan::new();
        for ((device, period, on_demand), points) in groups {
            let kind = if on_demand {
                BatchKind::OnDemand
            } else {
                BatchKind::Poll
            };
            plan.push(device, kind, period, &points, 50_000, 50_000, &[]);
        }
        Ok(plan)
    }
}

/// Reads the current trigger: the bracketed word of `trigger`, or the only
/// word.
pub fn current_trigger(text: &str) -> Option<&str> {
    let mut words = text.split_whitespace();
    let bracketed = text
        .split_whitespace()
        .find_map(|w| w.strip_prefix('[').and_then(|w| w.strip_suffix(']')));
    match (bracketed, words.next(), words.next()) {
        (Some(b), _, _) => Some(b),
        (None, Some(only), None) => Some(only),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Led {
    point: PointId,
    dir: String,
    output: bool,
    max: Option<u32>,
    saved_trigger: Option<String>,
    broken: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Point {
    Led(usize),
    Thermal(PointId, String),
}

/// The runtime half of `sysfs`.
#[derive(Debug)]
pub struct SysfsDriver {
    paths: DevicePaths,
    leds: Vec<Led>,
    points: Vec<(PointId, Point)>,
    errors: u64,
    dropped_samples: u64,
}

fn read_number(paths: &DevicePaths, file: &str) -> Result<i64, std::io::Error> {
    let text = paths.read(file)?;
    text.trim()
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "not a number"))
}

fn io_status(e: &std::io::Error) -> RawStatus {
    match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
            RawStatus::bad(Reason::CONFIG_ERROR)
        }
        std::io::ErrorKind::InvalidData => RawStatus::bad(Reason::DEVICE_FAULT),
        _ => RawStatus::bad(Reason::COMM_FAILURE),
    }
}

impl SysfsDriver {
    fn led_max(&mut self, i: usize) -> Option<u32> {
        let led = &mut self.leds[i];
        if led.max.is_none() {
            led.max = read_number(&self.paths, &std::format!("{}/max_brightness", led.dir))
                .ok()
                .and_then(|m| u32::try_from(m).ok())
                .filter(|m| *m > 0);
        }
        led.max
    }

    fn read_point(&mut self, point: PointId, now: u64) -> RawSample<'static> {
        let Some(p) = self
            .points
            .iter()
            .find(|p| p.0 == point)
            .map(|p| p.1.clone())
        else {
            return RawSample::failed(point, RawStatus::bad(Reason::CONFIG_ERROR));
        };
        let result = match p {
            Point::Led(i) if self.leds[i].broken => Err(RawStatus::bad(Reason::CONFIG_ERROR)),
            Point::Led(i) => read_number(
                &self.paths,
                &std::format!("{}/brightness", self.leds[i].dir),
            )
            .map(|b| RawValue::Bool(b > 0))
            .map_err(|e| io_status(&e)),
            Point::Thermal(_, dir) => read_number(&self.paths, &std::format!("{dir}/temp"))
                .map(|m| RawValue::Float(m as f64 / 1000.0))
                .map_err(|e| io_status(&e)),
        };
        match result {
            Ok(value) => RawSample {
                ts_hw: Some(now),
                ..RawSample::good(point, value)
            },
            Err(status) => {
                self.errors += 1;
                RawSample::failed(point, status)
            }
        }
    }

    fn write_led(&mut self, i: usize, on: bool) -> WriteStatus {
        if self.leds[i].broken || !self.leds[i].output {
            return WriteStatus::DeviceRejected;
        }
        let Some(max) = self.led_max(i) else {
            return WriteStatus::DeviceRejected;
        };
        let dir = self.leds[i].dir.clone();
        let level = if on { max } else { 0 };
        if let Err(e) = self
            .paths
            .write(&std::format!("{dir}/brightness"), &std::format!("{level}"))
        {
            self.errors += 1;
            return match e.kind() {
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound => {
                    WriteStatus::DeviceRejected
                }
                _ => WriteStatus::CommFailure,
            };
        }
        match read_number(&self.paths, &std::format!("{dir}/brightness")) {
            Ok(b) if (b > 0) == on => WriteStatus::Ok,
            Ok(_) => WriteStatus::DeviceRejected,
            Err(_) => WriteStatus::CommFailure,
        }
    }
}

impl Driver for SysfsDriver {
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError> {
        let Link::Paths(paths) = link else {
            return Err(DriverError::new(
                joker_hal::code!("J0873"),
                "sysfs needs the device paths granted by the manifest",
            ));
        };
        let mut leds = Vec::new();
        let mut points = Vec::new();
        for p in args.request().points {
            let addr = check(p).map_err(|d| DriverError::new(d.code, d.message))?;
            let point = match addr {
                Address::Led(_) => {
                    leds.push(Led {
                        point: p.id,
                        dir: addr.dir(),
                        output: p.direction == Direction::Output,
                        max: None,
                        saved_trigger: None,
                        broken: false,
                    });
                    Point::Led(leds.len() - 1)
                }
                Address::Thermal(_) => Point::Thermal(p.id, addr.dir()),
            };
            points.push((p.id, point));
        }
        Ok(Self {
            paths,
            leds,
            points,
            errors: 0,
            dropped_samples: 0,
        })
    }

    async fn connect(
        &mut self,
        cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        let mut broken = 0;
        for i in 0..self.leds.len() {
            self.leds[i].broken = false;
            let max = self.led_max(i);
            let led = &mut self.leds[i];
            if max.is_none() {
                led.broken = true;
                broken += 1;
                continue;
            }
            if !led.output || led.saved_trigger.is_some() {
                continue;
            }
            let file = std::format!("{}/trigger", led.dir);
            let current = self
                .paths
                .read(&file)
                .ok()
                .and_then(|t| current_trigger(&t).map(String::from));
            match current {
                Some(t) if self.paths.write(&file, "none").is_ok() => led.saved_trigger = Some(t),
                _ => {
                    led.broken = true;
                    broken += 1;
                }
            }
        }
        if broken > 0 {
            cx.report_link(
                LinkState::Degraded,
                "LED not writable: check the udev rule (jk doctor)",
            );
        }
        Ok(())
    }

    async fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let now = cx.now().mono_ns;
        for &point in batch.points {
            let sample = self.read_point(point, now);
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
            let led = self.points.iter().find_map(|p| match p {
                (id, Point::Led(i)) if *id == w.point => Some(*i),
                _ => None,
            });
            let status = match (led, w.value, w.kind) {
                (_, _, WriteKind::Relinquish) => WriteStatus::Unsupported,
                (Some(i), RawValue::Bool(on), _) => self.write_led(i, on),
                _ => WriteStatus::Unsupported,
            };
            acks.set(k, status, Some(now));
        }
    }

    fn health(&self, out: &mut HealthOut<'_>) {
        out.counter("errors", self.errors);
        out.counter("dropped_samples", self.dropped_samples);
        out.counter(
            "leds_broken",
            self.leds.iter().filter(|l| l.broken).count() as u64,
        );
    }

    async fn disconnect(&mut self, _cx: &mut Cx<'_>) {
        for led in &mut self.leds {
            if let Some(t) = led.saved_trigger.take() {
                let _ = self.paths.write(&std::format!("{}/trigger", led.dir), &t);
            }
        }
    }
}

impl SysfsDriver {
    /// The point of LED `name`, if this instance drives it.
    pub fn led_point(&self, name: &str) -> Option<PointId> {
        let dir = Address::Led(String::from(name)).dir();
        self.leds.iter().find(|l| l.dir == dir).map(|l| l.point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use joker_hal::cbor::Value;

    #[test]
    fn triggers_and_addresses() {
        assert_eq!(
            current_trigger("none [mmc0] timer heartbeat\n"),
            Some("mmc0")
        );
        assert_eq!(current_trigger("[none] mmc0"), Some("none"));
        assert_eq!(current_trigger("none\n"), Some("none"));
        assert_eq!(current_trigger("a b"), None);
        let led = Value::map([("led", Value::text("ACT"))]).encode();
        assert_eq!(
            Address::parse(Cbor::new(&led)),
            Ok(Address::Led(String::from("ACT")))
        );
        assert_eq!(Address::Led(String::from("ACT")).dir(), "class/leds/ACT");
        for bad in [
            Value::map([("led", Value::text("../x"))]),
            Value::map([("led", Value::text("a")), ("thermal", Value::text("b"))]),
            Value::map([]),
            Value::map([("gpio", Value::text("a"))]),
        ] {
            assert!(Address::parse(Cbor::new(&bad.encode())).is_err());
        }
    }
}
