//! Turns manifest/profile TOML data (`joker_manifest::value::Node`) into
//! the CBOR a driver's `plan()` expects (ref/06 §2.4.3), and the resolved
//! per-device/per-point data into the owned plan-request types
//! (`joker_hal::spec::owned`) any driver's `Planner`/`Driver` can run
//! against. The driver never reads TOML; this is where the translation
//! happens, once, for every driver family.

use std::collections::BTreeMap;

use joker_hal::cbor::Value as Cbor;
use joker_manifest::value::{Node, Value};

/// Converts one TOML value into the deterministic CBOR a driver's address,
/// config or quirks schema expects. Datetimes have no CBOR equivalent in
/// this subset (ref/06 §2.4.3) and are kept as text, as written.
pub fn node_to_cbor(node: &Node) -> Cbor {
    match &node.value {
        Value::String(s) => Cbor::Text(s.clone()),
        Value::Integer(i) => Cbor::Int(*i),
        Value::Float(f) => Cbor::Float(*f),
        Value::Bool(b) => Cbor::Bool(*b),
        Value::Datetime(s) => Cbor::Text(s.clone()),
        Value::Array(items) => Cbor::Array(items.iter().map(node_to_cbor).collect()),
        Value::Table(table) => Cbor::Map(
            table
                .iter()
                .map(|(k, entry)| (k.clone(), node_to_cbor(&entry.node)))
                .collect(),
        ),
    }
}

/// Encodes a plain `key -> Node` map (as `Device::config` holds it) as one
/// CBOR table.
pub fn table_to_cbor(table: &BTreeMap<String, Node>) -> Vec<u8> {
    Cbor::Map(
        table
            .iter()
            .map(|(k, node)| (k.clone(), node_to_cbor(node)))
            .collect(),
    )
    .encode()
}

use joker_hal::types::{Direction, RawKind};
use joker_manifest::context::{Context, Direction as ManifestDirection};
use joker_manifest::model::{Device, Manifest};
use joker_units::Duration;

/// One point of a [`DevicePlan`]: a signal this device's driver reads or
/// writes, with the address CBOR its own `plan()` understands.
#[derive(Debug, Clone, PartialEq)]
pub struct DevicePointPlan {
    /// Canonical path (`<device>.<name>`).
    pub path: String,
    /// The address, in the driver's own CBOR schema (ref/03 §2.2.5).
    pub addr: Vec<u8>,
    /// The profile's or the manifest's raw `type` (`"u16"`, `"bool"`, …).
    pub type_name: String,
    /// Direction.
    pub direction: Direction,
    /// Poll period; `None` is on-demand.
    pub poll_ns: Option<u64>,
    /// The raw form and width the core expects (ref/06 §2.7), resolved by
    /// [`point_shape`] from `type_name` (and, for a byte-buffer family,
    /// the address itself).
    pub expect: RawKind,
    /// Width in registers, bytes, or 0 for a scalar.
    pub width: u16,
}

/// One device this generation's manifest declares, resolved into what any
/// driver family's `Planner`/`Driver` needs (ref/06 §2.4).
#[derive(Debug, Clone, PartialEq)]
pub struct DevicePlan {
    /// Device name.
    pub name: String,
    /// `driver` (the concrete variant: `modbus-rtu`, `modbus-tcp`, `gpio`, …).
    pub driver: String,
    /// `bus`, if the driver needs one.
    pub bus: Option<String>,
    /// `[buses.<bus>]`'s kind, for the host to pick a transport.
    pub bus_kind: Option<joker_manifest::model::BusKind>,
    /// `[buses.<bus>]`'s own keys a driver's `plan()` reads (e.g. modbus-rtu's
    /// `baud`), as CBOR.
    pub bus_params: Vec<u8>,
    /// `[devices.<d>]`'s own keys, as CBOR (ref/01 appendix A).
    pub config: Vec<u8>,
    /// The profile's `[profile.quirks]`, as CBOR; empty without a profile.
    pub quirks: Vec<u8>,
    /// Deadline of a call on this device.
    pub timeout_ns: u64,
    /// The device's points.
    pub points: Vec<DevicePointPlan>,
}

/// Why a device could not be planned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeError(pub String);

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for BridgeError {}

fn err(message: impl Into<String>) -> BridgeError {
    BridgeError(message.into())
}

fn direction_of(value: ManifestDirection) -> Direction {
    match value {
        ManifestDirection::Input => Direction::Input,
        ManifestDirection::Output => Direction::Output,
        ManifestDirection::Param => Direction::Param,
    }
}

fn parse_direction(raw: &BTreeMap<String, Node>) -> Direction {
    match raw.get("direction").and_then(Node::as_str) {
        Some("output") => Direction::Output,
        Some("param") => Direction::Param,
        _ => Direction::Input,
    }
}

fn parse_poll(raw: &BTreeMap<String, Node>) -> Option<Duration> {
    match raw.get("poll").and_then(Node::as_str) {
        Some("on_demand") => None,
        Some(s) => Duration::parse(s).ok(),
        None => None,
    }
}

/// The raw form and width the core expects, from a family and a `type`
/// string (ref/06 §2.7). Scoped to the scalar/byte-buffer shapes the
/// drivers built so far actually use; a new family's raw type needs an
/// arm here (same spirit as `ref/03 §2.2.5`'s per-family address tables).
fn point_shape(driver: &str, type_name: &str, addr_len: Option<u8>) -> Option<(RawKind, u16)> {
    match driver {
        "modbus-rtu" | "modbus-tcp" => Some(match type_name {
            "bool" => (RawKind::Bool, 0),
            "u16" | "i16" => (RawKind::Regs, 1),
            "u32" | "i32" | "f32" => (RawKind::Regs, 2),
            "u64" | "i64" | "f64" => (RawKind::Regs, 4),
            _ => return None,
        }),
        "sysfs" => Some(match type_name {
            "bool" => (RawKind::Bool, 0),
            "f64" | "f32" => (RawKind::Float, 0),
            _ => return None,
        }),
        "gpio" => match type_name {
            "bool" => Some((RawKind::Bool, 0)),
            _ => None,
        },
        "i2c" => Some((RawKind::Bytes, u16::from(addr_len?))),
        _ => None,
    }
}

/// The CBOR a driver's `plan()` reads from a `[buses.<b>]` declaration
/// (ref/01 §5.6). Scoped to the one key an existing driver's `plan()`
/// actually reads today: modbus-rtu's `baud`; the other bus kinds' own
/// keys are not forwarded yet (no driver asks for them).
fn bus_params_cbor(bus: &joker_manifest::model::Bus) -> Vec<u8> {
    let mut entries = Vec::new();
    if let Some(baud) = bus.baud {
        entries.push(("baud".to_owned(), Cbor::Int(baud.value)));
    }
    Cbor::Map(entries).encode()
}

fn i2c_len(addr: &[u8]) -> Option<u8> {
    let cbor = joker_hal::cbor::Cbor::new(addr);
    u8::try_from(cbor.get("len").ok()??.as_u64().ok()?).ok()
}

fn point_from_raw(
    driver: &str,
    device: &str,
    signal: &str,
    type_name: &str,
    direction: Direction,
    poll_ns: Option<u64>,
    raw: &BTreeMap<String, Node>,
) -> Result<DevicePointPlan, BridgeError> {
    let addr = table_to_cbor(raw);
    let addr_len = if driver == "i2c" {
        i2c_len(&addr)
    } else {
        None
    };
    let (expect, width) = point_shape(driver, type_name, addr_len).ok_or_else(|| {
        err(format!(
            "{device}.{signal}: type `{type_name}` not supported by `{driver}`"
        ))
    })?;
    Ok(DevicePointPlan {
        path: format!("{device}.{signal}"),
        addr,
        type_name: type_name.to_owned(),
        direction,
        poll_ns,
        expect,
        width,
    })
}

/// Builds the [`DevicePlan`] of `name`, resolving its profile (if any)
/// from `context`. Scoped: per-signal poll only comes from the manifest's
/// own override or the device's default, not from a profile's per-signal
/// poll (`ProfileInfo` does not keep one — ref/03 §2.2.4's `[defaults]`
/// resolution beyond `poll`/`timeout` is not replicated here).
pub fn device_plan(
    name: &str,
    device: &Device,
    manifest: &Manifest,
    context: &Context,
) -> Result<DevicePlan, BridgeError> {
    let driver = device
        .driver
        .as_ref()
        .map(|s| s.value.clone())
        .ok_or_else(|| err(format!("device `{name}` has no `driver`")))?;
    let bus = device.bus.as_ref().map(|s| s.value.clone());
    let bus_decl = bus.as_ref().and_then(|b| manifest.buses.get(b));
    let bus_kind = bus_decl.map(|b| b.kind.value);
    let bus_params = bus_decl.map(bus_params_cbor).unwrap_or_default();
    let config = table_to_cbor(&device.config);
    let profile = device
        .profile
        .as_ref()
        .and_then(|p| context.profiles.get(&p.value));
    let quirks = profile
        .map(|p| table_to_cbor(&p.quirks))
        .unwrap_or_default();
    let device_poll_ns = device.poll.map(|p| p.value.as_nanos());
    let profile_poll_ns = profile.and_then(|p| p.poll).map(Duration::as_nanos);
    let timeout_ns = device
        .timeout
        .map(|t| t.value.as_nanos())
        .or_else(|| profile.and_then(|p| p.timeout).map(Duration::as_nanos))
        .unwrap_or(1_000_000_000);

    let mut points = Vec::new();
    if let Some(profile) = profile {
        for (signal, info) in &profile.signals {
            let Some(type_name) = &info.type_name else {
                continue;
            };
            let override_poll = device
                .signals
                .get(signal)
                .and_then(|s| parse_poll(&s.value).map(Duration::as_nanos));
            let poll_ns = override_poll.or(device_poll_ns).or(profile_poll_ns);
            points.push(point_from_raw(
                &driver,
                name,
                signal,
                type_name,
                direction_of(info.direction),
                poll_ns,
                &info.raw,
            )?);
        }
    } else {
        for (signal, raw) in &device.signals {
            let raw = &raw.value;
            let Some(type_name) = raw.get("type").and_then(Node::as_str) else {
                return Err(err(format!("{name}.{signal}: missing `type`")));
            };
            let address_only: BTreeMap<String, Node> = raw
                .iter()
                .filter(|(k, _)| {
                    !joker_manifest::profile::GENERIC_SIGNAL_KEYS.contains(&k.as_str())
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let poll_ns = parse_poll(raw).map(Duration::as_nanos).or(device_poll_ns);
            points.push(point_from_raw(
                &driver,
                name,
                signal,
                type_name,
                parse_direction(raw),
                poll_ns,
                &address_only,
            )?);
        }
    }

    Ok(DevicePlan {
        name: name.to_owned(),
        driver,
        bus,
        bus_kind,
        bus_params,
        config,
        quirks,
        timeout_ns,
        points,
    })
}

fn bus_kind_to_hal(kind: joker_manifest::model::BusKind) -> joker_hal::spec::BusKind {
    use joker_hal::spec::BusKind as H;
    use joker_manifest::model::BusKind as M;
    match kind {
        M::Ethernet => H::Ethernet,
        M::Serial => H::Serial,
        M::Can => H::Can,
        M::Ethercat => H::Ethercat,
        M::I2c => H::I2c,
        _ => H::None,
    }
}

/// Builds the owned plan-request (`joker_hal::spec::owned`) any driver's
/// `Planner`/`Driver` reads, from a resolved [`DevicePlan`]. One device
/// per request (`index` 0), matching every driver built so far
/// (`InstanceMode::Device`).
pub fn owned_request(plan: &DevicePlan) -> joker_hal::spec::owned::OwnedRequest {
    use joker_hal::spec::owned::{OwnedBus, OwnedDevice, OwnedPoint, OwnedRequest};
    use joker_hal::types::Poll;

    let bus = plan.bus_kind.map(|kind| OwnedBus {
        name: plan.bus.clone().unwrap_or_default(),
        kind: bus_kind_to_hal(kind),
        params: plan.bus_params.clone(),
    });
    let device = OwnedDevice {
        index: 0,
        name: plan.name.clone(),
        config: plan.config.clone(),
        profile_config: Cbor::Map(Vec::new()).encode(),
        quirks: plan.quirks.clone(),
        discovered: None,
        timeout_ns: plan.timeout_ns,
    };
    let points = plan
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| OwnedPoint {
            id: i as u32,
            device: 0,
            path: p.path.clone(),
            direction: p.direction,
            expect: p.expect,
            width: p.width,
            type_name: p.type_name.clone(),
            addr: p.addr.clone(),
            poll: p.poll_ns.map_or(Poll::OnDemand, Poll::Period),
            class: joker_hal::types::ReadClass::Cycle,
            coherent: None,
            debounce_ns: 0,
            timed: false,
        })
        .collect();
    OwnedRequest {
        bus,
        devices: vec![device],
        points,
    }
}

/// Builds the [`DevicePlan`] of every device in `manifest`.
pub fn device_plans(
    manifest: &Manifest,
    context: &Context,
) -> Result<Vec<DevicePlan>, BridgeError> {
    manifest
        .devices
        .iter()
        .map(|(name, device)| device_plan(name, device, manifest, context))
        .collect()
}
