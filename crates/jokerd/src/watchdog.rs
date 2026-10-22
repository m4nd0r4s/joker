//! Hardware watchdog feed and systemd's own software watchdog (docs/01
//! §8, ref/03 §2.3.1): keeps `/dev/watchdog*` and `sd_notify
//! WATCHDOG=1` fed while the active generation's `rt/supervisor` has
//! not found a stale group. Stops feeding the moment it has — the
//! intentional escalation that lets the board-level reset (or
//! systemd's `WatchdogSec`) recover what jokerd's own supervisor
//! could not (docs/01 §20's "Dead jokerd" row).
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

use crate::daemon::{SharedController, lock_controller};

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
    let Ok(socket_path) = std::env::var("NOTIFY_SOCKET") else {
        return;
    };
    let Ok(sock) = UnixDatagram::unbound() else {
        return;
    };
    let _ = sock.send_to(msg.as_bytes(), socket_path);
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
/// them, re-checking whichever generation is active at each feed).
#[derive(Debug)]
pub struct WatchdogFeeder {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl WatchdogFeeder {
    /// Starts feeding. `device` is the board's declared `[watchdog]`,
    /// if any (`BoardPeripherals::watchdog`, from the file `--board`
    /// named); without one, only `sd_notify` is fed, at
    /// [`DEFAULT_INTERVAL`]. A device that fails to open is a warning,
    /// not fatal: `sd_notify` still runs.
    pub fn spawn(watchdog: Option<Watchdog>, controller: SharedController) -> Self {
        let interval = feed_interval(watchdog.as_ref());
        let device_path = watchdog.and_then(|w| w.device).map(PathBuf::from);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("svc/watchdog".into())
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
                loop {
                    if flag.load(Ordering::Acquire) {
                        return;
                    }
                    let stale = lock_controller(&controller)
                        .current()
                        .is_some_and(|(_, _, r)| r.is_stale());
                    if stale {
                        eprintln!(
                            "jokerd: watchdog: the supervisor found a stale group; \
                             stopping the feed so the hardware/systemd watchdog can \
                             restart the daemon (AUR-274)"
                        );
                        return;
                    }
                    if let Some(f) = device.as_mut() {
                        let _ = f.write_all(&[0]);
                    }
                    sd_notify("WATCHDOG=1\n");
                    std::thread::sleep(interval);
                }
            })
            .ok();
        Self { stop, handle }
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

    #[test]
    fn sd_notify_without_notify_socket_does_not_panic() {
        // No assertion beyond "did not panic": there is no observable
        // side effect to check without a real systemd socket, and
        // $NOTIFY_SOCKET is unset in the test environment.
        sd_notify("WATCHDOG=1\n");
    }
}
