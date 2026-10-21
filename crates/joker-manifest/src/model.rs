//! The typed model of the effective manifest, M0 subset (ref/01 §5.1–§5.3,
//! §5.6, §5.8, §5.10–§5.12, docs/26 §6).
//!
//! Every value keeps the span where it is written. The v1.0 sections outside
//! the M0 subset are kept untyped in [`Manifest::pending`] until their
//! decoders exist, so a manifest that uses them is neither rejected nor
//! silently trimmed.

use std::collections::BTreeMap;

use joker_units::Duration;

use crate::source::Span;
use crate::value::Node;

/// A value and the span where it is written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spanned<T> {
    /// The value.
    pub value: T,
    /// Where it is written.
    pub span: Span,
}

impl<T> Spanned<T> {
    /// Pairs a value with its span.
    pub const fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}

/// `[project] mode` (ref/01 §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Mode {
    /// Development: `error-in-prod` codes are `W` warnings.
    Dev,
    /// Production, the default.
    #[default]
    Prod,
}

/// A `"MAJOR.MINOR"` schema and runtime version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion {
    /// The major version.
    pub major: u16,
    /// The minor version.
    pub minor: u16,
}

/// The effective manifest.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Manifest {
    /// `[project]`.
    pub project: Project,
    /// `[target]`.
    pub target: Option<Target>,
    /// `[buses.<bus>]`.
    pub buses: BTreeMap<String, Bus>,
    /// `[devices.<device>]`.
    pub devices: BTreeMap<String, Device>,
    /// `[outputs."<path>"]`, keyed by the literal key.
    pub outputs: BTreeMap<String, Envelope>,
    /// `[cycles.<group>]`.
    pub cycles: BTreeMap<String, Cycle>,
    /// `[apps.<app>]`.
    pub apps: BTreeMap<String, App>,
    /// v1.0 sections outside the M0 subset, kept as written.
    pub pending: BTreeMap<String, Node>,
    /// Root `x_*` keys.
    pub extensions: BTreeMap<String, Node>,
}

/// `[project]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Project {
    /// The site identifier, a DNS label.
    pub name: Option<Spanned<String>>,
    /// The minimum runtime and schema version.
    pub joker: Option<Spanned<SchemaVersion>>,
    /// `dev` or `prod`.
    pub mode: Mode,
}

/// `[target] kernel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kernel {
    /// A kernel with PREEMPT_RT.
    PreemptRt,
    /// Any other kernel, the conservative default.
    #[default]
    Standard,
}

/// `[target]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The board name and major, resolved against the known boards.
    pub board: Option<Spanned<BoardRef>>,
    /// The architecture.
    pub arch: Option<Spanned<String>>,
    /// The declared kernel.
    pub kernel: Kernel,
    /// Where the table is.
    pub span: Span,
}

/// `"<board>"` or `"<board>@<major>"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardRef {
    /// The board name.
    pub name: String,
    /// The major, when written.
    pub major: Option<u32>,
}

/// `[buses.<bus>] kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BusKind {
    /// An Ethernet interface.
    Ethernet,
    /// A serial port.
    Serial,
    /// A CAN interface.
    Can,
    /// An EtherCAT master.
    Ethercat,
    /// An I²C bus.
    I2c,
    /// A SPI device.
    Spi,
}

impl BusKind {
    /// The manifest spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ethernet => "ethernet",
            Self::Serial => "serial",
            Self::Can => "can",
            Self::Ethercat => "ethercat",
            Self::I2c => "i2c",
            Self::Spi => "spi",
        }
    }

    /// Every kind.
    pub const ALL: [Self; 6] = [
        Self::Ethernet,
        Self::Serial,
        Self::Can,
        Self::Ethercat,
        Self::I2c,
        Self::Spi,
    ];
}

/// `[buses.<bus>]`, with the keys of every kind; the decoder only fills the
/// keys of the declared kind (ref/01 §5.6).
#[derive(Debug, Clone, PartialEq)]
pub struct Bus {
    /// The kind.
    pub kind: Spanned<BusKind>,
    /// `iface` (ethernet, can, ethercat).
    pub iface: Option<Spanned<String>>,
    /// `port` (serial).
    pub port: Option<Spanned<String>>,
    /// `dev` (i2c, spi).
    pub dev: Option<Spanned<String>>,
    /// `baud` (serial).
    pub baud: Option<Spanned<i64>>,
    /// `parity` (serial), default `none`.
    pub parity: String,
    /// `stop` (serial), default 1.
    pub stop: i64,
    /// `data_bits` (serial), default 8.
    pub data_bits: i64,
    /// `turnaround` (serial); the default depends on `baud`.
    pub turnaround: Option<Spanned<Duration>>,
    /// `dtr` (serial), default `low`.
    pub dtr: String,
    /// `rts` (serial), default `low`.
    pub rts: String,
    /// `duplex` (serial), default `full`.
    pub duplex: String,
    /// `bitrate` (can).
    pub bitrate: Option<Spanned<i64>>,
    /// `fd` (can), default false.
    pub fd: bool,
    /// `data_bitrate` (can, with `fd`).
    pub data_bitrate: Option<Spanned<i64>>,
    /// `cycle` (ethercat).
    pub cycle: Option<Spanned<Duration>>,
    /// `dc` (ethercat), default false.
    pub dc: bool,
    /// `mode` (spi), default 0.
    pub spi_mode: i64,
    /// `speed` (spi), as written; default `"1 MHz"`.
    pub speed: Option<Spanned<String>>,
    /// Where the table is.
    pub span: Span,
}

/// `[devices.<device>]` (ref/01 §5.8).
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// `driver`.
    pub driver: Option<Spanned<String>>,
    /// `bus`.
    pub bus: Option<Spanned<String>>,
    /// `profile`, in its full form (`sunspec` expanded).
    pub profile: Option<Spanned<String>>,
    /// `poll`.
    pub poll: Option<Spanned<Duration>>,
    /// `timeout`.
    pub timeout: Option<Spanned<Duration>>,
    /// `retries`, default 1.
    pub retries: i64,
    /// `align`.
    pub align: Option<Spanned<String>>,
    /// `discover`, default false.
    pub discover: Option<Spanned<bool>>,
    /// `fallback.configure`, default false.
    pub fallback_configure: Option<Spanned<bool>>,
    /// `fallback.keepalive`.
    pub fallback_keepalive: Option<Spanned<Duration>>,
    /// `[devices.<d>.signals.<s>]`, kept as written; checked against the
    /// profile or the driver.
    pub signals: BTreeMap<String, Spanned<BTreeMap<String, Node>>>,
    /// `[devices.<d>.actions.<a>]`, kept as written.
    pub actions: BTreeMap<String, Node>,
    /// Driver-specific keys (ref/01 annex A).
    pub config: BTreeMap<String, Node>,
    /// Where the table is.
    pub span: Span,
    /// Where the device name is.
    pub key: Span,
}

/// A literal `safe` value, typed later against the output (ref/01 §3.6).
#[derive(Debug, Clone, PartialEq)]
pub enum SafeValue {
    /// `true` or `false`.
    Bool(bool),
    /// An integer.
    Integer(i64),
    /// A float.
    Float(f64),
    /// A string: a quantity, an enum variant, a text or `base64:` bytes.
    Text(String),
    /// `"relinquish"`.
    Relinquish,
}

/// `[outputs."<path>"]`, the envelope (ref/01 §5.10).
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    /// `safe`.
    pub safe: Option<Spanned<SafeValue>>,
    /// `safer`, default `equal`.
    pub safer: String,
    /// `safe_ramp`, as written.
    pub safe_ramp: Option<Spanned<String>>,
    /// `interlock`, an expression compiled by `joker-expr`.
    pub interlock: Option<Spanned<String>>,
    /// `permit_uncertain`, default false.
    pub permit_uncertain: bool,
    /// `on_interlock`, default `safe`.
    pub on_interlock: String,
    /// `requires_good`.
    pub requires_good: Vec<Spanned<String>>,
    /// `grace`, default `0s`.
    pub grace: Duration,
    /// `range`: two bounds, each a quantity or an expression.
    pub range: Option<Spanned<Vec<Spanned<String>>>>,
    /// `on_range`, default `clamp`.
    pub on_range: String,
    /// `rate`, as written.
    pub rate: Option<Spanned<String>>,
    /// `deadband`, as written.
    pub deadband: Option<Spanned<String>>,
    /// `min_interval`, default `0s`.
    pub min_interval: Duration,
    /// `max_actuations`.
    pub max_actuations: Option<Spanned<Budget>>,
    /// `max_lease`, default `15m`.
    pub max_lease: Duration,
    /// `max_manual`, default `8h`.
    pub max_manual: Duration,
    /// `verify`, kept as written.
    pub verify: Option<Node>,
    /// `reset_when`, an expression.
    pub reset_when: Option<Spanned<String>>,
    /// `group`.
    pub group: Option<Spanned<String>>,
    /// `keepalive`.
    pub keepalive: Option<Spanned<Duration>>,
    /// `pulse`.
    pub pulse: Option<Spanned<Duration>>,
    /// `accept_missing_fallback`; goes to the lock, not the effective manifest.
    pub accept_missing_fallback: Option<Spanned<String>>,
    /// Where the key is.
    pub key: Span,
    /// Where the table is.
    pub span: Span,
}

/// An actuation budget `"<n>/<period>"` (ref/01 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Actuations per window.
    pub count: u32,
    /// The sliding window.
    pub window: Duration,
}

/// `[cycles.<group>]` (ref/01 §5.11).
#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    /// `period`.
    pub period: Option<Spanned<Duration>>,
    /// `resources`.
    pub resources: Option<Spanned<String>>,
    /// `overrun`, default `skip`.
    pub overrun: String,
    /// `trip_after`.
    pub trip_after: Option<i64>,
    /// `alarm_after`, default 10.
    pub alarm_after: i64,
    /// `recover_after`, default 10.
    pub recover_after: i64,
    /// `order`.
    pub order: Vec<Spanned<String>>,
    /// `trace.continuous`.
    pub trace_continuous: Option<Spanned<Duration>>,
    /// `trace.async`, default false.
    pub trace_async: bool,
    /// `cpu`.
    pub cpu: Option<Spanned<i64>>,
    /// `priority`.
    pub priority: Option<Spanned<i64>>,
    /// Where the group name is.
    pub key: Span,
    /// Where the table is.
    pub span: Span,
}

/// An arbiter level `"<band>"` or `"<band>:<priority>"` (ref/01 §3.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    /// The band.
    pub band: Band,
    /// The priority, 1–16.
    pub priority: u8,
}

/// An arbiter band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    /// Priorities 1–2.
    Safety,
    /// Priorities 3–5, users only.
    Manual,
    /// Priorities 6–8.
    Supervisor,
    /// Priorities 9–12.
    Control,
    /// Priorities 13–15.
    Optimizer,
    /// Priority 16.
    Schedule,
}

impl Band {
    /// The manifest spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Safety => "safety",
            Self::Manual => "manual",
            Self::Supervisor => "supervisor",
            Self::Control => "control",
            Self::Optimizer => "optimizer",
            Self::Schedule => "schedule",
        }
    }

    /// The priorities of the band.
    pub const fn priorities(self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::Safety => 1..=2,
            Self::Manual => 3..=5,
            Self::Supervisor => 6..=8,
            Self::Control => 9..=12,
            Self::Optimizer => 13..=15,
            Self::Schedule => 16..=16,
        }
    }

    /// Every band, in arbiter order.
    pub const ALL: [Self; 6] = [
        Self::Safety,
        Self::Manual,
        Self::Supervisor,
        Self::Control,
        Self::Optimizer,
        Self::Schedule,
    ];
}

/// `[apps.<app>] writer`.
#[derive(Debug, Clone, PartialEq)]
pub struct Writer {
    /// The level, with the band default priority when none is written.
    pub level: Level,
    /// `lease`; the default is three periods of the owner group.
    pub lease: Option<Spanned<Duration>>,
    /// `kind = "proposal"` (M18, 1.1).
    pub proposal: bool,
    /// Where the table is.
    pub span: Span,
}

/// A `bind` value (ref/01 §5.12.1).
#[derive(Debug, Clone, PartialEq)]
pub enum Binding {
    /// A path, instance, selector or reference.
    Target(Spanned<String>),
    /// A list of paths or instances.
    List(Vec<Spanned<String>>),
    /// `{ signal, as, optional }`.
    Signal {
        /// The signal path.
        signal: Spanned<String>,
        /// `fraction` or `percent`.
        conversion: Option<Spanned<String>>,
        /// Whether a missing signal leaves the port unbound.
        optional: bool,
    },
    /// `{ each, map }`, kept as written.
    Each(Node),
}

/// `[apps.<app>]` (ref/01 §5.12).
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    /// `source`.
    pub source: Option<Spanned<String>>,
    /// `path`, relative to the project root.
    pub path: Option<Spanned<String>>,
    /// `digest`.
    pub digest: Option<Spanned<String>>,
    /// `kind`, default `wasm`.
    pub kind: String,
    /// `writer`.
    pub writer: Option<Writer>,
    /// `bind`.
    pub bind: BTreeMap<String, Spanned<Binding>>,
    /// `params`, typed against the app interface.
    pub params: BTreeMap<String, Spanned<Node>>,
    /// `cycles`: task to group.
    pub cycles: BTreeMap<String, Spanned<String>>,
    /// `period_tolerance`.
    pub period_tolerance: Option<Spanned<String>>,
    /// `resources`, kept as written.
    pub resources: Option<Node>,
    /// `net.allow`.
    pub net_allow: Vec<Spanned<String>>,
    /// `secrets`: local name to `secret:<name>`.
    pub secrets: BTreeMap<String, Spanned<String>>,
    /// `requires_clock`, default `any`.
    pub requires_clock: String,
    /// `keep_previous`, default `10m`.
    pub keep_previous: Duration,
    /// `crashloop`, kept as written.
    pub crashloop: Option<Node>,
    /// `shadow` (M16), kept as written until its decoder exists.
    pub shadow: Option<Node>,
    /// Where the app name is.
    pub key: Span,
    /// Where the table is.
    pub span: Span,
}
