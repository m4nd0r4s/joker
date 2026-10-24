//! The node's clock quality (M14, docs/01 §18, ADR 0069).
//!
//! `jokerd` does not synchronise the clock: it reads when the system's
//! time daemon last synchronised it ([`SyncSource`]) and derives the
//! quality from how long ago that was. [`ClockMonitor::refresh`] runs on
//! the daemon's service loop, never on the tick path; every clock that
//! stamps instants (the cycle pacer, the device hosts, the API) reads the
//! one [`ClockQualityCell`] it updates.

use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, UNIX_EPOCH};

use joker_core::ClockQuality;
use joker_hal::std_host::ClockQualityCell;
use joker_manifest::model::Manifest;
use joker_manifest::value::Value;

/// How long after the last synchronisation the clock is still `synced`:
/// twice the longest poll interval of `systemd-timesyncd` (2048 s), so a
/// daemon that is still synchronising always refreshes it in time.
pub const SYNC_FRESHNESS: Duration = Duration::from_secs(2 * 2048);

/// The default `[clock] max_holdover` (ref/01 §5.5).
pub const DEFAULT_MAX_HOLDOVER: Duration = Duration::from_secs(24 * 3600);

/// The file `systemd-timesyncd` touches on every accepted sample.
pub const TIMESYNC_FILE: &str = "/run/systemd/timesync/synchronized";

/// When the system clock was last synchronised.
pub trait SyncSource: Send + Sync + Debug {
    /// The UTC time of the last synchronisation, in ns, or `None` if the
    /// clock has not been synchronised since the machine booted.
    fn last_sync_utc_ns(&self) -> Option<i64>;
}

/// `systemd-timesyncd`'s state: the modification time of
/// [`TIMESYNC_FILE`], which lives on a tmpfs and so is absent until the
/// first synchronisation of each boot.
#[derive(Debug, Clone)]
pub struct TimesyncFile {
    path: PathBuf,
}

impl TimesyncFile {
    /// The source at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl Default for TimesyncFile {
    fn default() -> Self {
        Self::at(TIMESYNC_FILE)
    }
}

impl SyncSource for TimesyncFile {
    fn last_sync_utc_ns(&self) -> Option<i64> {
        let modified = std::fs::metadata(&self.path).ok()?.modified().ok()?;
        let since_epoch = modified.duration_since(UNIX_EPOCH).ok()?;
        i64::try_from(since_epoch.as_nanos()).ok()
    }
}

/// What the running generation's `[clock]` says about the quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockPolicy {
    /// False with `sync = "none"`: the quality is always `unsynced`.
    pub sync: bool,
    /// `max_holdover`, in ns.
    pub max_holdover_ns: u64,
}

impl Default for ClockPolicy {
    fn default() -> Self {
        Self {
            sync: true,
            max_holdover_ns: duration_ns(DEFAULT_MAX_HOLDOVER),
        }
    }
}

impl ClockPolicy {
    /// The policy of `manifest`'s `[clock]`, with the defaults of ref/01
    /// §5.5 for what it does not say.
    pub fn of(manifest: &Manifest) -> Self {
        let mut policy = Self::default();
        let Some(clock) = manifest.pending.get("clock").and_then(|c| c.as_table()) else {
            return policy;
        };
        if let Some(Value::String(sync)) = clock.get("sync").map(|s| &s.node.value) {
            policy.sync = sync != "none";
        }
        if let Some(Value::String(text)) = clock.get("max_holdover").map(|s| &s.node.value)
            && let Ok(ns) = crate::wire::parse_duration(text)
        {
            policy.max_holdover_ns = ns;
        }
        policy
    }
}

fn duration_ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// The quality of a clock last synchronised at `last_sync_utc_ns`, at
/// `now_utc_ns`: `synced` within [`SYNC_FRESHNESS`] of it, `holdover`
/// up to `max_holdover` after it, `unsynced` after that, before any
/// synchronisation, or with `sync = "none"` (ADR 0069).
pub fn evaluate(
    last_sync_utc_ns: Option<i64>,
    now_utc_ns: i64,
    policy: ClockPolicy,
) -> ClockQuality {
    let Some(last) = last_sync_utc_ns.filter(|_| policy.sync) else {
        return ClockQuality::Unsynced;
    };
    let age_ns = u64::try_from(now_utc_ns.saturating_sub(last)).unwrap_or(0);
    if age_ns <= duration_ns(SYNC_FRESHNESS) {
        ClockQuality::Synced
    } else if age_ns <= policy.max_holdover_ns {
        ClockQuality::Holdover
    } else {
        ClockQuality::Unsynced
    }
}

/// The clock as last evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockState {
    /// The quality.
    pub quality: ClockQuality,
    /// The last synchronisation, UTC ns; with `holdover`, when the
    /// holdover started counting.
    pub last_sync_utc_ns: Option<i64>,
}

/// A change of the clock quality found by [`ClockMonitor::refresh`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    /// The quality before.
    pub from: ClockQuality,
    /// The quality now.
    pub to: ClockQuality,
}

/// The node's clock quality, re-evaluated from a [`SyncSource`].
#[derive(Debug)]
pub struct ClockMonitor {
    cell: Arc<ClockQualityCell>,
    source: Box<dyn SyncSource>,
    last_sync_utc_ns: Mutex<Option<i64>>,
}

impl Default for ClockMonitor {
    fn default() -> Self {
        Self::new(Box::new(TimesyncFile::default()))
    }
}

impl ClockMonitor {
    /// A monitor over `source`, `unsynced` until its first refresh.
    pub fn new(source: Box<dyn SyncSource>) -> Self {
        Self {
            cell: Arc::new(ClockQualityCell::new(ClockQuality::Unsynced)),
            source,
            last_sync_utc_ns: Mutex::new(None),
        }
    }

    /// The cell every clock of the node reads its quality from.
    pub fn cell(&self) -> Arc<ClockQualityCell> {
        Arc::clone(&self.cell)
    }

    /// The quality as last evaluated.
    pub fn quality(&self) -> ClockQuality {
        self.cell.get()
    }

    /// The clock as last evaluated.
    pub fn state(&self) -> ClockState {
        ClockState {
            quality: self.cell.get(),
            last_sync_utc_ns: *self.lock(),
        }
    }

    /// Reads the source and re-evaluates the quality at `now_utc_ns`
    /// under `policy`; returns the transition when the quality changed.
    pub fn refresh(&self, policy: ClockPolicy, now_utc_ns: i64) -> Option<Transition> {
        let last = self.source.last_sync_utc_ns();
        *self.lock() = last;
        let to = evaluate(last, now_utc_ns, policy);
        let from = self.cell.get();
        self.cell.set(to);
        (from != to).then_some(Transition { from, to })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<i64>> {
        self.last_sync_utc_ns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i64 = 1_000_000_000;
    const NOW: i64 = 1_800_000_000 * S;

    #[derive(Debug)]
    struct At(Option<i64>);

    impl SyncSource for At {
        fn last_sync_utc_ns(&self) -> Option<i64> {
            self.0
        }
    }

    #[test]
    fn never_synchronised_is_unsynced() {
        assert_eq!(
            evaluate(None, NOW, ClockPolicy::default()),
            ClockQuality::Unsynced
        );
    }

    #[test]
    fn a_recent_synchronisation_is_synced() {
        assert_eq!(
            evaluate(Some(NOW - 60 * S), NOW, ClockPolicy::default()),
            ClockQuality::Synced
        );
        assert_eq!(
            evaluate(Some(NOW + 5 * S), NOW, ClockPolicy::default()),
            ClockQuality::Synced
        );
    }

    #[test]
    fn a_stale_synchronisation_is_holdover_until_max_holdover() {
        let policy = ClockPolicy::default();
        assert_eq!(
            evaluate(Some(NOW - 2 * 3600 * S), NOW, policy),
            ClockQuality::Holdover
        );
        assert_eq!(
            evaluate(Some(NOW - 24 * 3600 * S), NOW, policy),
            ClockQuality::Holdover
        );
        assert_eq!(
            evaluate(Some(NOW - 24 * 3600 * S - 1), NOW, policy),
            ClockQuality::Unsynced
        );
    }

    #[test]
    fn sync_none_is_always_unsynced() {
        let policy = ClockPolicy {
            sync: false,
            ..ClockPolicy::default()
        };
        assert_eq!(evaluate(Some(NOW), NOW, policy), ClockQuality::Unsynced);
    }

    #[test]
    fn the_policy_comes_from_the_clock_section() {
        let mut fs = joker_manifest::MemoryFileSystem::default();
        fs.file(
            "joker.toml",
            "[project]\nname = \"t\"\njoker = \"1.0\"\nmode = \"dev\"\n\n[clock]\nsync = \"none\"\nmax_holdover = \"2h\"\n",
        );
        let loaded = crate::load::load(
            &fs,
            std::path::Path::new("joker.toml"),
            &crate::native::NativeTasks,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let policy = ClockPolicy::of(&loaded.manifest);
        assert!(!policy.sync);
        assert_eq!(policy.max_holdover_ns, 2 * 3600 * 1_000_000_000);
    }

    #[test]
    fn a_refresh_reports_each_change_once() {
        let monitor = ClockMonitor::new(Box::new(At(Some(NOW))));
        let policy = ClockPolicy::default();
        assert_eq!(
            monitor.refresh(policy, NOW),
            Some(Transition {
                from: ClockQuality::Unsynced,
                to: ClockQuality::Synced
            })
        );
        assert_eq!(monitor.refresh(policy, NOW + S), None);
        assert_eq!(
            monitor.refresh(policy, NOW + 2 * 3600 * S),
            Some(Transition {
                from: ClockQuality::Synced,
                to: ClockQuality::Holdover
            })
        );
        assert_eq!(monitor.state().last_sync_utc_ns, Some(NOW));
        assert_eq!(monitor.cell().get(), ClockQuality::Holdover);
    }

    #[test]
    fn the_timesync_file_is_read_by_its_modification_time() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("synchronized");
        let source = TimesyncFile::at(&path);
        assert_eq!(source.last_sync_utc_ns(), None);
        std::fs::write(&path, b"").unwrap_or_else(|e| panic!("{e}"));
        let last = source.last_sync_utc_ns().unwrap_or(0);
        assert!((crate::wire::utc_now_ns() - last).abs() < 60 * S, "{last}");
    }
}
