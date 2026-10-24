//! The daemon process: boot the last confirmed generation (or the given
//! manifest), run the simulated devices on tokio, roll back unconfirmed
//! generations when their window ends, and stop cleanly on SIGTERM.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::clock::{ClockMonitor, ClockPolicy};
use crate::control::{Config, Controller};
use crate::decisions::DecisionLog;
use crate::devices::{Combined, Devices, SimDevices};
use crate::pacer::{Monotonic, SystemPacer};
use crate::real_devices::RealDevices;
use crate::task::TaskProvider;

/// Command-line options of `jokerd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// State directory (decision log, generations).
    pub state_dir: PathBuf,
    /// Entry manifest to start with.
    pub entry: Option<PathBuf>,
    /// The API socket.
    pub socket: PathBuf,
    /// The node's board profile, whose `[watchdog]` is fed until the
    /// board is resolved from the generation's lock (ref/02 §2.2.3, ADR
    /// 0070).
    pub board: Option<PathBuf>,
}

impl Options {
    /// Parses `--state-dir <dir>`, `--socket <path>`, `--manifest
    /// <entry>` and `--board <file>` (ref/02 §2.2.3).
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut state_dir = PathBuf::from("/var/lib/joker");
        let mut entry = None;
        let mut socket = PathBuf::from(joker_api::server::DEFAULT_SOCKET);
        let mut board = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--state-dir" => {
                    state_dir = args.next().ok_or("--state-dir needs a directory")?.into();
                }
                "--socket" => {
                    socket = args.next().ok_or("--socket needs a path")?.into();
                }
                "--manifest" => {
                    entry = Some(args.next().ok_or("--manifest needs a file")?.into());
                }
                "--board" => {
                    board = Some(args.next().ok_or("--board needs a file")?.into());
                }
                "--help" | "-h" => {
                    return Err(
                        "usage: jokerd [--state-dir DIR] [--socket PATH] [--manifest joker.toml] [--board board.toml]"
                            .into(),
                    );
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Self {
            state_dir,
            entry,
            socket,
            board,
        })
    }
}

/// Reads and parses `options.board` into its declared `[watchdog]`, if
/// any. A missing file, a file that fails to parse, or no `--board` at
/// all are all `None`: feeding degrades to `sd_notify`-only rather than
/// failing the daemon over a peripheral it does not strictly need to
/// start (same leniency as a missing hardware watchdog device itself).
fn board_watchdog(options: &Options) -> Option<joker_manifest::board::Watchdog> {
    let path = options.board.as_ref()?;
    let text = std::fs::read_to_string(path)
        .inspect_err(|e| eprintln!("jokerd: --board {}: {e}", path.display()))
        .ok()?;
    joker_manifest::board::parse(path, &text)
        .peripherals
        .watchdog
}

/// Locks the process memory on Linux (docs/01 §2, AGENTS.md invariant 6).
pub fn lock_memory() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use nix::sys::mman::{MlockAllFlags, mlockall};
        mlockall(MlockAllFlags::MCL_CURRENT | MlockAllFlags::MCL_FUTURE)
            .map_err(|e| format!("mlockall: {e}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(())
    }
}

/// The controller of a daemon, shared by the API and the timers.
pub type SharedController = Arc<Mutex<Controller>>;

/// Locks the controller, recovering from a poisoned lock.
pub fn lock_controller(c: &SharedController) -> std::sync::MutexGuard<'_, Controller> {
    c.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Builds the controller of a state directory, with the Wasm apps of its
/// generations run from their artifacts and the node's AOT cache (ADR
/// 0067) ahead of `tasks`, boots the last confirmed generation, and, when
/// there is none, proposes and activates `entry`.
pub fn controller(
    options: &Options,
    tasks: Arc<dyn TaskProvider>,
    devices: Arc<dyn Devices>,
    clock: Monotonic,
    utc_origin_ns: i64,
    quality: Arc<ClockMonitor>,
) -> Result<Controller, String> {
    let log = DecisionLog::open(&options.state_dir.join("decisions.jsonl"))
        .map_err(|e| format!("decision log: {e}"))?;
    let log = Arc::new(Mutex::new(log));
    quality.refresh(ClockPolicy::default(), crate::wire::utc_now_ns());
    let cell = quality.cell();
    let c = Controller::new(
        &options.state_dir,
        log,
        tasks,
        devices,
        Box::new(move || Box::new(SystemPacer::tracking(clock, Arc::clone(&cell)))),
        Config {
            utc_origin_ns,
            clock: quality,
            ..Config::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let apps = crate::apps::NodeApps::open(&options.state_dir)
        .map_err(|e| format!("the Wasm engine of this node: {e}"))?;
    let mut c = c.with_apps(Arc::new(apps));
    if c.boot().map_err(|e| e.to_string())?.is_none()
        && let Some(entry) = &options.entry
    {
        let manifest =
            std::fs::read_to_string(entry).map_err(|e| format!("{}: {e}", entry.display()))?;
        let lock_path = entry.with_extension("lock");
        let lock = std::fs::read_to_string(lock_path).unwrap_or_default();
        let g = c
            .propose(&manifest, &lock, Some("initial manifest".into()), false)
            .map_err(|e| {
                format!(
                    "{e}\n{}",
                    e.errors
                        .iter()
                        .map(|(_, r)| r.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            })?;
        c.activate(g.number, None, None)
            .map_err(|e| e.to_string())?;
    }
    Ok(c)
}

/// Binds the API socket and only then calls `ready` (`sd_notify
/// READY=1`): a daemon is ready once clients can connect, and a socket
/// that cannot be bound is a startup error, not a silent background
/// failure. Must be called within a Tokio runtime.
pub fn bind_then_ready(
    socket: &std::path::Path,
    ready: impl FnOnce(),
) -> Result<tokio::net::UnixListener, String> {
    let listener = joker_api::server::bind_unix(socket)
        .map_err(|e| format!("api socket {}: {e}", socket.display()))?;
    ready();
    Ok(listener)
}

/// How many service-loop polls (100 ms each) pass between two
/// re-evaluations of the clock quality (ADR 0069): once a second.
pub const CLOCK_REFRESH_POLLS: u32 = 10;

/// Runs the daemon until a termination signal.
pub fn run(options: Options, tasks: Arc<dyn TaskProvider>) -> Result<(), String> {
    std::fs::create_dir_all(&options.state_dir)
        .map_err(|e| format!("{}: {e}", options.state_dir.display()))?;
    if let Err(e) = lock_memory() {
        eprintln!("warning: {e}; the cycle thread may page");
    }
    let clock = Monotonic::start();
    let quality = Arc::new(ClockMonitor::default());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("io")
        .enable_all()
        .build()
        .map_err(|e| format!("tokio: {e}"))?;
    let devices = Arc::new(Combined(
        SimDevices::new(
            runtime.handle().clone(),
            clock,
            std::time::Duration::from_millis(10),
        ),
        RealDevices::new(runtime.handle().clone()).with_clock(quality.cell()),
    ));
    let utc_origin_ns = crate::wire::utc_now_ns().saturating_sub(clock.now_ns() as i64);
    let controller: SharedController = Arc::new(Mutex::new(controller(
        &options,
        tasks,
        devices,
        clock,
        utc_origin_ns,
        quality,
    )?));
    let node = crate::api::Node::new(
        Arc::clone(&controller),
        clock,
        utc_origin_ns,
        &options.state_dir,
    )
    .map_err(|e| format!("node state: {e}"))?;
    let heartbeat = lock_controller(&controller).heartbeat();
    let watchdog = crate::watchdog::WatchdogFeeder::spawn(
        board_watchdog(&options),
        heartbeat,
        crate::watchdog::notify_socket(),
    )
    .map_err(|e| format!("cannot start the watchdog feed: {e}"))?;
    let (stop_api, api_stopped) = tokio::sync::oneshot::channel::<()>();
    let listener = runtime.block_on(async {
        bind_then_ready(&options.socket, || crate::watchdog::sd_notify("READY=1\n"))
    })?;
    let api = runtime.spawn(async move {
        joker_api::server::serve_listener(listener, Arc::new(node), async {
            let _ = api_stopped.await;
        })
        .await
    });
    let timer = Arc::clone(&controller);
    runtime.block_on(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| format!("signal: {e}"))?;
        let mut poll = tokio::time::interval(std::time::Duration::from_millis(100));
        let mut polls: u32 = 0;
        loop {
            tokio::select! {
                _ = term.recv() => break,
                _ = tokio::signal::ctrl_c() => break,
                _ = poll.tick() => {
                    let timer = Arc::clone(&timer);
                    polls = polls.wrapping_add(1);
                    let refresh = polls.is_multiple_of(CLOCK_REFRESH_POLLS);
                    let result = tokio::task::spawn_blocking(move || {
                        let mut c = lock_controller(&timer);
                        if refresh {
                            c.refresh_clock();
                        }
                        c.poll()
                    })
                    .await;
                    if let Ok(Err(e)) = result {
                        eprintln!("jokerd: {e}");
                    }
                }
            }
        }
        Ok::<(), String>(())
    })?;
    let _ = stop_api.send(());
    if let Ok(Err(e)) = runtime.block_on(api) {
        eprintln!("jokerd: api: {e}");
    }
    watchdog.stop();
    lock_controller(&controller).shutdown();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn board_defaults_to_none() {
        let o = Options::parse(args("--manifest joker.toml")).unwrap();
        assert_eq!(o.board, None);
    }

    #[test]
    fn board_is_parsed() {
        let o = Options::parse(args("--manifest joker.toml --board board.toml")).unwrap();
        assert_eq!(o.board, Some(PathBuf::from("board.toml")));
    }

    #[test]
    fn ready_is_signalled_only_after_the_socket_is_bound() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let socket = dir.path().join("run").join("api.sock");
        let mut bound_when_ready = None;
        let listener = runtime.block_on(async {
            bind_then_ready(&socket, || bound_when_ready = Some(socket.exists()))
        });
        assert!(listener.is_ok());
        assert_eq!(bound_when_ready, Some(true));

        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"").unwrap();
        let mut ready = false;
        let failed =
            runtime.block_on(async { bind_then_ready(&file.join("api.sock"), || ready = true) });
        assert!(failed.is_err());
        assert!(!ready);
    }

    #[test]
    fn board_without_a_file_is_a_usage_error() {
        assert!(Options::parse(args("--board")).is_err());
    }
}
