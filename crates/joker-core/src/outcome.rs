//! Write outcomes with the numeric codes of ref/00 §4.4 and docs/02 §7.1.

use core::fmt;

use crate::output::Level;

/// Why a write was rejected (ref/00 §4.4).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RejectReason {
    /// NaN, infinite, or not a value of the output's type (docs/16 E10).
    WrongType = 1,
    /// The permissive failed.
    Interlock = 2,
    /// A `requires_good` dependency is not good (docs/16 E4).
    Quality = 3,
    /// Out of range with `on_range = "reject"`.
    Range = 4,
    /// The atomic group did not pass as a whole.
    AtomicGroup = 5,
    /// The writer does not own the level (docs/16 A12, A13).
    NotOwner = 6,
    /// The writer requires a synchronised clock (docs/16 E1).
    ClockUnsynced = 7,
    /// The actuation budget is spent.
    Budget = 8,
}

impl RejectReason {
    const ALL: [Self; 8] = [
        Self::WrongType,
        Self::Interlock,
        Self::Quality,
        Self::Range,
        Self::AtomicGroup,
        Self::NotOwner,
        Self::ClockUnsynced,
        Self::Budget,
    ];

    /// Decodes a numeric reason.
    pub fn from_u8(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|r| *r as u8 == code)
    }

    /// The canonical lower-case name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::WrongType => "wrong_type",
            Self::Interlock => "interlock",
            Self::Quality => "quality",
            Self::Range => "range",
            Self::AtomicGroup => "atomic_group",
            Self::NotOwner => "not_owner",
            Self::ClockUnsynced => "clock_unsynced",
            Self::Budget => "budget",
        }
    }
}

/// The result of a write, or of an output in a tick (ADR 0033).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// No proposal in this tick.
    None,
    /// Applied as proposed.
    Accepted,
    /// Applied, limited by the range.
    ClampedRange,
    /// Applied, limited by the rate.
    ClampedRate,
    /// Within the deadband or `min_interval`; nothing sent.
    Suppressed,
    /// A higher level is active, or the writer proposed again in the tick.
    Overridden(Level),
    /// Async write waiting for the owner group's tick.
    Deferred,
    /// Recorded, not applied: the app is in shadow mode.
    Shadowed,
    /// Refused.
    Rejected(RejectReason),
    /// The output is, or just went, in trip.
    Tripped,
}

impl Outcome {
    /// The outcome code of ref/00 §4.4.
    pub const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Accepted => 1,
            Self::ClampedRange => 2,
            Self::ClampedRate => 3,
            Self::Suppressed => 4,
            Self::Overridden(_) => 5,
            Self::Deferred => 6,
            Self::Shadowed => 7,
            Self::Rejected(_) => 8,
            Self::Tripped => 9,
        }
    }

    /// The `detail` byte of the cycle ABI (ref/04 §3.2): the reason of a
    /// rejection, the level of an override, otherwise 0.
    pub const fn detail(self) -> u8 {
        match self {
            Self::Rejected(reason) => reason as u8,
            Self::Overridden(level) => level.get(),
            _ => 0,
        }
    }

    /// Rebuilds an outcome from its code and detail.
    pub fn from_code(code: u8, detail: u8) -> Option<Self> {
        Some(match code {
            0 => Self::None,
            1 => Self::Accepted,
            2 => Self::ClampedRange,
            3 => Self::ClampedRate,
            4 => Self::Suppressed,
            5 => Self::Overridden(Level::new(detail)?),
            6 => Self::Deferred,
            7 => Self::Shadowed,
            8 => Self::Rejected(RejectReason::from_u8(detail)?),
            9 => Self::Tripped,
            _ => return None,
        })
    }

    /// The canonical name of the outcome code (`"clamped_rate"`, `"rejected"`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Accepted => "accepted",
            Self::ClampedRange => "clamped_range",
            Self::ClampedRate => "clamped_rate",
            Self::Suppressed => "suppressed",
            Self::Overridden(_) => "overridden",
            Self::Deferred => "deferred",
            Self::Shadowed => "shadowed",
            Self::Rejected(_) => "rejected",
            Self::Tripped => "tripped",
        }
    }

    /// True for `rejected(…)`.
    pub const fn is_rejected(self) -> bool {
        matches!(self, Self::Rejected(_))
    }

    /// Parses an outcome literal: the canonical lower-case forms
    /// (`"clamped_rate"`, `"rejected(range)"`) and the CLI forms
    /// (`"Clamped(Rate)"`, `"Rejected(NotOwner)"`), which are exact synonyms
    /// (ref/00 §2). `"overridden"` takes an optional level
    /// (`"overridden(manual:4)"`); without one, the level is `None` and only
    /// the code is meant.
    pub fn parse(text: &str) -> Option<(Self, bool)> {
        let (head, arg) = match text.split_once('(') {
            Some((head, rest)) => (head, Some(rest.strip_suffix(')')?)),
            None => (text, None),
        };
        let mut key = [0u8; 32];
        let head = squash(head, &mut key)?;
        let simple = |outcome| match arg {
            None => Some((outcome, true)),
            Some(_) => None,
        };
        match head {
            b"none" => simple(Self::None),
            b"accepted" => simple(Self::Accepted),
            b"clampedrange" => simple(Self::ClampedRange),
            b"clampedrate" => simple(Self::ClampedRate),
            b"suppressed" => simple(Self::Suppressed),
            b"deferred" => simple(Self::Deferred),
            b"shadowed" => simple(Self::Shadowed),
            b"tripped" => simple(Self::Tripped),
            b"clamped" => {
                let mut arg_key = [0u8; 32];
                match squash(arg?, &mut arg_key)? {
                    b"range" => Some((Self::ClampedRange, true)),
                    b"rate" => Some((Self::ClampedRate, true)),
                    _ => None,
                }
            }
            b"overridden" => match arg {
                None => Some((Self::Overridden(Level::MANUAL_DEFAULT), false)),
                Some(level) => Some((Self::Overridden(Level::parse(level)?), true)),
            },
            b"rejected" => {
                let mut arg_key = [0u8; 32];
                let arg = squash(arg?, &mut arg_key)?;
                RejectReason::ALL
                    .into_iter()
                    .find(|r| {
                        let mut name_key = [0u8; 32];
                        squash(r.name(), &mut name_key) == Some(arg)
                    })
                    .map(|r| (Self::Rejected(r), true))
            }
            _ => None,
        }
    }
}

fn squash<'k>(text: &str, key: &'k mut [u8; 32]) -> Option<&'k [u8]> {
    let mut len = 0;
    for b in text.trim().bytes().filter(|b| *b != b'_') {
        *key.get_mut(len)? = b.to_ascii_lowercase();
        len += 1;
    }
    Some(&key[..len])
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(reason) => write!(f, "rejected({})", reason.name()),
            Self::Overridden(level) => write!(f, "overridden({level})"),
            other => f.write_str(other.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    #[test]
    fn codes_round_trip() {
        let all = [
            Outcome::None,
            Outcome::Accepted,
            Outcome::ClampedRange,
            Outcome::ClampedRate,
            Outcome::Suppressed,
            Outcome::Overridden(Level::MANUAL_DEFAULT),
            Outcome::Deferred,
            Outcome::Shadowed,
            Outcome::Rejected(RejectReason::ClockUnsynced),
            Outcome::Tripped,
        ];
        for (i, o) in all.into_iter().enumerate() {
            assert_eq!(o.code() as usize, i);
            assert_eq!(Outcome::from_code(o.code(), o.detail()), Some(o));
        }
    }

    #[test]
    fn parse_canonical_and_cli_forms() {
        assert_eq!(
            Outcome::parse("clamped_rate"),
            Some((Outcome::ClampedRate, true))
        );
        assert_eq!(
            Outcome::parse("Clamped(Rate)"),
            Some((Outcome::ClampedRate, true))
        );
        assert_eq!(
            Outcome::parse("clamped(range)"),
            Some((Outcome::ClampedRange, true))
        );
        assert_eq!(
            Outcome::parse("rejected(not_owner)"),
            Some((Outcome::Rejected(RejectReason::NotOwner), true))
        );
        assert_eq!(
            Outcome::parse("Rejected(NotOwner)"),
            Some((Outcome::Rejected(RejectReason::NotOwner), true))
        );
        assert_eq!(
            Outcome::parse("overridden(manual:3)"),
            Some((Outcome::Overridden(Level::new(3).unwrap()), true))
        );
        assert_eq!(Outcome::parse("overridden").map(|(o, _)| o.code()), Some(5));
        assert_eq!(Outcome::parse("none"), Some((Outcome::None, true)));
        assert_eq!(Outcome::parse("rejected"), None);
        assert_eq!(Outcome::parse("accepted(x)"), None);
        assert_eq!(Outcome::parse("bogus"), None);
    }

    #[test]
    fn display_is_canonical() {
        assert_eq!(
            Outcome::Rejected(RejectReason::Range).to_string(),
            "rejected(range)"
        );
        assert_eq!(Outcome::ClampedRate.to_string(), "clamped_rate");
        assert_eq!(
            Outcome::Overridden(Level::new(13).unwrap()).to_string(),
            "overridden(optimizer:13)"
        );
    }
}
