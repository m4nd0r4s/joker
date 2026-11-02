//! Time types of the SDK (ref/04 §4.7).

use core::ops::{Add, Sub};

pub use core::time::Duration;

/// Monotonic nanoseconds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct MonoNanos(pub u64);

/// UTC nanoseconds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct UtcNanos(pub i64);

/// UTC seconds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct UtcSeconds(pub i64);

impl UtcSeconds {
    /// The earliest.
    pub const MIN: Self = Self(i64::MIN);
    /// The latest.
    pub const MAX: Self = Self(i64::MAX);
}

impl Add<i64> for UtcSeconds {
    type Output = Self;
    fn add(self, s: i64) -> Self {
        Self(self.0.saturating_add(s))
    }
}

impl Sub<i64> for UtcSeconds {
    type Output = Self;
    fn sub(self, s: i64) -> Self {
        Self(self.0.saturating_sub(s))
    }
}

fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

impl Add<Duration> for MonoNanos {
    type Output = Self;
    fn add(self, d: Duration) -> Self {
        Self(self.0.saturating_add(nanos(d)))
    }
}

impl Sub<Duration> for MonoNanos {
    type Output = Self;
    fn sub(self, d: Duration) -> Self {
        Self(self.0.saturating_sub(nanos(d)))
    }
}

/// Quality of the node's wall clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockQuality {
    /// Synchronised.
    Synced,
    /// Was synchronised, now free-running.
    Holdover,
    /// Never synchronised in this boot.
    Unsynced,
}

impl ClockQuality {
    /// The clock quality from its code (ref/00 §6).
    pub fn from_u8(code: u8) -> Self {
        match code {
            0 => Self::Synced,
            1 => Self::Holdover,
            _ => Self::Unsynced,
        }
    }
}

/// An instant: monotonic, UTC and the clock quality.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Instant {
    /// Monotonic time.
    pub mono: MonoNanos,
    /// UTC time.
    pub utc: UtcNanos,
    /// Quality of `utc`.
    pub clock: ClockQuality,
}

impl Instant {
    /// The UTC time in whole seconds, rounded down.
    pub fn utc_seconds(&self) -> UtcSeconds {
        UtcSeconds(self.utc.0.div_euclid(1_000_000_000))
    }
}

impl Add<Duration> for Instant {
    type Output = Self;
    fn add(self, d: Duration) -> Self {
        let n = nanos(d);
        Self {
            mono: MonoNanos(self.mono.0.saturating_add(n)),
            utc: UtcNanos(
                self.utc
                    .0
                    .saturating_add(i64::try_from(n).unwrap_or(i64::MAX)),
            ),
            clock: self.clock,
        }
    }
}

impl Sub<Duration> for Instant {
    type Output = Self;
    fn sub(self, d: Duration) -> Self {
        let n = nanos(d);
        Self {
            mono: MonoNanos(self.mono.0.saturating_sub(n)),
            utc: UtcNanos(
                self.utc
                    .0
                    .saturating_sub(i64::try_from(n).unwrap_or(i64::MAX)),
            ),
            clock: self.clock,
        }
    }
}

impl Sub for Instant {
    type Output = Duration;
    fn sub(self, other: Self) -> Duration {
        Duration::from_nanos(self.mono.0.saturating_sub(other.mono.0))
    }
}

impl From<Instant> for MonoNanos {
    fn from(i: Instant) -> Self {
        i.mono
    }
}

/// Duration literals: `200.ms()`, `15.mins()`.
pub trait DurationExt {
    /// Nanoseconds.
    fn ns(self) -> Duration;
    /// Microseconds.
    fn us(self) -> Duration;
    /// Milliseconds.
    fn ms(self) -> Duration;
    /// Seconds.
    fn s(self) -> Duration;
    /// Minutes.
    fn mins(self) -> Duration;
    /// Hours.
    fn h(self) -> Duration;
    /// Days.
    fn d(self) -> Duration;
}

impl DurationExt for u64 {
    fn ns(self) -> Duration {
        Duration::from_nanos(self)
    }
    fn us(self) -> Duration {
        Duration::from_micros(self)
    }
    fn ms(self) -> Duration {
        Duration::from_millis(self)
    }
    fn s(self) -> Duration {
        Duration::from_secs(self)
    }
    fn mins(self) -> Duration {
        Duration::from_secs(self.saturating_mul(60))
    }
    fn h(self) -> Duration {
        Duration::from_secs(self.saturating_mul(3600))
    }
    fn d(self) -> Duration {
        Duration::from_secs(self.saturating_mul(86_400))
    }
}

fn from_secs_f64(s: f64) -> Duration {
    if s.is_nan() || s <= 0.0 {
        Duration::ZERO
    } else {
        Duration::try_from_secs_f64(s).unwrap_or(Duration::MAX)
    }
}

impl DurationExt for f64 {
    fn ns(self) -> Duration {
        from_secs_f64(self * 1e-9)
    }
    fn us(self) -> Duration {
        from_secs_f64(self * 1e-6)
    }
    fn ms(self) -> Duration {
        from_secs_f64(self * 1e-3)
    }
    fn s(self) -> Duration {
        from_secs_f64(self)
    }
    fn mins(self) -> Duration {
        from_secs_f64(self * 60.0)
    }
    fn h(self) -> Duration {
        from_secs_f64(self * 3600.0)
    }
    fn d(self) -> Duration {
        from_secs_f64(self * 86_400.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals() {
        assert_eq!(200.ms(), Duration::from_millis(200));
        assert_eq!(15.mins(), Duration::from_secs(900));
        assert_eq!(1.5.s(), Duration::from_millis(1500));
        assert_eq!((-1.0).s(), Duration::ZERO);
        assert_eq!(f64::NAN.ms(), Duration::ZERO);
        assert_eq!(1e300.s(), Duration::MAX);
    }

    #[test]
    fn instants_move_on_both_clocks() {
        let now = Instant {
            mono: MonoNanos(5_000_000_000),
            utc: UtcNanos(1_790_244_000_500_000_000),
            clock: ClockQuality::Synced,
        };
        let before = now - 24.h();
        assert_eq!(before.mono, MonoNanos(0));
        assert_eq!(before.utc_seconds(), UtcSeconds(1_790_244_000 - 86_400));
        assert_eq!(now - before, Duration::from_secs(5));
        assert_eq!(now.utc_seconds() + 30 * 60, UtcSeconds(1_790_245_800));
        assert!(UtcSeconds(-1) < UtcSeconds(0));
        assert_eq!(MonoNanos(1) - 1.s(), MonoNanos(0));
    }
}
