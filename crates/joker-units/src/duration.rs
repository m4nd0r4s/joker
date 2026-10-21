use core::fmt;
use core::str::FromStr;

use crate::error::{Error, ErrorKind};
use crate::number;

/// A non-negative duration in nanoseconds (ref/00 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration {
    nanos: u64,
}

const NS_PER_US: u64 = 1_000;
const NS_PER_MS: u64 = 1_000_000;
const NS_PER_S: u64 = 1_000_000_000;
const NS_PER_MIN: u64 = 60 * NS_PER_S;
const NS_PER_H: u64 = 60 * NS_PER_MIN;
const NS_PER_D: u64 = 24 * NS_PER_H;

const UNITS: [(&str, u64); 10] = [
    ("ns", 1),
    ("us", NS_PER_US),
    ("\u{b5}s", NS_PER_US),
    ("\u{3bc}s", NS_PER_US),
    ("ms", NS_PER_MS),
    ("s", NS_PER_S),
    ("m", NS_PER_MIN),
    ("min", NS_PER_MIN),
    ("h", NS_PER_H),
    ("d", NS_PER_D),
];

const CANONICAL: [(&str, u64); 7] = [
    ("d", NS_PER_D),
    ("h", NS_PER_H),
    ("m", NS_PER_MIN),
    ("s", NS_PER_S),
    ("ms", NS_PER_MS),
    ("us", NS_PER_US),
    ("ns", 1),
];

const MAX_FRACTION_DIGITS: usize = 24;

impl Duration {
    /// The zero duration.
    pub const ZERO: Duration = Duration { nanos: 0 };
    /// The longest window of an expression (docs/16 §4).
    pub const MAX_WINDOW: Duration = Duration::from_secs(15 * 60);

    /// A duration of `nanos` nanoseconds.
    pub const fn from_nanos(nanos: u64) -> Duration {
        Duration { nanos }
    }

    /// A duration of `micros` microseconds, saturating.
    pub const fn from_micros(micros: u64) -> Duration {
        Duration::from_nanos(micros.saturating_mul(NS_PER_US))
    }

    /// A duration of `millis` milliseconds, saturating.
    pub const fn from_millis(millis: u64) -> Duration {
        Duration::from_nanos(millis.saturating_mul(NS_PER_MS))
    }

    /// A duration of `secs` seconds, saturating.
    pub const fn from_secs(secs: u64) -> Duration {
        Duration::from_nanos(secs.saturating_mul(NS_PER_S))
    }

    /// The duration in nanoseconds.
    pub const fn as_nanos(self) -> u64 {
        self.nanos
    }

    /// The duration in seconds, as `f64`.
    pub fn as_secs_f64(self) -> f64 {
        self.nanos as f64 / NS_PER_S as f64
    }

    /// True for the zero duration.
    pub const fn is_zero(self) -> bool {
        self.nanos == 0
    }

    /// The sum, or `None` on overflow.
    pub const fn checked_add(self, other: Duration) -> Option<Duration> {
        match self.nanos.checked_add(other.nanos) {
            Some(nanos) => Some(Duration { nanos }),
            None => None,
        }
    }

    /// Parses a duration literal: `number " "? unit`, with `unit` one of
    /// `ns us µs ms s m min h d` and `m` meaning minutes. Decimals are
    /// rounded to the nearest nanosecond, halves away from zero. There is
    /// no sign and no compound form (`"1h30m"`).
    pub fn parse(text: &str) -> Result<Duration, Error> {
        if text.is_empty() {
            return Err(Error::at(ErrorKind::Empty, 0, 0));
        }
        let scanned = number::scan(text, false, false).ok_or(Error::at(
            ErrorKind::InvalidDuration,
            0,
            text.len(),
        ))?;
        let mut unit_start = scanned.end;
        if text.get(unit_start..unit_start + 1) == Some(" ") {
            unit_start += 1;
        }
        let unit_text = text.get(unit_start..).unwrap_or("");
        let scale = UNITS
            .iter()
            .find(|(s, _)| *s == unit_text)
            .map(|(_, ns)| *ns)
            .ok_or(Error::at(
                ErrorKind::InvalidDuration,
                unit_start,
                text.len(),
            ))?;
        let overflow = Error::at(ErrorKind::DurationOverflow, 0, text.len());
        let int: u128 = text
            .get(..scanned.int_end)
            .and_then(|d| d.parse().ok())
            .ok_or(overflow)?;
        let mut total = int.checked_mul(u128::from(scale)).ok_or(overflow)?;
        if let Some((start, end)) = scanned.frac {
            let digits = text.get(start..end).unwrap_or("");
            let kept = digits
                .get(..digits.len().min(MAX_FRACTION_DIGITS))
                .unwrap_or("");
            let numerator: u128 = kept.parse().map_err(|_| overflow)?;
            let denominator = 10u128.pow(u32::try_from(kept.len()).unwrap_or(0));
            let scaled = numerator * u128::from(scale);
            let rounded = (scaled + denominator / 2) / denominator;
            total = total.checked_add(rounded).ok_or(overflow)?;
        }
        let nanos = u64::try_from(total).map_err(|_| overflow)?;
        Ok(Duration { nanos })
    }

    /// Parses an instant of a scenario or conformance case: one or more
    /// durations joined by `+` (`"2m+1.2s"`, ref/00 §3.1).
    pub fn parse_sum(text: &str) -> Result<Duration, Error> {
        let mut total = Duration::ZERO;
        let mut offset = 0usize;
        for term in text.split('+') {
            let d = Duration::parse(term).map_err(|e| Error {
                span: e.span.shifted(u32::try_from(offset).unwrap_or(u32::MAX)),
                ..e
            })?;
            total = total.checked_add(d).ok_or(Error::at(
                ErrorKind::DurationOverflow,
                0,
                text.len(),
            ))?;
            offset += term.len() + 1;
        }
        Ok(total)
    }
}

impl From<Duration> for core::time::Duration {
    fn from(d: Duration) -> core::time::Duration {
        core::time::Duration::from_nanos(d.nanos)
    }
}

impl FromStr for Duration {
    type Err = Error;

    fn from_str(s: &str) -> Result<Duration, Error> {
        Duration::parse(s)
    }
}

/// The canonical form: an integer in the largest exact unit, with `m` for
/// minutes (`"90m"`, `"1500ms"`, `"2h"`, `"0s"`).
impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.nanos == 0 {
            return f.write_str("0s");
        }
        for (unit, ns) in CANONICAL {
            if self.nanos.is_multiple_of(ns) {
                return write!(f, "{}{}", self.nanos / ns, unit);
            }
        }
        write!(f, "{}ns", self.nanos)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    fn d(s: &str) -> Duration {
        Duration::parse(s).unwrap()
    }

    #[test]
    fn units() {
        assert_eq!(d("200ms"), d("200 ms"));
        assert_eq!(d("200ms"), Duration::from_millis(200));
        assert_eq!(d("1m"), Duration::from_secs(60));
        assert_eq!(d("1min"), Duration::from_secs(60));
        assert_eq!(d("2h"), Duration::from_secs(7200));
        assert_eq!(d("1d"), Duration::from_secs(86400));
        assert_eq!(d("5us"), d("5µs"));
        assert_eq!(d("5us"), d("5\u{3bc}s"));
        assert_eq!(d("7ns").as_nanos(), 7);
        assert_eq!(d("0s"), Duration::ZERO);
        assert_eq!(d("15m"), Duration::MAX_WINDOW);
    }

    #[test]
    fn decimals_round_to_ns() {
        assert_eq!(d("1.5s"), Duration::from_millis(1500));
        assert_eq!(d("0.0000000005s").as_nanos(), 1);
        assert_eq!(d("0.0000000004s").as_nanos(), 0);
        assert_eq!(d("1.5ns").as_nanos(), 2);
        assert_eq!(d("0.1h"), Duration::from_secs(360));
    }

    #[test]
    fn rejects() {
        for s in [
            "", "200", "-1s", "1h30m", "1 h ", "1.s", ".5s", "1e3s", "1  s", "1 sec", "5 kW",
        ] {
            let e = Duration::parse(s).unwrap_err();
            assert!(matches!(e.code(), "J0152" | "J0151"), "{s:?}");
        }
        assert_eq!(
            Duration::parse("99999999999d").unwrap_err().kind,
            ErrorKind::DurationOverflow
        );
    }

    #[test]
    fn sums() {
        assert_eq!(
            Duration::parse_sum("2m+1.2s").unwrap(),
            Duration::from_millis(121_200)
        );
        assert_eq!(Duration::parse_sum("0ms").unwrap(), Duration::ZERO);
        let e = Duration::parse_sum("1s+2x").unwrap_err();
        assert_eq!(e.span.range(), 4..5);
    }

    #[test]
    fn canonical_form() {
        assert_eq!(Duration::from_secs(5400).to_string(), "90m");
        assert_eq!(Duration::from_millis(1500).to_string(), "1500ms");
        assert_eq!(Duration::from_secs(7200).to_string(), "2h");
        assert_eq!(Duration::ZERO.to_string(), "0s");
        assert_eq!(Duration::from_nanos(1001).to_string(), "1001ns");
        assert_eq!(Duration::from_secs(86400 * 2).to_string(), "2d");
        for s in ["90m", "1500ms", "2h", "3d", "7us", "13ns", "45s"] {
            assert_eq!(d(s).to_string(), s);
        }
    }
}
