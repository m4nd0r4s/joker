//! A hot swap of a Wasm app (docs/01 §9, ADR 0107): the new instance is
//! created and initialized off the cycle thread while the old one serves,
//! and the group swaps it in between two ticks, so that no tick goes
//! without the app.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use joker_core::trace::{Header, Item, Ticks};
use joker_host::RunMode;
use joker_manifest::MemoryFileSystem;

use jokerd::compress::decompress;
use jokerd::decisions::DecisionLog;
use jokerd::load::{Loaded, instantiate, load};
use jokerd::pacer::{Monotonic, SystemPacer};
use jokerd::runner::{Running, Start};
use jokerd::trace_ring::{Cut, digest_bytes, lock};
use jokerd::wasm::{WasmTasks, drop_retired, swap_in, warm_app};

const PERIOD_NS: u64 = 10_000_000;

fn manifest(level: f64) -> String {
    format!(
        r#"
[project]
name  = "swap"
joker = "1.0"
mode  = "dev"

[outputs."pi.out"]
safe = 0.0

[cycles.main]
period = "10ms"

[apps.level]
path   = "apps/level"
bind   = {{ out = "pi.out" }}
params = {{ level = {level:?} }}
cycles = {{ run = "main" }}

[devices.pi]
driver = "sim"

[devices.pi.signals]
out = {{ type = "f64", direction = "output", unit = "1" }}
"#
    )
}

fn component() -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fixtures");
    let status = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-wasip2",
            "-p",
            "jokerd-fixture-level",
        ])
        .arg("--target-dir")
        .arg(&target_dir)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .status()
        .unwrap();
    assert!(status.success(), "building jokerd-fixture-level failed");
    std::fs::read(target_dir.join("wasm32-wasip2/release/jokerd_fixture_level.wasm")).unwrap()
}

fn loaded(level: f64, provider: &WasmTasks) -> Loaded {
    let mut fs = MemoryFileSystem::default();
    fs.dir("apps/level");
    fs.file("joker.toml", manifest(level));
    load(&fs, Path::new("joker.toml"), provider).unwrap_or_else(|e| panic!("{e}"))
}

fn wait_for_ticks(running: &Running, ticks: u64) {
    let start = Instant::now();
    while running.shared.groups[0].ticks.get() < ticks {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the group did not reach {ticks} ticks"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_hot_swap_of_an_app_misses_no_tick() {
    let mut provider = WasmTasks::new(RunMode::Normal).unwrap();
    provider.add(&component(), true).unwrap();
    let old = loaded(1.0, &provider);
    let new = loaded(2.0, &provider);
    let tasks = instantiate(&old.plan, &old.manifest, &provider).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let clock = Monotonic::start();
    let running = Running::start(
        Arc::new(old.plan),
        tasks,
        Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced)),
        log,
        Start {
            generation: 1,
            inherit: Default::default(),
            retention: Duration::from_secs(600),
            max_ticks: None,
            utc_origin_ns: 0,
            cpus: Vec::new(),
        },
    )
    .unwrap();
    wait_for_ticks(&running, 20);
    let pool = || provider.engine().pool_usage().unwrap().instances;
    assert_eq!(pool(), 1);

    let created = || provider.engine().instances_created();
    assert_eq!(created(), 1);
    let warm = warm_app(&running.plan, &new.manifest, &provider, "level").unwrap();
    assert_eq!(warm.len(), 1);
    assert_eq!((warm[0].group(), warm[0].index()), (0, 0));
    assert_eq!(
        pool(),
        2,
        "the new instance exists before its group sees it"
    );
    assert_eq!(created(), 2);
    let swapped_at = running.shared.groups[0].ticks.get();
    assert_eq!(swap_in(&running.shared, warm).unwrap(), 1);
    wait_for_ticks(&running, swapped_at + 20);
    assert_eq!(created(), 2, "the group created no instance of its own");
    assert_eq!(pool(), 2);
    assert_eq!(drop_retired(&running.shared), 1);
    assert_eq!(pool(), 1, "the retired instance gave its slot back");

    let plan = Arc::clone(&running.plan);
    let shared = Arc::clone(&running.shared);
    let ring = Arc::clone(&running.ring);
    running.stop().unwrap();
    let stats = &shared.groups[0];
    assert_eq!(stats.task_errors.get(), 0);
    assert_eq!(stats.overruns.get(), 0);
    assert!(
        stats.max_duration_ns.get() < PERIOD_NS,
        "a tick took {} ns",
        stats.max_duration_ns.get()
    );

    let ring = lock(&ring);
    let (first, last) = ring.bounds().unwrap();
    let header = Header {
        flags: 0,
        generation_hash: digest_bytes(&plan.manifest_hash),
        registry_hash: plan.registry_hash,
        epoch_mono_ns: first,
        epoch_utc_ns: 0,
        clock: 0,
    };
    let bytes = ring
        .export(
            header,
            Cut {
                from_mono_ns: first,
                to_mono_ns: last,
            },
            None,
            &Default::default(),
        )
        .unwrap();
    let (_, rest) = Header::decode(&bytes).unwrap();
    let out = plan.id("pi.out").unwrap();
    let mut seen = Vec::new();
    for item in joker_core::trace::Items::new(rest) {
        let Item::Ticks {
            n_ticks, payload, ..
        } = item.unwrap()
        else {
            continue;
        };
        let raw = decompress(payload).unwrap();
        for t in Ticks::new(&raw, n_ticks, plan.groups[0].ports.len()) {
            let t = t.unwrap();
            let proposals: Vec<_> = t.proposals().filter(|p| p.signal == out).collect();
            assert_eq!(proposals.len(), 1, "tick {} has no proposal", t.index);
            seen.push((t.index, f64::from_bits(proposals[0].value)));
        }
    }
    assert!(seen.len() >= 40, "{} ticks in the trace", seen.len());
    assert!(
        seen.windows(2).all(|w| w[1].0 == w[0].0 + 1),
        "a tick is missing: {seen:?}"
    );
    let switch = seen.iter().position(|&(_, v)| v == 2.0).unwrap();
    assert!(switch > 0);
    assert!(seen[..switch].iter().all(|&(_, v)| v == 1.0));
    assert!(seen[switch..].iter().all(|&(_, v)| v == 2.0));
}
