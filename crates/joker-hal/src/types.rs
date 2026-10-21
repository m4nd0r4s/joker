//! The records shared by every tier (ref/06 §2.5–§2.9, §3.2), with the names,
//! field order and numeric values of `spec/cddl/jdp.cddl`.

pub use joker_core::QualityCode;
use joker_core::{Quality, QualityReason};

/// Dense index of a point in a driver instance (ref/06 §2.5).
pub type PointId = u32;
/// Index of a batch in a plan (ref/06 §2.6).
pub type BatchId = u32;
/// Index of a device in a driver instance; 0 when `instance = device`.
pub type DeviceIx = u16;
/// Monotonic nanoseconds, the node's `CLOCK_MONOTONIC`.
pub type MonoNs = u64;

/// Numeric reason of ref/00 §4.3; 240–255 are private to the driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reason(pub u8);

impl Reason {
    /// No particular reason.
    pub const NONE: Self = Self(0);
    /// Value forced on the equipment.
    pub const OVERRIDDEN: Self = Self(1);
    /// The equipment is in local mode.
    pub const LOCAL_OVERRIDE: Self = Self(2);
    /// The equipment reports a stale value.
    pub const STALE: Self = Self(3);
    /// The equipment substituted the value.
    pub const SUBSTITUTED: Self = Self(5);
    /// The sensor is in calibration.
    pub const SENSOR_CAL: Self = Self(7);
    /// The source timestamp comes from an unsynchronised clock.
    pub const CLOCK_UNSYNCED: Self = Self(8);
    /// The equipment has no value yet.
    pub const INITIAL: Self = Self(9);
    /// Communication with the equipment failed.
    pub const COMM_FAILURE: Self = Self(12);
    /// The equipment reports a fault.
    pub const DEVICE_FAULT: Self = Self(13);
    /// The equipment reports a value out of its range.
    pub const OUT_OF_RANGE: Self = Self(14);
    /// The point or the device is misconfigured.
    pub const CONFIG_ERROR: Self = Self(15);
    /// The point is not connected on the equipment.
    pub const NOT_CONNECTED: Self = Self(16);

    /// A private reason: `240 + (n % 16)`.
    pub const fn private(n: u8) -> Self {
        Self(240 + (n % 16))
    }
}

/// The quality the equipment reports for one sample (ref/06 §2.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawStatus {
    /// Quality code.
    pub code: QualityCode,
    /// Reason.
    pub reason: Reason,
}

impl RawStatus {
    /// `good`, no reason.
    pub const OK: Self = Self {
        code: QualityCode::Good,
        reason: Reason::NONE,
    };

    /// `bad(reason)`.
    pub const fn bad(reason: Reason) -> Self {
        Self {
            code: QualityCode::Bad,
            reason,
        }
    }

    /// `uncertain(reason)`.
    pub const fn uncertain(reason: Reason) -> Self {
        Self {
            code: QualityCode::Uncertain,
            reason,
        }
    }

    /// True when a driver may report this pair (ref/06 §2.8): the reasons of
    /// each code in the table, or a private reason with any code.
    pub const fn is_allowed(self) -> bool {
        if self.reason.0 >= 240 {
            return true;
        }
        match self.code {
            QualityCode::Good => matches!(self.reason.0, 0..=2),
            QualityCode::Uncertain => matches!(self.reason.0, 3 | 5 | 7 | 9 | 8),
            QualityCode::Bad => matches!(self.reason.0, 3 | 12..=16),
        }
    }

    /// The core's quality word for this status, without flags.
    pub const fn quality(self) -> Quality {
        Quality::new(self.code, QualityReason::from_u8(self.reason.0))
    }
}

impl From<RawStatus> for Quality {
    fn from(status: RawStatus) -> Self {
        status.quality()
    }
}

/// Raw form of a value (ref/06 §2.7), with the codes of the CDDL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RawKind {
    /// No value; only in failures.
    None = 0,
    /// `bool`.
    Bool = 1,
    /// `i64`.
    Int = 2,
    /// `u64`.
    Uint = 3,
    /// `f64`.
    Float = 4,
    /// 16-bit registers in protocol order.
    Regs = 5,
    /// Bytes, at most 256.
    Bytes = 6,
    /// UTF-8 text, at most 256 bytes.
    Text = 7,
}

/// Largest raw `regs`, `bytes` or `text` value in bytes (ref/06 §2.15).
pub const MAX_RAW_BYTES: usize = 256;

/// A borrowed raw value; the sink copies it. No allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawValue<'a> {
    /// No value.
    None,
    /// `bool`.
    Bool(bool),
    /// `i64`.
    Int(i64),
    /// `u64`.
    Uint(u64),
    /// `f64`.
    Float(f64),
    /// 16-bit registers, in protocol order.
    Regs(&'a [u16]),
    /// Bytes.
    Bytes(&'a [u8]),
    /// UTF-8 text.
    Text(&'a str),
}

impl RawValue<'_> {
    /// The kind of this value.
    pub const fn kind(&self) -> RawKind {
        match self {
            Self::None => RawKind::None,
            Self::Bool(_) => RawKind::Bool,
            Self::Int(_) => RawKind::Int,
            Self::Uint(_) => RawKind::Uint,
            Self::Float(_) => RawKind::Float,
            Self::Regs(_) => RawKind::Regs,
            Self::Bytes(_) => RawKind::Bytes,
            Self::Text(_) => RawKind::Text,
        }
    }

    /// The size in bytes of a `regs`, `bytes` or `text` payload, 0 otherwise.
    pub const fn payload_len(&self) -> usize {
        match self {
            Self::Regs(r) => r.len() * 2,
            Self::Bytes(b) => b.len(),
            Self::Text(t) => t.len(),
            _ => 0,
        }
    }
}

/// One sample delivered by a driver (ref/06 §2.8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawSample<'a> {
    /// The point.
    pub point: PointId,
    /// The raw value.
    pub value: RawValue<'a>,
    /// The quality the equipment reports.
    pub status: RawStatus,
    /// Equipment timestamp, UTC ns.
    pub ts_src: Option<i64>,
    /// Hardware timestamp, monotonic ns.
    pub ts_hw: Option<MonoNs>,
}

impl<'a> RawSample<'a> {
    /// A `good` sample without timestamps.
    pub const fn good(point: PointId, value: RawValue<'a>) -> Self {
        Self {
            point,
            value,
            status: RawStatus::OK,
            ts_src: None,
            ts_hw: None,
        }
    }

    /// A sample without a value and with `status`.
    pub const fn failed(point: PointId, status: RawStatus) -> Self {
        Self {
            point,
            value: RawValue::None,
            status,
            ts_src: None,
            ts_hw: None,
        }
    }
}

/// Link state a driver may report (ref/06 §2.12); `drift` and `stopped` are
/// the core's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LinkState {
    /// Before the first successful `connect`.
    Connecting = 0,
    /// Connected.
    Up = 1,
    /// Some batches fail.
    Degraded = 2,
    /// Not connected.
    Down = 3,
}

/// Direction of a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Direction {
    /// Read by the driver.
    Input = 0,
    /// Written by the core, never read cyclically.
    Output = 1,
    /// Parameter, read on demand by default.
    Param = 2,
}

/// How a point is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Poll {
    /// Every `n` ns.
    Period(u64),
    /// Only when asked.
    OnDemand,
    /// Pushed by the equipment.
    Push,
}

/// Who needs a point, for the bus scheduler's priorities (ref/06 §2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReadClass {
    /// Read by a cycle group.
    Cycle = 0,
    /// Telemetry only.
    Telemetry = 1,
    /// `joker.*` and parameters.
    Diagnostic = 2,
}

/// Kind of a batch (ref/06 §2.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BatchKind {
    /// Periodic.
    Poll = 0,
    /// On demand.
    OnDemand = 1,
    /// Pushed.
    Push = 2,
}

/// Why a write reaches the driver (ref/06 §2.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WriteKind {
    /// A commit of the owner group.
    Set = 0,
    /// Renewal of the equipment fallback.
    Keepalive = 1,
    /// The supervisor's `force_safe` bit.
    ForceSafe = 2,
    /// Hand control back to the equipment.
    Relinquish = 3,
    /// Resend of the applied value after `connect`.
    Resync = 4,
}

/// Position of a write in an atomic group (M20).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AtomicRef {
    /// The group.
    pub group: u32,
    /// Position in the declared order, `0..count`.
    pub index: u16,
    /// Writes in the group.
    pub count: u16,
}

/// The driver's answer to one write (ref/06 §2.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WriteStatus {
    /// Confirmed by the equipment.
    Ok = 0,
    /// No answer by the deadline.
    Timeout = 1,
    /// The transport failed.
    CommFailure = 2,
    /// The equipment refused the write.
    DeviceRejected = 3,
    /// Not connected.
    NotConnected = 4,
    /// The driver cannot do this write.
    Unsupported = 5,
    /// A timed write could not be applied on time.
    Late = 6,
    /// Not attempted: an earlier write of its atomic group failed.
    Skipped = 7,
}

/// Failure of a whole batch (ref/06 §2.8). `detail` is for humans and at
/// most 40 bytes on the JDP wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchFailure {
    /// No answer by the deadline.
    Timeout,
    /// The transport failed.
    CommFailure(&'static str),
    /// The equipment answered with a fault.
    DeviceFault(&'static str),
    /// Not connected.
    NotConnected,
    /// The request cannot be built from the configuration.
    ConfigError(&'static str),
}

impl BatchFailure {
    /// The JDP code of the variant.
    pub const fn code(&self) -> u8 {
        match self {
            Self::Timeout => 0,
            Self::CommFailure(_) => 1,
            Self::DeviceFault(_) => 2,
            Self::NotConnected => 3,
            Self::ConfigError(_) => 4,
        }
    }

    /// The quality the core gives to every point of the batch (ref/06 §2.8).
    pub const fn status(&self) -> RawStatus {
        match self {
            Self::Timeout | Self::CommFailure(_) | Self::NotConnected => {
                RawStatus::bad(Reason::COMM_FAILURE)
            }
            Self::DeviceFault(_) => RawStatus::bad(Reason::DEVICE_FAULT),
            Self::ConfigError(_) => RawStatus::bad(Reason::CONFIG_ERROR),
        }
    }

    /// True when the failure counts against the link (ref/06 §2.8).
    pub const fn counts_for_link(&self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::CommFailure(_) | Self::NotConnected
        )
    }
}

/// A registry code (ref/00 §8), such as `J0856`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code(&'static str);

impl Code {
    /// The code, if `id` is a `J` followed by four digits.
    pub const fn new(id: &'static str) -> Option<Self> {
        let b = id.as_bytes();
        if b.len() != 5 || b[0] != b'J' {
            return None;
        }
        let mut i = 1;
        while i < 5 {
            if !b[i].is_ascii_digit() {
                return None;
            }
            i += 1;
        }
        Some(Self(id))
    }

    /// The code text.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Builds a [`Code`] checked at compile time: `code!("J0856")`.
#[macro_export]
macro_rules! code {
    ($id:literal) => {{
        #[allow(clippy::panic)]
        const CODE: $crate::types::Code = match $crate::types::Code::new($id) {
            Some(code) => code,
            None => panic!("a registry code is `J` and four digits"),
        };
        CODE
    }};
}

/// Plan invalid (ref/06 §2.6).
pub const J0856: Code = code!("J0856");
/// Invalid sample from a driver (ref/06 §2.8).
pub const J0869: Code = code!("J0869");
/// Device configuration invalid for the driver's schema.
pub const J0852: Code = code!("J0852");
/// Point address invalid for the driver's schema.
pub const J0853: Code = code!("J0853");
/// A `type` the driver cannot deliver for the address (ref/03 §2.9).
pub const J0809: Code = code!("J0809");
/// Function code or address invalid for the direction (ref/03 §2.9).
pub const J0810: Code = code!("J0810");
/// Overlapping addresses (ref/03 §2.9).
pub const J0811: Code = code!("J0811");
/// A capability the plan needs and the driver lacks (ref/06 §2.13).
pub const J0854: Code = code!("J0854");

/// Fixed-capacity text for an error detail; longer text is cut at a UTF-8
/// boundary. No allocation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Detail {
    buf: [u8; Detail::CAPACITY],
    len: u8,
}

impl Detail {
    /// Bytes kept.
    pub const CAPACITY: usize = 64;

    /// An empty detail.
    pub const EMPTY: Self = Self {
        buf: [0; Self::CAPACITY],
        len: 0,
    };

    /// Copies `text`, cut to [`Detail::CAPACITY`] bytes.
    pub fn new(text: &str) -> Self {
        let mut end = text.len().min(Self::CAPACITY);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut buf = [0; Self::CAPACITY];
        buf[..end].copy_from_slice(&text.as_bytes()[..end]);
        Self {
            buf,
            len: end as u8,
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..usize::from(self.len)]).unwrap_or("")
    }
}

impl core::fmt::Debug for Detail {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_str(), f)
    }
}

/// An error with a registry code (ref/00 §8). No allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverError {
    /// The code.
    pub code: Code,
    /// For humans.
    pub detail: Detail,
}

impl DriverError {
    /// An error with `code` and `detail`.
    pub fn new(code: Code, detail: &str) -> Self {
        Self {
            code,
            detail: Detail::new(detail),
        }
    }
}

/// Failure of a native action (ref/06 §2.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionFailure {
    /// No answer by the deadline.
    Timeout,
    /// The equipment refused or failed.
    Device(DriverError),
    /// The arguments are invalid for the equipment.
    Invalid(DriverError),
    /// Not connected.
    NotConnected,
    /// The driver has no such action.
    Unsupported,
    /// Cancelled by the core.
    Cancelled,
}

/// A diagnostic of `plan` (ref/06 §2.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic<'a> {
    /// The code.
    pub code: Code,
    /// The point it is about.
    pub point: Option<PointId>,
    /// For humans.
    pub message: &'a str,
}

impl<'a> Diagnostic<'a> {
    /// A diagnostic about `point`.
    pub const fn at(code: Code, point: PointId, message: &'a str) -> Self {
        Self {
            code,
            point: Some(point),
            message,
        }
    }

    /// A diagnostic about the whole request.
    pub const fn general(code: Code, message: &'a str) -> Self {
        Self {
            code,
            point: None,
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_failures_map_to_the_qualities_of_the_table() {
        let comm = RawStatus::bad(Reason::COMM_FAILURE);
        assert_eq!(BatchFailure::Timeout.status(), comm);
        assert_eq!(BatchFailure::CommFailure("crc").status(), comm);
        assert_eq!(BatchFailure::NotConnected.status(), comm);
        assert_eq!(
            BatchFailure::DeviceFault("exception 2").status(),
            RawStatus::bad(Reason::DEVICE_FAULT)
        );
        assert_eq!(
            BatchFailure::ConfigError("fc").status(),
            RawStatus::bad(Reason::CONFIG_ERROR)
        );
        assert!(BatchFailure::Timeout.counts_for_link());
        assert!(!BatchFailure::DeviceFault("x").counts_for_link());
        assert_eq!(BatchFailure::ConfigError("x").code(), 4);
    }

    #[test]
    fn statuses_convert_to_core_qualities() {
        let q: Quality = RawStatus::bad(Reason::COMM_FAILURE).into();
        assert_eq!(q.code(), QualityCode::Bad);
        assert_eq!(q.reason(), QualityReason::CommFailure);
        assert_eq!(Quality::from(RawStatus::OK), Quality::GOOD);
    }

    #[test]
    fn only_the_pairs_of_the_table_are_allowed() {
        assert!(RawStatus::OK.is_allowed());
        assert!(RawStatus::uncertain(Reason::STALE).is_allowed());
        assert!(RawStatus::bad(Reason::NOT_CONNECTED).is_allowed());
        assert!(!RawStatus::bad(Reason(17)).is_allowed());
        assert!(!RawStatus::uncertain(Reason::COMM_FAILURE).is_allowed());
        assert!(
            RawStatus {
                code: QualityCode::Good,
                reason: Reason::private(3)
            }
            .is_allowed()
        );
    }

    #[test]
    fn codes_and_details() {
        assert_eq!(J0856.as_str(), "J0856");
        assert_eq!(Code::new("J085"), None);
        assert_eq!(Code::new("X0856"), None);
        let long = "é".repeat(40);
        let d = Detail::new(&long);
        assert_eq!(d.as_str().len(), 64);
        assert_eq!(
            DriverError::new(J0852, "unit_id").detail.as_str(),
            "unit_id"
        );
    }
}
