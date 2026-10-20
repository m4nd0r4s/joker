//! Time as an input of the core (ADR 0007, docs/01 §18, ref/00 §3).

/// Quality of the node's wall clock, with the numeric codes of ref/00 §6.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ClockQuality {
    /// Synchronised (NTP, PTP or GNSS).
    Synced = 0,
    /// Was synchronised and is now free-running.
    Holdover = 1,
    /// Never synchronised in this boot.
    Unsynced = 2,
}

impl ClockQuality {
    /// Decodes a numeric clock quality.
    pub const fn from_u8(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Synced),
            1 => Some(Self::Holdover),
            2 => Some(Self::Unsynced),
            _ => None,
        }
    }

    /// The canonical lower-case name (ref/00 §3).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::Holdover => "holdover",
            Self::Unsynced => "unsynced",
        }
    }
}

/// One reading of the clock: monotonic time, its UTC translation and the
/// quality of that translation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Instant {
    /// Monotonic nanoseconds (`CLOCK_MONOTONIC` on Linux).
    pub mono_ns: u64,
    /// UTC nanoseconds since 1970-01-01T00:00:00Z.
    pub utc_ns: i64,
    /// Quality of `utc_ns`.
    pub clock: ClockQuality,
}

impl Instant {
    /// An instant with only a monotonic time and an unsynchronised wall clock.
    pub const fn mono(mono_ns: u64) -> Self {
        Self {
            mono_ns,
            utc_ns: 0,
            clock: ClockQuality::Unsynced,
        }
    }
}

/// The only way time enters the core (ADR 0007). The host reads its clocks;
/// tests and replay use [`VirtualClock`].
pub trait Clock {
    /// The current instant.
    fn now(&self) -> Instant;
}

/// A clock that only moves when told to, for tests, conformance and replay.
#[derive(Debug)]
pub struct VirtualClock {
    now: core::cell::Cell<Instant>,
}

impl VirtualClock {
    /// A clock stopped at `start`.
    pub const fn new(start: Instant) -> Self {
        Self {
            now: core::cell::Cell::new(start),
        }
    }

    /// Moves the monotonic and UTC time forward by `ns`.
    pub fn advance(&self, ns: u64) {
        let now = self.now.get();
        self.now.set(Instant {
            mono_ns: now.mono_ns.saturating_add(ns),
            utc_ns: now
                .utc_ns
                .saturating_add(i64::try_from(ns).unwrap_or(i64::MAX)),
            clock: now.clock,
        });
    }

    /// Sets the wall clock quality.
    pub fn set_quality(&self, clock: ClockQuality) {
        let now = self.now.get();
        self.now.set(Instant { clock, ..now });
    }

    /// Sets the whole instant.
    pub fn set(&self, instant: Instant) {
        self.now.set(instant);
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> Instant {
        self.now.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_clock_moves_only_when_told() {
        let clock = VirtualClock::new(Instant {
            mono_ns: 10,
            utc_ns: 1_000,
            clock: ClockQuality::Synced,
        });
        assert_eq!(clock.now().mono_ns, 10);
        clock.advance(5);
        assert_eq!(clock.now().mono_ns, 15);
        assert_eq!(clock.now().utc_ns, 1_005);
        clock.set_quality(ClockQuality::Holdover);
        assert_eq!(clock.now().clock, ClockQuality::Holdover);
    }

    #[test]
    fn clock_quality_codes() {
        for code in 0..3 {
            assert_eq!(ClockQuality::from_u8(code).map(|q| q as u8), Some(code));
        }
        assert_eq!(ClockQuality::from_u8(3), None);
    }
}
