//! The io-domain half of a real driver: the async task that opens a
//! `Driver`, polls it on a real clock, and feeds `Shared`'s slots and a
//! device's `CommitRx` (docs/01 §3.4, ref/06 §2.1).
//!
//! The acquisition step of ref/06 §3.6 lives here: [`raw_to_bits`]
//! carries the already typed scalar raw kinds (`Bool`/`Int`/`Uint`/
//! `Float`) as the core's transport bits, and [`decode_to_bits`] decodes
//! `Regs` (modbus registers) and `Bytes` (i2c buffers) from the point's
//! raw `type`, `word_order`, `byte_order` and `bit` (ref/03 §3, step 1).
//! A value that cannot become the signal's type reads as
//! `bad(config_error)`. `scale`, `offset` and `invert` are not applied:
//! the manifest does not carry them to the device plan yet, so a profile
//! point that declares them is decoded unscaled.

use std::sync::Arc;

use joker_core::value::NO_TS_SRC;
use joker_core::{Quality, QualityCode, QualityReason, SignalId, ValueType};
use joker_hal::acquire::{OwnedRaw, SampleBuffer, Settled, settle};
use joker_hal::cx::Host;
use joker_hal::driver::{AckSlot, ConnectOut, Driver, RawSink, ReadBatch, WriteAcks};
use joker_hal::plan::PlanView;
use joker_hal::spec::PlanRequest;
use joker_hal::types::BatchKind;
use joker_hal::{CommitRx, Cx, RxState};

use crate::bridge::DeviceStep;
use crate::plan::Plan;
use crate::shared::Shared;

/// A request to run a device action's compiled program against the
/// device's real driver, from inside the io-domain task's own loop — the
/// only place that owns the driver instance (ref/06 §2.10, ADR 0058).
#[derive(Debug)]
pub struct ActionCall {
    /// The action's compiled steps (`jokerd::bridge::DeviceStep`).
    pub steps: Vec<DeviceStep>,
    /// Where the outcome goes; dropped silently if the caller stopped
    /// waiting.
    pub reply: tokio::sync::oneshot::Sender<ActionOutcome>,
}

/// What an action invocation produced. Distinguishes the two causes the
/// API maps to their own documented code (ref/04 §7.1): a timeout
/// (`J-E1406`) and an equipment/comm failure (`J-E1407`), from every
/// other interpreter failure (`J-E1408`, the catch-all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    /// Every step completed.
    Ok,
    /// An `until` step never saw its target before its own `timeout`
    /// (`joker_hal::interpreter::Failure::TimedOut`, ref/06 §2.10).
    TimedOut,
    /// The driver reported a communication failure
    /// (`Failure::Device`); the message is its `Debug` rendering.
    Device(String),
    /// Any other interpreter failure (`FailedCondition`/`Unsupported`);
    /// the message is `Failure`'s own `Debug` rendering.
    Failed(String),
}

/// The core's transport bits for a settled scalar raw value, or `None`
/// when `value`'s kind cannot become `ty` without scale/word decoding
/// this crate does not implement yet (see the module doc).
pub fn raw_to_bits(value: &OwnedRaw, ty: ValueType) -> Option<u64> {
    match (value, ty) {
        (OwnedRaw::Bool(b), ValueType::Bool) => Some(u64::from(*b)),
        (OwnedRaw::Int(i), ValueType::I64) => Some(*i as u64),
        (OwnedRaw::Uint(u), ValueType::U64 | ValueType::Enum | ValueType::Bits) => Some(*u),
        (OwnedRaw::Float(f), ValueType::F64) => Some(f.to_bits()),
        (OwnedRaw::Float(f), ValueType::F32) => ValueType::F32.f64_to_bits(*f),
        _ => None,
    }
}

/// A raw `type` of ref/03 §2.2.7 a register or byte buffer decodes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawType {
    /// `bool`.
    Bool,
    /// `u8`.
    U8,
    /// `i8`.
    I8,
    /// `u16`.
    U16,
    /// `i16`.
    I16,
    /// `u32`.
    U32,
    /// `i32`.
    I32,
    /// `u64`.
    U64,
    /// `i64`.
    I64,
    /// `f32`.
    F32,
    /// `f64`.
    F64,
    /// Any other `type`: the value is already typed by the driver, or does
    /// not fit a scalar slot.
    Other,
}

impl RawType {
    /// The raw type named `name` in a profile or a manifest.
    pub fn parse(name: &str) -> Self {
        match name {
            "bool" => Self::Bool,
            "u8" => Self::U8,
            "i8" => Self::I8,
            "u16" | "bits16" => Self::U16,
            "i16" => Self::I16,
            "u32" | "bits32" => Self::U32,
            "i32" => Self::I32,
            "u64" | "bits64" => Self::U64,
            "i64" => Self::I64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            _ => Self::Other,
        }
    }

    fn bytes(self) -> Option<usize> {
        match self {
            Self::Bool | Self::Other => None,
            Self::U8 | Self::I8 => Some(1),
            Self::U16 | Self::I16 => Some(2),
            Self::U32 | Self::I32 | Self::F32 => Some(4),
            Self::U64 | Self::I64 | Self::F64 => Some(8),
        }
    }
}

/// How one point's raw value is decoded (ref/06 §2.7, ref/03 §3 step 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decode {
    /// The point's raw `type`.
    pub raw: RawType,
    /// `word_order = "little"`: the first register holds the least
    /// significant word.
    pub words_little: bool,
    /// `byte_order = "little"`: the least significant byte comes first,
    /// within each register or across a byte buffer.
    pub bytes_little: bool,
    /// `bit`: the bit of a register a `bool` reads.
    pub bit: Option<u8>,
}

impl Decode {
    /// No decoding: the driver delivers an already typed value.
    pub const TYPED: Self = Self {
        raw: RawType::Other,
        words_little: false,
        bytes_little: false,
        bit: None,
    };

    /// The decoding of a point of raw `type_name` at `addr` (the address
    /// CBOR of its driver, where `word_order`, `byte_order` and `bit`
    /// live; both orders default to `big`).
    pub fn of_point(type_name: &str, addr: &[u8]) -> Self {
        let cbor = joker_hal::Cbor::new(addr);
        let little = |key: &str| {
            cbor.get(key)
                .ok()
                .flatten()
                .and_then(|v| v.as_str().ok())
                .is_some_and(|v| v == "little")
        };
        Self {
            raw: RawType::parse(type_name),
            words_little: little("word_order"),
            bytes_little: little("byte_order"),
            bit: cbor
                .get("bit")
                .ok()
                .flatten()
                .and_then(|v| v.as_u64().ok())
                .and_then(|b| u8::try_from(b).ok())
                .filter(|b| *b < 64),
        }
    }
}

enum Number {
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
}

fn number_to_bits(number: Number, ty: ValueType) -> Option<u64> {
    match (number, ty) {
        (Number::Bool(b), ValueType::Bool) => Some(u64::from(b)),
        (Number::Int(i), ValueType::I64) => Some(i as u64),
        (Number::Int(i), ValueType::U64 | ValueType::Enum | ValueType::Bits) => {
            u64::try_from(i).ok()
        }
        (Number::Uint(u), ValueType::U64 | ValueType::Enum | ValueType::Bits) => Some(u),
        (Number::Uint(u), ValueType::I64) => i64::try_from(u).ok().map(|i| i as u64),
        (Number::Int(i), ValueType::F64 | ValueType::F32) => ty.f64_to_bits(i as f64),
        (Number::Uint(u), ValueType::F64 | ValueType::F32) => ty.f64_to_bits(u as f64),
        (Number::Float(f), ValueType::F64 | ValueType::F32) => ty.f64_to_bits(f),
        _ => None,
    }
}

fn word_bits(words: &[u16], decode: &Decode) -> u64 {
    let word = |w: &u16| {
        if decode.bytes_little {
            w.swap_bytes()
        } else {
            *w
        }
    };
    let fold = |acc: u64, w: &u16| (acc << 16) | u64::from(word(w));
    if decode.words_little {
        words.iter().rev().fold(0, fold)
    } else {
        words.iter().fold(0, fold)
    }
}

fn byte_bits(bytes: &[u8], decode: &Decode) -> u64 {
    let fold = |acc: u64, b: &u8| (acc << 8) | u64::from(*b);
    if decode.bytes_little {
        bytes.iter().rev().fold(0, fold)
    } else {
        bytes.iter().fold(0, fold)
    }
}

fn typed(bits: u64, decode: &Decode) -> Option<Number> {
    Some(match decode.raw {
        RawType::Bool => Number::Bool(match decode.bit {
            Some(b) => bits >> b & 1 == 1,
            None => bits != 0,
        }),
        RawType::U8 | RawType::U16 | RawType::U32 | RawType::U64 => Number::Uint(bits),
        RawType::I8 => Number::Int(i64::from(bits as u8 as i8)),
        RawType::I16 => Number::Int(i64::from(bits as u16 as i16)),
        RawType::I32 => Number::Int(i64::from(bits as u32 as i32)),
        RawType::I64 => Number::Int(bits as i64),
        RawType::F32 => Number::Float(f64::from(f32::from_bits(bits as u32))),
        RawType::F64 => Number::Float(f64::from_bits(bits)),
        RawType::Other => return None,
    })
}

/// The core's transport bits for a settled raw value of a point decoded
/// as `decode`: register words and byte buffers are decoded first (raw
/// type, word and byte order, `bit`; ref/03 §3 step 1), already typed
/// scalars go through [`raw_to_bits`]. `None` when the value does not
/// match the point's raw type (wrong length, an unknown `type`) or cannot
/// become `ty` (a negative value for an unsigned signal, a float for an
/// integer one).
pub fn decode_to_bits(value: &OwnedRaw, decode: &Decode, ty: ValueType) -> Option<u64> {
    let bits = match value {
        OwnedRaw::Regs(words) => {
            let expected = decode.raw.bytes().map_or(1, |n| n / 2).max(1);
            if words.len() != expected {
                return None;
            }
            word_bits(words, decode)
        }
        OwnedRaw::Bytes(bytes) => {
            let fits = match decode.raw.bytes() {
                Some(n) => bytes.len() == n,
                None => decode.raw == RawType::Bool && (1..=8).contains(&bytes.len()),
            };
            if !fits {
                return None;
            }
            byte_bits(bytes, decode)
        }
        _ => return raw_to_bits(value, ty),
    };
    number_to_bits(typed(bits, decode)?, ty)
}

/// What one settled point becomes in its slot (ref/06 §2.8, docs/16 §3):
/// the value's bits and the driver's quality when the value converts to
/// the signal's type, otherwise no value at all. A `bad` sample keeps its
/// own reason; a value that does not convert, or a non-`bad` sample
/// without a value, is `bad(config_error)`. Quality never disappears: a
/// failed read always reaches the slot, it never leaves the last good
/// value in place.
pub fn settled_sample(target: &PointTarget, settled: &Settled) -> (u64, Quality) {
    let status: Quality = settled.status.into();
    if status.is_bad() {
        return (0, status);
    }
    match decode_to_bits(&settled.value, &target.decode, target.ty) {
        Some(bits) => (bits, status),
        None => (
            0,
            Quality::new(QualityCode::Bad, QualityReason::ConfigError),
        ),
    }
}

fn store_settled(shared: &Shared, target: &PointTarget, settled: &Settled, now: u64) {
    let (bits, quality) = settled_sample(target, settled);
    if let Some(slot) = shared.slots.get(target.signal.index()) {
        slot.write(bits, quality, NO_TS_SRC, settled.ts_hw.unwrap_or(now));
    }
}

/// One point's signal id and core type, for [`run_driver`] to write a
/// settled read into the right slot.
#[derive(Clone, Copy, Debug)]
pub struct PointTarget {
    /// The point's id in the request (matches `OwnedPoint::id`).
    pub point: joker_hal::types::PointId,
    /// The core signal to write (an input), or read (an output's applied
    /// value is never read here — writes reach the driver through
    /// `CommitRx`, not this list).
    pub signal: SignalId,
    /// The core type, for [`decode_to_bits`].
    pub ty: ValueType,
    /// How the point's raw value is decoded.
    pub decode: Decode,
}

/// The [`PointTarget`]s of `plan.devices[device_index]`'s read points
/// (input and param; output points never reach a `RawSink`, they only
/// reach the driver as `CommittedWrite`s).
pub fn point_targets(plan: &Plan, device_index: usize) -> Vec<PointTarget> {
    let Some(device) = plan.devices.get(device_index) else {
        return Vec::new();
    };
    device
        .points
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            if p.direction == joker_hal::Direction::Output {
                return None;
            }
            let signal = plan.id(&p.path)?;
            let ty = plan.signals.iter().find(|s| s.path == p.path)?.ty;
            Some(PointTarget {
                point: i as u32,
                signal,
                ty,
                decode: Decode::of_point(&p.type_name, &p.addr),
            })
        })
        .collect()
}

/// Runs one real device's driver on `host`'s clock until `stop` returns
/// true: connects, then loops writes-before-reads like
/// `joker_hal::testing::Bench::run_until`, but for real (`host` is a
/// `TokioHost` in production; a test can still use `VirtualHost` plus its
/// own `run`, since this is a plain `async fn`). Reads land in
/// `shared.slots` through `targets`, every settled point of every read
/// (a failed read or a value that does not convert writes a `bad` sample,
/// see [`settled_sample`]). Writes are drained from the `CommitRx` on
/// `shared.commit_bindings[device_index]`/`commit_boxes[device_index]`.
#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is a distinct, independently-sourced piece (the driver, the clock, the plan request/view, a deadline, the slot targets, the shared commit channel, which device, and the stop condition); a wrapper struct would only rename this list, not shrink it"
)]
pub async fn run_driver<D: Driver>(
    mut driver: D,
    host: &dyn Host,
    request: PlanRequest<'_>,
    plan: PlanView<'_>,
    timeout_ns: u64,
    targets: &[PointTarget],
    shared: &Arc<Shared>,
    device_index: usize,
    fallback_keepalive_ns: Option<u64>,
    mut action_rx: tokio::sync::mpsc::Receiver<ActionCall>,
    mut stop: impl FnMut() -> bool,
) {
    let deadline = host.now().mono_ns + timeout_ns;
    let mut cx = Cx::new(host, deadline);
    let mut connect_sink = ();
    let mut out = ConnectOut::new(&mut connect_sink);
    if driver.connect(&mut cx, &mut out).await.is_err() {
        return;
    }

    let bindings = &shared.commit_bindings[device_index];
    let boxes = &shared.commit_boxes[device_index];
    let mut rx_state = vec![RxState::default(); bindings.len()];
    let Ok(mut rx) = CommitRx::new(bindings, boxes, &mut rx_state) else {
        return;
    };

    // On-demand batches (`fc`/params read only when something asks) are
    // not scheduled here at all: there is no periodic "due" for them to
    // begin with, and treating them as always-due-again spun this loop
    // at the virtual clock's own resolution once already (fixed by
    // scoping the scheduler to `Poll` batches only). A real on-demand
    // read needs its own request-driven trigger, not built yet.
    let mut due: Vec<u64> = vec![host.now().mono_ns; plan.len()];
    let mut buf = SampleBuffer::with_capacity(4096);
    const IDLE_NS: u64 = 100_000_000;
    // docs/01 §8, M1: the device fallback keepalive is renewed regardless
    // of whether the applied value changed; `applies_to` (which outputs
    // it actually covers, ref/03 §2.2.11) is not resolved, so every
    // binding of this device is marked due, same as `resync`.
    let mut next_keepalive = fallback_keepalive_ns.map(|ns| host.now().mono_ns + ns);
    while !stop() {
        if let (Some(ns), Some(next)) = (fallback_keepalive_ns, next_keepalive)
            && host.now().mono_ns >= next
        {
            for i in 0..bindings.len() {
                rx.keepalive(i);
            }
            next_keepalive = Some(host.now().mono_ns + ns);
        }
        let mut writes = Vec::new();
        rx.drain_into(&mut writes);
        if !writes.is_empty() {
            let mut slots = vec![AckSlot::default(); writes.len()];
            if let Some(mut acks) = WriteAcks::new(&writes, &mut slots) {
                let deadline = host.now().mono_ns + timeout_ns;
                let mut cx = Cx::new(host, deadline);
                driver.write(&mut cx, &writes, &mut acks).await;
            }
        }

        let next = plan
            .iter()
            .filter(|b| b.head.kind == BatchKind::Poll)
            .map(|b| due[b.head.id as usize])
            .chain(next_keepalive)
            .min()
            .unwrap_or(host.now().mono_ns + IDLE_NS);
        tokio::select! {
            () = core::future::poll_fn(|task_cx| host.poll_sleep(next, task_cx)) => {}
            call = action_rx.recv() => {
                if let Some(call) = call {
                    let views: Vec<_> = call.steps.iter().map(DeviceStep::view).collect();
                    let deadline = host.now().mono_ns + timeout_ns;
                    let mut acx = Cx::new(host, deadline);
                    let result = joker_hal::interpreter::run(&mut driver, &mut acx, 0, &views).await;
                    let _ = call.reply.send(match result {
                        Ok(()) => ActionOutcome::Ok,
                        Err(joker_hal::interpreter::Failure::TimedOut) => ActionOutcome::TimedOut,
                        Err(e @ joker_hal::interpreter::Failure::Device(_)) => {
                            ActionOutcome::Device(format!("{e:?}"))
                        }
                        Err(e) => ActionOutcome::Failed(format!("{e:?}")),
                    });
                }
                continue;
            }
        }
        if host.now().mono_ns < next {
            continue;
        }
        let ids: Vec<_> = plan
            .iter()
            .filter(|b| {
                b.head.kind == BatchKind::Poll && due[b.head.id as usize] <= host.now().mono_ns
            })
            .map(|b| b.head.id)
            .collect();
        for id in ids {
            let Some(spec) = plan.batch(id) else { continue };
            let deadline = host.now().mono_ns + timeout_ns;
            let batch = ReadBatch::new(&spec, deadline);
            let mut cx = Cx::new(host, deadline);
            buf.clear();
            let mut sink = RawSink::new(&mut buf);
            let result = driver.read(&mut cx, &batch, &mut sink).await;
            let settled = settle(&request, &batch, buf.samples(), result);
            let now = host.now().mono_ns;
            for s in &settled {
                if let Some(target) = targets.iter().find(|t| t.point == s.point) {
                    store_settled(shared, target, s, now);
                }
            }
            // One step, not a catch-up loop: an on-demand batch's period
            // is 0, and a `while *d <= now { *d += period }` with a near-0
            // period would spin a huge number of times before `now`
            // itself ever advances (`now` only moves on through the
            // `poll_sleep` stall above, not inside this inner loop).
            let period = spec.head.period_ns.max(1);
            let d = &mut due[id as usize];
            if *d <= now {
                *d = now + period;
            }
        }
    }
}
