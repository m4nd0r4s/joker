//! The schema of the manifest (ref/01 §4–§6) as data: every section, key,
//! type, phase and target. It drives the generic validation of the sections
//! without a typed decoder, the reservation of the 1.1 keys (`J0290`), the
//! target rules of a Joker Micro manifest (`J0145`, `J0720`), and the
//! generated JSON Schema (docs/16 §10).

use std::fmt::Write as _;

use joker_units::{Duration, Literal};

use crate::diag::{Diagnostic, Diagnostics};
use crate::grammar;
use crate::value::{Entry, Node, Table, Value};

/// The version a key appears in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Part of 1.0.
    V1_0,
    /// Reserved for 1.1: in the schema, refused by a 1.0 runtime (`J0290`).
    V1_1,
}

/// Where a key applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Linux nodes and Joker Micro.
    Any,
    /// Linux nodes only: a key (`J0145` on a Joker Micro).
    Linux,
    /// Linux nodes only: a section of ref/01 §5.22 (`J0720` on a Joker Micro).
    LinuxSection,
    /// Joker Micro only (`J0145` on a Linux node).
    Micro,
}

/// How the keys of a free table are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// `Nome` (docs/16 P5).
    Name,
    /// A driver name (`[a-z][a-z0-9-]*`).
    Driver,
    /// A path or a selector.
    PathOrSelector,
    /// A path.
    Path,
    /// Any string.
    Text,
}

/// The type of a value (ref/01 §2.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty {
    /// `Text`.
    Text,
    /// `Bool`.
    Bool,
    /// `Integer` within a range.
    Int(i64, i64),
    /// `Number`.
    Number,
    /// `Name`.
    Name,
    /// `DnsName`.
    DnsName,
    /// `Duration`.
    Duration,
    /// `Quantity`.
    Quantity,
    /// `Rate`.
    Rate,
    /// `Budget`.
    Budget,
    /// `Size`.
    Size,
    /// `DataVolume`.
    Volume,
    /// `Percentage`.
    Percent,
    /// `Value<S>`, typed against the signal.
    Value,
    /// `Expr<T>`, compiled by `joker-expr`.
    Expr,
    /// `Path`.
    Path,
    /// `Selector`.
    Selector,
    /// `Path` or `Selector`.
    PathOrSelector,
    /// `File`.
    File,
    /// `Ref` (`oci://`, `app:`, `class:`, …).
    Ref,
    /// `Level`.
    Level,
    /// A `"secret:<name>"` reference (ref/01 §3.10).
    Secret,
    /// One of the strings.
    Choice(&'static [&'static str]),
    /// An ordered list.
    List(&'static Ty),
    /// A set (order does not count, duplicates merge).
    Set(&'static Ty),
    /// A table with fixed keys.
    Table(&'static [Key]),
    /// A table with fixed keys that also accepts other keys (driver keys).
    Open(&'static [Key]),
    /// A free table.
    Map(KeyKind, &'static Ty),
    /// One of several forms.
    OneOf(&'static [Ty]),
    /// Any value.
    Any,
}

/// A key of a table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    /// The key.
    pub name: &'static str,
    /// Its type.
    pub ty: Ty,
    /// Whether the effective manifest must have it.
    pub required: bool,
    /// The version it appears in.
    pub phase: Phase,
    /// Where it applies.
    pub target: Target,
}

const fn k(name: &'static str, ty: Ty) -> Key {
    Key {
        name,
        ty,
        required: false,
        phase: Phase::V1_0,
        target: Target::Any,
    }
}

const fn req(name: &'static str, ty: Ty) -> Key {
    Key {
        required: true,
        ..k(name, ty)
    }
}

const fn v11(name: &'static str, ty: Ty) -> Key {
    Key {
        phase: Phase::V1_1,
        ..k(name, ty)
    }
}

const fn micro(name: &'static str, ty: Ty) -> Key {
    Key {
        target: Target::Micro,
        ..k(name, ty)
    }
}

const fn linux(name: &'static str, ty: Ty) -> Key {
    Key {
        target: Target::Linux,
        ..k(name, ty)
    }
}

const fn section(name: &'static str, ty: Ty) -> Key {
    Key {
        target: Target::LinuxSection,
        ..k(name, ty)
    }
}

const TEXTS: Ty = Ty::List(&Ty::Text);
const PATHS: Ty = Ty::List(&Ty::Path);
const SELECTORS: Ty = Ty::Set(&Ty::PathOrSelector);
const FILES: Ty = Ty::List(&Ty::File);

/// `[project]` (§5.2).
pub const PROJECT: &[Key] = &[
    req("name", Ty::DnsName),
    k("joker", Ty::Text),
    k("mode", Ty::Choice(&["dev", "prod"])),
];

/// The capacity names of a Joker Micro build (ref/08 §2.13.3).
pub const CAPACITY: &[Key] = &[
    k("signals", Ty::Int(0, u32::MAX as i64)),
    k("paths", Ty::Int(0, u32::MAX as i64)),
    k("outputs", Ty::Int(0, u32::MAX as i64)),
    k("groups", Ty::Int(0, u32::MAX as i64)),
    k("tasks", Ty::Int(0, u32::MAX as i64)),
    k("writers", Ty::Int(0, u32::MAX as i64)),
    k("output_writers", Ty::Int(0, u32::MAX as i64)),
    k("atomic", Ty::Int(0, u32::MAX as i64)),
    k("programs", Ty::Int(0, u32::MAX as i64)),
    k("code_bytes", Ty::Size),
    k("consts", Ty::Int(0, u32::MAX as i64)),
    k("expr_state_bytes", Ty::Size),
    k("alarms", Ty::Int(0, u32::MAX as i64)),
    k("classes", Ty::Int(0, u32::MAX as i64)),
    k("members", Ty::Int(0, u32::MAX as i64)),
    k("instances", Ty::Int(0, u32::MAX as i64)),
    k("methods", Ty::Int(0, u32::MAX as i64)),
    k("actions", Ty::Int(0, u32::MAX as i64)),
    k("events", Ty::Int(0, u32::MAX as i64)),
    k("apps", Ty::Int(0, u32::MAX as i64)),
    k("app_ports", Ty::Int(0, u32::MAX as i64)),
    k("params", Ty::Int(0, u32::MAX as i64)),
    k("devices", Ty::Int(0, u32::MAX as i64)),
    k("links", Ty::Int(0, u32::MAX as i64)),
    k("strings_bytes", Ty::Size),
    k("blobs_bytes", Ty::Size),
    k("enums", Ty::Int(0, u32::MAX as i64)),
    k("variants", Ty::Int(0, u32::MAX as i64)),
    k("u32pool", Ty::Int(0, u32::MAX as i64)),
    k("image_entries", Ty::Int(0, u32::MAX as i64)),
    k("text_signals", Ty::Int(0, u32::MAX as i64)),
    k("inbox", Ty::Int(0, u32::MAX as i64)),
    k("method_queue", Ty::Int(0, u32::MAX as i64)),
    k("action_pending", Ty::Int(0, u32::MAX as i64)),
    k("event_queue", Ty::Int(0, u32::MAX as i64)),
    k("trace_bytes", Ty::Size),
    k("kv_bytes", Ty::Size),
    k("window_samples", Ty::Int(0, u32::MAX as i64)),
    k("manual_writes", Ty::Int(0, u32::MAX as i64)),
    k("app_state_bytes", Ty::Size),
    k("radio_heap_bytes", Ty::Size),
];

/// `[target]` (§5.3, §5.22).
pub const TARGET: &[Key] = &[
    k("board", Ty::Text),
    k("arch", Ty::Text),
    k("kernel", Ty::Choice(&["preempt_rt", "standard"])),
    micro("accept_unsafe_reset", Ty::Text),
    micro("capacity", Ty::Table(CAPACITY)),
    micro("extern", Ty::Table(&[k("expose", SELECTORS)])),
    micro(
        "micro",
        Ty::Table(&[
            k("service_deadline", Ty::Duration),
            k("freeze_on_trip", Ty::Bool),
            k("samples_ring", Ty::Int(1, u32::MAX as i64)),
            k("event_queue", Ty::Int(1, u32::MAX as i64)),
            k("confirm", Ty::Duration),
        ]),
    ),
];

const RESOURCE: Ty = Ty::Table(&[
    k("cpus", Ty::Set(&Ty::Int(0, 4095))),
    k("memory", Ty::Size),
    v11("accel", TEXTS),
]);

/// `[resources]` (§5.4): the four fixed classes.
pub const RESOURCES: &[Key] = &[
    k("rt", RESOURCE),
    k("io", RESOURCE),
    k("ml", RESOURCE),
    k("bg", RESOURCE),
];

/// `[clock]` (§5.5).
pub const CLOCK: &[Key] = &[
    k(
        "sync",
        Ty::Choice(&["ntp", "ptp", "gnss", "link", "extern", "none"]),
    ),
    k("servers", TEXTS),
    k("iface", Ty::Text),
    k("max_holdover", Ty::Duration),
];

/// `[buses.<bus>]` (§5.6), every kind; the decoder checks which keys apply.
pub const BUS: &[Key] = &[
    req(
        "kind",
        Ty::Choice(&["ethernet", "serial", "can", "ethercat", "i2c", "spi"]),
    ),
    k("iface", Ty::Text),
    k("port", Ty::Text),
    k("baud", Ty::Int(300, 4_000_000)),
    k("parity", Ty::Choice(&["none", "even", "odd"])),
    k("stop", Ty::Int(1, 2)),
    k("data_bits", Ty::Int(7, 8)),
    k("turnaround", Ty::Duration),
    linux("dtr", Ty::Choice(&["keep", "low", "high"])),
    linux("rts", Ty::Choice(&["keep", "low", "high"])),
    k("duplex", Ty::Choice(&["full", "half"])),
    k("bitrate", Ty::Int(1, u32::MAX as i64)),
    k("fd", Ty::Bool),
    k("data_bitrate", Ty::Int(1, u32::MAX as i64)),
    k("cycle", Ty::Duration),
    k("dc", Ty::Bool),
    k("dev", Ty::File),
    k("mode", Ty::Int(0, 3)),
    k("speed", Ty::Quantity),
];

/// `[drivers.<driver>]` (§5.7).
pub const DRIVER: &[Key] = &[
    req("tier", Ty::Choice(&["w", "x"])),
    k("source", Ty::Text),
    k("digest", Ty::Text),
    k("exec", Ty::File),
    k("args", TEXTS),
    k("launch", Ty::Choice(&["systemd", "spawn", "external"])),
    k("user", Ty::Text),
    k("groups", Ty::Set(&Ty::Text)),
    k("memory", Ty::Size),
    k("cpu", Ty::Percent),
    k("cpu_per_call", Ty::Duration),
    k("plan_fuel", Ty::Int(1, 10_000_000_000)),
    k("isolation", Ty::Choice(&["process", "per_device"])),
    k("net", Ty::Set(&Ty::Text)),
    k("devices", Ty::Set(&Ty::File)),
    k("raw_net", Ty::Bool),
    k("env", Ty::Map(KeyKind::Text, &Ty::Text)),
    k(
        "restart",
        Ty::Table(&[k("min", Ty::Duration), k("max", Ty::Duration)]),
    ),
];

const RECORD: Ty = Ty::Table(&[
    k("every", Ty::Duration),
    k("on_change", Ty::Bool),
    k("keep", Ty::Map(KeyKind::Text, &Ty::Duration)),
    v11(
        "blocks",
        Ty::OneOf(&[Ty::Bool, Ty::Choice(&["on_trigger"])]),
    ),
    v11("trigger", Ty::Expr),
    v11("pre", Ty::Duration),
    v11("post", Ty::Duration),
]);

/// The adjustments of a device signal (§5.8.1); the driver's address keys
/// are accepted too.
pub const SIGNAL_TUNING: &[Key] = &[
    k("alias", Ty::OneOf(&[Ty::Path, Ty::Set(&Ty::Path)])),
    k("max_age", Ty::Duration),
    k("limits", Ty::List(&Ty::Quantity)),
    k("stuck_detect", Ty::Duration),
    k("deadband", Ty::Quantity),
    k("debounce", Ty::Duration),
    k(
        "scale",
        Ty::OneOf(&[
            Ty::Number,
            Ty::Table(&[
                req("from", Ty::List(&Ty::Quantity)),
                req("to", Ty::List(&Ty::Quantity)),
            ]),
        ]),
    ),
    k("offset", Ty::OneOf(&[Ty::Number, Ty::Quantity])),
    k("unit", Ty::Text),
    k("type", Ty::Text),
    k("direction", Ty::Choice(&["input", "output", "param"])),
    k(
        "poll",
        Ty::OneOf(&[Ty::Duration, Ty::Choice(&["on_demand"])]),
    ),
    k("record", RECORD),
    v11("kind", Ty::Choice(&["scalar", "block"])),
    v11("block", Ty::Int(1, u32::MAX as i64)),
];

/// `[devices.<d>.actions.<a>]` and `[instances.<i>.actions.<a>]` (§5.8.2).
pub const ACTION: &[Key] = &[
    k("permit", Ty::Expr),
    k("role", Ty::Choice(&["viewer", "operator", "admin"])),
    k("timeout", Ty::Duration),
    k("dedup_window", Ty::Duration),
];

/// `[devices.<device>]` (§5.8): the common keys; driver keys are open.
pub const DEVICE: &[Key] = &[
    req("driver", Ty::Text),
    k("bus", Ty::Name),
    k("profile", Ty::Text),
    k("poll", Ty::Duration),
    k("timeout", Ty::Duration),
    k("retries", Ty::Int(0, 10)),
    k("align", Ty::Name),
    k("discover", Ty::Bool),
    k(
        "fallback",
        Ty::Table(&[k("configure", Ty::Bool), k("keepalive", Ty::Duration)]),
    ),
    k("signals", Ty::Map(KeyKind::Path, &Ty::Open(SIGNAL_TUNING))),
    k("actions", Ty::Map(KeyKind::Name, &Ty::Table(ACTION))),
];

/// `[signals."<path>"]` (§5.9).
pub const SIGNAL: &[Key] = &[
    k("debounce", Ty::Duration),
    k("deadband", Ty::Quantity),
    k("max_age", Ty::Duration),
    k("limits", Ty::List(&Ty::Quantity)),
    k("stuck_detect", Ty::Duration),
    k("record", RECORD),
];

/// `[outputs."<path>"]` (§5.10).
pub const ENVELOPE: &[Key] = &[
    req("safe", Ty::Value),
    k("safer", Ty::Choice(&["lower", "higher", "equal"])),
    k("safe_ramp", Ty::Rate),
    k("interlock", Ty::Expr),
    k("permit_uncertain", Ty::Bool),
    k("on_interlock", Ty::Choice(&["safe", "hold"])),
    k("requires_good", SELECTORS),
    k("grace", Ty::Duration),
    k("range", Ty::List(&Ty::OneOf(&[Ty::Quantity, Ty::Expr]))),
    k("on_range", Ty::Choice(&["clamp", "reject", "trip"])),
    k("rate", Ty::Rate),
    k("deadband", Ty::Quantity),
    k("min_interval", Ty::Duration),
    k("max_actuations", Ty::Budget),
    k("max_lease", Ty::Duration),
    k("max_manual", Ty::Duration),
    k(
        "verify",
        Ty::Table(&[
            req("readback", Ty::Path),
            k("tolerance", Ty::Quantity),
            k("within", Ty::Duration),
        ]),
    ),
    k("reset_when", Ty::Expr),
    k("group", Ty::Name),
    k("keepalive", Ty::Duration),
    k("pulse", Ty::Duration),
    micro("remote_levels", Ty::OneOf(&[Ty::Set(&Ty::Level), Ty::Text])),
    k("accept_missing_fallback", Ty::Text),
    k(
        "widen",
        Ty::OneOf(&[Ty::Text, Ty::Map(KeyKind::Text, &Ty::Text)]),
    ),
    v11("schedule_depth", Ty::Int(0, 256)),
    v11("timed", Ty::Bool),
    v11(
        "on_late",
        Ty::OneOf(&[
            Ty::Choice(&["apply_now", "drop"]),
            Ty::Table(&[req("apply", Ty::Value)]),
        ]),
    ),
    v11("late_margin", Ty::Duration),
];

/// `[cycles.<group>]` (§5.11).
pub const CYCLE: &[Key] = &[
    req("period", Ty::Duration),
    k("resources", Ty::Choice(&["rt"])),
    k(
        "overrun",
        Ty::Choice(&["skip", "catch_up", "degrade", "trip"]),
    ),
    k("trip_after", Ty::Int(1, u32::MAX as i64)),
    k("alarm_after", Ty::Int(1, u32::MAX as i64)),
    k("recover_after", Ty::Int(1, u32::MAX as i64)),
    k("order", TEXTS),
    k(
        "trace",
        Ty::Table(&[k("continuous", Ty::Duration), k("async", Ty::Bool)]),
    ),
    k("cpu", Ty::Int(0, 4095)),
    k("priority", Ty::Int(1, 98)),
];

const BINDING: Ty = Ty::OneOf(&[
    Ty::Text,
    Ty::List(&Ty::Text),
    Ty::Table(&[
        req("signal", Ty::Path),
        k("as", Ty::Choice(&["fraction", "percent"])),
        k("optional", Ty::Bool),
    ]),
    Ty::Table(&[
        req("each", Ty::Selector),
        req("map", Ty::Map(KeyKind::Name, &Ty::Text)),
    ]),
]);

/// `[apps.<app>]` (§5.12).
pub const APP: &[Key] = &[
    k("source", Ty::Ref),
    k("path", Ty::File),
    k("digest", Ty::Text),
    k("kind", Ty::Choice(&["wasm", "native", "extern"])),
    k(
        "writer",
        Ty::Table(&[
            k("level", Ty::Level),
            k("priority", Ty::Int(1, 16)),
            k("lease", Ty::Duration),
            k("kind", Ty::Choice(&["direct", "proposal"])),
        ]),
    ),
    k("bind", Ty::Map(KeyKind::Name, &BINDING)),
    k(
        "params",
        Ty::Map(
            KeyKind::Name,
            &Ty::OneOf(&[
                Ty::Value,
                Ty::Table(&[
                    req("value", Ty::Value),
                    k("runtime", Ty::Bool),
                    k("persist", Ty::Bool),
                ]),
            ]),
        ),
    ),
    k("cycles", Ty::Map(KeyKind::Name, &Ty::Name)),
    k("period_tolerance", Ty::Percent),
    k(
        "resources",
        Ty::Table(&[
            k("cycle", Ty::Choice(&["rt"])),
            k("async", Ty::Choice(&["rt", "io", "ml", "bg"])),
            k("memory", Ty::Size),
            k("cycle_memory", Ty::Size),
            k("async_memory", Ty::Size),
        ]),
    ),
    k("net", Ty::Table(&[k("allow", Ty::Set(&Ty::Text))])),
    k("secrets", Ty::Map(KeyKind::Name, &Ty::Secret)),
    k("requires_clock", Ty::Choice(&["any", "holdover", "synced"])),
    k("keep_previous", Ty::Duration),
    k(
        "crashloop",
        Ty::Table(&[
            k("traps", Ty::Int(1, u32::MAX as i64)),
            k("window", Ty::Duration),
            k("max_backoff", Ty::Duration),
        ]),
    ),
    k(
        "shadow",
        Ty::Table(&[
            req("of", Ty::Text),
            k(
                "until",
                Ty::Table(&[k("duration", Ty::Duration), k("max_divergence", Ty::Text)]),
            ),
            k(
                "promote",
                Ty::Table(&[
                    k("outputs", Ty::OneOf(&[Ty::Selector, PATHS])),
                    k("auto", Ty::Bool),
                ]),
            ),
        ]),
    ),
    micro("writes", SELECTORS),
    micro("reads", Ty::Set(&Ty::Selector)),
    v11(
        "fallback",
        Ty::Table(&[req("controller", Ty::Ref), req("when", Ty::Text)]),
    ),
];

/// `[instances.<instance>]` (§5.13).
pub const INSTANCE: &[Key] = &[
    req("class", Ty::Text),
    k("capabilities", Ty::Set(&Ty::Name)),
    k(
        "map",
        Ty::Map(KeyKind::Name, &Ty::OneOf(&[Ty::Text, TEXTS, Ty::Open(&[])])),
    ),
    k(
        "implemented_by",
        Ty::OneOf(&[
            Ty::Text,
            Ty::Table(&[
                req("app", Ty::Name),
                k("bind", Ty::Map(KeyKind::Name, &BINDING)),
                k("params", Ty::Map(KeyKind::Name, &Ty::Value)),
            ]),
        ]),
    ),
    k("sim", Ty::OneOf(&[Ty::Bool, Ty::Open(&[])])),
    k("actions", Ty::Map(KeyKind::Name, &Ty::Table(ACTION))),
];

/// `[alarms.<alarm>]` (§5.15).
pub const ALARM: &[Key] = &[
    req("when", Ty::Expr),
    req(
        "severity",
        Ty::Choice(&["low", "medium", "high", "critical"]),
    ),
    k("delay_on", Ty::Duration),
    k("delay_off", Ty::Duration),
    k("suppress_when", Ty::Expr),
    k("message", Ty::Text),
    k("actions", Ty::Table(&[k("trip", SELECTORS)])),
    k("max_shelve", Ty::Duration),
    k("group", Ty::Name),
];

/// `[atomic.<group>]` (§5.16).
pub const ATOMIC: &[Key] = &[
    req("outputs", Ty::Set(&Ty::Path)),
    k("order", PATHS),
    k("on_rate", Ty::Choice(&["independent", "proportional"])),
    k(
        "commit",
        Ty::Table(&[req("signal", Ty::Path), req("value", Ty::Value)]),
    ),
];

const CLASS_SELECTORS: Ty = Ty::OneOf(&[Ty::Selector, Ty::List(&Ty::Selector)]);

/// `[telemetry.uplink.<uplink>]` (§5.17.3), every kind.
pub const UPLINK: &[Key] = &[
    req(
        "kind",
        Ty::Choice(&["mqtt", "mqtt-sparkplug", "zenoh", "otlp", "http"]),
    ),
    k("classes", Ty::Map(KeyKind::Text, &CLASS_SELECTORS)),
    k("budget", Ty::Volume),
    k("live", Ty::Table(&[k("every", Ty::Duration)])),
    k(
        "history",
        Ty::Table(&[
            k("delivery", Ty::Choice(&["at_least_once"])),
            k("enabled", Ty::Bool),
        ]),
    ),
    k("username", Ty::Text),
    k("password", Ty::Secret),
    k("token", Ty::Secret),
    k("broker", Ty::Text),
    k("group_id", Ty::Text),
    k("node_id", Ty::Text),
    k("topic", Ty::Text),
    k("encoding", Ty::Choice(&["json", "cbor"])),
    k("qos", Ty::Int(0, 1)),
    k("endpoints", TEXTS),
    k("key_prefix", Ty::Text),
    k("mode", Ty::Choice(&["client", "peer"])),
    k("endpoint", Ty::Text),
    k(
        "signals",
        Ty::List(&Ty::Choice(&["metrics", "logs", "traces"])),
    ),
    k("every", Ty::Duration),
    k("url", Ty::Text),
    k("events", Ty::List(&Ty::Choice(&["alarms", "events"]))),
];

/// `[telemetry]` (§5.17).
pub const TELEMETRY: &[Key] = &[
    section("record", Ty::Map(KeyKind::PathOrSelector, &RECORD)),
    section(
        "store",
        Ty::Table(&[
            k("max_disk", Ty::Size),
            k("fsync", Ty::Choice(&["batch", "always", "never"])),
            k("fsync_interval", Ty::Duration),
        ]),
    ),
    k("uplink", Ty::Map(KeyKind::Name, &Ty::Table(UPLINK))),
];

/// `[bridges.<bridge>]` (§5.18), every kind.
pub const BRIDGE: &[Key] = &[
    req(
        "kind",
        Ty::Choice(&["opcua-server", "modbus-server", "zenoh-ros2"]),
    ),
    k(
        "expose",
        Ty::OneOf(&[
            Ty::List(&Ty::PathOrSelector),
            Ty::Map(KeyKind::Path, &Ty::OneOf(&[Ty::Text, Ty::Open(&[])])),
        ]),
    ),
    k(
        "writer",
        Ty::Table(&[
            k("level", Ty::Level),
            k("priority", Ty::Int(1, 16)),
            k("lease", Ty::Duration),
            k("writable", SELECTORS),
        ]),
    ),
    k("listen", Ty::Text),
    k(
        "security",
        Ty::Table(&[
            k(
                "policy",
                Ty::Choice(&[
                    "None",
                    "Basic256Sha256",
                    "Aes128_Sha256_RsaOaep",
                    "Aes256_Sha256_RsaPss",
                ]),
            ),
            k("mode", Ty::Choice(&["None", "Sign", "SignAndEncrypt"])),
            k("trust", FILES),
        ]),
    ),
    k("bus", Ty::Name),
    k("unit_id", Ty::Int(1, 247)),
    k("profile", Ty::Text),
    k(
        "subscribe",
        Ty::Map(
            KeyKind::Text,
            &Ty::Table(&[
                req("type", Ty::Text),
                k("fields", TEXTS),
                req("to", PATHS),
                k("max_age", Ty::Duration),
            ]),
        ),
    ),
    k("endpoints", TEXTS),
];

/// `[security]` (§5.19–§5.20).
pub const SECURITY: &[Key] = &[
    k(
        "trust",
        Ty::Open(&[
            k("config", FILES),
            k("runtime", FILES),
            k("firmware", FILES),
            k("apps", FILES),
            k("drivers", FILES),
            k("profiles", FILES),
            k("boards", FILES),
            k("classes", FILES),
            k("secrets", FILES),
            k("deck", FILES),
            v11("models", FILES),
            k("revoked", Ty::Set(&Ty::Text)),
        ]),
    ),
    section("api", Ty::Table(&[k("listen", Ty::Set(&Ty::Text))])),
    section(
        "roles",
        Ty::Table(&[k("viewer", TEXTS), k("operator", TEXTS), k("admin", TEXTS)]),
    ),
];

/// `[link]` of a Joker Micro (§5.22).
pub const LINK: &[Key] = &[
    k("bus", Ty::Name),
    k("addr", Ty::Int(0, 253)),
    k("address", Ty::Text),
    k("peer", Ty::Text),
    k("auth", Ty::Choice(&["required", "none"])),
];

/// `[net]` of a Joker Micro (§5.22).
pub const NET: &[Key] = &[k(
    "wifi",
    Ty::Table(&[
        req("ssid", Ty::Text),
        k("password", Ty::Secret),
        req("country", Ty::Text),
        k("hostname", Ty::Text),
    ]),
)];

const MODEL: &[Key] = &[
    req("source", Ty::Text),
    k("digest", Ty::Text),
    req("backend", TEXTS),
    k("resources", Ty::Text),
    req("deadline", Ty::Duration),
    k("isolation", Ty::Choice(&["thread", "process"])),
    k("warmup", Ty::Int(0, u32::MAX as i64)),
    k("max_inflight", Ty::Int(1, u32::MAX as i64)),
    k(
        "batch",
        Ty::Table(&[
            k("max", Ty::Int(1, u32::MAX as i64)),
            k("wait", Ty::Duration),
        ]),
    ),
    k("fallback_after", Ty::Int(1, u32::MAX as i64)),
    k(
        "shadow",
        Ty::Table(&[
            k("candidate", Ty::Text),
            k("auto", Ty::Bool),
            req("until", Ty::Open(&[])),
            req("promote_if", TEXTS),
            k("kpi", Ty::Text),
            k("on_regress", Ty::Choice(&["demote", "keep"])),
            k("record_disagreements", Ty::Bool),
        ]),
    ),
];

const STREAM: &[Key] = &[
    req("pool", Ty::Int(1, u32::MAX as i64)),
    k("mode", Ty::Text),
    k("record", Ty::Bool),
];

const PIPELINE: &[Key] = &[
    req("from", Ty::OneOf(&[Ty::Path, PATHS])),
    req("steps", Ty::List(&Ty::Open(&[req("op", Ty::Text)]))),
    req("to", Ty::Path),
    k("deadline", Ty::Duration),
];

const TRAINING: &[Key] = &[
    req("arch", Ty::Text),
    k("from", Ty::File),
    k("hyper", Ty::Open(&[])),
    req(
        "data",
        Ty::Table(&[
            req("signals", TEXTS),
            req("window", Ty::Duration),
            k("where", Ty::Expr),
        ]),
    ),
    req("schedule", Ty::Text),
    k("resources", Ty::Text),
    k("priority", Ty::Choice(&["idle", "normal"])),
    k(
        "budget",
        Ty::Table(&[k("max_time", Ty::Duration), k("max_memory", Ty::Size)]),
    ),
    k("checkpoint", Ty::Duration),
    req("validate", Ty::Open(&[])),
    k("output", Ty::Choice(&["candidate"])),
];

/// The root of the manifest (§4), in the order of the effective manifest.
pub const ROOT: &[Key] = &[
    k("extends", FILES),
    req("project", Ty::Table(PROJECT)),
    k("target", Ty::Table(TARGET)),
    section("resources", Ty::Table(RESOURCES)),
    k("clock", Ty::Table(CLOCK)),
    k("buses", Ty::Map(KeyKind::Name, &Ty::Table(BUS))),
    section("drivers", Ty::Map(KeyKind::Driver, &Ty::Table(DRIVER))),
    k("devices", Ty::Map(KeyKind::Name, &Ty::Open(DEVICE))),
    k(
        "signals",
        Ty::Map(KeyKind::PathOrSelector, &Ty::Table(SIGNAL)),
    ),
    k(
        "outputs",
        Ty::Map(KeyKind::PathOrSelector, &Ty::Table(ENVELOPE)),
    ),
    k("cycles", Ty::Map(KeyKind::Name, &Ty::Table(CYCLE))),
    k("apps", Ty::Map(KeyKind::Name, &Ty::Table(APP))),
    k("instances", Ty::Map(KeyKind::Name, &Ty::Table(INSTANCE))),
    k(
        "calc",
        Ty::Map(
            KeyKind::Name,
            &Ty::OneOf(&[
                Ty::Expr,
                Ty::Table(&[req("expr", Ty::Expr), k("group", Ty::Name)]),
            ]),
        ),
    ),
    k("alarms", Ty::Map(KeyKind::Name, &Ty::Table(ALARM))),
    k("atomic", Ty::Map(KeyKind::Name, &Ty::Table(ATOMIC))),
    k("telemetry", Ty::Table(TELEMETRY)),
    section("bridges", Ty::Map(KeyKind::Name, &Ty::Table(BRIDGE))),
    k("security", Ty::Table(SECURITY)),
    micro("link", Ty::Table(LINK)),
    micro("net", Ty::Table(NET)),
    v11("models", Ty::Map(KeyKind::Name, &Ty::Table(MODEL))),
    v11("streams", Ty::Map(KeyKind::Path, &Ty::Table(STREAM))),
    v11("pipelines", Ty::Map(KeyKind::Name, &Ty::Table(PIPELINE))),
    v11("training", Ty::Map(KeyKind::Name, &Ty::Table(TRAINING))),
];

/// The key of the root with a name.
pub fn root_key(name: &str) -> Option<&'static Key> {
    ROOT.iter().find(|k| k.name == name)
}

/// Options of the generic validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// The manifest is for a Joker Micro.
    pub micro: bool,
    /// Check required keys (the effective manifest; not a single layer).
    pub required: bool,
    /// The runtime is 1.1 or later, so the 1.1 keys are accepted.
    pub v1_1: bool,
    /// The manifest declares `[project] joker` 1.1 or later; without it, a
    /// 1.1 key on a 1.1 runtime is `J0148`.
    pub declared_v1_1: bool,
}

/// Validates a table entry against its key: the phase and target rules,
/// then the value.
pub fn validate_entry(
    key: &Key,
    entry: &Entry,
    at: &str,
    options: Options,
    diags: &mut Diagnostics,
) {
    let mut v = Validator { options, diags };
    v.rules(key, entry, at);
    v.value(&key.ty, &entry.node, entry.key, at);
}

/// Validates a node against a type, recording into `diags`. `at` is the
/// key path, for messages.
pub fn validate(
    ty: &Ty,
    node: &Node,
    key_span: crate::source::Span,
    at: &str,
    options: Options,
    diags: &mut Diagnostics,
) {
    Validator { options, diags }.value(ty, node, key_span, at);
}

struct Validator<'a> {
    options: Options,
    diags: &'a mut Diagnostics,
}

fn type_name(ty: &Ty) -> String {
    match ty {
        Ty::Text
        | Ty::Name
        | Ty::DnsName
        | Ty::Path
        | Ty::Selector
        | Ty::PathOrSelector
        | Ty::File
        | Ty::Ref
        | Ty::Level
        | Ty::Secret
        | Ty::Expr => "a string".to_owned(),
        Ty::Duration => "a duration string".to_owned(),
        Ty::Quantity | Ty::Rate | Ty::Percent => "a quantity string".to_owned(),
        Ty::Budget => "an actuation budget".to_owned(),
        Ty::Size => "a size".to_owned(),
        Ty::Volume => "a data volume".to_owned(),
        Ty::Bool => "a boolean".to_owned(),
        Ty::Int(..) => "an integer".to_owned(),
        Ty::Number => "a number".to_owned(),
        Ty::Value | Ty::Any => "a value".to_owned(),
        Ty::Choice(options) => format!(
            "one of {}",
            options
                .iter()
                .map(|o| format!("`{o}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::List(_) | Ty::Set(_) => "a list".to_owned(),
        Ty::Table(_) | Ty::Open(_) | Ty::Map(..) => "a table".to_owned(),
        Ty::OneOf(forms) => forms.iter().map(type_name).collect::<Vec<_>>().join(" or "),
    }
}

impl Validator<'_> {
    fn push(&mut self, d: Diagnostic) {
        self.diags.push(d);
    }

    fn wrong(&mut self, ty: &Ty, node: &Node) {
        self.push(
            Diagnostic::new("J0143", "wrong value type")
                .at(node.span)
                .label(format!(
                    "expected {}, found {}",
                    type_name(ty),
                    node.value.type_name()
                )),
        );
    }

    fn accepts(&self, ty: &Ty, node: &Node) -> bool {
        let mut scratch = Diagnostics::default();
        Validator {
            options: self.options,
            diags: &mut scratch,
        }
        .value(ty, node, node.span, "");
        scratch.is_empty()
    }

    fn shape_fits(ty: &Ty, node: &Node) -> bool {
        matches!(
            (ty, &node.value),
            (Ty::Bool, Value::Bool(_))
                | (Ty::Int(..), Value::Integer(_))
                | (Ty::Number, Value::Integer(_) | Value::Float(_))
                | (Ty::List(_) | Ty::Set(_), Value::Array(_))
                | (Ty::Table(_) | Ty::Open(_) | Ty::Map(..), Value::Table(_))
                | (Ty::Value | Ty::Any, _)
        ) || (matches!(node.value, Value::String(_))
            && !matches!(
                ty,
                Ty::Bool
                    | Ty::Int(..)
                    | Ty::Number
                    | Ty::List(_)
                    | Ty::Set(_)
                    | Ty::Table(_)
                    | Ty::Open(_)
                    | Ty::Map(..)
            ))
    }

    fn value(&mut self, ty: &Ty, node: &Node, key_span: crate::source::Span, at: &str) {
        if let Some(table) = node.as_table()
            && table.len() == 1
            && (matches!(
                table.get("unset").map(|e| &e.node.value),
                Some(Value::Bool(true))
            ) || matches!(
                table.get("append").map(|e| &e.node.value),
                Some(Value::Array(_))
            ))
            && !matches!(ty, Ty::Table(keys) | Ty::Open(keys) if keys.iter().any(|k| k.name == "unset" || k.name == "append"))
        {
            if let (Some(append), Ty::List(item) | Ty::Set(item)) = (table.get("append"), ty)
                && let Value::Array(items) = &append.node.value
            {
                for element in items {
                    self.value(item, element, element.span, at);
                }
            }
            return;
        }
        let text = node.as_str();
        match ty {
            Ty::Any | Ty::Value => {}
            Ty::Text | Ty::Expr | Ty::File | Ty::Level => {
                if text.is_none() {
                    self.wrong(ty, node);
                }
            }
            Ty::Bool => {
                if !matches!(node.value, Value::Bool(_)) {
                    self.wrong(ty, node);
                }
            }
            Ty::Int(lo, hi) => match node.value {
                Value::Integer(i) if (*lo..=*hi).contains(&i) => {}
                Value::Integer(_) => self.push(
                    Diagnostic::new("J0144", "value outside the allowed range")
                        .at(node.span)
                        .label(format!("expected {lo}–{hi}")),
                ),
                _ => self.wrong(ty, node),
            },
            Ty::Number => {
                if !matches!(node.value, Value::Integer(_) | Value::Float(_)) {
                    self.wrong(ty, node);
                }
            }
            Ty::Name => match text {
                Some(t) if grammar::is_name(t) => {}
                Some(_) => self.push(
                    Diagnostic::new("J0150", "invalid name")
                        .at(node.span)
                        .label("names follow `[a-z][a-z0-9_]*`, up to 32 characters"),
                ),
                None => self.wrong(ty, node),
            },
            Ty::DnsName => match text {
                Some(t) if grammar::is_dns_name(t) => {}
                Some(_) => self.push(
                    Diagnostic::new("J0150", "invalid name")
                        .at(node.span)
                        .label("a lowercase DNS label"),
                ),
                None => self.wrong(ty, node),
            },
            Ty::Path => match text {
                Some(t) if grammar::is_path(t) => {}
                Some(_) => self.push(Diagnostic::new("J0153", "invalid path").at(node.span)),
                None => self.wrong(ty, node),
            },
            Ty::Selector | Ty::PathOrSelector => match text {
                Some(t) if grammar::is_selector(t) => {}
                Some(_) => self.push(Diagnostic::new("J0154", "invalid selector").at(node.span)),
                None => self.wrong(ty, node),
            },
            Ty::Ref => match text {
                Some(t) if t.contains(':') => {}
                Some(_) => self.push(Diagnostic::new("J0157", "invalid reference").at(node.span)),
                None => self.wrong(ty, node),
            },
            Ty::Secret => match text {
                Some(t) if t.strip_prefix("secret:").is_some_and(|n| !n.is_empty()) => {}
                Some(_) => self.push(
                    Diagnostic::new("J0294", "plain secret in a secret key")
                        .at(node.span)
                        .label("expected \"secret:<name>\""),
                ),
                None => self.wrong(ty, node),
            },
            Ty::Duration => match &node.value {
                Value::String(t) => {
                    if Duration::parse(t).is_err() {
                        self.push(Diagnostic::new("J0152", "invalid duration").at(node.span));
                    }
                }
                Value::Integer(_) | Value::Float(_) => self.push(
                    Diagnostic::new(
                        "J0151",
                        "number without a unit where a duration is required",
                    )
                    .at(node.span),
                ),
                _ => self.wrong(ty, node),
            },
            Ty::Quantity | Ty::Rate | Ty::Percent => match &node.value {
                Value::String(t) => {
                    if let Err(e) = Literal::parse(t) {
                        self.push(Diagnostic::new(e.code(), e.kind.message()).at(node.span));
                    } else if *ty == Ty::Percent && !t.trim_end().ends_with('%') {
                        self.push(
                            Diagnostic::new("J0107", "incompatible unit (dimension)")
                                .at(node.span)
                                .label("expected a percentage"),
                        );
                    }
                }
                Value::Integer(_) | Value::Float(_) => {}
                _ => self.wrong(ty, node),
            },
            Ty::Budget => match text {
                Some(t) if grammar::budget(t).is_some() => {}
                Some(_) => {
                    self.push(Diagnostic::new("J0159", "invalid actuation budget").at(node.span))
                }
                None => self.wrong(ty, node),
            },
            Ty::Size => match text {
                Some(t) if grammar::size(t).is_some() => {}
                Some(_) => self.push(
                    Diagnostic::new("J0159", "invalid size")
                        .at(node.span)
                        .label("expected a size with an IEC prefix, such as \"512 MiB\""),
                ),
                None => self.wrong(ty, node),
            },
            Ty::Volume => match text {
                Some(t) if volume(t) => {}
                Some(_) => self.push(
                    Diagnostic::new("J0159", "invalid data volume")
                        .at(node.span)
                        .label("expected a volume such as \"300 MB/month\""),
                ),
                None => self.wrong(ty, node),
            },
            Ty::Choice(options) => match text {
                Some(t) if options.contains(&t) => {}
                Some(_) => self.push(
                    Diagnostic::new("J0144", "value outside the allowed set")
                        .at(node.span)
                        .label(format!("expected {}", type_name(ty))),
                ),
                None => self.wrong(ty, node),
            },
            Ty::List(item) | Ty::Set(item) => match &node.value {
                Value::Array(items) => {
                    for (i, element) in items.iter().enumerate() {
                        self.value(item, element, element.span, &format!("{at}[{i}]"));
                    }
                }
                _ => self.wrong(ty, node),
            },
            Ty::Table(keys) | Ty::Open(keys) => match &node.value {
                Value::Table(table) => {
                    self.table(keys, matches!(ty, Ty::Open(_)), table, key_span, at)
                }
                _ => self.wrong(ty, node),
            },
            Ty::Map(kind, item) => match &node.value {
                Value::Table(table) => {
                    for (key, entry) in table {
                        if key.starts_with("x_") {
                            continue;
                        }
                        self.map_key(*kind, key, entry);
                        self.value(item, &entry.node, entry.key, &format!("{at}.{key}"));
                    }
                }
                _ => self.wrong(ty, node),
            },
            Ty::OneOf(forms) => {
                let fitting: Vec<&Ty> =
                    forms.iter().filter(|f| Self::shape_fits(f, node)).collect();
                match fitting.as_slice() {
                    [] => self.wrong(ty, node),
                    [only] => self.value(only, node, key_span, at),
                    many => {
                        if !many.iter().any(|f| self.accepts(f, node))
                            && let Some(first) = many.first()
                        {
                            self.value(first, node, key_span, at);
                        }
                    }
                }
            }
        }
    }

    fn map_key(&mut self, kind: KeyKind, key: &str, entry: &Entry) {
        let ok = match kind {
            KeyKind::Name => grammar::is_name(key),
            KeyKind::Driver => grammar::is_driver_name(key),
            KeyKind::Path => grammar::is_path(key),
            KeyKind::PathOrSelector => grammar::is_selector(key),
            KeyKind::Text => true,
        };
        if !ok {
            let code = match kind {
                KeyKind::Path => "J0153",
                KeyKind::PathOrSelector => "J0154",
                _ => "J0150",
            };
            self.push(Diagnostic::new(code, format!("invalid key `{key}`")).at(entry.key));
        }
    }

    fn table(
        &mut self,
        keys: &[Key],
        open: bool,
        table: &Table,
        key_span: crate::source::Span,
        at: &str,
    ) {
        for (name, entry) in table {
            if name.starts_with("x_") {
                continue;
            }
            match keys.iter().find(|k| k.name == name) {
                Some(key) => {
                    self.rules(key, entry, at);
                    self.value(&key.ty, &entry.node, entry.key, &format!("{at}.{name}"));
                }
                None if open => {}
                None if name.starts_with('_') => self
                    .push(Diagnostic::new("J0141", format!("reserved key `{name}`")).at(entry.key)),
                None => {
                    let mut d = Diagnostic::new(
                        "J0140",
                        format!("unknown key `{name}` in `{}`", at.trim_start_matches('.')),
                    )
                    .at(entry.key)
                    .label("unknown key");
                    if let Some(s) = grammar::suggest(name, keys.iter().map(|k| k.name)) {
                        d = d.help(format!("did you mean `{s}`?"));
                    }
                    self.push(d);
                }
            }
        }
        if self.options.required {
            for key in keys.iter().filter(|k| k.required && k.phase == Phase::V1_0) {
                if !table.contains_key(key.name) {
                    self.push(
                        Diagnostic::new(
                            "J0142",
                            format!(
                                "missing required key `{}` in `{}`",
                                key.name,
                                at.trim_start_matches('.')
                            ),
                        )
                        .at(key_span),
                    );
                }
            }
        }
    }

    fn rules(&mut self, key: &Key, entry: &Entry, at: &str) {
        if key.phase == Phase::V1_1 && !self.options.v1_1 {
            self.push(
                Diagnostic::new(
                    "J0290",
                    format!("`{}` is reserved for 1.1; this runtime is 1.0", key.name),
                )
                .at(entry.key)
                .label(format!("in `{}`", at.trim_start_matches('.'))),
            );
        } else if key.phase == Phase::V1_1 && !self.options.declared_v1_1 {
            self.push(
                Diagnostic::new("J0148", format!("`{}` was introduced in 1.1", key.name))
                    .at(entry.key)
                    .help("declare `[project] joker = \"1.1\"`"),
            );
        }
        match (key.target, self.options.micro) {
            (Target::Micro, false) => self.push(
                Diagnostic::new(
                    "J0145",
                    format!("`{}` only applies in a Joker Micro manifest", key.name),
                )
                .at(entry.key),
            ),
            (Target::Linux, true) => self.push(
                Diagnostic::new(
                    "J0145",
                    format!("`{}` does not apply on a Joker Micro", key.name),
                )
                .at(entry.key),
            ),
            (Target::LinuxSection, true) => self.push(
                Diagnostic::new(
                    "J0720",
                    format!("`{}` does not apply on a Joker Micro", key.name),
                )
                .at(entry.key),
            ),
            _ => {}
        }
    }
}

fn volume(text: &str) -> bool {
    let Some((size, per)) = text.rsplit_once('/') else {
        return false;
    };
    let split = size
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(size.len());
    let (number, unit) = size.split_at(split);
    matches!(per, "day" | "month")
        && number.parse::<f64>().is_ok_and(|n| n >= 0.0)
        && matches!(
            unit.trim_start_matches(' '),
            "kB" | "MB" | "GB" | "KiB" | "MiB" | "GiB"
        )
}

/// The JSON Schema (2020-12) of the manifest, generated from [`ROOT`].
pub fn json_schema() -> String {
    let mut out = String::new();
    out.push_str("{\n  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n  \"$id\": \"https://joker.dev/schema/joker-manifest-1.0.json\",\n  \"title\": \"joker.toml\",\n  ");
    let body = table_schema(ROOT, false, 1);
    out.push_str(body.trim_start_matches("{\n").trim_start());
    out.push('\n');
    out
}

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn string_schema(pattern: Option<&str>, format: &str) -> String {
    match pattern {
        Some(p) => format!(
            "{{ \"type\": \"string\", \"x-joker-format\": {}, \"pattern\": {} }}",
            json_string(format),
            json_string(p)
        ),
        None => format!(
            "{{ \"type\": \"string\", \"x-joker-format\": {} }}",
            json_string(format)
        ),
    }
}

fn type_schema(ty: &Ty, level: usize) -> String {
    match ty {
        Ty::Text => "{ \"type\": \"string\" }".to_owned(),
        Ty::Bool => "{ \"type\": \"boolean\" }".to_owned(),
        Ty::Int(lo, hi) => format!("{{ \"type\": \"integer\", \"minimum\": {lo}, \"maximum\": {hi} }}"),
        Ty::Number => "{ \"type\": \"number\" }".to_owned(),
        Ty::Name => string_schema(Some("^[a-z][a-z0-9_]{0,31}$"), "name"),
        Ty::DnsName => string_schema(Some("^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$"), "dns-name"),
        Ty::Duration => string_schema(Some("^[0-9]+(\\.[0-9]+)? ?(ns|us|µs|ms|s|m|min|h|d)$"), "duration"),
        Ty::Quantity => "{ \"type\": [\"string\", \"number\"], \"x-joker-format\": \"quantity\" }".to_owned(),
        Ty::Rate => "{ \"type\": [\"string\", \"number\"], \"x-joker-format\": \"rate\" }".to_owned(),
        Ty::Percent => string_schema(Some("%$"), "percent"),
        Ty::Budget => string_schema(Some("^[0-9]+/"), "budget"),
        Ty::Size => string_schema(Some("^[0-9]+(\\.[0-9]+)? ?(B|KiB|MiB|GiB|TiB)$"), "size"),
        Ty::Volume => string_schema(Some("/(day|month)$"), "data-volume"),
        Ty::Value => "{ \"type\": [\"string\", \"number\", \"integer\", \"boolean\"], \"x-joker-format\": \"value\" }".to_owned(),
        Ty::Expr => string_schema(None, "expression"),
        Ty::Path => string_schema(Some("^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$"), "path"),
        Ty::Selector | Ty::PathOrSelector => string_schema(None, "selector"),
        Ty::File => string_schema(None, "file"),
        Ty::Ref => string_schema(Some(":"), "reference"),
        Ty::Level => string_schema(Some("^(safety|manual|supervisor|control|optimizer|schedule)(:[0-9]+)?$"), "level"),
        Ty::Secret => string_schema(Some("^secret:.+$"), "secret"),
        Ty::Choice(options) => format!(
            "{{ \"enum\": [{}] }}",
            options.iter().map(|o| json_string(o)).collect::<Vec<_>>().join(", ")
        ),
        Ty::List(item) => format!("{{ \"type\": \"array\", \"items\": {} }}", type_schema(item, level)),
        Ty::Set(item) => format!("{{ \"type\": \"array\", \"uniqueItems\": true, \"items\": {} }}", type_schema(item, level)),
        Ty::Table(keys) => table_schema(keys, false, level),
        Ty::Open(keys) => table_schema(keys, true, level),
        Ty::Map(kind, item) => {
            let names = match kind {
                KeyKind::Name => ", \"propertyNames\": { \"pattern\": \"^([a-z][a-z0-9_]{0,31}|x_.*)$\" }",
                KeyKind::Driver => ", \"propertyNames\": { \"pattern\": \"^([a-z][a-z0-9-]{0,31}|x_.*)$\" }",
                _ => "",
            };
            format!(
                "{{ \"type\": \"object\"{names}, \"additionalProperties\": {} }}",
                type_schema(item, level)
            )
        }
        Ty::OneOf(forms) => format!(
            "{{ \"oneOf\": [{}] }}",
            forms.iter().map(|f| type_schema(f, level)).collect::<Vec<_>>().join(", ")
        ),
        Ty::Any => "{}".to_owned(),
    }
}

fn table_schema(keys: &[Key], open: bool, level: usize) -> String {
    let pad = indent(level + 1);
    let mut out = String::from("{\n");
    let _ = writeln!(out, "{pad}\"type\": \"object\",");
    let required: Vec<String> = keys
        .iter()
        .filter(|k| k.required && k.phase == Phase::V1_0)
        .map(|k| json_string(k.name))
        .collect();
    if !required.is_empty() {
        let _ = writeln!(out, "{pad}\"required\": [{}],", required.join(", "));
    }
    let _ = writeln!(out, "{pad}\"properties\": {{");
    for (i, key) in keys.iter().enumerate() {
        let mut schema = type_schema(&key.ty, level + 2);
        let mut extra = Vec::new();
        if key.phase == Phase::V1_1 {
            extra.push("\"x-joker-since\": \"1.1\"".to_owned());
        }
        match key.target {
            Target::Micro => extra.push("\"x-joker-target\": \"micro\"".to_owned()),
            Target::Linux | Target::LinuxSection => {
                extra.push("\"x-joker-target\": \"linux\"".to_owned())
            }
            Target::Any => {}
        }
        if !extra.is_empty() {
            schema = format!("{{ \"allOf\": [{schema}], {} }}", extra.join(", "));
        }
        let comma = if i + 1 < keys.len() { "," } else { "" };
        let _ = writeln!(
            out,
            "{}{}: {schema}{comma}",
            indent(level + 2),
            json_string(key.name)
        );
    }
    let _ = writeln!(out, "{pad}}},");
    let _ = writeln!(out, "{pad}\"patternProperties\": {{ \"^x_\": {{}} }},");
    let _ = writeln!(
        out,
        "{pad}\"additionalProperties\": {}",
        if open { "true" } else { "false" }
    );
    let _ = write!(out, "{}}}", indent(level));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_sections_are_unique() {
        let mut names: Vec<&str> = ROOT.iter().map(|k| k.name).collect();
        names.sort_unstable();
        let len = names.len();
        names.dedup();
        assert_eq!(names.len(), len);
    }

    #[test]
    fn volumes() {
        assert!(
            volume("300 MB/month")
                && volume("1GB/day")
                && !volume("300 MB")
                && !volume("3 TB/month")
        );
    }
}
