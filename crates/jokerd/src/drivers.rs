//! The io-domain half of a real driver: the async task that opens a
//! `Driver`, polls it on a real clock, and feeds `Shared`'s slots and a
//! device's `CommitRx` (docs/01 §3.4, ref/06 §2.1).
//!
//! The acquisition stage of ref/06 §3.6 lives here, in the order of
//! ref/03 §3 (ADR 0064): [`Decode`] turns `Regs` (modbus registers) and
//! `Bytes` (i2c buffers) into a number from the point's raw `type`,
//! `word_order`, `byte_order` and `bit`; [`acquire`] then applies the
//! profile's `invert`, maps an `enum` integer to its variant, applies
//! `scale` and `offset`, and checks the result against the signal's type;
//! [`Deadband`] keeps the last published value while a new one stays
//! within the point's `deadband`. A value that cannot become the signal's
//! value reads as `bad` with the reason of ref/03 §2.2.7–§2.2.8, never as a
//! number.

use std::sync::Arc;

use joker_core::value::NO_TS_SRC;
use joker_core::{Quality, QualityCode, QualityReason, SignalId, ValueType};
use joker_hal::acquire::{OwnedRaw, SampleBuffer, Settled, settle};
use joker_hal::cx::Host;
use joker_hal::driver::{
    AckSlot, CommittedWrite, ConnectOut, Driver, RawSink, ReadBatch, WriteAcks,
};
use joker_hal::plan::PlanView;
use joker_hal::spec::PlanRequest;
use joker_hal::types::{BatchFailure, BatchKind, LinkState, WriteKind, WriteStatus};
use joker_hal::{CommitRx, Cx, OutputBinding, RxState};
use joker_manifest::context::Acquisition;
use joker_units::Duration;

use crate::plan::Plan;
use crate::shared::{Confirmation, OutputStage, Shared, WriteAckRecord};

pub use crate::action_exec::{ActionCall, ActionOutcome};

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

fn number_to_bits(number: Number, ty: ValueType) -> Result<u64, QualityReason> {
    use QualityReason::{ConfigError, OutOfRange};
    match (number, ty) {
        (Number::Bool(b), ValueType::Bool) => Ok(u64::from(b)),
        (Number::Int(i), ValueType::I64) => Ok(i as u64),
        (Number::Int(i), ValueType::U64 | ValueType::Enum | ValueType::Bits) => {
            u64::try_from(i).map_err(|_| OutOfRange)
        }
        (Number::Uint(u), ValueType::U64 | ValueType::Enum | ValueType::Bits) => Ok(u),
        (Number::Uint(u), ValueType::I64) => {
            i64::try_from(u).map(|i| i as u64).map_err(|_| OutOfRange)
        }
        (Number::Int(i), ValueType::F64 | ValueType::F32) => {
            ty.f64_to_bits(i as f64).ok_or(OutOfRange)
        }
        (Number::Uint(u), ValueType::F64 | ValueType::F32) => {
            ty.f64_to_bits(u as f64).ok_or(OutOfRange)
        }
        (Number::Float(f), ValueType::F64 | ValueType::F32) => ty.f64_to_bits(f).ok_or(OutOfRange),
        _ => Err(ConfigError),
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

/// The number a raw value carries: register words and byte buffers are
/// decoded first (raw type, word and byte order, `bit`; ref/03 §3 step
/// 1), already typed scalars are taken as they are. `None` when the value
/// does not match the point's raw type (wrong length, an unknown `type`)
/// or is not a number (`Text`, `None`).
fn number_of(value: &OwnedRaw, decode: &Decode) -> Option<Number> {
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
        OwnedRaw::Bool(b) => return Some(Number::Bool(*b)),
        OwnedRaw::Int(i) => return Some(Number::Int(*i)),
        OwnedRaw::Uint(u) => return Some(Number::Uint(*u)),
        OwnedRaw::Float(f) => return Some(Number::Float(*f)),
        _ => return None,
    };
    typed(bits, decode)
}

/// The core's transport bits for a settled raw value of a point decoded
/// as `decode`, without any acquisition step: [`acquire`] with
/// [`Acquisition::NONE`]. `None` when the value does not match the point's
/// raw type or cannot become `ty`.
pub fn decode_to_bits(value: &OwnedRaw, decode: &Decode, ty: ValueType) -> Option<u64> {
    acquire(value, decode, &Acquisition::NONE, ty).ok()
}

/// The core's transport bits for a settled raw value of a point, after
/// the acquisition steps of ref/03 §3 (ADR 0064): decoding, `invert`, the
/// `enum` mapping (the raw integer becomes the position of its variant),
/// `scale` and `offset`, then the NaN check and the range of the signal's
/// type. The error is the quality reason of the sample: `config_error`
/// for a value of the wrong form, `nan` for a NaN or an infinity,
/// `out_of_range` for an integer that does not fit or an enum integer
/// without a variant.
pub fn acquire(
    value: &OwnedRaw,
    decode: &Decode,
    acquisition: &Acquisition,
    ty: ValueType,
) -> Result<u64, QualityReason> {
    let number = number_of(value, decode).ok_or(QualityReason::ConfigError)?;
    let number = match number {
        Number::Bool(b) if acquisition.invert => Number::Bool(!b),
        other => other,
    };
    if ty == ValueType::Enum && !acquisition.enum_values.is_empty() {
        let raw = match number {
            Number::Int(i) => i,
            Number::Uint(u) => i64::try_from(u).map_err(|_| QualityReason::OutOfRange)?,
            Number::Bool(_) | Number::Float(_) => return Err(QualityReason::ConfigError),
        };
        return acquisition
            .enum_values
            .iter()
            .position(|v| *v == raw)
            .map(|i| i as u64)
            .ok_or(QualityReason::OutOfRange);
    }
    let number = if acquisition.scale.is_some() || acquisition.offset.is_some() {
        let raw = match number {
            Number::Int(i) => i as f64,
            Number::Uint(u) => u as f64,
            Number::Float(f) => f,
            Number::Bool(_) => return Err(QualityReason::ConfigError),
        };
        Number::Float(raw * acquisition.scale.unwrap_or(1.0) + acquisition.offset.unwrap_or(0.0))
    } else {
        number
    };
    if let Number::Float(f) = number
        && !f.is_finite()
    {
        return Err(QualityReason::Nan);
    }
    if let Some((lo, hi)) = acquisition.limits {
        let value = match number {
            Number::Int(i) => i as f64,
            Number::Uint(u) => u as f64,
            Number::Float(f) => f,
            Number::Bool(_) => return Err(QualityReason::ConfigError),
        };
        if value < lo || value > hi {
            return Err(QualityReason::OutOfRange);
        }
    }
    number_to_bits(number, ty)
}

/// What one settled point becomes in its slot (ref/06 §2.8, docs/16 §3):
/// the acquired value's bits and the driver's quality when the value
/// converts (see [`acquire`]), otherwise no value at all. A `bad` sample
/// keeps its own reason; a value that does not convert is `bad` with the
/// reason [`acquire`] gives. Quality never disappears: a failed read always
/// reaches the slot, it never leaves the last good value in place.
pub fn settled_sample(target: &PointTarget, settled: &Settled) -> (u64, Quality) {
    let status: Quality = settled.status.into();
    if status.is_bad() {
        return (0, status);
    }
    match acquire(
        &settled.value,
        &target.decode,
        &target.acquisition,
        target.ty,
    ) {
        Ok(bits) => (bits, status),
        Err(reason) => (0, Quality::new(QualityCode::Bad, reason)),
    }
}

/// The acquisition deadband of one point (ref/03 §2.2.4, ADR 0064): the
/// last value published while it was `good`. A new `good` value closer to
/// it than the point's `deadband` publishes the last value again, with the
/// new acquisition time, so the signal stays fresh without moving; any
/// other sample is published as it is and becomes the new reference.
#[derive(Clone, Copy, Debug, Default)]
pub struct Deadband {
    last: Option<(f64, u64)>,
}

impl Deadband {
    /// What to publish for `bits` with `quality`, for a point of type `ty`
    /// with a deadband of `band`.
    pub fn filter(&mut self, band: Option<f64>, ty: ValueType, bits: u64, quality: Quality) -> u64 {
        let Some(band) = band else {
            return bits;
        };
        let value = if quality.is_good() {
            numeric_value(ty, bits)
        } else {
            None
        };
        match (value, self.last) {
            (Some(v), Some((last, last_bits))) if (v - last).abs() < band => last_bits,
            (Some(v), _) => {
                self.last = Some((v, bits));
                bits
            }
            (None, _) => {
                self.last = None;
                bits
            }
        }
    }
}

/// `stuck_detect` of one point (ref/03 §2.2.4, ADR 0075): a `good` value
/// that stays bit for bit equal for the window is `uncertain(frozen)`. The
/// value compared is the acquired one, before the deadband, so a value
/// that moves inside its deadband is not frozen.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stuck {
    last: Option<(u64, u64)>,
}

impl Stuck {
    /// The quality to publish for `bits` read at `now` with `quality`.
    pub fn filter(
        &mut self,
        window_ns: Option<u64>,
        bits: u64,
        quality: Quality,
        now: u64,
    ) -> Quality {
        let Some(window) = window_ns else {
            return quality;
        };
        if !quality.is_good() {
            self.last = None;
            return quality;
        }
        match self.last {
            Some((last, since)) if last == bits => {
                if now.saturating_sub(since) >= window {
                    Quality::new(QualityCode::Uncertain, QualityReason::Frozen)
                } else {
                    quality
                }
            }
            _ => {
                self.last = Some((bits, now));
                quality
            }
        }
    }
}

/// The `debounce` of a `bool` read by poll (docs/16 §3.4, D1–D5).
#[derive(Clone, Copy, Debug, Default)]
pub struct Debounce {
    published: Option<bool>,
    candidate: Option<(bool, u64)>,
}

impl Debounce {
    /// What to publish for `bits` read at `ts` with `quality`: the bits and
    /// the instant to write, or `None` while a change waits for its
    /// confirmation (the slot is not written, D3).
    pub fn filter(
        &mut self,
        window_ns: Option<u64>,
        bits: u64,
        quality: Quality,
        ts: u64,
    ) -> Option<(u64, u64)> {
        let Some(window) = window_ns else {
            return Some((bits, ts));
        };
        if quality.is_bad() {
            self.candidate = None;
            return Some((bits, ts));
        }
        let value = bits != 0;
        match self.published {
            Some(p) if p != value => {
                let first = self
                    .candidate
                    .filter(|(v, _)| *v == value)
                    .map_or(ts, |(_, first)| first);
                if ts.saturating_sub(first) >= window {
                    self.published = Some(value);
                    self.candidate = None;
                    Some((bits, first))
                } else {
                    self.candidate = Some((value, first));
                    None
                }
            }
            _ => {
                self.published = Some(value);
                self.candidate = None;
                Some((bits, ts))
            }
        }
    }
}

/// The per-point state of the acquisition stage.
#[derive(Clone, Copy, Debug, Default)]
pub struct PointFilters {
    /// `deadband`.
    pub deadband: Deadband,
    /// `stuck_detect`.
    pub stuck: Stuck,
    /// `debounce`.
    pub debounce: Debounce,
}

fn numeric_value(ty: ValueType, bits: u64) -> Option<f64> {
    match ty {
        ValueType::I64 => Some(bits as i64 as f64),
        ValueType::U64 => Some(bits as f64),
        ValueType::F64 | ValueType::F32 => ty.bits_to_f64(bits),
        _ => None,
    }
}

/// The last executions of one batch, as the link counts them (ref/06
/// §2.12): a batch is affected by a `degraded` link while one of its own
/// last [`DEGRADED_WINDOW`] executions failed against the link.
#[derive(Clone, Copy, Debug, Default)]
struct BatchWindow(u32);

impl BatchWindow {
    /// Records one execution; true while the batch is affected.
    fn execute(&mut self, failed: bool) -> bool {
        self.0 = (self.0 << 1 | u32::from(failed)) & ((1 << DEGRADED_WINDOW) - 1);
        self.0 != 0
    }
}

fn store_settled(
    shared: &Shared,
    target: &PointTarget,
    filters: &mut PointFilters,
    settled: &Settled,
    stale: bool,
    now: u64,
) {
    let (bits, quality) = settled_sample(target, settled);
    let quality = if stale && quality.is_good() {
        Quality::new(QualityCode::Uncertain, QualityReason::Stale)
    } else {
        quality
    };
    let acquisition = &target.acquisition;
    let stuck_ns = acquisition.stuck_detect.map(Duration::as_nanos);
    let quality = filters.stuck.filter(stuck_ns, bits, quality, now);
    let bits = filters
        .deadband
        .filter(acquisition.deadband, target.ty, bits, quality);
    let debounce_ns = acquisition.debounce.map(Duration::as_nanos);
    let ts = settled.ts_hw.unwrap_or(now);
    let Some((bits, ts)) = filters.debounce.filter(debounce_ns, bits, quality, ts) else {
        return;
    };
    if let Some(slot) = shared.slots.get(target.signal.index()) {
        slot.write(bits, quality, NO_TS_SRC, ts);
    }
}

/// One point's signal id and core type, for [`run_driver`] to write a
/// settled read into the right slot.
#[derive(Clone, Debug)]
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
    /// The acquisition steps after decoding (scale, offset, invert, enum,
    /// deadband), from the point's profile.
    pub acquisition: Acquisition,
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
                acquisition: p.acquisition.clone(),
            })
        })
        .collect()
}

/// The first reconnection delay after a link goes `down` (ref/06 §2.12).
pub const BACKOFF_MIN_NS: u64 = 250_000_000;
/// The longest reconnection delay (ref/06 §2.12).
pub const BACKOFF_MAX_NS: u64 = 30_000_000_000;
/// Consecutive rounds in which every batch failed against the link before
/// it goes `down` (ref/06 §2.12).
pub const DOWN_AFTER_FAILED_ROUNDS: u32 = 3;
/// Consecutive write timeouts before the link goes `down` (ref/06 §2.12).
pub const DOWN_AFTER_WRITE_TIMEOUTS: u32 = 3;
const DEGRADED_WINDOW: u32 = 20;
const IDLE_NS: u64 = 100_000_000;

/// The reconnection delays of ref/06 §2.12: 250 ms doubling up to 30 s,
/// each with a jitter of ±20 %. The jitter comes from a small xorshift
/// generator seeded by the caller, so a run on a virtual clock repeats.
#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    base_ns: u64,
    state: u64,
}

impl Backoff {
    /// A backoff at its first delay, its jitter seeded by `seed`.
    pub fn new(seed: u64) -> Self {
        Self {
            base_ns: BACKOFF_MIN_NS,
            state: seed | 1,
        }
    }

    /// Back to the first delay, after a successful connection.
    pub fn reset(&mut self) {
        self.base_ns = BACKOFF_MIN_NS;
    }

    /// The next delay, then doubles the base up to [`BACKOFF_MAX_NS`].
    pub fn next_delay_ns(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        let base = self.base_ns;
        let span = base / 5;
        self.base_ns = base.saturating_mul(2).min(BACKOFF_MAX_NS);
        base - span + self.state % (2 * span + 1)
    }
}

/// The effective link state of one device as its io task computes it from
/// the calls (ref/06 §2.12): `down` after a failed `connect`, a
/// `not-connected`, [`DOWN_AFTER_FAILED_ROUNDS`] rounds in which every
/// batch failed or [`DOWN_AFTER_WRITE_TIMEOUTS`] write timeouts in a row;
/// `degraded` while at least 20 % of the last 20 batch executions failed
/// against the link, back `up` under 5 %.
#[derive(Clone, Copy, Debug)]
struct LinkTracker {
    state: LinkState,
    window: u32,
    failed_rounds: u32,
    write_timeouts: u32,
}

impl LinkTracker {
    fn up() -> Self {
        Self {
            state: LinkState::Up,
            window: 0,
            failed_rounds: 0,
            write_timeouts: 0,
        }
    }

    fn batch(&mut self, failed: bool) {
        self.window = (self.window << 1 | u32::from(failed)) & ((1 << DEGRADED_WINDOW) - 1);
        let failures = self.window.count_ones();
        self.state = match self.state {
            LinkState::Up if failures * 5 >= DEGRADED_WINDOW => LinkState::Degraded,
            LinkState::Degraded if failures * 20 < DEGRADED_WINDOW => LinkState::Up,
            other => other,
        };
    }

    fn round(&mut self, executed: u32, failed: u32) -> bool {
        if executed == 0 {
            return false;
        }
        if failed == executed {
            self.failed_rounds += 1;
        } else {
            self.failed_rounds = 0;
        }
        self.failed_rounds >= DOWN_AFTER_FAILED_ROUNDS
    }

    fn write_timeout(&mut self, timed_out: bool) -> bool {
        if timed_out {
            self.write_timeouts += 1;
        } else {
            self.write_timeouts = 0;
        }
        self.write_timeouts >= DOWN_AFTER_WRITE_TIMEOUTS
    }
}

fn publish_link(shared: &Shared, device_index: usize, state: LinkState) {
    if let Some(stats) = shared.devices.get(device_index) {
        stats.link.set(state as u64);
    }
}

fn mark_all(shared: &Shared, targets: &[PointTarget], quality: Quality, now: u64) {
    for target in targets {
        if let Some(slot) = shared.slots.get(target.signal.index()) {
            slot.write(0, quality, NO_TS_SRC, now);
        }
    }
}

/// Writes `bad(reason)` into the slot of every read point of `targets`:
/// how a device that cannot run, or whose link is down, shows on its
/// signals (ref/06 §2.8, the link quality).
pub fn mark_bad(shared: &Shared, targets: &[PointTarget], reason: QualityReason, now: u64) {
    mark_all(shared, targets, Quality::new(QualityCode::Bad, reason), now);
}

enum Ended {
    Stopped,
    LinkDown,
}

/// Waits until `until` on `host`'s clock, answering every action call
/// with a device failure (the link is not up), and polling `stop` at
/// least every 100 ms. True when `stop` asked to end.
async fn pause(
    host: &dyn Host,
    until: u64,
    action_rx: &mut tokio::sync::mpsc::Receiver<ActionCall>,
    actions_open: &mut bool,
    stop: &mut impl FnMut() -> bool,
) -> bool {
    loop {
        if stop() {
            return true;
        }
        let now = host.now().mono_ns;
        if now >= until {
            return false;
        }
        let wake = until.min(now + IDLE_NS);
        let sleep = core::future::poll_fn(|task_cx| host.poll_sleep(wake, task_cx));
        if *actions_open {
            tokio::select! {
                () = sleep => {}
                call = action_rx.recv() => match call {
                    Some(call) => crate::action_exec::not_connected(call),
                    None => *actions_open = false,
                },
            }
        } else {
            sleep.await;
        }
    }
}

/// Runs one real device's driver on `host`'s clock until `stop` returns
/// true, following the lifecycle of ref/06 §2.1: `connect`, then
/// writes-before-reads on the poll batches' periods (`host` is a
/// `TokioHost` in production, a `VirtualHost` in tests). Reads land in
/// `shared.slots` through `targets`, every settled point of every read
/// (a failed read or a value that does not convert writes a `bad` sample,
/// see [`settled_sample`]). Writes are drained from the `CommitRx` on
/// `shared.commit_bindings[device_index]`/`commit_boxes[device_index]`,
/// through the output stage of `shared.stages[device_index]` (ADR 0072):
/// each output its device fallback covers is renewed with `keepalive` at
/// its own period, or at `fallback_keepalive_ns`, and a `pulse` output is
/// written back to `safe` once its pulse has elapsed.
///
/// The link state of ref/06 §2.12 goes to `shared.devices[device_index]`.
/// When the link goes `down` (a failed `connect`, a `not-connected`, three
/// rounds in which every batch failed, three write timeouts in a row) the
/// driver is disconnected, every read point turns `bad(comm_failure)`, and
/// `connect` is retried after a [`Backoff`] delay; after each `connect`
/// the applied value of every output is resent (`resync`). Every write the
/// driver does not ack `ok` goes to `shared.write_acks`, for the decision
/// log (ref/06 §2.9, AGENTS.md invariant 8).
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
    let bindings = shared
        .commit_bindings
        .get(device_index)
        .map_or(&[][..], Vec::as_slice);
    let boxes = shared
        .commit_boxes
        .get(device_index)
        .map_or(&[][..], Vec::as_slice);
    let mut rx_state = vec![RxState::default(); bindings.len()];
    let Ok(mut rx) = CommitRx::new(bindings, boxes, &mut rx_state) else {
        mark_bad(
            shared,
            targets,
            QualityReason::ConfigError,
            host.now().mono_ns,
        );
        return;
    };
    let mut backoff = Backoff::new(host.now().mono_ns ^ (device_index as u64) << 32);
    let mut actions_open = true;
    let mut connected_before = false;
    publish_link(shared, device_index, LinkState::Connecting);
    while !stop() {
        let deadline = host.now().mono_ns + timeout_ns;
        let mut cx = Cx::new(host, deadline);
        let mut connect_sink = ();
        let mut out = ConnectOut::new(&mut connect_sink);
        let connected = driver.connect(&mut cx, &mut out).await.is_ok();
        if connected {
            if connected_before && let Some(stats) = shared.devices.get(device_index) {
                stats.reconnects.bump();
            }
            connected_before = true;
            backoff.reset();
            rx.resync();
            let ended = serve(
                &mut driver,
                host,
                &request,
                plan,
                timeout_ns,
                targets,
                shared,
                device_index,
                fallback_keepalive_ns,
                &mut rx,
                &mut action_rx,
                &mut actions_open,
                &mut stop,
            )
            .await;
            if matches!(ended, Ended::Stopped) {
                break;
            }
            let mut cx = Cx::new(host, host.now().mono_ns + timeout_ns);
            driver.disconnect(&mut cx).await;
        }
        publish_link(shared, device_index, LinkState::Down);
        mark_bad(
            shared,
            targets,
            QualityReason::CommFailure,
            host.now().mono_ns,
        );
        let delay = backoff.next_delay_ns();
        eprintln!(
            "warning: device `{}`: link down, connecting again in {} ms",
            request.devices.first().map_or("?", |d| d.name),
            delay / 1_000_000
        );
        let until = host.now().mono_ns + delay;
        if pause(host, until, &mut action_rx, &mut actions_open, &mut stop).await {
            break;
        }
        publish_link(shared, device_index, LinkState::Connecting);
    }
    let mut cx = Cx::new(host, host.now().mono_ns + timeout_ns);
    driver.disconnect(&mut cx).await;
}

#[allow(
    clippy::too_many_arguments,
    reason = "the connected half of run_driver, with the same pieces plus the state run_driver keeps across reconnections"
)]
async fn serve<D: Driver>(
    driver: &mut D,
    host: &dyn Host,
    request: &PlanRequest<'_>,
    plan: PlanView<'_>,
    timeout_ns: u64,
    targets: &[PointTarget],
    shared: &Shared,
    device_index: usize,
    fallback_keepalive_ns: Option<u64>,
    rx: &mut CommitRx<'_>,
    action_rx: &mut tokio::sync::mpsc::Receiver<ActionCall>,
    actions_open: &mut bool,
    stop: &mut impl FnMut() -> bool,
) -> Ended {
    let stats = shared.devices.get(device_index);
    let mut link = LinkTracker::up();
    publish_link(shared, device_index, link.state);
    let mut due: Vec<u64> = vec![host.now().mono_ns; plan.len()];
    let mut buf = SampleBuffer::with_capacity(4096);
    let mut filters = vec![PointFilters::default(); targets.len()];
    let mut batch_windows = vec![BatchWindow::default(); plan.len()];
    let mut rejected = vec![false; shared.commit_bindings.get(device_index).map_or(0, Vec::len)];
    let stages = shared
        .stages
        .get(device_index)
        .map_or(&[][..], Vec::as_slice);
    let mut output_stage = OutputStageState::new(stages, fallback_keepalive_ns, host.now().mono_ns);
    while !stop() {
        let mut writes = Vec::new();
        output_stage.due(host.now().mono_ns, rx, &mut writes);
        rx.drain_into(&mut writes);
        output_stage.pulses(
            host.now().mono_ns,
            shared
                .commit_bindings
                .get(device_index)
                .map_or(&[][..], Vec::as_slice),
            rx,
            &mut writes,
        );
        let bindings = shared
            .commit_bindings
            .get(device_index)
            .map_or(&[][..], Vec::as_slice);
        let mut words = vec![[0u16; 4]; writes.len()];
        let writes = encode_writes(
            shared,
            device_index,
            bindings,
            stages,
            &writes,
            &mut words,
            &mut rejected,
        );
        if !writes.is_empty() {
            let mut slots = vec![AckSlot::default(); writes.len()];
            if let Some(mut acks) = WriteAcks::new(&writes, &mut slots) {
                let deadline = host.now().mono_ns + timeout_ns * (1 + writes.len() as u64 / 8);
                let mut cx = Cx::new(host, deadline);
                driver.write(&mut cx, &writes, &mut acks).await;
                record_failed_acks(shared, device_index, &writes, &acks, host.now().mono_ns);
                confirm_outputs(shared, device_index, &writes, &acks, &mut rejected);
                let mut down = false;
                for ack in acks.iter() {
                    down |= ack.status == WriteStatus::NotConnected
                        || link.write_timeout(ack.status == WriteStatus::Timeout);
                }
                if down {
                    return Ended::LinkDown;
                }
            }
        }

        let next = plan
            .iter()
            .filter(|b| b.head.kind == BatchKind::Poll)
            .map(|b| due[b.head.id as usize])
            .chain(output_stage.next())
            .min()
            .unwrap_or(host.now().mono_ns + IDLE_NS)
            .min(host.now().mono_ns + IDLE_NS);
        let sleep = core::future::poll_fn(|task_cx| host.poll_sleep(next, task_cx));
        if *actions_open {
            tokio::select! {
                () = sleep => {}
                call = action_rx.recv() => {
                    match call {
                        Some(call) => crate::action_exec::run(driver, host, timeout_ns, call).await,
                        None => *actions_open = false,
                    }
                    continue;
                }
            }
        } else {
            sleep.await;
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
        let (mut executed, mut failed) = (0, 0);
        for id in ids {
            let Some(spec) = plan.batch(id) else { continue };
            let deadline = host.now().mono_ns + timeout_ns;
            let batch = ReadBatch::new(&spec, deadline);
            let mut cx = Cx::new(host, deadline);
            buf.clear();
            let mut sink = RawSink::new(&mut buf);
            let result = driver.read(&mut cx, &batch, &mut sink).await;
            if let Some(stats) = stats {
                stats.dropped_samples.add(u64::from(sink.dropped()));
            }
            let settled = settle(request, &batch, buf.samples(), result);
            let now = host.now().mono_ns;
            let against_link = result.is_err_and(|f| f.counts_for_link());
            executed += 1;
            failed += u32::from(against_link);
            link.batch(against_link);
            let stale = batch_windows
                .get_mut(id as usize)
                .is_some_and(|w| w.execute(against_link) && link.state == LinkState::Degraded);
            for s in &settled {
                if s.invalid.is_some()
                    && let Some(stats) = stats
                {
                    stats.invalid_samples.bump();
                }
                if let Some(i) = targets.iter().position(|t| t.point == s.point)
                    && let (Some(target), Some(state)) = (targets.get(i), filters.get_mut(i))
                {
                    store_settled(shared, target, state, s, stale, now);
                }
            }
            if matches!(result, Err(BatchFailure::NotConnected)) {
                return Ended::LinkDown;
            }
            let period = spec.head.period_ns.max(1);
            let d = &mut due[id as usize];
            if *d <= now {
                *d = now + period;
            }
        }
        if link.round(executed, failed) {
            return Ended::LinkDown;
        }
        publish_link(shared, device_index, link.state);
    }
    Ended::Stopped
}

/// The writes as the driver receives them (ref/06 §2.7, ADR 0075): each
/// output with a conversion gets its raw value, encoded into `words` for a
/// register point. A value its raw type cannot carry never reaches the
/// driver: the plan gives such an output the range of its raw type, so
/// this only happens on a configuration the plan should have refused; the
/// write is dropped, reported, and the output is left
/// `uncertain(not_confirmed)`.
fn encode_writes<'w>(
    shared: &Shared,
    device_index: usize,
    bindings: &[OutputBinding],
    stages: &[OutputStage],
    writes: &[CommittedWrite<'static>],
    words: &'w mut [[u16; 4]],
    rejected: &mut [bool],
) -> Vec<CommittedWrite<'w>> {
    let mut out = Vec::with_capacity(writes.len());
    for (w, buf) in writes.iter().zip(words.iter_mut()) {
        let i = bindings.iter().position(|b| b.point == w.point);
        let conversion = i
            .and_then(|i| stages.get(i))
            .and_then(|s| s.conversion.as_ref());
        let Some(conversion) = conversion else {
            out.push(*w);
            continue;
        };
        match conversion.codec().encode(w, buf) {
            Ok(raw) => out.push(raw),
            Err(e) => {
                let Some(binding) = i.and_then(|i| bindings.get(i)) else {
                    continue;
                };
                eprintln!(
                    "warning: device {device_index}: output {:?} not written: {e:?}",
                    binding.signal
                );
                if let Some(flag) = i.and_then(|i| rejected.get_mut(i)) {
                    *flag = true;
                }
                let confirmation = Confirmation {
                    output: binding.signal,
                    confirmed: false,
                };
                if shared.confirmations.push(confirmation).is_err() {
                    shared.confirmations_dropped.bump();
                }
            }
        }
    }
    out
}

/// The timers of the output stage of one device (ref/06 §2.9, ADR 0072):
/// when each output's next `keepalive` is due, and when a pulse in
/// progress ends.
struct OutputStageState {
    periods: Vec<Option<u64>>,
    keepalive_at: Vec<Option<u64>>,
    pulse_ns: Vec<Option<u64>>,
    pulse_end: Vec<Option<u64>>,
}

impl OutputStageState {
    fn new(stages: &[OutputStage], device_keepalive_ns: Option<u64>, now: u64) -> Self {
        let periods: Vec<Option<u64>> = stages
            .iter()
            .map(|s| s.keepalive_period(device_keepalive_ns))
            .collect();
        Self {
            keepalive_at: periods.iter().map(|p| p.map(|ns| now + ns)).collect(),
            periods,
            pulse_ns: stages.iter().map(|s| s.pulse_ns).collect(),
            pulse_end: vec![None; stages.len()],
        }
    }

    /// Queues the writes whose time has come: the end of each pulse that
    /// has elapsed (the declared `safe`), and the keepalive of each output
    /// whose period has elapsed.
    fn due(&mut self, now: u64, rx: &mut CommitRx<'_>, writes: &mut Vec<CommittedWrite<'static>>) {
        for (i, end) in self.pulse_end.iter_mut().enumerate() {
            if end.is_some_and(|at| now >= at) {
                *end = None;
                if let Some(w) = rx.safe_write(i, WriteKind::Set) {
                    writes.push(w);
                }
            }
        }
        for (i, (at, period)) in self.keepalive_at.iter_mut().zip(&self.periods).enumerate() {
            if let (Some(due), Some(ns)) = (*at, period)
                && now >= due
            {
                rx.keepalive(i);
                *at = Some(now + ns);
            }
        }
    }

    /// Applies `pulse` to the drained writes: a write of a value other
    /// than `safe` starts a pulse, a write of `safe` ends it, and a
    /// `resync` or `keepalive` outside a pulse renews `safe`, the state the
    /// equipment rests in, instead of starting the pulse again.
    fn pulses(
        &mut self,
        now: u64,
        bindings: &[OutputBinding],
        rx: &mut CommitRx<'_>,
        writes: &mut [CommittedWrite<'static>],
    ) {
        for w in writes.iter_mut() {
            let Some(i) = bindings.iter().position(|b| b.point == w.point) else {
                continue;
            };
            let Some(Some(ns)) = self.pulse_ns.get(i).copied() else {
                continue;
            };
            let Some(end) = self.pulse_end.get_mut(i) else {
                continue;
            };
            if rx.is_safe(i, w) {
                if w.kind == WriteKind::Set || w.kind == WriteKind::ForceSafe {
                    *end = None;
                }
                continue;
            }
            match w.kind {
                WriteKind::Set => *end = Some(now + ns),
                WriteKind::Resync | WriteKind::Keepalive if end.is_none() => {
                    if let Some(safe) = rx.safe_write(i, w.kind) {
                        *w = safe;
                    }
                }
                _ => {}
            }
        }
    }

    /// When the next timer of the output stage is due.
    fn next(&self) -> Option<u64> {
        self.keepalive_at
            .iter()
            .chain(&self.pulse_end)
            .flatten()
            .copied()
            .min()
    }
}

fn record_failed_acks(
    shared: &Shared,
    device_index: usize,
    writes: &[CommittedWrite<'_>],
    acks: &WriteAcks<'_>,
    now: u64,
) {
    let bindings = shared
        .commit_bindings
        .get(device_index)
        .map_or(&[][..], Vec::as_slice);
    for (write, ack) in writes.iter().zip(acks.iter()) {
        if ack.status == WriteStatus::Ok {
            continue;
        }
        let Some(binding) = bindings.iter().find(|b| b.point == write.point) else {
            continue;
        };
        let record = WriteAckRecord {
            device: device_index as u32,
            output: binding.signal,
            seq: ack.seq,
            kind: write.kind,
            status: ack.status,
            at_mono_ns: now,
        };
        if shared.write_acks.push(record).is_err() {
            shared.write_acks_dropped.bump();
        }
    }
}

/// Tells the cycle thread which outputs the equipment rejected, and which
/// rejected ones a later write confirmed (ref/06 §2.9): a
/// `device-rejected` ack leaves the output `uncertain(not_confirmed)`, an
/// `ok` ack of the same output clears it. `rejected` is per binding of the
/// device, so only the transitions are sent.
fn confirm_outputs(
    shared: &Shared,
    device_index: usize,
    writes: &[CommittedWrite<'_>],
    acks: &WriteAcks<'_>,
    rejected: &mut [bool],
) {
    let bindings = shared
        .commit_bindings
        .get(device_index)
        .map_or(&[][..], Vec::as_slice);
    for (write, ack) in writes.iter().zip(acks.iter()) {
        let Some(i) = bindings.iter().position(|b| b.point == write.point) else {
            continue;
        };
        let (Some(binding), Some(flag)) = (bindings.get(i), rejected.get_mut(i)) else {
            continue;
        };
        let confirmed = match ack.status {
            WriteStatus::DeviceRejected => false,
            WriteStatus::Ok if *flag => true,
            _ => continue,
        };
        *flag = !confirmed;
        let confirmation = Confirmation {
            output: binding.signal,
            confirmed,
        };
        if shared.confirmations.push(confirmation).is_err() {
            shared.confirmations_dropped.bump();
        }
    }
}
