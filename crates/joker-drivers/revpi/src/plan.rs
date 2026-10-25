//! The plan half of `revpi`: the address key `var` (ref/03 §2.2.5, ADR
//! 0078), the signal types a variable carries, and one batch per poll
//! period.

use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;

use joker_hal::descriptor::INTERFACE;
use joker_hal::plan::Plan;
use joker_hal::types::{Diagnostic, J0809, J0810, J0853};
use joker_hal::{
    BatchKind, BusKind, Capabilities, Capability, Cbor, Descriptor, Direction, InstanceMode,
    PlanRequest, Planner, PointId, PointSpec, Poll, RawKind, Tier,
};

/// The descriptor of `revpi`.
pub const DESCRIPTOR: Descriptor = Descriptor {
    name: "revpi",
    version: "0.1.0",
    tier: Tier::N,
    protocols: &["revpi"],
    buses: &[BusKind::None],
    instance: InstanceMode::Device,
    capabilities: Capabilities::of(&[Capability::Write]),
    interface: INTERFACE,
    micro: false,
    license: "Apache-2.0 OR MIT",
    description: "Revolution Pi I/O modules through the piControl process image (ADR 0078)",
};

/// What a variable carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `bool`: a variable of one bit.
    Bool,
    /// `i64`: a variable of 8, 16 or 32 bits, sign-extended.
    Signed,
    /// `u64`: a variable of 8, 16 or 32 bits, zero-extended.
    Unsigned,
}

impl Kind {
    /// True when a variable of `length` bits carries this kind.
    pub fn fits(self, length: u16) -> bool {
        match self {
            Self::Bool => length == 1,
            Self::Signed | Self::Unsigned => matches!(length, 8 | 16 | 32),
        }
    }
}

/// The variable name of a point's address `{ var }`.
pub fn var_of(addr: Cbor<'_>) -> Result<String, &'static str> {
    let mut var = None;
    for entry in addr.entries().map_err(|_| "the address is not a table")? {
        let (k, v) = entry.map_err(|_| "the address is not a table")?;
        match k {
            "var" => var = Some(v.as_str().map_err(|_| "`var` is a string")?),
            _ => return Err("unknown address key"),
        }
    }
    let var = var.ok_or("missing `var`")?;
    if var.is_empty() || var.len() > joker_picontrol_sys::MAX_NAME || var.contains('\0') {
        return Err("`var` is a piControl variable name of 1 to 31 bytes");
    }
    Ok(String::from(var))
}

/// Checks one point: a `bool`, `i64` or `u64` signal, input, parameter or
/// output.
pub fn check_point(p: &PointSpec<'_>) -> Result<(String, Kind), Diagnostic<'static>> {
    let var = var_of(p.addr).map_err(|m| Diagnostic::at(J0853, p.id, m))?;
    let kind = match (p.expect, p.width) {
        (RawKind::Bool, 0) => Kind::Bool,
        (RawKind::Int, 0) => Kind::Signed,
        (RawKind::Uint, 0) => Kind::Unsigned,
        _ => {
            return Err(Diagnostic::at(
                J0809,
                p.id,
                "a revpi variable is `bool`, `i64` or `u64`",
            ));
        }
    };
    Ok((var, kind))
}

/// The plan half of `revpi`: one batch per (device, period, on-demand)
/// group. The process image is local memory, so a batch costs no bus
/// time.
#[derive(Clone, Copy, Debug, Default)]
pub struct RevPiPlanner;

impl Planner for RevPiPlanner {
    const DESCRIPTOR: &'static Descriptor = &DESCRIPTOR;

    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>> {
        let mut errors = Vec::new();
        let mut groups: BTreeMap<(u16, u64, bool), Vec<PointId>> = BTreeMap::new();
        for p in req.points {
            if let Err(d) = check_point(p) {
                errors.push(d);
                continue;
            }
            match (p.direction, p.poll) {
                (Direction::Output, _) => {}
                (_, Poll::Period(ns)) => {
                    groups.entry((p.device, ns, false)).or_default().push(p.id)
                }
                (_, Poll::OnDemand) => groups.entry((p.device, 0, true)).or_default().push(p.id),
                (_, Poll::Push) => {
                    errors.push(Diagnostic::at(J0810, p.id, "revpi variables are polled"))
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
            plan.push(device, kind, period, &points, 10_000, 10_000, &[]);
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use joker_hal::cbor::Value;

    #[test]
    fn var_is_a_short_name() {
        let ok = Value::map([("var", Value::text("O_1"))]).encode();
        assert_eq!(var_of(Cbor::new(&ok)).unwrap(), "O_1");
        let long = Value::map([("var", Value::text(&"x".repeat(32)))]).encode();
        assert!(var_of(Cbor::new(&long)).is_err());
        let other = Value::map([("line", Value::Uint(1))]).encode();
        assert!(var_of(Cbor::new(&other)).is_err());
        assert!(var_of(Cbor::new(&Value::map([]).encode())).is_err());
    }

    #[test]
    fn kinds_fit_their_lengths() {
        assert!(Kind::Bool.fits(1) && !Kind::Bool.fits(8));
        assert!(Kind::Signed.fits(16) && Kind::Unsigned.fits(32) && !Kind::Unsigned.fits(1));
        assert!(!Kind::Signed.fits(64));
    }
}
