//! The plan half of `modbus-rtu`: point addresses and quirks of the
//! `modbus` family (ref/03 §2.2.5–§2.2.6), and the coalescing of points into
//! one request per batch.

use std::collections::BTreeMap;
use std::vec::Vec;

use joker_hal::descriptor::INTERFACE;
use joker_hal::link::SerialInfo;
use joker_hal::plan::Plan;
use joker_hal::types::{Code, Diagnostic, J0809, J0810, J0852, J0853};
use joker_hal::{
    BatchKind, BusKind, Capabilities, Capability, Cbor, Descriptor, Direction, InstanceMode,
    PlanRequest, Planner, PointId, PointSpec, Poll, RawKind, Tier, code,
};

use crate::rtu::read_response_len;

/// The descriptor of `modbus-rtu`.
pub const DESCRIPTOR: Descriptor = Descriptor {
    name: "modbus-rtu",
    version: "0.1.0",
    tier: Tier::N,
    protocols: &["modbus"],
    buses: &[BusKind::Serial],
    instance: InstanceMode::Device,
    capabilities: Capabilities::of(&[Capability::Write, Capability::RawAccess]),
    interface: INTERFACE,
    micro: false,
    license: "Apache-2.0 OR MIT",
    description: "Modbus RTU client over a serial bus (ref/03 §2.2)",
};

/// Unknown or out-of-range quirk.
pub const J0818: Code = code!("J0818");

/// A point's address (ref/03 §2.2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    /// Protocol address, base 0.
    pub at: u16,
    /// Function code.
    pub fc: u8,
    /// Extracted bit, with FC 3/4 and `bool`.
    pub bit: Option<u8>,
    /// `word_order = "little"`: the first register is the least
    /// significant.
    pub word_little: bool,
    /// `byte_order = "little"`: the bytes of each register are swapped.
    pub byte_little: bool,
}

impl Address {
    /// Reads `{ at, fc, word_order?, byte_order?, bit? }`.
    pub fn parse(addr: Cbor<'_>) -> Result<Self, &'static str> {
        let mut at = None;
        let mut fc = None;
        let mut bit = None;
        let mut word_little = false;
        let mut byte_little = false;
        for entry in addr.entries().map_err(|_| "the address is not a table")? {
            let (k, v) = entry.map_err(|_| "the address is not a table")?;
            match k {
                "at" => {
                    at = Some(
                        v.as_u64()
                            .ok()
                            .and_then(|a| u16::try_from(a).ok())
                            .ok_or("`at` is 0–65535")?,
                    )
                }
                "fc" => {
                    fc = Some(
                        v.as_u64()
                            .ok()
                            .and_then(|f| u8::try_from(f).ok())
                            .ok_or("invalid `fc`")?,
                    )
                }
                "bit" => {
                    bit = Some(v.as_u64().ok().filter(|b| *b < 16).ok_or("`bit` is 0–15")? as u8)
                }
                "word_order" | "byte_order" => {
                    let little = match v.as_str().map_err(|_| "expected big or little")? {
                        "big" => false,
                        "little" => true,
                        _ => return Err("expected big or little"),
                    };
                    if k == "word_order" {
                        word_little = little;
                    } else {
                        byte_little = little;
                    }
                }
                _ => return Err("unknown address key"),
            }
        }
        Ok(Self {
            at: at.ok_or("missing `at`")?,
            fc: fc.ok_or("missing `fc`")?,
            bit,
            word_little,
            byte_little,
        })
    }
}

/// The `quirks` of the `modbus` family that M0 uses (ref/03 §2.2.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quirks {
    /// Registers per read request.
    pub max_regs: u16,
    /// Bits per read request.
    pub max_bits: u16,
    /// Largest gap of unused registers coalesced into a request.
    pub max_gap: u16,
    /// Minimum time between two requests to the equipment.
    pub min_interval_ns: u64,
    /// Response latency of the equipment.
    pub latency_ns: u64,
}

impl Default for Quirks {
    fn default() -> Self {
        Self {
            max_regs: 125,
            max_bits: 2000,
            max_gap: 8,
            min_interval_ns: 0,
            latency_ns: 10_000_000,
        }
    }
}

impl Quirks {
    /// Reads the quirks CBOR; durations are ns.
    pub fn parse(quirks: Cbor<'_>) -> Result<Self, &'static str> {
        let mut q = Self::default();
        if quirks.is_null() {
            return Ok(q);
        }
        for entry in quirks.entries().map_err(|_| "quirks are not a table")? {
            let (k, v) = entry.map_err(|_| "quirks are not a table")?;
            let n = v.as_u64().map_err(|_| "expected a number");
            match k {
                "max_regs" => q.max_regs = n?.clamp(0, 125) as u16,
                "max_bits" => q.max_bits = n?.clamp(0, 2000) as u16,
                "max_gap" => q.max_gap = n?.clamp(0, 125) as u16,
                "min_interval" => q.min_interval_ns = n?,
                "latency" => q.latency_ns = n?,
                "forbidden" | "single_write" | "connect_timeout" => {}
                _ => return Err("unknown quirk of the modbus family"),
            }
        }
        if q.max_regs == 0 || q.max_bits == 0 {
            return Err("max_regs and max_bits are at least 1");
        }
        Ok(q)
    }
}

/// Registers or bits a point occupies.
pub fn span(p: &PointSpec<'_>) -> u16 {
    if p.expect == RawKind::Regs {
        p.width.max(1)
    } else {
        1
    }
}

/// Checks one point against ref/03 §2.2.5 and the M0 write subset.
pub fn check_point(p: &PointSpec<'_>) -> Result<Address, Diagnostic<'static>> {
    let a = Address::parse(p.addr).map_err(|m| Diagnostic::at(J0853, p.id, m))?;
    let fc_ok = match p.direction {
        Direction::Input | Direction::Param => matches!(a.fc, 1..=4),
        Direction::Output => a.fc == 5,
    };
    if !fc_ok {
        let m = if p.direction == Direction::Output {
            "outputs are coils (FC 5) in M0; register writes come with the output stage (ADR 0047)"
        } else {
            "inputs and parameters read with FC 1, 2, 3 or 4"
        };
        return Err(Diagnostic::at(J0810, p.id, m));
    }
    let bits = matches!(a.fc, 1 | 2 | 5);
    let type_ok = if bits {
        p.expect == RawKind::Bool && p.width == 0 && p.type_name == "bool" && a.bit.is_none()
    } else {
        p.expect == RawKind::Regs && p.width >= 1 && (a.bit.is_none() || p.type_name == "bool")
    };
    if !type_ok {
        return Err(Diagnostic::at(
            J0809,
            p.id,
            "type does not fit the function code",
        ));
    }
    if u32::from(a.at) + u32::from(span(p)) - 1 > 65_535 {
        return Err(Diagnostic::at(J0810, p.id, "address past 65535"));
    }
    Ok(a)
}

/// A planned read request: what the batch's private bytes hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// Function code.
    pub fc: u8,
    /// First address.
    pub start: u16,
    /// Registers or bits.
    pub count: u16,
}

impl Request {
    /// The five private bytes of a batch.
    pub fn encode(&self) -> [u8; 5] {
        let [s0, s1] = self.start.to_be_bytes();
        let [c0, c1] = self.count.to_be_bytes();
        [self.fc, s0, s1, c0, c1]
    }

    /// Reads the private bytes of a batch.
    pub fn decode(private: &[u8]) -> Option<Self> {
        let [fc, s0, s1, c0, c1] = *private else {
            return None;
        };
        Some(Self {
            fc,
            start: u16::from_be_bytes([s0, s1]),
            count: u16::from_be_bytes([c0, c1]),
        })
    }
}

fn baud(req: &PlanRequest<'_>) -> Result<SerialInfo, Diagnostic<'static>> {
    let bus = req
        .bus
        .filter(|b| b.kind == BusKind::Serial)
        .ok_or(Diagnostic::general(
            code!("J0171"),
            "modbus-rtu needs a serial bus",
        ))?;
    let baud = bus
        .params
        .get("baud")
        .ok()
        .flatten()
        .and_then(|b| b.as_u64().ok())
        .and_then(|b| u32::try_from(b).ok())
        .ok_or(Diagnostic::general(
            J0852,
            "the serial bus has no valid `baud`",
        ))?;
    Ok(SerialInfo::new_8n1(baud))
}

/// The unit id of device `config` (1–247, ref/01 appendix A).
pub fn unit_id(config: Cbor<'_>) -> Result<u8, &'static str> {
    config
        .get("unit_id")
        .map_err(|_| "config is not a table")?
        .ok_or("`unit_id` is required")?
        .as_u64()
        .ok()
        .filter(|u| (1..=247).contains(u))
        .map(|u| u as u8)
        .ok_or("`unit_id` is 1–247")
}

/// Device, function code, period and on-demand flag of a group of points.
type GroupKey = (u16, u8, u64, bool);

/// The plan half of `modbus-rtu`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModbusPlanner;

impl Planner for ModbusPlanner {
    const DESCRIPTOR: &'static Descriptor = &DESCRIPTOR;

    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>> {
        let mut errors = Vec::new();
        let info = match baud(req) {
            Ok(i) => i,
            Err(d) => return Err(std::vec![d]),
        };
        let mut quirks = BTreeMap::new();
        for d in req.devices {
            if let Err(m) = unit_id(d.config) {
                errors.push(Diagnostic::general(J0852, m));
            }
            match Quirks::parse(d.quirks) {
                Ok(q) => {
                    quirks.insert(d.index, q);
                }
                Err(m) => errors.push(Diagnostic::general(J0818, m)),
            }
        }
        let mut groups: BTreeMap<GroupKey, Vec<(u16, u16, PointId)>> = BTreeMap::new();
        for p in req.points {
            let a = match check_point(p) {
                Ok(a) => a,
                Err(d) => {
                    errors.push(d);
                    continue;
                }
            };
            let key = match (p.direction, p.poll) {
                (Direction::Output, _) => continue,
                (_, Poll::Period(ns)) => (p.device, a.fc, ns, false),
                (_, Poll::OnDemand) => (p.device, a.fc, 0, true),
                (_, Poll::Push) => {
                    errors.push(Diagnostic::at(J0809, p.id, "modbus points are polled"));
                    continue;
                }
            };
            groups.entry(key).or_default().push((a.at, span(p), p.id));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut plan = Plan::new();
        for ((device, fc, period, on_demand), mut points) in groups {
            let q = quirks.get(&device).copied().unwrap_or_default();
            let max = if matches!(fc, 1 | 2) {
                q.max_bits
            } else {
                q.max_regs
            };
            points.sort();
            let mut current: Option<(u16, u32, Vec<PointId>)> = None;
            let flush = |c: (u16, u32, Vec<PointId>), plan: &mut Plan| {
                let count = (c.1 - u32::from(c.0)) as u16;
                let r = Request {
                    fc,
                    start: c.0,
                    count,
                };
                let bytes = 8 + read_response_len(fc, count) as u32;
                let cost = info.wire_ns(bytes) + q.latency_ns + 2 * info.wire_ns(4);
                let kind = if on_demand {
                    BatchKind::OnDemand
                } else {
                    BatchKind::Poll
                };
                plan.push(
                    device,
                    kind,
                    period,
                    &c.2,
                    cost,
                    info.wire_ns(16) + q.latency_ns,
                    &r.encode(),
                );
            };
            for (at, n, id) in points {
                let end = u32::from(at) + u32::from(n);
                current = match current.take() {
                    Some((start, cur_end, mut ids))
                        if u32::from(at) <= cur_end + u32::from(q.max_gap)
                            && end.max(cur_end) - u32::from(start) <= u32::from(max) =>
                    {
                        ids.push(id);
                        Some((start, end.max(cur_end), ids))
                    }
                    Some(c) => {
                        flush(c, &mut plan);
                        Some((at, end, std::vec![id]))
                    }
                    None => Some((at, end, std::vec![id])),
                };
            }
            if let Some(c) = current {
                flush(c, &mut plan);
            }
        }
        Ok(plan)
    }
}

/// The descriptor of `modbus-tcp`.
pub const DESCRIPTOR_TCP: Descriptor = Descriptor {
    name: "modbus-tcp",
    version: "0.1.0",
    tier: Tier::N,
    protocols: &["modbus"],
    buses: &[BusKind::Ethernet],
    instance: InstanceMode::Device,
    capabilities: Capabilities::of(&[Capability::Write, Capability::RawAccess]),
    interface: INTERFACE,
    micro: false,
    license: "Apache-2.0 OR MIT",
    description: "Modbus TCP client (ref/03 §2.2)",
};

/// The plan half of `modbus-tcp`. Shares point checking, addressing and
/// request encoding with `modbus-rtu`; only the cost model differs (ADR
/// 0049 §2: a TCP device without a shared bus is not budgeted, and the
/// cost of a batch on a shared `ethernet` bus is the slave latency alone,
/// with no byte-serialization term).
#[derive(Clone, Copy, Debug, Default)]
pub struct ModbusTcpPlanner;

impl Planner for ModbusTcpPlanner {
    const DESCRIPTOR: &'static Descriptor = &DESCRIPTOR_TCP;

    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>> {
        let mut errors = Vec::new();
        let mut quirks = BTreeMap::new();
        for d in req.devices {
            if let Err(m) = unit_id(d.config) {
                errors.push(Diagnostic::general(J0852, m));
            }
            match Quirks::parse(d.quirks) {
                Ok(q) => {
                    quirks.insert(d.index, q);
                }
                Err(m) => errors.push(Diagnostic::general(J0818, m)),
            }
        }
        let mut groups: BTreeMap<GroupKey, Vec<(u16, u16, PointId)>> = BTreeMap::new();
        for p in req.points {
            let a = match check_point(p) {
                Ok(a) => a,
                Err(d) => {
                    errors.push(d);
                    continue;
                }
            };
            let key = match (p.direction, p.poll) {
                (Direction::Output, _) => continue,
                (_, Poll::Period(ns)) => (p.device, a.fc, ns, false),
                (_, Poll::OnDemand) => (p.device, a.fc, 0, true),
                (_, Poll::Push) => {
                    errors.push(Diagnostic::at(J0809, p.id, "modbus points are polled"));
                    continue;
                }
            };
            groups.entry(key).or_default().push((a.at, span(p), p.id));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut plan = Plan::new();
        for ((device, fc, period, on_demand), mut points) in groups {
            let q = quirks.get(&device).copied().unwrap_or_default();
            let max = if matches!(fc, 1 | 2) {
                q.max_bits
            } else {
                q.max_regs
            };
            points.sort();
            let mut current: Option<(u16, u32, Vec<PointId>)> = None;
            let flush = |c: (u16, u32, Vec<PointId>), plan: &mut Plan| {
                let count = (c.1 - u32::from(c.0)) as u16;
                let r = Request {
                    fc,
                    start: c.0,
                    count,
                };
                let kind = if on_demand {
                    BatchKind::OnDemand
                } else {
                    BatchKind::Poll
                };
                plan.push(
                    device,
                    kind,
                    period,
                    &c.2,
                    q.latency_ns,
                    q.latency_ns,
                    &r.encode(),
                );
            };
            for (at, n, id) in points {
                let end = u32::from(at) + u32::from(n);
                current = match current.take() {
                    Some((start, cur_end, mut ids))
                        if u32::from(at) <= cur_end + u32::from(q.max_gap)
                            && end.max(cur_end) - u32::from(start) <= u32::from(max) =>
                    {
                        ids.push(id);
                        Some((start, end.max(cur_end), ids))
                    }
                    Some(c) => {
                        flush(c, &mut plan);
                        Some((at, end, std::vec![id]))
                    }
                    None => Some((at, end, std::vec![id])),
                };
            }
            if let Some(c) = current {
                flush(c, &mut plan);
            }
        }
        Ok(plan)
    }
}
