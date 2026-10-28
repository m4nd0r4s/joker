//! How the cycle thread reads time and waits for the next tick (ADR 0007).
//!
//! [`SystemPacer`] reads `CLOCK_MONOTONIC` and the wall clock and sleeps to
//! absolute deadlines. [`VirtualPacer`] jumps straight to each deadline, for
//! tests and replay: the same plan gives the same ticks on any machine.

use std::time::{Instant as StdInstant, SystemTime, UNIX_EPOCH};

use std::sync::Arc;

use joker_core::time::{ClockQuality, Instant};
use joker_hal::std_host::ClockQualityCell;

use crate::clock::{ClockPolicy, SyncSource, TimesyncFile, evaluate};

/// The cycle thread's source of time.
pub trait Pacer: Send {
    /// The current instant.
    fn now(&self) -> Instant;
    /// Waits until the monotonic time reaches `mono_ns`.
    fn wait_until(&mut self, mono_ns: u64);
    /// The real monotonic clock behind [`Pacer::now`], when there is one:
    /// `rt/supervisor` reads it to judge the tick stamps on the same
    /// origin. `None` for a virtual clock, which has no real time to
    /// supervise.
    fn monotonic(&self) -> Option<Monotonic> {
        None
    }

    /// A pacer on the same clock for another thread, when each cycle group
    /// may tick on its own (ADR 0089). `None` keeps every group on one
    /// thread: a virtual clock, which must stay deterministic.
    fn fork(&self) -> Option<Box<dyn Pacer>> {
        None
    }
}

/// The daemon's monotonic clock: nanoseconds since the daemon started,
/// shared by the cycle thread and the acquisition stages so that `ts_acq`
/// and latch instants are on one time base.
#[derive(Debug, Clone, Copy)]
pub struct Monotonic {
    origin: StdInstant,
}

impl Monotonic {
    /// A clock whose zero is now.
    pub fn start() -> Self {
        Self {
            origin: StdInstant::now(),
        }
    }

    /// The instant this clock reads as zero.
    pub fn origin(&self) -> StdInstant {
        self.origin
    }

    /// Nanoseconds since the origin.
    pub fn now_ns(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// The host's clocks.
#[derive(Debug)]
pub struct SystemPacer {
    mono: Monotonic,
    quality: Arc<ClockQualityCell>,
}

impl SystemPacer {
    /// A pacer on the daemon's monotonic clock, with the wall clock judged
    /// `quality` for good.
    pub fn new(mono: Monotonic, quality: ClockQuality) -> Self {
        Self::tracking(mono, Arc::new(ClockQualityCell::new(quality)))
    }

    /// A pacer on the daemon's monotonic clock whose instants carry the
    /// quality `quality` holds when each is read: one atomic load, so the
    /// cycle thread sees every re-evaluation of the clock (ADR 0069).
    pub fn tracking(mono: Monotonic, quality: Arc<ClockQualityCell>) -> Self {
        Self { mono, quality }
    }

    /// The wall clock quality the host reports now, under the default
    /// `[clock]` (docs/01 §18, ADR 0069). Reads a file: never call it on
    /// the tick path.
    pub fn host_clock_quality() -> ClockQuality {
        evaluate(
            TimesyncFile::default().last_sync_utc_ns(),
            crate::wire::utc_now_ns(),
            ClockPolicy::default(),
        )
    }
}

impl Pacer for SystemPacer {
    fn now(&self) -> Instant {
        let mono = self.mono.now_ns();
        let utc = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        Instant {
            mono_ns: mono,
            utc_ns: utc,
            clock: self.quality.get(),
        }
    }

    fn wait_until(&mut self, mono_ns: u64) {
        let now = self.now().mono_ns;
        if mono_ns > now {
            std::thread::sleep(std::time::Duration::from_nanos(mono_ns - now));
        }
    }

    fn monotonic(&self) -> Option<Monotonic> {
        Some(self.mono)
    }

    fn fork(&self) -> Option<Box<dyn Pacer>> {
        Some(Box::new(Self {
            mono: self.mono,
            quality: Arc::clone(&self.quality),
        }))
    }
}

/// A clock that moves only when the cycle thread waits.
#[derive(Debug, Clone, Copy)]
pub struct VirtualPacer {
    now: Instant,
}

impl VirtualPacer {
    /// A virtual clock at `start`.
    pub fn new(start: Instant) -> Self {
        Self { now: start }
    }
}

impl Pacer for VirtualPacer {
    fn now(&self) -> Instant {
        self.now
    }

    fn wait_until(&mut self, mono_ns: u64) {
        if mono_ns > self.now.mono_ns {
            let delta = mono_ns - self.now.mono_ns;
            self.now.mono_ns = mono_ns;
            self.now.utc_ns = self
                .now
                .utc_ns
                .saturating_add(i64::try_from(delta).unwrap_or(i64::MAX));
        }
    }
}
