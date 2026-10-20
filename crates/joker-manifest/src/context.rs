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
}

impl Default for Context {
    fn default() -> Self {
        Self {
            runtime: SchemaVersion { major: 1, minor: 0 },
            drivers: BTreeMap::new(),
            profiles: BTreeMap::new(),
            boards: BTreeMap::new(),
            apps: BTreeMap::new(),
        }
    }
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
