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
use joker_manifest::check::PathRegistry;
use joker_manifest::context::{Context, Direction as ManifestDirection, PointPoll, SignalType};
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
    /// The acquisition steps of the point (ref/03 §3, ADR 0064): the
    /// profile's `scale`, `offset`, `invert`, `enum` integers and
    /// `deadband`; none for a signal declared in the manifest.
    pub acquisition: joker_manifest::context::Acquisition,
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
    /// How often to renew the device fallback's keepalive (docs/01 §8,
    /// M1), if the profile declares one: the manifest's own
    /// `fallback.keepalive` override, or the profile's `fallback_timeout`
    /// / 5 default. `None` without a declared fallback. Applies to every
    /// output of this device for now — `applies_to`/`values` (ref/03
    /// §2.2.11, which output a fallback actually covers) is not resolved
    /// here yet.
    pub fallback_keepalive_ns: Option<u64>,
    /// The outputs the device fallback covers (`[fallback] applies_to`,
    /// ref/03 §2.2.11), as paths relative to the device; `None` covers
    /// every output. Only covered outputs are renewed with `keepalive`.
    pub fallback_outputs: Option<std::collections::BTreeSet<String>>,
    /// The device's points.
    pub points: Vec<DevicePointPlan>,
    /// The device's `[actions.<a>]`, compiled (ref/03 §2.2.12); empty
    /// without a profile or without any action declaring `steps`.
    pub actions: Vec<DeviceActionPlan>,
}

/// One compiled `[actions.<a>]` of a device.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeviceActionPlan {
    /// Action name.
    pub name: String,
    /// The action's timeout: the manifest's `[devices.<d>.actions.<a>]
    /// .timeout` when declared, else the profile's (explicit, or derived
    /// from its steps' `wait`/`until` budgets, ref/03 §2.2.12).
    pub timeout_ns: u64,
    /// `[devices.<d>.actions.<a>].dedup_window`, the manifest's own
    /// override (ref/09 §5.10); `None` falls back to the 60s default of
    /// document 16 E9.
    pub dedup_window_ns: Option<u64>,
    /// The minimum caller role to invoke this action: the manifest's
    /// own `role`, with `viewer` already floored to `operator`
    /// (`J0298`, ref/09 §5.10). Defaults to `operator` undeclared.
    pub role: String,
    /// The compiled steps, in order; empty for a native action.
    pub steps: Vec<DeviceStep>,
    /// `args`: argument name to its declared type (ref/03 §2.2.12).
    pub args: BTreeMap<String, String>,
    /// The enums of the profile, by name, for an argument whose type is
    /// one (ADR 0066 §5).
    pub enums: BTreeMap<String, BTreeMap<String, i64>>,
    /// `result`: field name to its declared type.
    pub result: BTreeMap<String, String>,
    /// `long`: progress and cancellation (M19).
    pub long: bool,
    /// `native`, as CBOR, for an action the driver runs itself
    /// (`invoke`, ref/06 §2.10); `None` for a step program.
    pub native: Option<Vec<u8>>,
    /// The `ActionSpec::id` of a native action: its position among the
    /// device's native actions.
    pub native_id: u32,
    /// `description`.
    pub description: Option<String>,
    /// The class `action` members of the device's implicit instance that
    /// run this action (`[implements] map`, ref/03 §2.2.14, ADR 0066 §17).
    pub implements: Vec<String>,
}

/// One step of a compiled device action, address-resolved into the
/// driver's own CBOR the same way a point's address is (ref/03 §2.2.12).
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceStep {
    /// Writes a raw value, or (with `value = "$name"`) an argument.
    Write {
        /// Where, in the driver's own CBOR schema.
        addr: Vec<u8>,
        /// The driver's `type` for this address.
        type_name: Option<String>,
        /// What to write.
        value: DeviceStepValue,
    },
    /// Reads and stores the result under `into`.
    Read {
        /// Where, in the driver's own CBOR schema.
        addr: Vec<u8>,
        /// The driver's `type` for this address.
        type_name: Option<String>,
        /// The result field this fills.
        into: String,
    },
    /// Waits before the next step.
    Wait(Duration),
    /// Reads until the value matches `equals`, or fails with `timeout`.
    Until {
        /// Where, in the driver's own CBOR schema.
        addr: Vec<u8>,
        /// The driver's `type` for this address.
        type_name: Option<String>,
        /// What it waits to see.
        equals: joker_hal::acquire::OwnedRaw,
        /// How long it waits.
        timeout: Duration,
    },
    /// Ends the action with `message` if the value matches `equals`.
    FailIf {
        /// Where, in the driver's own CBOR schema.
        addr: Vec<u8>,
        /// The driver's `type` for this address.
        type_name: Option<String>,
        /// What ends the action.
        equals: joker_hal::acquire::OwnedRaw,
        /// Why.
        message: String,
    },
}

/// A step's literal value, or a named argument substituted at invocation
/// (`"$name"`, ref/03 §2.2.12).
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceStepValue {
    /// A literal value.
    Literal(joker_hal::acquire::OwnedRaw),
    /// `"$name"`: the value of that argument, substituted at invocation.
    Arg(String),
}

impl DeviceStep {
    /// The address the step reaches, in the driver's CBOR; `None` for a
    /// `wait`.
    pub fn addr(&self) -> Option<&[u8]> {
        match self {
            Self::Write { addr, .. }
            | Self::Read { addr, .. }
            | Self::Until { addr, .. }
            | Self::FailIf { addr, .. } => Some(addr),
            Self::Wait(_) => None,
        }
    }

    /// The borrowed form the interpreter (`joker_hal::interpreter`) walks.
    pub fn view(&self) -> joker_hal::steps::Step<'_> {
        use joker_hal::steps::{Step, StepValue};
        match self {
            Self::Write {
                addr,
                type_name,
                value,
            } => Step::Write {
                addr: joker_hal::cbor::Cbor::new(addr),
                type_name: type_name.as_deref().unwrap_or(""),
                value: match value {
                    DeviceStepValue::Literal(v) => StepValue::Literal(v.view()),
                    DeviceStepValue::Arg(name) => StepValue::Arg(name),
                },
            },
            Self::Read {
                addr,
                type_name,
                into,
            } => Step::Read {
                addr: joker_hal::cbor::Cbor::new(addr),
                type_name: type_name.as_deref().unwrap_or(""),
                into,
            },
            Self::Wait(d) => Step::Wait(*d),
            Self::Until {
                addr,
                type_name,
                equals,
                timeout,
            } => Step::Until {
                addr: joker_hal::cbor::Cbor::new(addr),
                type_name: type_name.as_deref().unwrap_or(""),
                equals: equals.view(),
                timeout: *timeout,
            },
            Self::FailIf {
                addr,
                type_name,
                equals,
                message,
            } => Step::FailIf {
                addr: joker_hal::cbor::Cbor::new(addr),
                type_name: type_name.as_deref().unwrap_or(""),
                equals: equals.view(),
                message,
            },
        }
    }
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
/// A Modbus `bool` with a `bit` is one register (ref/06 §2.7).
fn point_shape(
    driver: &str,
    type_name: &str,
    addr_len: Option<u8>,
    has_bit: bool,
) -> Option<(RawKind, u16)> {
    match driver {
        "modbus-rtu" | "modbus-tcp" => Some(match type_name {
            "bool" if has_bit => (RawKind::Regs, 1),
            "bool" => (RawKind::Bool, 0),
            "u16" | "i16" | "bits16" => (RawKind::Regs, 1),
            "u32" | "i32" | "f32" | "bits32" => (RawKind::Regs, 2),
            "u64" | "i64" | "f64" | "bits64" => (RawKind::Regs, 4),
            "string" | "bytes" => (RawKind::Regs, u16::from(addr_len?)),
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
        "revpi" => match type_name {
            "bool" => Some((RawKind::Bool, 0)),
            "i64" => Some((RawKind::Int, 0)),
            "u64" => Some((RawKind::Uint, 0)),
            _ => None,
        },
        _ => None,
    }
}

/// The CBOR a driver's `plan()`, or the host's own `Link` construction,
/// reads from a `[buses.<b>]` declaration (ref/01 §5.6): the serial keys
/// `joker_hal::std_host::SerialConfig::from_params` reads (`port`, `baud`,
/// `parity`, `stop`, `data_bits`, `dtr`, `rts`) and the `dev` of an
/// `i2c` bus; the other bus kinds' own keys are not forwarded yet (no
/// driver or host construction asks for them).
fn bus_params_cbor(bus: &joker_manifest::model::Bus) -> Vec<u8> {
    let mut entries = Vec::new();
    if let Some(port) = &bus.port {
        entries.push(("port".to_owned(), Cbor::Text(port.value.clone())));
    }
    if let Some(dev) = &bus.dev {
        entries.push(("dev".to_owned(), Cbor::Text(dev.value.clone())));
    }
    if let Some(baud) = bus.baud {
        entries.push(("baud".to_owned(), Cbor::Int(baud.value)));
    }
    entries.push(("parity".to_owned(), Cbor::Text(bus.parity.clone())));
    entries.push(("stop".to_owned(), Cbor::Int(bus.stop)));
    entries.push(("data_bits".to_owned(), Cbor::Int(bus.data_bits)));
    entries.push(("dtr".to_owned(), Cbor::Text(bus.dtr.clone())));
    entries.push(("rts".to_owned(), Cbor::Text(bus.rts.clone())));
    Cbor::Map(entries).encode()
}

/// The `len` of a point address: bytes for `i2c`, registers for a Modbus
/// `string` or `bytes` point (ref/03 §2.2.4).
fn addr_len(addr: &[u8]) -> Option<u8> {
    let cbor = joker_hal::cbor::Cbor::new(addr);
    u8::try_from(cbor.get("len").ok()??.as_u64().ok()?).ok()
}

fn action_value_to_owned_raw(value: &Value) -> Result<joker_hal::acquire::OwnedRaw, BridgeError> {
    use joker_hal::acquire::OwnedRaw;
    match value {
        Value::Bool(b) => Ok(OwnedRaw::Bool(*b)),
        Value::Integer(i) => Ok(OwnedRaw::Int(*i)),
        Value::Float(f) => Ok(OwnedRaw::Float(*f)),
        Value::String(s) => Ok(OwnedRaw::Text(s.clone())),
        other => Err(err(format!(
            "action step: a {} is not a supported literal",
            other.type_name()
        ))),
    }
}

/// Compiles one [`joker_manifest::context::ActionStep`] into a
/// [`DeviceStep`], resolving its address into CBOR the same way a point's
/// address is (`table_to_cbor`). Scoped: a step always needs an explicit
/// `type` here (ref/03 §2.2.12's alternate `signal = "<output>"` write
/// form, which would let `write` infer its type and scale from a
/// declared signal, is not supported).
fn compile_action_step(
    device: &str,
    action: &str,
    step: &joker_manifest::context::ActionStep,
) -> Result<DeviceStep, BridgeError> {
    use joker_manifest::context::{ActionStep, ActionValue};
    Ok(match step {
        ActionStep::Write {
            addr,
            type_name,
            value,
        } => DeviceStep::Write {
            addr: table_to_cbor(addr),
            type_name: type_name.clone(),
            value: match value {
                ActionValue::Literal(v) => DeviceStepValue::Literal(action_value_to_owned_raw(v)?),
                ActionValue::Arg(name) => DeviceStepValue::Arg(name.clone()),
            },
        },
        ActionStep::Read {
            addr,
            type_name,
            into,
        } => DeviceStep::Read {
            addr: table_to_cbor(addr),
            type_name: type_name.clone(),
            into: into.clone(),
        },
        ActionStep::Wait(d) => DeviceStep::Wait(*d),
        ActionStep::Until {
            addr,
            type_name,
            equals,
            timeout,
        } => DeviceStep::Until {
            addr: table_to_cbor(addr),
            type_name: type_name.clone(),
            equals: action_value_to_owned_raw(equals)
                .map_err(|_| err(format!("{device}.{action}: unsupported `until` literal")))?,
            timeout: *timeout,
        },
        ActionStep::FailIf {
            addr,
            type_name,
            equals,
            message,
        } => DeviceStep::FailIf {
            addr: table_to_cbor(addr),
            type_name: type_name.clone(),
            equals: action_value_to_owned_raw(equals)
                .map_err(|_| err(format!("{device}.{action}: unsupported `fail_if` literal")))?,
            message: message.clone(),
        },
    })
}

/// Compiles every `[actions.<a>]` of `profile` into
/// [`DeviceActionPlan`]s: a step program, or a native action. An action
/// with neither (an empty `steps`) is skipped.
fn actions_of_profile(
    device_name: &str,
    device: &Device,
    profile: &joker_manifest::context::ProfileInfo,
) -> Result<Vec<DeviceActionPlan>, BridgeError> {
    let mut out = Vec::new();
    let mut native_id = 0u32;
    for name in profile.actions.keys() {
        let decl = profile.action_decls.get(name).cloned().unwrap_or_default();
        let steps = profile
            .action_steps
            .get(name)
            .map_or(&[][..], Vec::as_slice);
        let native = decl.native.as_ref().map(table_to_cbor);
        if steps.is_empty() && native.is_none() {
            continue;
        }
        let compiled: Vec<DeviceStep> = steps
            .iter()
            .map(|s| compile_action_step(device_name, name, s))
            .collect::<Result<_, _>>()?;
        let timeout_ns = timeout_override(device, name)
            .or_else(|| {
                profile
                    .actions
                    .get(name)
                    .copied()
                    .flatten()
                    .map(Duration::as_nanos)
            })
            .unwrap_or(1_000_000_000);
        let id = native_id;
        if native.is_some() {
            native_id += 1;
        }
        out.push(DeviceActionPlan {
            name: name.clone(),
            timeout_ns,
            dedup_window_ns: dedup_window_override(device, name),
            role: role_of(device, name),
            steps: compiled,
            args: decl.args,
            enums: profile.enums.clone(),
            result: decl.result,
            long: decl.long,
            native,
            native_id: id,
            description: decl.description,
            implements: profile
                .implemented_actions
                .iter()
                .filter(|(_, action)| *action == name)
                .map(|(member, _)| member.clone())
                .collect(),
        });
    }
    Ok(out)
}

/// `[devices.<d>.actions.<a>].timeout`: the manifest wins over the
/// profile (ref/01 §5.8.2, document 17 §4.2).
fn timeout_override(device: &Device, action: &str) -> Option<u64> {
    let text = device
        .actions
        .get(action)?
        .as_table()?
        .get("timeout")?
        .node
        .as_str()?;
    Duration::parse(text).ok().map(Duration::as_nanos)
}

/// `[devices.<d>.actions.<a>].role`, floored to `operator` (`J0298`: a
/// declared `viewer` still counts as `operator`, the API's own minimum),
/// defaulting to `operator` when undeclared.
fn role_of(device: &Device, action: &str) -> String {
    let declared = device
        .actions
        .get(action)
        .and_then(Node::as_table)
        .and_then(|t| t.get("role"))
        .and_then(|e| e.node.as_str());
    match declared {
        Some("viewer") | None => "operator".to_owned(),
        Some(other) => other.to_owned(),
    }
}

/// `[devices.<d>.actions.<a>].dedup_window`, already validated by
/// `joker-manifest` (`schema.rs`'s `Ty::Duration`) before this ever
/// runs; reparsed here with the same canonical parser the manifest
/// decoder itself uses, since `Device::actions` is kept as raw, written
/// `Node`s (not decoded into a struct — the manifest crate's own
/// comment on that field says so).
fn dedup_window_override(device: &Device, action: &str) -> Option<u64> {
    let text = device
        .actions
        .get(action)?
        .as_table()?
        .get("dedup_window")?
        .node
        .as_str()?;
    Duration::parse(text).ok().map(Duration::as_nanos)
}

/// `[devices.<d>.actions.<a>].permit`'s raw source text, already
/// type-checked as a boolean expression by `joker-manifest` (`J0304`)
/// before this ever runs — not compiled here, just carried through
/// (same raw-`Node` reasoning as [`dedup_window_override`]/[`role_of`]).
/// Compiling it against a real `SignalId` space needs a finished
/// `Plan`, which does not exist yet while this device is being built;
/// `expr_plan::bind` compiles it once the plan resolves its ids.
fn permit_source_of(device: &Device, action: &str) -> Option<String> {
    device
        .actions
        .get(action)?
        .as_table()?
        .get("permit")?
        .node
        .as_str()
        .map(str::to_owned)
}

/// Every declared `permit` source of `devices`' actions, by `(device,
/// action)` name, read from `manifest` (ref/01 §5.8.2).
pub(crate) fn permit_sources(
    manifest: &Manifest,
    devices: &[DevicePlan],
) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    for device_plan in devices {
        let Some(device) = manifest.devices.get(&device_plan.name) else {
            continue;
        };
        for action in &device_plan.actions {
            if let Some(source) = permit_source_of(device, &action.name) {
                out.insert((device_plan.name.clone(), action.name.clone()), source);
            }
        }
    }
    out
}

/// The acquisition steps of `<device>.<signal>`: `base`'s (the profile's,
/// or none for a declared signal) composed with the manifest's
/// adjustments of the signal (ADR 0075). The check already reported what
/// does not fit; an error here is the first of them.
fn adjusted(
    base: &joker_manifest::context::SignalInfo,
    from_profile: bool,
    manifest: &Manifest,
    paths: &PathRegistry,
    device: &str,
    signal: &str,
) -> Result<joker_manifest::context::Acquisition, BridgeError> {
    let tuning = joker_manifest::adjust::tuning(manifest, paths, device, signal);
    let tuning_unit = tuning
        .get("unit")
        .and_then(Node::as_str)
        .and_then(|u| joker_units::Unit::parse(u).ok());
    let base = if from_profile {
        base.clone()
    } else {
        let mut declared = base.clone();
        declared.acquisition = joker_manifest::context::Acquisition::NONE;
        declared.unit = tuning_unit;
        declared
    };
    joker_manifest::adjust::adjust(&base, from_profile, &tuning)
        .map(|a| a.info.acquisition)
        .map_err(|errors| {
            err(errors
                .first()
                .map_or_else(String::new, |e| format!("{device}.{signal}: {e:?}")))
        })
}

/// What the output stage needs to write one output point (ref/06 §2.7,
/// ADR 0075): its raw type, the composed `scale` and `offset`, `invert`,
/// the `enum` integers and the register layout. `None` for a point whose
/// raw type the core does not convert (a typed family's `string`, an
/// already typed value of another family).
#[derive(Debug, Clone, PartialEq)]
pub struct OutputConversion {
    /// `invert` of a `bool`.
    pub invert: bool,
    /// The raw integer of each `enum` variant.
    pub enum_raw: Vec<i64>,
    /// Engineering value = raw × `scale` + `offset`.
    pub scale: f64,
    /// See `scale`.
    pub offset: f64,
    /// The raw type.
    pub raw: joker_hal::encode::RawNumber,
    /// The registers of a register point.
    pub regs: Option<joker_hal::encode::RegLayout>,
}

impl OutputConversion {
    /// The codec that borrows this conversion.
    pub fn codec(&self) -> joker_hal::encode::OutputCodec<'_> {
        joker_hal::encode::OutputCodec {
            invert: self.invert,
            enum_raw: &self.enum_raw,
            scale: self.scale,
            offset: self.offset,
            raw: self.raw,
            regs: self.regs,
        }
    }
}

/// The [`OutputConversion`] of an output point; an error for a point the
/// output stage cannot write. A `bool` in a register bit stays a `bool`
/// after `invert`: the driver sets or clears the bit by read-modify-write
/// (ADR 0076).
pub fn output_conversion(point: &DevicePointPlan) -> Result<Option<OutputConversion>, String> {
    use crate::drivers::RawType;
    use joker_hal::encode::{RawNumber, RegLayout};
    let raw = match RawType::parse(&point.type_name) {
        RawType::Bool => RawNumber::Bool,
        RawType::U8 => RawNumber::Unsigned(8),
        RawType::I8 => RawNumber::Signed(8),
        RawType::U16 => RawNumber::Unsigned(16),
        RawType::I16 => RawNumber::Signed(16),
        RawType::U32 => RawNumber::Unsigned(32),
        RawType::I32 => RawNumber::Signed(32),
        RawType::U64 => RawNumber::Unsigned(64),
        RawType::I64 => RawNumber::Signed(64),
        RawType::F32 => RawNumber::F32,
        RawType::F64 => RawNumber::F64,
        RawType::Other => return Ok(None),
    };
    let regs = match point.expect {
        RawKind::Regs if raw == RawNumber::Bool => None,
        RawKind::Regs => {
            let decode = crate::drivers::Decode::of_point(&point.type_name, &point.addr);
            Some(RegLayout {
                words: u8::try_from(point.width)
                    .map_err(|_| format!("`{}`: too many registers", point.path))?,
                words_little: decode.words_little,
                bytes_little: decode.bytes_little,
            })
        }
        RawKind::Bool | RawKind::Int | RawKind::Uint | RawKind::Float => None,
        _ => return Ok(None),
    };
    let a = &point.acquisition;
    Ok(Some(OutputConversion {
        invert: a.invert,
        enum_raw: a.enum_values.clone(),
        scale: a.scale.unwrap_or(1.0),
        offset: a.offset.unwrap_or(0.0),
        raw,
        regs,
    }))
}

/// The type name of a manifest signal declared without `type`, when its
/// driver's address implies one unambiguously: a sysfs `led` is `bool` and a
/// `thermal` zone `f64` (ref/01, driver table). An integer is left out: its
/// width cannot be recovered from the checked type.
fn implied_type_name(ty: &SignalType) -> Option<&'static str> {
    match ty {
        SignalType::Bool => Some("bool"),
        SignalType::Float => Some("f64"),
        SignalType::Text => Some("string"),
        SignalType::Bytes => Some("bytes"),
        _ => None,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the resolved pieces of one point, each from a different place of the manifest or the profile"
)]
fn point_from_raw(
    driver: &str,
    device: &str,
    signal: &str,
    type_name: &str,
    direction: Direction,
    poll_ns: Option<u64>,
    raw: &BTreeMap<String, Node>,
    acquisition: joker_manifest::context::Acquisition,
) -> Result<DevicePointPlan, BridgeError> {
    let addr = if driver == "revpi" && !raw.contains_key("var") {
        joker_hal::cbor::Value::map([("var", joker_hal::cbor::Value::text(signal))]).encode()
    } else {
        table_to_cbor(raw)
    };
    let addr_len = if matches!(driver, "i2c" | "modbus-rtu" | "modbus-tcp") {
        addr_len(&addr)
    } else {
        None
    };
    let has_bit = raw.contains_key("bit");
    let (expect, width) = point_shape(driver, type_name, addr_len, has_bit).ok_or_else(|| {
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
        acquisition,
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
    paths: &PathRegistry,
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
    let fallback_keepalive_ns = profile.filter(|p| p.fallback).and_then(|p| {
        device
            .fallback_keepalive
            .map(|k| k.value.as_nanos())
            .or_else(|| p.fallback_timeout.map(|t| t.as_nanos() / 5))
    });

    let mut points = Vec::new();
    if let Some(profile) = profile {
        for (signal, info) in &profile.signals {
            let Some(type_name) = &info.type_name else {
                return Err(err(format!("{name}.{signal}: missing `type`")));
            };
            let override_poll = device
                .signals
                .get(signal)
                .and_then(|s| parse_poll(&s.value).map(Duration::as_nanos));
            let poll_ns = match (override_poll, info.acquisition.poll) {
                (Some(ns), _) => Some(ns),
                (None, Some(PointPoll::Period(d))) => Some(d.as_nanos()),
                (None, Some(PointPoll::OnDemand)) => None,
                (None, None) => device_poll_ns.or(profile_poll_ns),
            };
            let acquisition = adjusted(info, true, manifest, paths, name, signal)?;
            points.push(point_from_raw(
                &driver,
                name,
                signal,
                type_name,
                direction_of(info.direction),
                poll_ns,
                &info.raw,
                acquisition,
            )?);
        }
    } else {
        for (signal, raw) in &device.signals {
            let raw = &raw.value;
            let resolved = paths.get(&format!("{name}.{signal}"));
            let Some(type_name) = raw
                .get("type")
                .and_then(Node::as_str)
                .or_else(|| resolved.and_then(|r| implied_type_name(&r.info.ty)))
            else {
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
            let acquisition = match resolved {
                Some(r) => adjusted(&r.info, false, manifest, paths, name, signal)?,
                None => joker_manifest::context::Acquisition::NONE,
            };
            points.push(point_from_raw(
                &driver,
                name,
                signal,
                type_name,
                parse_direction(raw),
                poll_ns,
                &address_only,
                acquisition,
            )?);
        }
    }

    let actions = match profile {
        Some(profile) => actions_of_profile(name, device, profile)?,
        None => Vec::new(),
    };

    Ok(DevicePlan {
        name: name.to_owned(),
        driver,
        bus,
        bus_kind,
        bus_params,
        config,
        quirks,
        timeout_ns,
        fallback_keepalive_ns,
        fallback_outputs: profile
            .filter(|p| p.fallback)
            .and_then(|p| p.fallback_outputs.clone()),
        points,
        actions,
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

/// Builds the [`DevicePlan`] of every device in `manifest` this bridge
/// knows how to resolve. `driver = "sim"` is skipped on purpose: it never
/// goes through a real `Driver`, `devices.rs`'s own simulated path covers
/// it. A device whose driver this build cannot yet resolve into a shape
/// (a family `point_shape` has no arm for, or a bad config) is reported in
/// the second element and otherwise skipped, the same leniency
/// `catalog::profiles_from_dir` uses for a broken profile file: one
/// device's gap does not stop the rest of the generation from planning.
pub fn device_plans(
    manifest: &Manifest,
    context: &Context,
    paths: &PathRegistry,
) -> (Vec<DevicePlan>, Vec<String>) {
    let mut plans = Vec::new();
    let mut errors = Vec::new();
    for (name, device) in &manifest.devices {
        if device.driver.as_ref().is_some_and(|d| d.value == "sim") {
            continue;
        }
        match device_plan(name, device, manifest, context, paths) {
            Ok(plan) => plans.push(plan),
            Err(e) => errors.push(format!("device `{name}`: {e}")),
        }
    }
    (plans, errors)
}
