//! The AOT cache, the checks made at load and cross-compilation for the
//! bench targets.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::PathBuf;
use std::time::Instant;

use common::{Bench, C, TASK_ECHO};
use joker_host::{AotCache, AppComponent, HostEngine, RuntimeMode};
use wasmtime::{Engine, Precompiled};

fn cache(name: &str) -> AotCache {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("aot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    AotCache::new(dir)
}

#[test]
fn production_loads_only_from_the_cache() {
    let engine = HostEngine::native().unwrap();
    let wasm = common::echo();
    let cache = cache("prod");
    let err = cache.load(&engine, &wasm, RuntimeMode::Prod).unwrap_err();
    assert_eq!(err.code(), "J-E0010");
    let path = cache.install(&engine, &wasm).unwrap();
    assert!(
        path.starts_with(
            cache
                .path(engine.id(), &joker_host::component::digest(&wasm))
                .parent()
                .unwrap()
        )
    );
    assert!(path.to_string_lossy().contains(engine.id().as_str()));
    let started = Instant::now();
    let app = cache.load(&engine, &wasm, RuntimeMode::Prod).unwrap();
    let mut bench = Bench::new();
    let mut instance = bench.instance(&engine, &app);
    let startup = started.elapsed();
    let (report, _) = bench.tick(&mut instance, 0, Some(4.0), TASK_ECHO);
    assert_eq!(report.staged, 3);
    println!("load from the AOT cache to a ready instance: {startup:?}");
    assert!(
        startup.as_secs() < 2,
        "startup took {startup:?} (docs/01 §21: ≤ 2 s)"
    );
}

#[test]
fn dev_compiles_and_fills_the_cache_on_a_miss() {
    let engine = HostEngine::native().unwrap();
    let wasm = common::echo();
    let cache = cache("dev");
    assert!(!cache.contains(&engine, &wasm));
    cache.load(&engine, &wasm, RuntimeMode::Dev).unwrap();
    assert!(cache.contains(&engine, &wasm));
}

#[test]
fn the_laptop_cross_compiles_for_the_bench_nodes() {
    let wasm = common::echo();
    let cache = cache("cross");
    for triple in ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"] {
        let engine = HostEngine::for_target(triple).unwrap();
        let path = cache.install(&engine, &wasm).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            Engine::detect_precompiled(&bytes),
            Some(Precompiled::Component)
        );
        assert!(path.to_string_lossy().contains(triple));
    }
    let native = HostEngine::native().unwrap();
    let cross = HostEngine::for_target(native.triple()).unwrap();
    cache.install(&cross, &wasm).unwrap();
    cache.load(&native, &wasm, RuntimeMode::Prod).unwrap();
}

#[test]
fn a_cross_engine_does_not_run_code() {
    let triple = if std::env::consts::ARCH == "x86_64" {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    let engine = HostEngine::for_target(triple).unwrap();
    let err = cache("foreign")
        .load(&engine, &common::echo(), RuntimeMode::Dev)
        .unwrap_err();
    assert_eq!(err.code(), "J-E0010");
}

fn compile(meta: Option<&C>, mode: RuntimeMode) -> Result<AppComponent, joker_host::HostError> {
    let engine = HostEngine::native().unwrap();
    let wasm = match meta {
        Some(m) => common::with_section(common::echo_raw(), "joker:meta", &m.bytes()),
        None => common::echo_raw().to_vec(),
    };
    AppComponent::compile(&engine, &wasm, mode)
}

#[test]
fn load_checks_follow_ref04() {
    assert_eq!(
        compile(None, RuntimeMode::Dev).unwrap_err().code(),
        "J-E0004"
    );
    let newer = common::echo_meta("joker:runtime@0.2.0", &[]);
    assert_eq!(
        compile(Some(&newer), RuntimeMode::Dev).unwrap_err().code(),
        "J-E0001"
    );
    let unstable = common::echo_meta("joker:runtime@0.1.0", &["ml"]);
    assert_eq!(
        compile(Some(&unstable), RuntimeMode::Prod)
            .unwrap_err()
            .code(),
        "J-E0003"
    );
    assert!(compile(Some(&unstable), RuntimeMode::Dev).is_ok());
    let C::M(mut entries) = common::echo_meta("joker:runtime@0.1.0", &[]) else {
        unreachable!()
    };
    entries[5].1 = C::U(2);
    assert_eq!(
        compile(Some(&C::M(entries)), RuntimeMode::Dev)
            .unwrap_err()
            .code(),
        "J-E0002"
    );
    let engine = HostEngine::native().unwrap();
    assert_eq!(
        AppComponent::compile(&engine, b"\0asm\x01\0\0\0", RuntimeMode::Dev)
            .unwrap_err()
            .code(),
        "J-E0009"
    );
}
