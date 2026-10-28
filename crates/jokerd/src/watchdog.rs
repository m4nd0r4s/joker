//! Hardware watchdog feed and systemd's own software watchdog (docs/01
//! §8, ref/03 §2.3.1): keeps `/dev/watchdog*` and `sd_notify
//! WATCHDOG=1` fed while the running generation's `rt/supervisor` beats
//! the daemon's [`Heartbeat`] and has not found a stale group (ADR
//! 0061). A supervisor that stops beating (dead, or starved) skips the
//! feed; one that forced `safe` stops it for good — the intentional
//! escalation that lets the board-level reset (or systemd's
//! `WatchdogSec`) recover what jokerd's own supervisor could not
//! (docs/01 §20's "Dead jokerd" row). The feed reads only the
//! heartbeat's atomics: it never takes the controller's lock.
//!
//! The board a node runs on is not resolved from the manifest/lock at
//! runtime anywhere in this codebase yet (that pipeline — `[target]
//! board`, `joker.lock`'s `[boards."<board>"]`, the content-addressed
//! artifact store, ref/02 §2.1.4/§2.2.4 — is not built). Rather than
//! build that whole resolution chain as a side effect of this feature,
//! `jokerd` takes an explicit `--board <file>` flag (ref/03 §2.3, same
//! shape as `--manifest`); a future board-resolution feature can
//! replace how this module *gets* a `Watchdog`, without changing what
//! it does with one.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use joker_manifest::board::Watchdog;

use crate::supervisor::{Feed, HEARTBEAT_MAX_AGE, Heartbeat};

/// Feeds at least this often when the board declares no `[watchdog]`
/// (only `sd_notify` has anything to feed then): matches the
/// `WatchdogSec=10` of `dist/jokerd.service` at a 1/3 margin.
const DEFAULT_INTERVAL: Duration = Duration::from_secs(3);

/// Sends `msg` to `$NOTIFY_SOCKET` (systemd's `sd_notify` protocol): a
/// no-op when the daemon was not started under `systemd` with
/// `Type=notify` (no `$NOTIFY_SOCKET`) — pure `std`, no `unsafe`: a
/// `SOCK_DGRAM` Unix socket send, nothing `jokerd`'s own
/// `unsafe_code = "forbid"` needs an exception for.
pub fn sd_notify(msg: &str) {
    if let Some(socket) = notify_socket() {
        sd_notify_to(&socket, msg);
    }
}

/// `$NOTIFY_SOCKET`, when the daemon runs under `systemd` with
/// `Type=notify`.
pub fn notify_socket() -> Option<PathBuf> {
    std::env::var_os("NOTIFY_SOCKET").map(PathBuf::from)
}

/// Sends `msg` to the `sd_notify` socket at `socket`, best-effort.
pub fn sd_notify_to(socket: &Path, msg: &str) {
    let Ok(sock) = UnixDatagram::unbound() else {
        return;
    };
    let _ = sock.send_to(msg.as_bytes(), socket);
}

/// How often to feed, from the board's declared `timeout`/`max_timeout`
/// (a third of the lower of the two, the same margin `jk board probe`
/// documents for `timeout` itself, ref/03 §2.3.7): or [`DEFAULT_INTERVAL`]
/// without a parseable one.
fn feed_interval(watchdog: Option<&Watchdog>) -> Duration {
    let Some(w) = watchdog else {
        return DEFAULT_INTERVAL;
    };
    let ns = [w.timeout.as_deref(), w.max_timeout.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(|t| crate::wire::parse_duration(t).ok())
        .min();
    match ns {
        Some(ns) => Duration::from_nanos(ns / 3).max(Duration::from_millis(100)),
        None => DEFAULT_INTERVAL,
    }
}

/// The `svc/watchdog` thread of a daemon (not of one generation: the
/// board does not change across a generation switch, so this spans
/// them, reading the heartbeat every generation's supervisor beats).
#[derive(Debug)]
pub struct WatchdogFeeder {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl WatchdogFeeder {
    /// Starts feeding. `watchdog` is the board's declared `[watchdog]`,
    /// if any (`BoardPeripherals::watchdog`, from the file `--board`
    /// named); `notify` is the `sd_notify` socket, if any. Without a
    /// device, only `sd_notify` is fed, at [`DEFAULT_INTERVAL`]. A device
    /// that fails to open is a warning, not fatal: `sd_notify` still
    /// runs. A thread that cannot be started is an error.
    pub fn spawn(
        watchdog: Option<Watchdog>,
        heartbeat: Arc<Heartbeat>,
        notify: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let interval = feed_interval(watchdog.as_ref());
        let device_path = watchdog.and_then(|w| w.device).map(PathBuf::from);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("svc/watchdog".into())
            .stack_size(crate::shared::SERVICE_STACK)
            .spawn(move || {
                let mut device = device_path.as_deref().and_then(open_watchdog);
                if let Some(path) = &device_path
                    && device.is_none()
                {
                    eprintln!(
                        "jokerd: watchdog: could not open {}; feeding sd_notify only",
                        path.display()
                    );
                }
                let mut skipping = false;
                while !flag.load(Ordering::Acquire) {
                    match heartbeat.feed(HEARTBEAT_MAX_AGE) {
                        Feed::Stop => {
                            eprintln!(
                                "jokerd: watchdog: the supervisor found a stale group; \
                                 stopping the feed so the hardware/systemd watchdog can \
                                 restart the daemon (AUR-274)"
                            );
                            return;
                        }
                        Feed::Skip => {
                            if !skipping {
                                eprintln!(
                                    "jokerd: watchdog: the supervisor has not beaten in \
                                     {HEARTBEAT_MAX_AGE:?}; not feeding (ADR 0061)"
                                );
                            }
                            skipping = true;
                        }
                        Feed::Feed => {
                            skipping = false;
                            if let Some(f) = device.as_mut() {
                                let _ = f.write_all(&[0]);
                            }
                            if let Some(socket) = &notify {
                                sd_notify_to(socket, "WATCHDOG=1\n");
                            }
                        }
                    }
                    std::thread::sleep(interval);
                }
            })?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Stops feeding and waits for the thread to end.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for WatchdogFeeder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn open_watchdog(path: &Path) -> Option<std::fs::File> {
    OpenOptions::new().write(true).open(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_systemd_unit_waits_for_ready_and_is_fed_three_times_per_watchdog_period() {
        let unit = include_str!("../dist/jokerd.service");
        assert!(
            unit.lines()
                .all(|l| !l.trim_start().starts_with('#') && !l.trim_start().starts_with(';')),
            "the unit carries no comments"
        );
        let key = |name: &str| {
            unit.lines()
                .find_map(|l| l.strip_prefix(name)?.strip_prefix('='))
                .unwrap_or_else(|| panic!("no {name} in the unit"))
        };
        assert_eq!(key("Type"), "notify");
        assert_eq!(key("NotifyAccess"), "main");
        assert_eq!(key("RuntimeDirectory"), "joker");
        let watchdog: u64 = key("WatchdogSec").parse().unwrap_or(0);
        assert!(
            Duration::from_secs(watchdog) >= 3 * DEFAULT_INTERVAL,
            "{watchdog}"
        );
    }

    #[test]
    fn without_a_board_watchdog_the_default_interval_applies() {
        assert_eq!(feed_interval(None), DEFAULT_INTERVAL);
    }

    #[test]
    fn the_interval_is_a_third_of_the_lower_declared_timeout() {
        let w = Watchdog {
            device: None,
            timeout: Some("15s".to_owned()),
            min_timeout: None,
            max_timeout: Some("30s".to_owned()),
        };
        assert_eq!(feed_interval(Some(&w)), Duration::from_secs(5));
    }

    #[test]
    fn an_unparseable_timeout_falls_back_to_the_default() {
        let w = Watchdog {
            device: None,
            timeout: Some("not a duration".to_owned()),
            min_timeout: None,
            max_timeout: None,
        };
        assert_eq!(feed_interval(Some(&w)), DEFAULT_INTERVAL);
    }

    fn fast_board() -> Option<Watchdog> {
        Some(Watchdog {
            device: None,
            timeout: Some("300ms".to_owned()),
            min_timeout: None,
            max_timeout: None,
        })
    }

    fn listener(dir: &tempfile::TempDir) -> (UnixDatagram, PathBuf) {
        let path = dir.path().join("notify");
        let sock = UnixDatagram::bind(&path).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        (sock, path)
    }

    fn feeds_within(sock: &UnixDatagram, window: Duration) -> usize {
        let start = std::time::Instant::now();
        let mut buf = [0u8; 64];
        let mut n = 0;
        while start.elapsed() < window {
            if let Ok(len) = sock.recv(&mut buf) {
                assert_eq!(&buf[..len], b"WATCHDOG=1\n");
                n += 1;
            }
        }
        n
    }

    #[test]
    fn with_no_supervisor_running_the_feed_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let (sock, path) = listener(&dir);
        let feeder =
            WatchdogFeeder::spawn(fast_board(), Arc::new(Heartbeat::new()), Some(path)).unwrap();
        assert!(feeds_within(&sock, Duration::from_millis(400)) >= 2);
        feeder.stop();
    }

    #[test]
    fn a_supervisor_that_stops_beating_stops_the_feed() {
        let dir = tempfile::tempdir().unwrap();
        let (sock, path) = listener(&dir);
        let heartbeat = Arc::new(Heartbeat::new());
        heartbeat.arm();
        let beats = Arc::clone(&heartbeat);
        let supervisor = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_millis(400) {
                beats.beat();
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let feeder =
            WatchdogFeeder::spawn(fast_board(), Arc::clone(&heartbeat), Some(path)).unwrap();
        assert!(feeds_within(&sock, Duration::from_millis(350)) >= 2);
        supervisor.join().unwrap();
        feeds_within(&sock, HEARTBEAT_MAX_AGE + Duration::from_millis(150));
        assert_eq!(feeds_within(&sock, Duration::from_millis(400)), 0);
        heartbeat.beat();
        assert!(feeds_within(&sock, Duration::from_millis(300)) >= 1);
        feeder.stop();
    }

    #[test]
    fn a_forced_safe_stops_the_feed_for_good() {
        let dir = tempfile::tempdir().unwrap();
        let (sock, path) = listener(&dir);
        let heartbeat = Arc::new(Heartbeat::new());
        heartbeat.force();
        let feeder =
            WatchdogFeeder::spawn(fast_board(), Arc::clone(&heartbeat), Some(path)).unwrap();
        assert_eq!(feeds_within(&sock, Duration::from_millis(300)), 0);
        feeder.stop();
    }
}
