//! `jk dev`: the manifest on a local runtime with simulated devices,
//! reloaded as a new generation on every save (docs/07 §2).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use jokerd::control::{Config, Controller};
use jokerd::daemon::lock_controller;
use jokerd::decisions::DecisionLog;
use jokerd::devices::SimDevices;
use jokerd::pacer::{Monotonic, SystemPacer};
use jokerd::task::TaskProvider;

use crate::Failure;
use crate::project::{self, Entry};

fn stamp(files: &[std::path::PathBuf]) -> Vec<Option<SystemTime>> {
    files
        .iter()
        .map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
        .collect()
}

fn apply(c: &mut Controller, entry: &Entry, tasks: &dyn TaskProvider) {
    let report = project::check(entry, tasks);
    if report.has_errors() {
        eprintln!("{}", report.render());
        eprintln!("✗ not applied: the running generation stays");
        return;
    }
    let manifest = match project::effective_toml(entry) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("✗ cannot render the effective manifest: {e}");
            return;
        }
    };
    match c
        .propose(
            &manifest,
            &project::lock_text(entry),
            Some("jk dev".into()),
            false,
        )
        .and_then(|g| c.activate(g.number, None, None))
    {
        Ok(s) => println!(
            "✓ generation {} running ({})",
            s.generation.number,
            &s.generation.hash[..19.min(s.generation.hash.len())]
        ),
        Err(e) => {
            eprintln!("{e}");
            for (_, rendered) in e.errors {
                if !rendered.is_empty() {
                    eprintln!("{rendered}");
                }
            }
        }
    }
}

/// Runs until Ctrl-C.
pub fn run(entry: &Entry, socket: &Path) -> Result<u8, Failure> {
    let dir = Path::new(".joker/dev");
    let io = |e: std::io::Error| Failure::usage(format!("{}: {e}", dir.display()));
    std::fs::create_dir_all(dir).map_err(io)?;
    let clock = Monotonic::start();
    let utc_origin_ns = jokerd::wire::utc_now_ns().saturating_sub(clock.now_ns() as i64);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Failure::usage(format!("tokio: {e}")))?;
    let log = DecisionLog::open(&dir.join("decisions.jsonl")).map_err(io)?;
    let tasks: Arc<dyn TaskProvider> = Arc::new(project::tasks(entry)?);
    let controller = Controller::new(
        dir,
        Arc::new(Mutex::new(log)),
        Arc::clone(&tasks),
        Arc::new(SimDevices::new(
            runtime.handle().clone(),
            clock,
            Duration::from_millis(10),
        )),
        Box::new(move || Box::new(SystemPacer::new(clock, SystemPacer::host_clock_quality()))),
        Config {
            utc_origin_ns,
            ..Config::default()
        },
    )
    .map_err(|e| Failure::usage(e.to_string()))?;
    let controller = Arc::new(Mutex::new(controller));
    apply(&mut lock_controller(&controller), entry, tasks.as_ref());
    let node =
        jokerd::api::Node::new(Arc::clone(&controller), clock, utc_origin_ns, dir).map_err(io)?;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let socket_path = socket.to_owned();
    let api = runtime.spawn(async move {
        joker_api::server::serve_unix(&socket_path, Arc::new(node), async {
            let _ = stopped.await;
        })
        .await
    });
    println!("local runtime on unix:{}", socket.display());
    println!("  jk --node unix:{} read '**'", socket.display());
    let project_dir = entry
        .manifest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let files = project::layers(project_dir);
    let mut seen = stamp(&files);
    let watcher = Arc::clone(&controller);
    let entry = entry.clone();
    runtime.block_on(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                _ = tick.tick() => {
                    let now = stamp(&files);
                    if now != seen {
                        seen = now;
                        println!("↻ manifest changed");
                        let c = Arc::clone(&watcher);
                        let e = entry.clone();
                        let t = Arc::clone(&tasks);
                        let _ = tokio::task::spawn_blocking(move || apply(&mut lock_controller(&c), &e, t.as_ref())).await;
                    }
                }
            }
        }
    });
    let _ = stop.send(());
    let _ = runtime.block_on(api);
    lock_controller(&controller).shutdown();
    Ok(0)
}
