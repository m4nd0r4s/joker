//! Sample quality: the three codes, the reasons and the packed word of ref/00 §4.3.

/// Quality code, the low byte of the quality word (ref/00 §4.3).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum QualityCode {
    /// The value can be used as is.
    Good = 0,
    /// The value exists but is not fully trusted.
    Uncertain = 1,
    /// The value must not be used as a number.
    Bad = 2,
}

impl QualityCode {
    /// Decodes a numeric quality code.
    pub const fn from_u8(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Good),
            1 => Some(Self::Uncertain),
            2 => Some(Self::Bad),
            _ => None,
        }
    }

    /// The canonical lower-case name used in TOML and JSON (ref/00 §2).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Uncertain => "uncertain",
            Self::Bad => "bad",
        }
    }
}

/// Quality reason, the second byte of the quality word (ref/00 §4.3).
///
/// Values 20–239 are reserved for future reasons and 240–255 are private to
/// drivers; both are kept as [`QualityReason::Other`] so that no information is
/// lost when a sample is copied inside the node.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QualityReason {
    /// No particular reason.
    None,
    /// Value forced in simulation or test.
    Overridden,
    /// The device is in local mode.
    LocalOverride,
    /// The sample is older than `max_age`.
    Stale,
    /// The readback did not confirm the written value.
    NotConfirmed,
    /// Replaced by a declared fallback.
    Substituted,
    /// Model input out of distribution.
    OutOfDistribution,
    /// Sensor in calibration.
    SensorCal,
    /// The timestamp depends on an unsynchronised clock.
    ClockUnsynced,
    /// No value since startup.
    Initial,
    /// Bit-identical value for longer than `stuck_detect`.
    Frozen,
    /// Atomic group partially written.
    PartialWrite,
    /// Communication with the device failed.
    CommFailure,
    /// The device reports a fault.
    DeviceFault,
    /// Outside the declared physical limits.
    OutOfRange,
    /// Configuration error.
    ConfigError,
    /// The port or signal is not connected.
    NotConnected,
    /// The value is not a number.
    Nan,
    /// The output is tripped.
    Tripped,
    /// A pipeline or model missed its deadline.
    Deadline,
    /// A reserved or driver-private reason, kept verbatim.
    Other(u8),
}

impl QualityReason {
    /// Decodes a numeric reason; every byte value is accepted.
    pub const fn from_u8(reason: u8) -> Self {
        match reason {
            0 => Self::None,
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
            other => Self::Other(other),
        }
    }

    /// The numeric reason of ref/00 §4.3.
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Overridden => 1,
            Self::LocalOverride => 2,
            Self::Stale => 3,
            Self::NotConfirmed => 4,
            Self::Substituted => 5,
            Self::OutOfDistribution => 6,
            Self::SensorCal => 7,
            Self::ClockUnsynced => 8,
            Self::Initial => 9,
            Self::Frozen => 10,
            Self::PartialWrite => 11,
            Self::CommFailure => 12,
            Self::DeviceFault => 13,
            Self::OutOfRange => 14,
            Self::ConfigError => 15,
            Self::NotConnected => 16,
            Self::Nan => 17,
            Self::Tripped => 18,
            Self::Deadline => 19,
            Self::Other(other) => other,
        }
    }

    /// The canonical lower-case name, or `None` for reserved and private values.
    pub const fn name(self) -> Option<&'static str> {
        Some(match self {
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
            Self::Other(_) => return None,
        })
    }
}

/// The packed quality word of a slot: code in bits 0–7, reason in bits 8–15 and
/// flags in bits 16–31 (docs/01 §3.2, ref/00 §4.3).
///
/// The word is carried verbatim through slots, the process image and the trace.
/// A word whose code byte is not a known code is treated as [`QualityCode::Bad`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Quality(u32);

impl Quality {
    /// `good`, reason `none`, no flags.
    pub const GOOD: Self = Self::new(QualityCode::Good, QualityReason::None);
    /// `uncertain(initial)`: the quality of a slot that never received a sample
    /// (ref/04 §3.2).
    pub const INITIAL: Self = Self::new(QualityCode::Uncertain, QualityReason::Initial);
    /// `bad(not_connected)`: the quality of an unbound port (ref/04 §3.2).
    pub const NOT_CONNECTED: Self = Self::new(QualityCode::Bad, QualityReason::NotConnected);

    /// Builds a quality word without flags.
    pub const fn new(code: QualityCode, reason: QualityReason) -> Self {
        Self(code as u32 | ((reason.to_u8() as u32) << 8))
    }

    /// Builds a quality word with flags.
    pub const fn with_flags(code: QualityCode, reason: QualityReason, flags: u16) -> Self {
        Self(Self::new(code, reason).0 | ((flags as u32) << 16))
    }

    /// Wraps a raw quality word, as found in a slot or a trace.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The raw quality word.
    pub const fn to_bits(self) -> u32 {
        self.0
    }

    /// The quality code; an unknown code byte reads as `Bad`.
    pub const fn code(self) -> QualityCode {
        match QualityCode::from_u8((self.0 & 0xFF) as u8) {
            Some(code) => code,
            None => QualityCode::Bad,
        }
    }

    /// The quality reason.
    pub const fn reason(self) -> QualityReason {
        QualityReason::from_u8(((self.0 >> 8) & 0xFF) as u8)
    }

    /// The flag bits 16–31, carried verbatim.
    pub const fn flags(self) -> u16 {
        (self.0 >> 16) as u16
    }

    /// True when the code is `Good`.
    pub const fn is_good(self) -> bool {
        matches!(self.code(), QualityCode::Good)
    }

    /// True when the code is `Bad`.
    pub const fn is_bad(self) -> bool {
        matches!(self.code(), QualityCode::Bad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_round_trip_through_every_byte() {
        for byte in 0..=u8::MAX {
            assert_eq!(QualityReason::from_u8(byte).to_u8(), byte);
        }
    }

    #[test]
    fn known_reasons_have_names_and_reserved_do_not() {
        assert_eq!(QualityReason::from_u8(12).name(), Some("comm_failure"));
        assert_eq!(QualityReason::from_u8(19).name(), Some("deadline"));
        assert_eq!(QualityReason::from_u8(20).name(), None);
        assert_eq!(QualityReason::from_u8(240).name(), None);
    }

    #[test]
    fn word_layout_matches_ref00() {
        let q = Quality::with_flags(QualityCode::Bad, QualityReason::CommFailure, 0xA5A5);
        assert_eq!(q.to_bits(), 0xA5A5_0C02);
        assert_eq!(q.code(), QualityCode::Bad);
        assert_eq!(q.reason(), QualityReason::CommFailure);
        assert_eq!(q.flags(), 0xA5A5);
    }

    #[test]
    fn unknown_code_reads_as_bad() {
        assert_eq!(Quality::from_bits(0x07).code(), QualityCode::Bad);
        assert!(Quality::from_bits(0x07).is_bad());
    }

    #[test]
    fn initial_quality_is_uncertain_initial() {
        assert_eq!(Quality::INITIAL.code(), QualityCode::Uncertain);
        assert_eq!(Quality::INITIAL.reason(), QualityReason::Initial);
    }
}
