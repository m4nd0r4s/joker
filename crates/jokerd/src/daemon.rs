//! The daemon process: load the manifest, start its generation, run the
//! simulated devices on tokio, and stop cleanly on SIGTERM or SIGINT.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::cycle::CycleError;
use crate::decisions::DecisionLog;
use crate::io::{DeviceIo, SimDevice};
use crate::load::{instantiate, load};
use crate::pacer::{Monotonic, SystemPacer};
use crate::runner::{Running, Start};
use crate::task::TaskProvider;
use crate::trace_ring::DEFAULT_RETENTION;

/// Command-line options of `jokerd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// State directory (decision log, generations).
    pub state_dir: PathBuf,
    /// Entry manifest to start with.
    pub entry: Option<PathBuf>,
}

impl Options {
    /// Parses `--state-dir <dir>` and `--manifest <entry>`.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut state_dir = PathBuf::from("/var/lib/joker");
        let mut entry = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--state-dir" => {
                    state_dir = args.next().ok_or("--state-dir needs a directory")?.into();
                }
                "--manifest" => {
                    entry = Some(args.next().ok_or("--manifest needs a file")?.into());
                }
                "--help" | "-h" => {
                    return Err(
                        "usage: jokerd [--state-dir DIR] [--manifest joker.<env>.toml]".into(),
                    );
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Self { state_dir, entry })
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

/// Starts the manifest at `entry` as generation 1 with the host clocks.
pub fn start_entry(
    entry: &Path,
    tasks: &dyn TaskProvider,
    log: crate::decisions::SharedLog,
    clock: Monotonic,
) -> Result<Running, String> {
    let fs = joker_manifest::OsFileSystem;
    let loaded = load(&fs, entry, tasks).map_err(|e| e.to_string())?;
    let groups = instantiate(&loaded.plan, &loaded.manifest, tasks).map_err(|e| e.to_string())?;
    let pacer = Box::new(SystemPacer::new(clock, SystemPacer::host_clock_quality()));
    Running::start(
        Arc::new(loaded.plan),
        groups,
        pacer,
        log,
        Start {
            generation: 1,
            inherit: Default::default(),
            retention: DEFAULT_RETENTION,
            max_ticks: None,
        },
    )
    .map_err(|e: CycleError| e.0)
}

/// Runs the daemon until a termination signal.
pub fn run(options: Options, tasks: &dyn TaskProvider) -> Result<(), String> {
    std::fs::create_dir_all(&options.state_dir)
        .map_err(|e| format!("{}: {e}", options.state_dir.display()))?;
    if let Err(e) = lock_memory() {
        eprintln!("warning: {e}; the cycle thread may page");
    }
    let log = DecisionLog::open(&options.state_dir.join("decisions.jsonl"))
        .map_err(|e| format!("decision log: {e}"))?;
    let log = Arc::new(Mutex::new(log));
    let clock = Monotonic::start();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("io")
        .enable_all()
        .build()
        .map_err(|e| format!("tokio: {e}"))?;
    let running = match &options.entry {
        Some(entry) => Some(start_entry(entry, tasks, Arc::clone(&log), clock)?),
        None => None,
    };
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    if let Some(r) = &running {
        let sims: Vec<String> = r
            .plan
            .signals
            .iter()
            .filter_map(|s| s.device.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for device in sims {
            let io = DeviceIo::new(&r.plan, Arc::clone(&r.shared), clock, &device);
            runtime.spawn(
                SimDevice::new(io).run(std::time::Duration::from_millis(10), stop_rx.clone()),
            );
        }
    }
    runtime.block_on(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| format!("signal: {e}"))?;
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        Ok::<(), String>(())
    })?;
    let _ = stop_tx.send(true);
    if let Some(r) = running {
        r.stop().map_err(|e| e.0)?;
    }
    Ok(())
}
