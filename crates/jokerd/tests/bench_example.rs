//! The bench example of the learning track (`examples/bench`, AUR-185) end
//! to end on the laptop: the `light` app built to a Wasm component, the
//! simulated bench manifest, the cycle thread for N ticks, and the trace
//! replayed with the same component (gate 5 of the M0 slice, docs/26 §6).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use joker_core::time::{ClockQuality, Instant};
use joker_core::trace::{Item, Items, Ticks};
use joker_host::RunMode;
use joker_manifest::OsFileSystem;
use jokerd::compress::decompress;
use jokerd::decisions::DecisionLog;
use jokerd::load::{instantiate, load};
use jokerd::pacer::VirtualPacer;
use jokerd::replay::replay;
use jokerd::runner::{Running, Start};
use jokerd::traces::Source;
use jokerd::wasm::WasmTasks;

const MS: u64 = 1_000_000;
const TICKS: u64 = 200;
const PERIOD_MS: u64 = 20;
const HALF_PERIOD_TICKS: u64 = 500 / PERIOD_MS;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn component() -> Vec<u8> {
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bench-example");
    let status = Command::new(env!("CARGO"))
        .current_dir(root())
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-wasip2",
            "--manifest-path",
            "examples/bench/apps/light/Cargo.toml",
        ])
        .arg("--target-dir")
        .arg(&target_dir)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .status()
        .unwrap();
    assert!(
        status.success(),
        "building examples/bench/apps/light failed"
    );
    std::fs::read(target_dir.join("wasm32-wasip2/release/light.wasm")).unwrap()
}

fn tasks(wasm: &[u8], mode: RunMode) -> WasmTasks {
    let mut tasks = WasmTasks::new(mode).unwrap();
    assert_eq!(tasks.add(wasm, true).unwrap(), "light");
    tasks
}

#[test]
fn the_light_app_blinks_on_the_simulated_bench_and_replays_identically() {
    let wasm = component();
    let live = tasks(&wasm, RunMode::Normal);
    let entry = root().join("examples/bench/joker.sim.toml");
    let loaded = load(&OsFileSystem, &entry, &live).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let start = Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_790_000_000_000_000_000,
        clock: ClockQuality::Synced,
    };
    let running = Running::start(
        Arc::new(loaded.plan.clone()),
        instantiate(&loaded.plan, &loaded.manifest, &live).unwrap(),
        Box::new(VirtualPacer::new(start)),
        Arc::clone(&log),
        Start {
            generation: 1,
            inherit: Default::default(),
            retention: Duration::from_secs(600),
            max_ticks: Some(TICKS),
            utc_origin_ns: 0,
            cpus: Vec::new(),
        },
    )
    .unwrap();
    let source = Source {
        plan: Arc::clone(&running.plan),
        shared: Arc::clone(&running.shared),
        ring: Arc::clone(&running.ring),
        generation: (1, loaded.plan.manifest_hash.clone()),
        utc_origin_ns: 0,
        clock: 2,
    };
    let exit = running.join().unwrap();
    assert_eq!(exit.ticks, TICKS);
    let file = source
        .cut(start.mono_ns, start.mono_ns + TICKS * PERIOD_MS * MS, None)
        .unwrap();

    let (header, rest) = joker_core::trace::Header::decode(&file).unwrap();
    let mut applied = Vec::new();
    for item in Items::new(rest) {
        let Item::Ticks {
            n_ticks, payload, ..
        } = item.unwrap()
        else {
            continue;
        };
        let raw = if header.uncompressed() {
            payload.to_vec()
        } else {
            decompress(payload).unwrap()
        };
        for t in Ticks::new(&raw, n_ticks, loaded.plan.groups[0].ports.len()) {
            let t = t.unwrap();
            let led: Vec<_> = t.results().collect();
            assert_eq!(led.len(), 1);
            applied.push((t.index, led[0].applied));
        }
    }
    assert_eq!(applied.len() as u64, TICKS);
    let toggles: Vec<u64> = applied
        .windows(2)
        .filter(|w| w[0].1 != w[1].1)
        .map(|w| w[1].0)
        .collect();
    assert!(toggles.len() >= 6, "the LED toggled at {toggles:?}");
    assert!(
        toggles.windows(2).all(|w| w[1] - w[0] == HALF_PERIOD_TICKS),
        "the LED toggled at {toggles:?}"
    );

    let entries = DecisionLog::read_all(&dir.path().join("decisions.jsonl")).unwrap();
    let outputs = entries
        .iter()
        .filter(|e| e["record"] == "output" && e["output"] == "pi.act_led")
        .count();
    assert_eq!(outputs, toggles.len() + 1);

    let replayed = tasks(&wasm, RunMode::Replay);
    let report = replay(&loaded.plan, &loaded.manifest, &replayed, &file).unwrap();
    assert_eq!(report.ticks, TICKS);
    assert_eq!(report.before_state, 0);
    assert!(report.identical(), "{:?}", report.divergences.first());
}
