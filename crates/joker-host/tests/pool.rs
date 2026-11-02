//! The instance pool of a node's engine (docs/01 §9, ADR 0107): sized at
//! startup, a slot per live instance, freed when the instance is dropped,
//! and an instance past the pool is refused instead of allocated.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Bench, TASK_ECHO};
use joker_host::{AppComponent, CycleStatus, HostEngine, PoolConfig, RuntimeMode};

fn pooled(instances: u32) -> (HostEngine, AppComponent) {
    let engine = HostEngine::pooled(PoolConfig { instances }).unwrap();
    let app = AppComponent::compile(&engine, &common::echo(), RuntimeMode::Prod).unwrap();
    (engine, app)
}

#[test]
fn every_live_instance_holds_one_slot_until_it_is_dropped() {
    let (engine, app) = pooled(4);
    let empty = engine.pool_usage().unwrap();
    assert_eq!((empty.instances, empty.memories, empty.capacity), (0, 0, 4));
    let mut bench = Bench::new();
    let mut old = bench.instance(&engine, &app);
    let warm = bench.instance(&engine, &app);
    let both = engine.pool_usage().unwrap();
    assert_eq!((both.instances, both.memories), (2, 2));
    let (report, _) = bench.tick(&mut old, 0, Some(1.0), TASK_ECHO);
    assert_eq!(report.status, CycleStatus::Ok);
    drop(old);
    let one = engine.pool_usage().unwrap();
    assert_eq!((one.instances, one.memories), (1, 1));
    drop(warm);
    assert_eq!(engine.pool_usage().unwrap().instances, 0);
}

#[test]
fn an_instance_past_the_pool_is_refused() {
    let (engine, app) = pooled(1);
    let bench = Bench::new();
    let first = bench.instance(&engine, &app);
    let second = bench
        .try_instance(&engine, &app, joker_host::InstanceLimits::default())
        .unwrap_err();
    assert_eq!(second.code(), "J-E0909", "{second}");
    drop(first);
    bench.instance(&engine, &app);
}

#[test]
fn the_pool_reserves_address_space_by_its_size() {
    let pool = PoolConfig { instances: 16 };
    let reservation = joker_host::MEMORY_RESERVATION_32;
    assert_eq!(
        pool.reserved_bytes(reservation, 64 << 10),
        16 * (reservation + (64 << 10)) + (64 << 10) + 32 * 10_000 * 8
    );
    let engine = HostEngine::pooled(PoolConfig::default()).unwrap();
    let usage = engine.pool_usage().unwrap();
    assert_eq!(usage.capacity, PoolConfig::default().instances);
    assert!(usage.reserved_bytes > 0);
}

#[test]
fn an_engine_without_a_pool_reports_none() {
    assert!(HostEngine::native().unwrap().pool_usage().is_none());
}

fn rss_kib() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[test]
#[ignore = "measurement; run alone with --ignored --nocapture"]
fn resident_memory_of_the_pool() {
    let app = AppComponent::compile(
        &HostEngine::native().unwrap(),
        &common::echo(),
        RuntimeMode::Prod,
    )
    .unwrap();
    drop(app);
    let before = rss_kib();
    let engine = HostEngine::pooled(PoolConfig::default()).unwrap();
    let reserved = rss_kib();
    let app = AppComponent::compile(&engine, &common::echo(), RuntimeMode::Prod).unwrap();
    let compiled = rss_kib();
    let bench = Bench::new();
    let one = bench.instance(&engine, &app);
    let live = rss_kib();
    let two = bench.instance(&engine, &app);
    let both = rss_kib();
    drop((one, two));
    let usage = engine.pool_usage().unwrap();
    eprintln!(
        "pool of {} instances: {} MiB of address space; RSS KiB: before {before}, \
         pool {reserved}, compiled {compiled}, one instance {live}, two {both}, after drop {}",
        usage.capacity,
        usage.reserved_bytes >> 20,
        rss_kib()
    );
}
