//! The plan half of `i2c`: point addresses of the `i2c`/`spi` family
//! (ref/03 §2.2.5) and device config (`address`, ref/03 §2.2.20).
//!
//! Scoped deliberately: only `reg`-addressed chips (`channel`, for chips
//! with their own read sequences such as multiplexed ADCs, needs per-chip
//! protocol logic this crate does not implement). `byte_order` is parsed
//! and validated here (so the manifest sees `J0806` on an invalid value)
//! but not applied: raw bytes go to the core as `RawValue::Bytes`, and
//! ref/06 §2.7's raw↔engineering decode (word/byte order, scale) happens
//! there, the same as modbus's own `word_order`/`byte_order` address keys.
//! The per-transfer cost is a fixed constant, not a real byte-serialization
//! model (ADR 0049 only specifies that model for modbus); revisit if a
//! second I²C-family driver needs real bus budgeting.

use std::collections::BTreeMap;
use std::vec::Vec;

use joker_hal::descriptor::INTERFACE;
use joker_hal::plan::Plan;
use joker_hal::types::{Diagnostic, J0809, J0810, J0852, J0853};
use joker_hal::{
    BatchKind, BusKind, Capabilities, Cbor, Descriptor, Direction, InstanceMode, PlanRequest,
    Planner, PointId, PointSpec, Poll, RawKind, Tier, code,
};

/// The descriptor of `i2c`.
pub const DESCRIPTOR: Descriptor = Descriptor {
    name: "i2c",
    version: "0.1.0",
    tier: Tier::N,
    protocols: &["i2c"],
    buses: &[BusKind::I2c],
    instance: InstanceMode::Device,
    capabilities: Capabilities::of(&[]),
    interface: INTERFACE,
    micro: false,
    license: "Apache-2.0 OR MIT",
    description: "Register-style I2C chips, read-only, chardev (ref/03 §2.2.5, docs/05 §10)",
};

/// A point's address (ref/03 §2.2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    /// The chip register.
    pub reg: u8,
    /// Bytes to transfer, 1–64.
    pub len: u8,
}

impl Address {
    /// Reads `{ reg, len, byte_order? }`. `channel` is not supported here
    /// (`J0821`-equivalent: "not supported by this version").
    pub fn parse(addr: Cbor<'_>) -> Result<Self, &'static str> {
        let mut reg = None;
        let mut len = None;
        for entry in addr.entries().map_err(|_| "the address is not a table")? {
            let (k, v) = entry.map_err(|_| "the address is not a table")?;
            match k {
                "reg" => {
                    reg = Some(
                        v.as_u64()
                            .ok()
                            .and_then(|r| u8::try_from(r).ok())
                            .ok_or("`reg` is 0–255")?,
                    );
                }
                "len" => {
                    len = Some(
                        v.as_u64()
                            .ok()
                            .filter(|l| (1..=64).contains(l))
                            .ok_or("`len` is 1–64")? as u8,
                    );
                }
                "byte_order" => {
                    let s = v.as_str().map_err(|_| "expected big or little")?;
                    if s != "big" && s != "little" {
                        return Err("expected big or little");
                    }
                }
                "channel" => return Err("not supported by this version of the driver"),
                _ => return Err("unknown address key"),
            }
        }
        Ok(Self {
            reg: reg.ok_or("missing `reg`")?,
            len: len.ok_or("`len` is required with `reg`")?,
        })
    }
}

/// Checks one point against ref/03 §2.2.5: the point's raw form is a byte
/// buffer the size of the address's `len`. Reads only: byte-width writes
/// have no encoding yet (ADR 0047's "output stage" is still future work,
/// the same boundary that keeps `modbus`'s own outputs to FC 5 coils in
/// M0).
pub fn check_point(p: &PointSpec<'_>) -> Result<Address, Diagnostic<'static>> {
    let addr = Address::parse(p.addr).map_err(|m| Diagnostic::at(J0853, p.id, m))?;
    if p.expect != RawKind::Bytes || p.width != u16::from(addr.len) {
        return Err(Diagnostic::at(
            J0809,
            p.id,
            "the point's width must match the address's `len`",
        ));
    }
    if !matches!(p.direction, Direction::Input | Direction::Param) {
        return Err(Diagnostic::at(
            J0810,
            p.id,
            "i2c has no output encoding yet (ADR 0047's output stage)",
        ));
    }
    Ok(addr)
}

/// The 7-bit device address of device `config` (ref/03 §2.2.20).
pub fn device_address(config: Cbor<'_>) -> Result<u8, &'static str> {
    config
        .get("address")
        .map_err(|_| "config is not a table")?
        .ok_or("`address` is required")?
        .as_u64()
        .ok()
        .filter(|a| (0x03..=0x77).contains(a))
        .map(|a| a as u8)
        .ok_or("`address` is 0x03\u{2013}0x77")
}

fn bus_ok(req: &PlanRequest<'_>) -> Result<(), Diagnostic<'static>> {
    req.bus
        .filter(|b| b.kind == BusKind::I2c)
        .map(|_| ())
        .ok_or(Diagnostic::general(code!("J0171"), "i2c needs an i2c bus"))
}

/// The plan half of `i2c`: one batch per (device, period, on-demand)
/// group, no coalescing (each point is an independent transfer).
#[derive(Clone, Copy, Debug, Default)]
pub struct I2cPlanner;

impl Planner for I2cPlanner {
    const DESCRIPTOR: &'static Descriptor = &DESCRIPTOR;

    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>> {
        let mut errors = Vec::new();
        if let Err(d) = bus_ok(req) {
            errors.push(d);
        }
        for d in req.devices {
            if let Err(m) = device_address(d.config) {
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
                    "i2c reads are polled in this version",
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
            plan.push(device, kind, period, &points, 50_000, 50_000, &[]);
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
    fn reg_and_len_are_read() {
        let bytes = addr(&[("reg", Value::Uint(3)), ("len", Value::Uint(2))]);
        let a = Address::parse(Cbor::new(&bytes)).unwrap();
        assert_eq!(a, Address { reg: 3, len: 2 });
    }

    #[test]
    fn byte_order_is_validated_but_not_kept() {
        let bytes = addr(&[
            ("reg", Value::Uint(0)),
            ("len", Value::Uint(1)),
            ("byte_order", Value::text("little")),
        ]);
        assert!(Address::parse(Cbor::new(&bytes)).is_ok());
        let bad = addr(&[
            ("reg", Value::Uint(0)),
            ("len", Value::Uint(1)),
            ("byte_order", Value::text("sideways")),
        ]);
        assert!(Address::parse(Cbor::new(&bad)).is_err());
    }

    #[test]
    fn len_out_of_range_is_an_error() {
        for len in [0, 65] {
            let bytes = addr(&[("reg", Value::Uint(0)), ("len", Value::Uint(len))]);
            assert!(Address::parse(Cbor::new(&bytes)).is_err(), "{len}");
        }
    }

    #[test]
    fn missing_reg_or_len_is_an_error() {
        assert!(Address::parse(Cbor::new(&addr(&[("len", Value::Uint(1))]))).is_err());
        assert!(Address::parse(Cbor::new(&addr(&[("reg", Value::Uint(0))]))).is_err());
    }

    #[test]
    fn channel_is_not_supported() {
        let bytes = addr(&[("channel", Value::Uint(0))]);
        assert!(Address::parse(Cbor::new(&bytes)).is_err());
    }

    #[test]
    fn device_address_is_a_valid_7_bit_address() {
        let ok = Value::map([("address", Value::Uint(0x40))]).encode();
        assert_eq!(device_address(Cbor::new(&ok)).unwrap(), 0x40);
        for bad in [
            Value::map([("address", Value::Uint(0x02))]),
            Value::map([("address", Value::Uint(0x78))]),
            Value::map([]),
        ] {
            assert!(device_address(Cbor::new(&bad.encode())).is_err());
        }
    }
}
