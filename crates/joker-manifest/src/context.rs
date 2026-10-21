//! What the checker needs to know beyond the manifest: the drivers of this
//! build, the resolved profiles and boards, and the interface each app
//! declares in `joker:meta` (ref/01 §8.1, step 4).
//!
//! The manifest front does not read profiles, boards or components itself:
//! the caller (`jk check`, the node) fills a [`Context`] from them, so this
//! crate stays independent of the driver and host fronts.

use std::collections::{BTreeMap, BTreeSet};

use joker_units::{Duration, Unit};

use crate::model::{BusKind, SchemaVersion};

/// The facts the semantic checks use.
#[derive(Debug, Clone)]
pub struct Context {
    /// The schema version of this runtime.
    pub runtime: SchemaVersion,
    /// The drivers of this build and those declared in `[drivers]`, by name.
    pub drivers: BTreeMap<String, DriverInfo>,
    /// The resolved profiles, by full reference (`"local/bench-esp32@1"`).
    pub profiles: BTreeMap<String, ProfileInfo>,
    /// The known boards, by name.
    pub boards: BTreeMap<String, BoardInfo>,
    /// The interfaces of the apps, by app name in the manifest.
    pub apps: BTreeMap<String, AppInterface>,
    /// The class contracts, by full reference (`"std/pump@1"`, docs/17).
    pub classes: BTreeMap<String, ClassInfo>,
    /// The measured WCET of each cycle task (`"<app>.<task>"`), from the
    /// valid `[wcet]` entries of the lock (ref/02 §2.1.13).
    pub measured: BTreeMap<String, Measured>,
    /// The read batches of each device, from the driver planners (ADR 0049).
    pub loads: BTreeMap<String, Vec<Batch>>,
    /// With `true`, a profile, board, class, driver or app that the context
    /// does not know is assumed to exist and its dependent checks are
    /// skipped: the mode for checking documentation examples.
    pub open_world: bool,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            runtime: SchemaVersion { major: 1, minor: 0 },
            drivers: BTreeMap::new(),
            profiles: BTreeMap::new(),
            boards: BTreeMap::new(),
            apps: BTreeMap::new(),
            classes: BTreeMap::new(),
            measured: BTreeMap::new(),
            loads: BTreeMap::new(),
            open_world: false,
        }
    }
}

/// The full reference of a class: `std/` is implied (ref/01 §3.10).
pub fn class_ref(text: &str) -> String {
    if text.contains('/') {
        text.to_owned()
    } else {
        format!("std/{text}")
    }
}

/// A class contract (docs/17 §6), as far as instances need it.
#[derive(Debug, Clone, Default)]
pub struct ClassInfo {
    /// The members, by name.
    pub members: BTreeMap<String, Member>,
    /// The optional capabilities.
    pub capabilities: BTreeSet<String>,
}

/// The kind of a class member (docs/17 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// `in`.
    In,
    /// `out`.
    Out,
    /// `param`.
    Param,
    /// `event`.
    Event,
    /// `action`.
    Action,
    /// `method`.
    Method,
    /// `query`.
    Query,
    /// A class member (`member boot: std/button@1`).
    Class,
}

/// A class member.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// The kind.
    pub kind: MemberKind,
    /// The value type, for `in`, `out` and `param`.
    pub ty: SignalType,
    /// The unit, for numeric members.
    pub unit: Option<Unit>,
    /// `@optional`.
    pub optional: bool,
    /// The capability that brings the member, if any.
    pub capability: Option<String>,
    /// For a class member: the class, the bounds and `@requires`.
    pub class: Option<ClassMember>,
}

impl Member {
    /// A required member of a kind and type.
    pub const fn new(kind: MemberKind, ty: SignalType) -> Self {
        Self {
            kind,
            ty,
            unit: None,
            optional: false,
            capability: None,
            class: None,
        }
    }
}

/// The class of a class member.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassMember {
    /// The full class reference.
    pub class: String,
    /// The smallest collection; 1 for a fixed member.
    pub min: u32,
    /// The largest collection; 1 for a fixed member.
    pub max: u32,
    /// Whether the member is a single instance rather than a collection.
    pub fixed: bool,
    /// `@requires`: the capabilities the bound instances must have.
    pub requires: Vec<String>,
}

/// A measured WCET (ref/02 §2.1.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    /// The 99.9th percentile.
    pub p999: Duration,
    /// The maximum observed.
    pub max: Duration,
}

/// One read batch of a device, as the driver planner gives it (ADR 0049).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Batch {
    /// Request bytes on the wire.
    pub request: u32,
    /// Response bytes on the wire.
    pub response: u32,
    /// The batch's own period, when it differs from the device `poll`.
    pub poll: Option<Duration>,
    /// The cost the plan gives (`cost-ns`), which wins over the formula.
    pub cost: Option<Duration>,
    /// CAN frames per poll.
    pub frames: u32,
}

/// The language tier of an app (ref/05 §19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tier {
    /// Rust (tier 1).
    #[default]
    Rust,
    /// TypeScript (tier 2).
    TypeScript,
    /// Python (tier 2).
    Python,
}

/// A driver descriptor, as far as the manifest is concerned (ref/06 §2.3).
#[derive(Debug, Clone, Default)]
pub struct DriverInfo {
    /// The bus kinds the driver accepts; empty when it takes no bus.
    pub buses: Vec<BusKind>,
    /// Whether a bus is required.
    pub bus_required: bool,
    /// Whether devices of this driver need a profile.
    pub profile_required: bool,
    /// The driver-specific device keys (ref/01 annex A).
    pub config: BTreeSet<String>,
    /// The address keys of signals declared in the manifest, each with the
    /// signal it implies (a `thermal` zone is a temperature input).
    pub address: BTreeMap<String, Option<SignalInfo>>,
    /// Whether the driver supports discovery.
    pub discover: bool,
    /// Whether the driver supports `safe = "relinquish"`.
    pub relinquish: bool,
}

/// A resolved profile.
#[derive(Debug, Clone, Default)]
pub struct ProfileInfo {
    /// The signals, by name.
    pub signals: BTreeMap<String, SignalInfo>,
    /// Whether the profile declares a `[fallback]`.
    pub fallback: bool,
    /// Whether `fallback.configure = true` is allowed (`configurable`).
    pub configurable: bool,
    /// The `timeout` of the profile's `[fallback]`.
    pub fallback_timeout: Option<Duration>,
    /// `quirks.latency`: the slave response latency (ref/03 §2.2.6).
    pub latency: Option<Duration>,
    /// `[defaults] poll`.
    pub poll: Option<Duration>,
    /// `[defaults] timeout`.
    pub timeout: Option<Duration>,
}

/// A known board.
#[derive(Debug, Clone, Default)]
pub struct BoardInfo {
    /// The majors available.
    pub majors: Vec<u32>,
    /// The architecture.
    pub arch: String,
    /// Whether the board is a microcontroller (`os = "none"`, ref/03 §2.3.3),
    /// so its manifest is a Joker Micro manifest (ref/01 §5.22).
    pub micro: bool,
    /// The CPU cores, when known.
    pub cpus: Option<u32>,
    /// The RAM, in bytes (`memory`, ref/03 §2.3.1).
    pub memory: Option<u64>,
    /// The runtime has no Wasm engine for the board's architecture
    /// (neither Cranelift nor Pulley, docs/01 §22).
    pub no_wasm: bool,
}

/// The type of a signal (ref/00 §6), as far as the manifest checks go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalType {
    /// `bool`.
    Bool,
    /// `i64`, `u64` and `bits<N>`.
    Integer,
    /// `f64` and `f32`.
    Float,
    /// `enum<E>`.
    Enum {
        /// The enum name.
        name: String,
        /// The variants.
        variants: Vec<String>,
    },
    /// `string`.
    Text,
    /// `bytes`.
    Bytes,
    /// Not known yet, such as a `calc` result before `joker-expr` types it.
    Unknown,
}

impl SignalType {
    /// Whether the type is a number.
    pub const fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer | Self::Float)
    }
}

/// Whether a signal is read, written or a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// An input.
    Input,
    /// An output: it needs an envelope.
    Output,
    /// A device parameter.
    Param,
}

/// One signal.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalInfo {
    /// The type.
    pub ty: SignalType,
    /// The unit, for numeric signals that have one.
    pub unit: Option<Unit>,
    /// The direction.
    pub direction: Direction,
}

impl SignalInfo {
    /// A signal of a type and direction without a unit.
    pub const fn new(ty: SignalType, direction: Direction) -> Self {
        Self {
            ty,
            unit: None,
            direction,
        }
    }

    /// The same signal with a unit.
    #[must_use]
    pub fn with_unit(mut self, unit: Unit) -> Self {
        self.unit = Some(unit);
        self
    }
}

/// The interface an app declares in `joker:meta` (ref/04).
#[derive(Debug, Clone, Default)]
pub struct AppInterface {
    /// The ports, by name.
    pub ports: BTreeMap<String, Port>,
    /// The parameters, by name.
    pub params: BTreeMap<String, Param>,
    /// The cycle tasks, by name, with the period each requests.
    pub tasks: BTreeMap<String, Option<Duration>>,
    /// The tasks marked `optional` (`overrun = "degrade"`, docs/01 §4).
    pub optional_tasks: BTreeSet<String>,
    /// The classes the app implements (`#[implements]`), by full reference.
    pub implements: Vec<String>,
    /// The language tier.
    pub tier: Tier,
    /// The WCET each cycle task declares in `joker:meta`.
    pub wcet: BTreeMap<String, Duration>,
    /// The memory suggestions of `joker:meta` (cycle, async), in bytes.
    pub memory: Option<(u64, u64)>,
}

/// The kind of a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    /// `Input<T>`.
    Input,
    /// `Output<T>`.
    Output,
    /// `Action`.
    Action,
    /// `InputMany<T>`.
    InputMany,
    /// `OutputMany<T>`.
    OutputMany,
}

/// A port.
#[derive(Debug, Clone, PartialEq)]
pub struct Port {
    /// The kind.
    pub kind: PortKind,
    /// The value type.
    pub ty: SignalType,
    /// The unit, for numeric ports.
    pub unit: Option<Unit>,
    /// Whether the port may stay unbound.
    pub optional: bool,
}

/// A parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// The value type.
    pub ty: SignalType,
    /// The unit, for numeric parameters.
    pub unit: Option<Unit>,
    /// Whether the app has a default.
    pub has_default: bool,
    /// The declared range, in `unit`.
    pub range: Option<(f64, f64)>,
}
