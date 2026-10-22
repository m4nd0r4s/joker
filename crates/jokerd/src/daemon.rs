//! The daemon process: boot the last confirmed generation (or the given
//! manifest), run the simulated devices on tokio, roll back unconfirmed
//! generations when their window ends, and stop cleanly on SIGTERM.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
}

impl Options {
    /// Parses `--state-dir <dir>` and `--manifest <entry>`.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut state_dir = PathBuf::from("/var/lib/joker");
        let mut entry = None;
        let mut socket = PathBuf::from(joker_api::server::DEFAULT_SOCKET);
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
                "--help" | "-h" => {
                    return Err(
                        "usage: jokerd [--state-dir DIR] [--socket PATH] [--manifest joker.toml]"
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
        })
    }
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

/// Builds the controller of a state directory, boots the last confirmed
/// generation, and, when there is none, proposes and activates `entry`.
pub fn controller(
    options: &Options,
    tasks: Arc<dyn TaskProvider>,
    devices: Arc<dyn Devices>,
    clock: Monotonic,
    utc_origin_ns: i64,
) -> Result<Controller, String> {
    let log = DecisionLog::open(&options.state_dir.join("decisions.jsonl"))
        .map_err(|e| format!("decision log: {e}"))?;
    let log = Arc::new(Mutex::new(log));
    let quality = SystemPacer::host_clock_quality();
    let mut c = Controller::new(
        &options.state_dir,
        log,
        tasks,
        devices,
        Box::new(move || Box::new(SystemPacer::new(clock, quality))),
        Config {
            utc_origin_ns,
            ..Config::default()
        },
    )
    .map_err(|e| e.to_string())?;
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

/// Runs the daemon until a termination signal.
pub fn run(options: Options, tasks: Arc<dyn TaskProvider>) -> Result<(), String> {
    std::fs::create_dir_all(&options.state_dir)
        .map_err(|e| format!("{}: {e}", options.state_dir.display()))?;
    if let Err(e) = lock_memory() {
        eprintln!("warning: {e}; the cycle thread may page");
    }
    let clock = Monotonic::start();
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
        RealDevices::new(runtime.handle().clone()),
    ));
    let utc_origin_ns = crate::wire::utc_now_ns().saturating_sub(clock.now_ns() as i64);
    let controller: SharedController = Arc::new(Mutex::new(controller(
        &options,
        tasks,
        devices,
        clock,
        utc_origin_ns,
    )?));
    let node = crate::api::Node::new(
        Arc::clone(&controller),
        clock,
        utc_origin_ns,
        &options.state_dir,
    )
    .map_err(|e| format!("traces: {e}"))?;
    let (stop_api, api_stopped) = tokio::sync::oneshot::channel::<()>();
    let socket = options.socket.clone();
    let api = runtime.spawn(async move {
        joker_api::server::serve_unix(&socket, Arc::new(node), async {
            let _ = api_stopped.await;
        })
        .await
    });
    let timer = Arc::clone(&controller);
    runtime.block_on(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| format!("signal: {e}"))?;
        let mut poll = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = term.recv() => break,
                _ = tokio::signal::ctrl_c() => break,
                _ = poll.tick() => {
                    let timer = Arc::clone(&timer);
                    let result = tokio::task::spawn_blocking(move || lock_controller(&timer).poll()).await;
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
    lock_controller(&controller).shutdown();
    Ok(())
}
