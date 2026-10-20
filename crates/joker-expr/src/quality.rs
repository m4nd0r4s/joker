use core::fmt;

/// The quality code of a sample (ref/00 §4.3). The order is the severity:
/// `Good < Uncertain < Bad`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(u8)]
pub enum QualityCode {
    /// `good`, 0.
    #[default]
    Good = 0,
    /// `uncertain`, 1.
    Uncertain = 1,
    /// `bad`, 2.
    Bad = 2,
}

/// The reason of a quality (ref/00 §4.3), kept as its numeric code so that
/// unknown and private reasons (20–255) pass through unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Reason(pub u8);

impl Reason {
    /// `none`.
    pub const NONE: Reason = Reason(0);
    /// `overridden`.
    pub const OVERRIDDEN: Reason = Reason(1);
    /// `local_override`.
    pub const LOCAL_OVERRIDE: Reason = Reason(2);
    /// `stale`.
    pub const STALE: Reason = Reason(3);
    /// `not_confirmed`.
    pub const NOT_CONFIRMED: Reason = Reason(4);
    /// `substituted`.
    pub const SUBSTITUTED: Reason = Reason(5);
    /// `out_of_distribution`.
    pub const OUT_OF_DISTRIBUTION: Reason = Reason(6);
    /// `sensor_cal`.
    pub const SENSOR_CAL: Reason = Reason(7);
    /// `clock_unsynced`.
    pub const CLOCK_UNSYNCED: Reason = Reason(8);
    /// `initial`.
    pub const INITIAL: Reason = Reason(9);
    /// `frozen`.
    pub const FROZEN: Reason = Reason(10);
    /// `partial_write`.
    pub const PARTIAL_WRITE: Reason = Reason(11);
    /// `comm_failure`.
    pub const COMM_FAILURE: Reason = Reason(12);
    /// `device_fault`.
    pub const DEVICE_FAULT: Reason = Reason(13);
    /// `out_of_range`.
    pub const OUT_OF_RANGE: Reason = Reason(14);
    /// `config_error`.
    pub const CONFIG_ERROR: Reason = Reason(15);
    /// `not_connected`.
    pub const NOT_CONNECTED: Reason = Reason(16);
    /// `nan`.
    pub const NAN: Reason = Reason(17);
    /// `tripped`.
    pub const TRIPPED: Reason = Reason(18);
    /// `deadline`.
    pub const DEADLINE: Reason = Reason(19);
}

/// A quality: code and reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Quality {
    /// The code.
    pub code: QualityCode,
    /// The reason.
    pub reason: Reason,
}

impl Quality {
    /// `good`.
    pub const GOOD: Quality = Quality::new(QualityCode::Good, Reason::NONE);
    /// `bad(nan)`, the quality of a non-finite value (docs/16 §4).
    pub const BAD_NAN: Quality = Quality::new(QualityCode::Bad, Reason::NAN);
    /// `uncertain(initial)`, a window that is not full yet (ADR 0034).
    pub const UNCERTAIN_INITIAL: Quality = Quality::new(QualityCode::Uncertain, Reason::INITIAL);

    /// Builds a quality.
    pub const fn new(code: QualityCode, reason: Reason) -> Quality {
        Quality { code, reason }
    }

    /// True for `good`.
    pub fn is_good(self) -> bool {
        self.code == QualityCode::Good
    }

    /// True for `bad`.
    pub fn is_bad(self) -> bool {
        self.code == QualityCode::Bad
    }

    /// The worse of two qualities; on a tie, `self`.
    pub fn worst(self, other: Quality) -> Quality {
        if other.code > self.code { other } else { self }
    }

    /// `self`, but never better than `code` with `reason`.
    pub fn at_most(self, code: QualityCode, reason: Reason) -> Quality {
        self.worst(Quality::new(code, reason))
    }

    /// The slot encoding of docs/01 §3.2: code in bits 0–7, reason in bits 8–15.
    pub fn to_bits(self) -> u32 {
        u32::from(self.code as u8) | (u32::from(self.reason.0) << 8)
    }

    /// Decodes the slot encoding; an unknown code counts as `bad`.
    pub fn from_bits(bits: u32) -> Quality {
        let code = match bits & 0xff {
            0 => QualityCode::Good,
            1 => QualityCode::Uncertain,
            _ => QualityCode::Bad,
        };
        Quality::new(code, Reason(((bits >> 8) & 0xff) as u8))
    }
}

impl fmt::Display for Quality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self.code {
            QualityCode::Good => "good",
            QualityCode::Uncertain => "uncertain",
            QualityCode::Bad => "bad",
        };
        if self.reason == Reason::NONE {
            f.write_str(code)
        } else {
            write!(f, "{code}({})", self.reason.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_and_bits() {
        let u = Quality::new(QualityCode::Uncertain, Reason::STALE);
        assert_eq!(Quality::GOOD.worst(u), u);
        assert_eq!(u.worst(Quality::BAD_NAN), Quality::BAD_NAN);
        assert_eq!(Quality::BAD_NAN.worst(u), Quality::BAD_NAN);
        for q in [Quality::GOOD, u, Quality::BAD_NAN] {
            assert_eq!(Quality::from_bits(q.to_bits()), q);
        }
        assert_eq!(Quality::from_bits(7).code, QualityCode::Bad);
        assert_eq!(
            Quality::GOOD.at_most(QualityCode::Uncertain, Reason::INITIAL),
            Quality::UNCERTAIN_INITIAL
        );
    }
}
