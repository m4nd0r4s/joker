//! The plan half of `gpio`: point addresses of the `gpio` family (ref/03
//! §2.2.5) and device config (`chip`, ref/01 appendix A).
//!
//! Scoped deliberately: only integer `line` indices (named lines need the
//! real chip's name→offset map, not available without the Linux adapter);
//! `bias` and `drive` are parsed and kept on the point (for the host's real
//! line configuration) but not applied by this crate's own logic, which
//! only inverts on `active_low`. `edges`, `timestamp` and `debounce` are a
//! manifest error here (push/interrupt-driven reads are not implemented).

use std::collections::BTreeMap;
use std::vec::Vec;

use joker_hal::descriptor::INTERFACE;
use joker_hal::plan::Plan;
use joker_hal::types::{Diagnostic, J0809, J0810, J0852, J0853};
use joker_hal::{
    BatchKind, BusKind, Capabilities, Capability, Cbor, Descriptor, Direction, InstanceMode,
    PlanRequest, Planner, PointId, PointSpec, Poll, RawKind, Tier,
};

/// The descriptor of `gpio`.
pub const DESCRIPTOR: Descriptor = Descriptor {
    name: "gpio",
    version: "0.1.0",
    tier: Tier::N,
    protocols: &["gpio"],
    buses: &[BusKind::None],
    instance: InstanceMode::Device,
    capabilities: Capabilities::of(&[Capability::Write]),
    interface: INTERFACE,
    micro: false,
    license: "Apache-2.0 OR MIT",
    description: "Digital lines of a Linux gpiochip, chardev v1 (ref/03 §2.2.5, docs/05 §10; \
        v1, not v2 — the bench board's kernel predates v2, which needs Linux 5.10+)",
};

/// `bias` (ref/03 §2.2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    /// `as-is` (default): leaves the line's current pull.
    AsIs,
    /// `pull-up`.
    PullUp,
    /// `pull-down`.
    PullDown,
    /// `disabled`.
    Disabled,
}

/// `drive` (ref/03 §2.2.5), outputs only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drive {
    /// `push-pull` (default).
    PushPull,
    /// `open-drain`.
    OpenDrain,
    /// `open-source`.
    OpenSource,
}

/// A point's address (ref/03 §2.2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    /// The line index (named lines are not supported here).
    pub line: u32,
    /// Inverts the logical value against the physical one.
    pub active_low: bool,
    /// Kept for the host's real line configuration; not applied here.
    pub bias: Bias,
    /// Kept for the host's real line configuration; not applied here.
    pub drive: Drive,
}

impl Address {
    /// Reads `{ line, active_low?, bias?, drive? }`. `edges`, `timestamp`
    /// and `debounce` are a manifest error (`J0821`): not implemented.
    pub fn parse(addr: Cbor<'_>) -> Result<Self, &'static str> {
        let mut line = None;
        let mut active_low = false;
        let mut bias = Bias::AsIs;
        let mut drive = Drive::PushPull;
        for entry in addr.entries().map_err(|_| "the address is not a table")? {
            let (k, v) = entry.map_err(|_| "the address is not a table")?;
            match k {
                "line" => {
                    line = Some(
                        v.as_u64()
                            .ok()
                            .and_then(|l| u32::try_from(l).ok())
                            .ok_or("`line` is a non-negative integer")?,
                    );
                }
                "active_low" => {
                    active_low = v.as_bool().map_err(|_| "`active_low` is a bool")?;
                }
                "bias" => {
                    bias = match v.as_str().map_err(|_| "invalid `bias`")? {
                        "as-is" => Bias::AsIs,
                        "pull-up" => Bias::PullUp,
                        "pull-down" => Bias::PullDown,
                        "disabled" => Bias::Disabled,
                        _ => return Err("invalid `bias`"),
                    };
                }
                "drive" => {
                    drive = match v.as_str().map_err(|_| "invalid `drive`")? {
                        "push-pull" => Drive::PushPull,
                        "open-drain" => Drive::OpenDrain,
                        "open-source" => Drive::OpenSource,
                        _ => return Err("invalid `drive`"),
                    };
                }
                "edges" | "timestamp" | "debounce" => {
                    return Err("not supported by this version of the driver");
                }
                _ => return Err("unknown address key"),
            }
        }
        Ok(Self {
            line: line.ok_or("missing `line`")?,
            active_low,
            bias,
            drive,
        })
    }
}

/// Checks one point against ref/03 §2.2.5: a gpio line is always `bool`.
pub fn check_point(p: &PointSpec<'_>) -> Result<Address, Diagnostic<'static>> {
    let addr = Address::parse(p.addr).map_err(|m| Diagnostic::at(J0853, p.id, m))?;
    if p.expect != RawKind::Bool || p.width != 0 {
        return Err(Diagnostic::at(J0809, p.id, "a gpio line is `bool`"));
    }
    if !matches!(p.direction, Direction::Input | Direction::Output) {
        return Err(Diagnostic::at(
            J0810,
            p.id,
            "a gpio line is `input` or `output`, not `param`",
        ));
    }
    Ok(addr)
}

/// The chip name of device `config` (ref/01 appendix A).
pub fn chip_id(config: Cbor<'_>) -> Result<std::string::String, &'static str> {
    let name = config
        .get("chip")
        .map_err(|_| "config is not a table")?
        .ok_or("`chip` is required")?
        .as_str()
        .map_err(|_| "`chip` is a string")?;
    if name.is_empty() {
        return Err("`chip` is required");
    }
    Ok(std::string::String::from(name))
}

/// The plan half of `gpio`: one batch per (device, period, on-demand)
/// group, no coalescing (each line is independent; ADR 0049 does not
/// budget a local chardev transport the way it budgets a bus).
#[derive(Clone, Copy, Debug, Default)]
pub struct GpioPlanner;

impl Planner for GpioPlanner {
    const DESCRIPTOR: &'static Descriptor = &DESCRIPTOR;

    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>> {
        let mut errors = Vec::new();
        for d in req.devices {
            if let Err(m) = chip_id(d.config) {
                errors.push(Diagnostic::general(J0852, m));
            }
        }
        let mut groups: BTreeMap<(u16, u64, bool), Vec<PointId>> = BTreeMap::new();
        for p in req.points {
            match check_point(p) {
                Ok(_) => {}
                Err(d) => {
                    errors.push(d);
                    continue;
                }
            }
            match (p.direction, p.poll) {
                (Direction::Output, _) => {}
                (_, Poll::Period(ns)) => {
                    groups.entry((p.device, ns, false)).or_default().push(p.id)
                }
                (_, Poll::OnDemand) => groups.entry((p.device, 0, true)).or_default().push(p.id),
                (_, Poll::Push) => errors.push(Diagnostic::at(
                    J0810,
                    p.id,
                    "gpio reads are polled in this version",
                )),
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
            plan.push(device, kind, period, &points, 10_000, 10_000, &[]);
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use joker_hal::cbor::Value;

    fn addr(pairs: &[(&str, Value)]) -> Vec<u8> {
        Value::Map(
            pairs
                .iter()
                .map(|(k, v)| (std::string::String::from(*k), v.clone()))
                .collect(),
        )
        .encode()
    }

    #[test]
    fn a_bare_line_defaults_active_low_bias_and_drive() {
        let bytes = addr(&[("line", Value::Uint(17))]);
        let a = Address::parse(Cbor::new(&bytes)).unwrap();
        assert_eq!(a.line, 17);
        assert!(!a.active_low);
        assert_eq!(a.bias, Bias::AsIs);
        assert_eq!(a.drive, Drive::PushPull);
    }

    #[test]
    fn active_low_bias_and_drive_are_read() {
        let bytes = addr(&[
            ("line", Value::Uint(5)),
            ("active_low", Value::Bool(true)),
            ("bias", Value::text("pull-up")),
            ("drive", Value::text("open-drain")),
        ]);
        let a = Address::parse(Cbor::new(&bytes)).unwrap();
        assert!(a.active_low);
        assert_eq!(a.bias, Bias::PullUp);
        assert_eq!(a.drive, Drive::OpenDrain);
    }

    #[test]
    fn missing_line_is_an_error() {
        assert!(Address::parse(Cbor::new(&addr(&[]))).is_err());
    }

    #[test]
    fn an_unknown_key_is_an_error() {
        let bytes = addr(&[("line", Value::Uint(1)), ("bogus", Value::Uint(1))]);
        assert!(Address::parse(Cbor::new(&bytes)).is_err());
    }

    #[test]
    fn edges_timestamp_and_debounce_are_not_supported() {
        for key in ["edges", "timestamp", "debounce"] {
            let bytes = addr(&[("line", Value::Uint(1)), (key, Value::text("rising"))]);
            assert!(Address::parse(Cbor::new(&bytes)).is_err(), "{key}");
        }
    }

    #[test]
    fn chip_id_requires_a_nonempty_string() {
        let ok = Value::map([("chip", Value::text("gpiochip0"))]).encode();
        assert_eq!(chip_id(Cbor::new(&ok)).unwrap(), "gpiochip0");
        let missing = Value::map([]).encode();
        assert!(chip_id(Cbor::new(&missing)).is_err());
        let empty = Value::map([("chip", Value::text(""))]).encode();
        assert!(chip_id(Cbor::new(&empty)).is_err());
    }
}
