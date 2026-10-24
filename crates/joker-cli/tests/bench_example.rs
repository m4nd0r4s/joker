//! `jk check` accepts the bench example of the learning track
//! (`examples/bench`, AUR-185) for the simulator and for the Pi 3, with the
//! `light` app built to a Wasm component by `jk` itself; and the deployed
//! `jokerd` binary runs that component from its AOT cache after `jk apply`
//! (gate 1 of the M0 slice, docs/26 §6, ADR 0067).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use joker_core::trace::{Header, Item, Items, Ticks};

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/bench")
        .canonicalize()
        .unwrap()
}

fn target_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bench-example-cli")
}

fn jk(args: &[&str]) -> Output {
    jk_with(args, None)
}

fn jk_with(args: &[&str], cache: Option<&Path>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jk"));
    if let Some(c) = cache {
        cmd.env("XDG_CACHE_HOME", c);
    }
    cmd.current_dir(bench())
        .args(args)
        .env("CARGO_TARGET_DIR", target_dir())
        .env_remove("JK_NODE")
        .env_remove("JK_ENV")
        .env_remove("JK_MANIFEST")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn the_bench_example_checks_for_the_simulator_and_the_pi() {
    for env in ["sim", "lab"] {
        let check = jk(&["--json", "check", "--env", env]);
        assert_eq!(check.status.code(), Some(0), "{env}: {}", text(&check));
        let v: serde_json::Value = serde_json::from_slice(&check.stdout).unwrap();
        assert_eq!(v["ok"], true, "{env}: {v}");
    }
}

const PERIOD_MS: u64 = 20;
const HALF_PERIOD_TICKS: u64 = 500 / PERIOD_MS;

/// The applied value of `pi.act_led` at every tick of a trace.
fn led_per_tick(trace: &[u8], ports: usize) -> Vec<(u64, u64)> {
    let (header, rest) = Header::decode(trace).unwrap();
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
            jokerd::compress::decompress(payload).unwrap()
        };
        for t in Ticks::new(&raw, n_ticks, ports) {
            let t = t.unwrap();
            let led: Vec<_> = t.results().collect();
            assert_eq!(led.len(), 1);
            applied.push((t.index, led[0].applied));
        }
    }
    applied
}

#[test]
fn the_light_app_runs_under_the_jokerd_binary_and_replays_identically() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("node");
    let cache = tmp.path().join("cache");
    std::fs::create_dir_all(&state).unwrap();
    let triple = joker_host::host_triple().unwrap();

    let build = jk_with(
        &["--json", "build", "--env", "sim", "--target", &triple],
        Some(&cache),
    );
    assert_eq!(build.status.code(), Some(0), "{}", text(&build));
    let built: serde_json::Value = serde_json::from_slice(&build.stdout).unwrap();
    let app = &built["apps"][0];
    assert_eq!(app["app"], "light", "{built}");
    let laptop_cwasm = PathBuf::from(app["aot"].as_str().unwrap());
    assert!(laptop_cwasm.starts_with(cache.join("joker/aot")), "{built}");
    let wasm = std::fs::read(target_dir().join("wasm32-wasip2/release/light.wasm")).unwrap();
    assert_eq!(app["digest"], jokerd::blobs::digest_of(&wasm));

    let daemon = common::spawn_empty_jokerd(&state);
    let node = format!("unix:{}", daemon.socket.display());
    let on_node = |args: &[&str]| {
        let mut all = vec!["--node", node.as_str()];
        all.extend_from_slice(args);
        jk_with(&all, Some(&cache))
    };
    let apply = on_node(&["--env", "sim", "apply", "-m", "gate 1"]);
    assert_eq!(apply.status.code(), Some(0), "{}", text(&apply));

    let engine = joker_host::HostEngine::native().unwrap();
    let digest = joker_host::component::digest(&wasm);
    let node_cwasm = joker_host::AotCache::new(state.join("aot")).path(engine.id(), &digest);
    assert_eq!(
        std::fs::read(&node_cwasm).unwrap(),
        std::fs::read(&laptop_cwasm).unwrap(),
        "the node runs the .cwasm jk precompiled"
    );
    let stored = std::fs::read_to_string(state.join("generations/1/artifacts.json")).unwrap();
    assert!(stored.contains("apps.light"), "{stored}");
    assert!(stored.contains("joker.app.v1+cwasm"), "{stored}");

    std::thread::sleep(Duration::from_secs(3));
    let trace = tmp.path().join("bench.jktrace");
    let dump = on_node(&["trace-dump", "--out", trace.to_str().unwrap()]);
    assert_eq!(dump.status.code(), Some(0), "{}", text(&dump));

    let mut tasks = jokerd::wasm::WasmTasks::new(joker_host::RunMode::Normal).unwrap();
    assert_eq!(tasks.add(&wasm, true).unwrap(), "light");
    let loaded = jokerd::load::load(
        &joker_manifest::OsFileSystem,
        &bench().join("joker.sim.toml"),
        &tasks,
    )
    .unwrap();
    let applied = led_per_tick(
        &std::fs::read(&trace).unwrap(),
        loaded.plan.groups[0].ports.len(),
    );
    assert!(applied.len() >= 100, "{} ticks recorded", applied.len());
    let toggles: Vec<u64> = applied
        .windows(2)
        .filter(|w| w[0].1 != w[1].1)
        .map(|w| w[1].0)
        .collect();
    assert!(toggles.len() >= 4, "the LED toggled at {toggles:?}");
    assert!(
        toggles.windows(2).all(|w| w[1] - w[0] == HALF_PERIOD_TICKS),
        "the LED toggled at {toggles:?}"
    );

    let replayed = jk(&["--env", "sim", "replay", trace.to_str().unwrap(), "--diff"]);
    assert_eq!(replayed.status.code(), Some(0), "{}", text(&replayed));
    assert!(
        text(&replayed).contains("0 divergence(s)"),
        "{}",
        text(&replayed)
    );

    common::terminate(daemon);
    let daemon = common::spawn_empty_jokerd(&state);
    let node = format!("unix:{}", daemon.socket.display());
    let on_node = |args: &[&str]| {
        let mut all = vec!["--node", node.as_str()];
        all.extend_from_slice(args);
        jk_with(&all, Some(&cache))
    };
    let gens = on_node(&["--json", "generations"]);
    let v: serde_json::Value = serde_json::from_slice(&gens.stdout).unwrap();
    assert_eq!(v["items"][0]["origin"], "boot", "{v}");
    assert_eq!(v["items"][0]["state"], "confirmed", "{v}");
    std::thread::sleep(Duration::from_secs(2));
    let rebooted = tmp.path().join("boot.jktrace");
    let dump = on_node(&["trace-dump", "--out", rebooted.to_str().unwrap()]);
    assert_eq!(dump.status.code(), Some(0), "{}", text(&dump));
    let applied = led_per_tick(
        &std::fs::read(&rebooted).unwrap(),
        loaded.plan.groups[0].ports.len(),
    );
    let toggles = applied.windows(2).filter(|w| w[0].1 != w[1].1).count();
    assert!(
        toggles >= 2,
        "after a restart the LED toggled {toggles} times"
    );
    let replayed = jk(&["--env", "sim", "replay", rebooted.to_str().unwrap()]);
    assert_eq!(replayed.status.code(), Some(0), "{}", text(&replayed));
    drop(daemon);
}
