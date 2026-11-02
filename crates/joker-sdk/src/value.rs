//! Port values, quality and outcomes (ref/04 §4.7).

use core::fmt;
use core::num::NonZeroU8;

use crate::time::{Instant, MonoNanos, UtcNanos};

#[doc(hidden)]
pub mod sealed {
    /// Only the SDK and its derives implement [`super::PortValue`].
    pub trait Sealed {}
}

/// A type a scalar port can carry: `bool`, `i64`, `u64`, `f64`, `f32`,
/// `Bits<N>` and the enums with `#[derive(joker::Enum)]`. Any other is
/// `J-E0707`.
#[diagnostic::on_unimplemented(
    message = "J-E0707: `{Self}` is not a port type",
    label = "not a port type",
    note = "a scalar port carries bool, i64, u64, f64, f32, Bits<N> or an enum with #[derive(joker::Enum)]"
)]
pub trait PortValue: Copy + PartialEq + fmt::Debug + 'static + sealed::Sealed {
    /// The signal type code (ref/00 §6).
    const TYPE: u8;
    /// The value from its bits (ref/00 §4.1).
    fn from_bits(bits: u64) -> Self;
    /// The bits of the value.
    fn to_bits(self) -> u64;
}

impl sealed::Sealed for bool {}
impl PortValue for bool {
    const TYPE: u8 = 0;
    fn from_bits(bits: u64) -> Self {
        bits != 0
    }
    fn to_bits(self) -> u64 {
        u64::from(self)
    }
}

impl sealed::Sealed for i64 {}
impl PortValue for i64 {
    const TYPE: u8 = 1;
    fn from_bits(bits: u64) -> Self {
        bits as i64
    }
    fn to_bits(self) -> u64 {
        self as u64
    }
}

impl sealed::Sealed for u64 {}
impl PortValue for u64 {
    const TYPE: u8 = 2;
    fn from_bits(bits: u64) -> Self {
        bits
    }
    fn to_bits(self) -> u64 {
        self
    }
}

impl sealed::Sealed for f64 {}
impl PortValue for f64 {
    const TYPE: u8 = 3;
    fn from_bits(bits: u64) -> Self {
        f64::from_bits(bits)
    }
    fn to_bits(self) -> u64 {
        f64::to_bits(self)
    }
}

impl<const N: usize> sealed::Sealed for crate::bounded::Bits<N> {}
impl<const N: usize> PortValue for crate::bounded::Bits<N> {
    const TYPE: u8 = 6;
    fn from_bits(bits: u64) -> Self {
        crate::bounded::Bits::new(bits)
    }
    fn to_bits(self) -> u64 {
        self.bits()
    }
}

impl sealed::Sealed for f32 {}
impl PortValue for f32 {
    const TYPE: u8 = 4;
    fn from_bits(bits: u64) -> Self {
        f64::from_bits(bits) as f32
    }
    fn to_bits(self) -> u64 {
        f64::from(self).to_bits()
    }
}

/// Quality code (ref/00 §4.3). Closed forever.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QualityCode {
    /// Good.
    Good = 0,
    /// Uncertain.
    Uncertain = 1,
    /// Bad.
    Bad = 2,
}

/// Quality reason (ref/00 §4.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum QualityReason {
    /// No reason.
    None = 0,
    /// Overridden.
    Overridden = 1,
    /// Local override.
    LocalOverride = 2,
    /// Stale.
    Stale = 3,
    /// Not confirmed.
    NotConfirmed = 4,
    /// Substituted.
    Substituted = 5,
    /// Out of distribution.
    OutOfDistribution = 6,
    /// Sensor calibration.
    SensorCal = 7,
    /// Clock unsynced.
    ClockUnsynced = 8,
    /// Never written.
    Initial = 9,
    /// Frozen.
    Frozen = 10,
    /// Partial write.
    PartialWrite = 11,
    /// Communication failure.
    CommFailure = 12,
    /// Device fault.
    DeviceFault = 13,
    /// Out of range.
    OutOfRange = 14,
    /// Configuration error.
    ConfigError = 15,
    /// Not connected.
    NotConnected = 16,
    /// Not a number.
    Nan = 17,
    /// Tripped.
    Tripped = 18,
    /// Deadline.
    Deadline = 19,
}

impl QualityReason {
    /// The reason of a code; unknown codes are `None` (ref/04 §2.8).
    pub fn from_u8(code: u8) -> Self {
        use QualityReason::*;
        const ALL: [QualityReason; 20] = [
            None,
            Overridden,
            LocalOverride,
            Stale,
            NotConfirmed,
            Substituted,
            OutOfDistribution,
            SensorCal,
            ClockUnsynced,
            Initial,
            Frozen,
            PartialWrite,
            CommFailure,
            DeviceFault,
            OutOfRange,
            ConfigError,
            NotConnected,
            Nan,
            Tripped,
            Deadline,
        ];
        ALL.get(code as usize).copied().unwrap_or(None)
    }

    /// The canonical name (`comm_failure`).
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Overridden => "overridden",
            Self::LocalOverride => "local_override",
            Self::Stale => "stale",
            Self::NotConfirmed => "not_confirmed",
            Self::Substituted => "substituted",
            Self::OutOfDistribution => "out_of_distribution",
            Self::SensorCal => "sensor_cal",
            Self::ClockUnsynced => "clock_unsynced",
            Self::Initial => "initial",
            Self::Frozen => "frozen",
            Self::PartialWrite => "partial_write",
            Self::CommFailure => "comm_failure",
            Self::DeviceFault => "device_fault",
            Self::OutOfRange => "out_of_range",
            Self::ConfigError => "config_error",
            Self::NotConnected => "not_connected",
            Self::Nan => "nan",
            Self::Tripped => "tripped",
            Self::Deadline => "deadline",
        }
    }
}

/// The quality of a sample.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quality {
    /// Code.
    pub code: QualityCode,
    /// Reason.
    pub reason: QualityReason,
}

impl Quality {
    /// `good`.
    pub const GOOD: Quality = Quality::good();

    /// `good`, without a reason.
    pub const fn good() -> Self {
        Self {
            code: QualityCode::Good,
            reason: QualityReason::None,
        }
    }

    /// `uncertain(reason)`.
    pub const fn uncertain(reason: QualityReason) -> Self {
        Self {
            code: QualityCode::Uncertain,
            reason,
        }
    }

    /// `bad(reason)`.
    pub const fn bad(reason: QualityReason) -> Self {
        Self {
            code: QualityCode::Bad,
            reason,
        }
    }

    /// From the code and reason bytes of the image; an unknown code is `bad`.
    pub fn from_codes(code: u8, reason: u8) -> Self {
        let code = match code {
            0 => QualityCode::Good,
            1 => QualityCode::Uncertain,
            _ => QualityCode::Bad,
        };
        Self {
            code,
            reason: QualityReason::from_u8(reason),
        }
    }

    /// True for `good`.
    pub fn is_good(&self) -> bool {
        self.code == QualityCode::Good
    }

    /// True for `uncertain`.
    pub fn is_uncertain(&self) -> bool {
        self.code == QualityCode::Uncertain
    }

    /// True for `bad`.
    pub fn is_bad(&self) -> bool {
        self.code == QualityCode::Bad
    }
}

impl fmt::Display for Quality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self.code {
            QualityCode::Good => "good",
            QualityCode::Uncertain => "uncertain",
            QualityCode::Bad => "bad",
        };
        if self.reason == QualityReason::None {
            f.write_str(code)
        } else {
            write!(f, "{code}({})", self.reason.name())
        }
    }
}

/// The state of an alarm (ref/00 §6, `alarm_state`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlarmState {
    /// Inactive and acknowledged.
    Normal,
    /// Active, not acknowledged.
    ActiveUnacked,
    /// Active, acknowledged.
    ActiveAcked,
    /// Returned to normal, not acknowledged.
    RtnUnacked,
    /// Not known.
    Unknown,
}

impl AlarmState {
    pub(crate) fn from_code(code: u64) -> Self {
        match code {
            0 => Self::Normal,
            1 => Self::ActiveUnacked,
            2 => Self::ActiveAcked,
            3 => Self::RtnUnacked,
            _ => Self::Unknown,
        }
    }
}

/// A value of any scalar or short wide type (ref/04 §4.7).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Value {
    /// `bool`.
    Bool(bool),
    /// `i64`.
    I64(i64),
    /// `u64`, an enum or `bits`.
    U64(u64),
    /// `f64` or `f32`.
    F64(f64),
    /// `string`.
    Text(crate::bounded::BoundedString<64>),
    /// `bytes`.
    Bytes(crate::bounded::BoundedBytes<64>),
}

impl Value {
    /// The value of a port of signal type `ty` from its bits.
    pub fn from_bits(ty: u8, bits: u64) -> Self {
        match ty {
            0 => Value::Bool(bits != 0),
            1 => Value::I64(bits as i64),
            3 | 4 => Value::F64(f64::from_bits(bits)),
            _ => Value::U64(bits),
        }
    }

    /// The bits of a scalar value; 0 for text and bytes.
    pub fn to_bits(&self) -> u64 {
        match self {
            Value::Bool(b) => u64::from(*b),
            Value::I64(i) => *i as u64,
            Value::U64(u) => *u,
            Value::F64(f) => f.to_bits(),
            Value::Text(_) | Value::Bytes(_) => 0,
        }
    }

    /// The value as a `T`, when its type is that of `T`.
    pub fn get<T: PortValue>(&self) -> Option<T> {
        let same = matches!(
            (self, T::TYPE),
            (Value::Bool(_), 0)
                | (Value::I64(_), 1)
                | (Value::U64(_), 2 | 5 | 6)
                | (Value::F64(_), 3 | 4)
        );
        same.then(|| T::from_bits(self.to_bits()))
    }
}

impl<T: PortValue> From<T> for Value {
    fn from(v: T) -> Self {
        Value::from_bits(T::TYPE, v.to_bits())
    }
}

impl PartialEq<f64> for Value {
    fn eq(&self, other: &f64) -> bool {
        matches!(self, Value::F64(v) if v == other)
    }
}

impl PartialEq<bool> for Value {
    fn eq(&self, other: &bool) -> bool {
        matches!(self, Value::Bool(v) if v == other)
    }
}

impl PartialEq<i64> for Value {
    fn eq(&self, other: &i64) -> bool {
        matches!(self, Value::I64(v) if v == other)
    }
}

impl PartialEq<u64> for Value {
    fn eq(&self, other: &u64) -> bool {
        matches!(self, Value::U64(v) if v == other)
    }
}

/// A complete sample of a port.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sample<T> {
    /// The value, `T::default()` when there never was one. Read it through
    /// [`Sample::good`] or the port's `get` unless the quality was checked.
    pub value: T,
    /// The quality.
    pub quality: Quality,
    /// Source timestamp, when the protocol gives one.
    pub ts_src: Option<UtcNanos>,
    /// Hardware capture instant.
    pub ts_hw: Option<MonoNanos>,
    /// Acquisition instant.
    pub ts_acq: MonoNanos,
    /// Sequence number.
    pub seq: u64,
}

impl<T> Sample<T> {
    /// The value if the sample is `good`.
    pub fn good(&self) -> Option<&T> {
        self.quality.is_good().then_some(&self.value)
    }

    /// Time since acquisition at `now`.
    pub fn age(&self, now: Instant) -> core::time::Duration {
        core::time::Duration::from_nanos(now.mono.0.saturating_sub(self.ts_acq.0))
    }
}

/// An arbiter level, 1 (highest) to 16 (ref/00 §5).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Level(NonZeroU8);

impl Level {
    /// A level from its number, if within 1–16.
    pub fn new(level: u8) -> Option<Self> {
        if level <= 16 {
            NonZeroU8::new(level).map(Self)
        } else {
            None
        }
    }

    /// The level number.
    pub fn get(self) -> u8 {
        self.0.get()
    }
}

/// What clamped a write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClampKind {
    /// The range.
    Range,
    /// The rate.
    Rate,
}

/// Why a write was rejected (ref/00 §4.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum RejectReason {
    /// Wrong type.
    WrongType = 1,
    /// Interlock.
    Interlock,
    /// Input quality.
    Quality,
    /// Range.
    Range,
    /// Atomic group.
    AtomicGroup,
    /// Not the owner.
    NotOwner,
    /// Clock unsynced.
    ClockUnsynced,
    /// Budget.
    Budget,
}

impl RejectReason {
    pub(crate) fn from_u8(code: u8) -> Self {
        match code {
            1 => Self::WrongType,
            2 => Self::Interlock,
            3 => Self::Quality,
            4 => Self::Range,
            5 => Self::AtomicGroup,
            6 => Self::NotOwner,
            7 => Self::ClockUnsynced,
            _ => Self::Budget,
        }
    }
}

/// The outcome of this writer's last proposal (ref/04 §2.2).
#[derive(Clone, Copy, PartialEq, Debug)]
#[non_exhaustive]
pub enum WriteOutcome<T> {
    /// Applied as proposed.
    Accepted,
    /// Applied, limited.
    Clamped {
        /// What limited it.
        by: ClampKind,
        /// The value applied.
        applied: T,
    },
    /// Within the deadband or `min_interval`.
    Suppressed,
    /// A higher level is active.
    Overridden {
        /// The winning level.
        level: Level,
    },
    /// Waiting for the owner group.
    Deferred,
    /// Shadow mode.
    Shadowed,
    /// Refused.
    Rejected(RejectReason),
    /// The output is in trip.
    Tripped,
}

impl<T: PortValue> WriteOutcome<T> {
    /// The outcome from the code, detail and applied bits of a result entry;
    /// `None` for code 0.
    pub fn from_entry(code: u8, detail: u8, applied: u64) -> Option<Self> {
        Some(match code {
            1 => Self::Accepted,
            2 | 3 => Self::Clamped {
                by: if code == 2 {
                    ClampKind::Range
                } else {
                    ClampKind::Rate
                },
                applied: T::from_bits(applied),
            },
            4 => Self::Suppressed,
            5 => Self::Overridden {
                level: Level::new(detail)?,
            },
            6 => Self::Deferred,
            7 => Self::Shadowed,
            8 => Self::Rejected(RejectReason::from_u8(detail)),
            9 => Self::Tripped,
            _ => return None,
        })
    }

    /// The outcome code (ref/00 §4.4).
    pub fn code(&self) -> u8 {
        match self {
            Self::Accepted => 1,
            Self::Clamped {
                by: ClampKind::Range,
                ..
            } => 2,
            Self::Clamped {
                by: ClampKind::Rate,
                ..
            } => 3,
            Self::Suppressed => 4,
            Self::Overridden { .. } => 5,
            Self::Deferred => 6,
            Self::Shadowed => 7,
            Self::Rejected(_) => 8,
            Self::Tripped => 9,
        }
    }

    /// True for `Accepted` and `Clamped`.
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Accepted | Self::Clamped { .. })
    }

    /// The outcome if applied; otherwise `Err(J-E0204)`.
    pub fn require_applied(self) -> crate::error::Result<Self> {
        if self.is_applied() {
            Ok(self)
        } else {
            Err(crate::error::Error::new(
                crate::error::Code::from_static("J-E0204"),
                format_args!("the write was not applied (outcome {})", self.code()),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::ToString;

    #[test]
    fn values_round_trip_through_their_bits() {
        assert!(bool::from_bits(true.to_bits()));
        assert_eq!(i64::from_bits((-3i64).to_bits()), -3);
        assert_eq!(f64::from_bits(PortValue::to_bits(2.5f64)), 2.5);
        assert_eq!(
            <f32 as PortValue>::from_bits(PortValue::to_bits(0.1f32)),
            0.1f32
        );
    }

    #[test]
    fn outcomes_from_result_entries() {
        assert_eq!(WriteOutcome::<f64>::from_entry(0, 0, 0), None);
        assert_eq!(
            WriteOutcome::<f64>::from_entry(3, 0, 2.0f64.to_bits()),
            Some(WriteOutcome::Clamped {
                by: ClampKind::Rate,
                applied: 2.0
            })
        );
        assert_eq!(
            WriteOutcome::<bool>::from_entry(5, 4, 0).map(|o| o.code()),
            Some(5)
        );
        assert_eq!(Quality::from_codes(2, 12).to_string(), "bad(comm_failure)");
    }
}
