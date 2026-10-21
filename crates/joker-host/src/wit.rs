//! Host-side mirrors of the `joker:runtime@0.1.0` types the cycle subset
//! passes across the boundary (`wit/joker-runtime.wit`). Every case of the
//! WIT types is mirrored, including the ones the cycle subset never sends.
//!
//! Wasmtime 49 runs the guest's post-return inside `call`, so a borrowed
//! `WasmList` result already points at freed memory when the call returns:
//! `cycle-result` is lifted into owned values, one allocation per call.

#![allow(dead_code)]

use wasmtime::component::{ComponentType, Lift, Lower};

#[derive(ComponentType, Lower, Lift, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum QualityCode {
    #[component(name = "good")]
    Good,
    #[component(name = "uncertain")]
    Uncertain,
    #[component(name = "bad")]
    Bad,
}

#[derive(ComponentType, Lower, Lift, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum QualityReason {
    #[component(name = "none")]
    None,
    #[component(name = "overridden")]
    Overridden,
    #[component(name = "local-override")]
    LocalOverride,
    #[component(name = "stale")]
    Stale,
    #[component(name = "not-confirmed")]
    NotConfirmed,
    #[component(name = "substituted")]
    Substituted,
    #[component(name = "out-of-distribution")]
    OutOfDistribution,
    #[component(name = "sensor-cal")]
    SensorCal,
    #[component(name = "clock-unsynced")]
    ClockUnsynced,
    #[component(name = "initial")]
    Initial,
    #[component(name = "frozen")]
    Frozen,
    #[component(name = "partial-write")]
    PartialWrite,
    #[component(name = "comm-failure")]
    CommFailure,
    #[component(name = "device-fault")]
    DeviceFault,
    #[component(name = "out-of-range")]
    OutOfRange,
    #[component(name = "config-error")]
    ConfigError,
    #[component(name = "not-connected")]
    NotConnected,
    #[component(name = "nan")]
    Nan,
    #[component(name = "tripped")]
    Tripped,
    #[component(name = "deadline")]
    Deadline,
}

impl QualityReason {
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Overridden,
            2 => Self::LocalOverride,
            3 => Self::Stale,
            4 => Self::NotConfirmed,
            5 => Self::Substituted,
            6 => Self::OutOfDistribution,
            7 => Self::SensorCal,
            8 => Self::ClockUnsynced,
            9 => Self::Initial,
            10 => Self::Frozen,
            11 => Self::PartialWrite,
            12 => Self::CommFailure,
            13 => Self::DeviceFault,
            14 => Self::OutOfRange,
            15 => Self::ConfigError,
            16 => Self::NotConnected,
            17 => Self::Nan,
            18 => Self::Tripped,
            19 => Self::Deadline,
            _ => Self::None,
        }
    }
}

#[derive(ComponentType, Lower, Lift, Copy, Clone, Debug, PartialEq, Eq)]
#[component(record)]
pub(crate) struct Quality {
    pub(crate) code: QualityCode,
    pub(crate) reason: QualityReason,
}

#[derive(ComponentType, Lower, Lift, Clone, Debug, PartialEq)]
#[component(variant)]
pub(crate) enum Value {
    #[component(name = "boolean")]
    Boolean(bool),
    #[component(name = "int")]
    Int(i64),
    #[component(name = "uint")]
    Uint(u64),
    #[component(name = "float")]
    Float(f64),
    #[component(name = "text")]
    Text(String),
    #[component(name = "blob")]
    Blob(Vec<u8>),
}

#[derive(ComponentType, Lower, Clone, Debug, PartialEq)]
#[component(record)]
pub(crate) struct Sample {
    pub(crate) value: Value,
    pub(crate) quality: Quality,
    #[component(name = "ts-src")]
    pub(crate) ts_src: Option<i64>,
    #[component(name = "ts-hw")]
    pub(crate) ts_hw: Option<u64>,
    #[component(name = "ts-acq")]
    pub(crate) ts_acq: u64,
    pub(crate) seq: u64,
}

#[derive(ComponentType, Lower, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum ClockQuality {
    #[component(name = "synced")]
    Synced,
    #[component(name = "holdover")]
    Holdover,
    #[component(name = "unsynced")]
    Unsynced,
}

#[derive(ComponentType, Lower, Copy, Clone, Debug, PartialEq, Eq)]
#[component(record)]
pub(crate) struct Instant {
    #[component(name = "mono-ns")]
    pub(crate) mono_ns: u64,
    #[component(name = "utc-ns")]
    pub(crate) utc_ns: i64,
    pub(crate) clock: ClockQuality,
}

#[derive(ComponentType, Lower, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum ClampKind {
    #[component(name = "range")]
    Range,
    #[component(name = "rate")]
    Rate,
}

#[derive(ComponentType, Lower, Clone, Debug, PartialEq)]
#[component(record)]
pub(crate) struct Clamp {
    pub(crate) by: ClampKind,
    pub(crate) applied: Value,
}

#[derive(ComponentType, Lower, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum RejectReason {
    #[component(name = "wrong-type")]
    WrongType,
    #[component(name = "interlock")]
    Interlock,
    #[component(name = "quality")]
    Quality,
    #[component(name = "range")]
    Range,
    #[component(name = "atomic-group")]
    AtomicGroup,
    #[component(name = "not-owner")]
    NotOwner,
    #[component(name = "clock-unsynced")]
    ClockUnsynced,
    #[component(name = "budget")]
    Budget,
}

#[derive(ComponentType, Lower, Clone, Debug, PartialEq)]
#[component(variant)]
pub(crate) enum WriteOutcome {
    #[component(name = "accepted")]
    Accepted,
    #[component(name = "clamped")]
    Clamped(Clamp),
    #[component(name = "suppressed")]
    Suppressed,
    #[component(name = "overridden")]
    Overridden(u8),
    #[component(name = "deferred")]
    Deferred,
    #[component(name = "shadowed")]
    Shadowed,
    #[component(name = "rejected")]
    Rejected(RejectReason),
    #[component(name = "tripped")]
    Tripped,
}

#[derive(ComponentType, Lower, Lift, Clone, Debug, PartialEq, Eq)]
#[component(record)]
pub(crate) struct Error {
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(ComponentType, Lift)]
#[component(record)]
pub(crate) struct CycleResult {
    pub(crate) proposals: Vec<u8>,
    pub(crate) error: Option<Error>,
}

#[derive(ComponentType, Lower, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum InstanceKind {
    #[component(name = "cycle")]
    Cycle,
    #[component(name = "task")]
    Task,
}

#[derive(ComponentType, Lower, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum PortOrigin {
    #[component(name = "declared")]
    Declared,
    #[component(name = "device-member")]
    DeviceMember,
    #[component(name = "command")]
    Command,
    #[component(name = "state")]
    State,
    #[component(name = "class-event")]
    ClassEvent,
}

#[derive(ComponentType, Lower, Clone, Debug, PartialEq, Eq)]
#[component(record)]
pub(crate) struct PortBinding {
    pub(crate) origin: PortOrigin,
    pub(crate) decl: u32,
    pub(crate) elem: u32,
    pub(crate) member: u32,
    pub(crate) name: String,
    pub(crate) bound: bool,
    pub(crate) slot: Option<u32>,
    pub(crate) outcome: Option<u32>,
}

#[derive(ComponentType, Lower, Clone, Debug, PartialEq, Eq)]
#[component(record)]
pub(crate) struct InstanceBinding {
    pub(crate) device: u32,
    pub(crate) elem: u32,
    pub(crate) name: String,
    #[component(name = "class-version")]
    pub(crate) class_version: String,
    pub(crate) capabilities: Vec<String>,
}

#[derive(ComponentType, Lower, Clone, Debug, PartialEq, Eq)]
#[component(record)]
pub(crate) struct TaskBinding {
    pub(crate) task: u32,
    pub(crate) group: String,
    #[component(name = "period-ns")]
    pub(crate) period_ns: u64,
}

#[derive(ComponentType, Lower, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum RunMode {
    #[component(name = "normal")]
    Normal,
    #[component(name = "shadow")]
    Shadow,
    #[component(name = "replay")]
    Replay,
    #[component(name = "test")]
    Test,
}

#[derive(ComponentType, Lower, Clone, Debug, PartialEq, Eq)]
#[component(record)]
pub(crate) struct InitContext {
    pub(crate) kind: InstanceKind,
    pub(crate) mode: RunMode,
    pub(crate) app: String,
    #[component(name = "app-version")]
    pub(crate) app_version: String,
    pub(crate) node: String,
    #[component(name = "served-world")]
    pub(crate) served_world: String,
    pub(crate) generation: u64,
    #[component(name = "generation-hash")]
    pub(crate) generation_hash: Vec<u8>,
    pub(crate) seed: u64,
    pub(crate) ports: Vec<PortBinding>,
    pub(crate) instances: Vec<InstanceBinding>,
    pub(crate) tasks: Vec<TaskBinding>,
    pub(crate) implements: Option<String>,
    #[component(name = "cycle-abi")]
    pub(crate) cycle_abi: u8,
    #[component(name = "image-len")]
    pub(crate) image_len: u32,
    #[component(name = "proposal-capacity")]
    pub(crate) proposal_capacity: u32,
}

#[derive(ComponentType, Lift, Copy, Clone, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum LogLevel {
    #[component(name = "trace")]
    Trace,
    #[component(name = "debug")]
    Debug,
    #[component(name = "info")]
    Info,
    #[component(name = "warn")]
    Warn,
    #[component(name = "error")]
    Error,
}
