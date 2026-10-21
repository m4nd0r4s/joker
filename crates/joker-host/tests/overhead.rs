//! The cost of an empty cycle call (docs/01 §21: ≤ 5 µs, empty task,
//! Cranelift). Ignored by default; run it in release:
//!
//! ```sh
//! cargo test -p joker-host --release --test overhead -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Instant;

use common::{Bench, TASK_ECHO, TASK_EMPTY};
use joker_host::{AppComponent, HostEngine, RuntimeMode};

#[test]
#[ignore = "measurement; run in release with --ignored"]
fn empty_cycle_call_overhead() {
    let engine = HostEngine::native().unwrap();
    let app = AppComponent::compile(&engine, &common::echo(), RuntimeMode::Dev).unwrap();
    let mut bench = Bench::new();
    let mut instance = bench.instance(&engine, &app);
    for (name, task) in [
        ("empty task", TASK_EMPTY),
        ("echo task (3 writes)", TASK_ECHO),
    ] {
        let n = 20_000u64;
        let mut host = std::time::Duration::ZERO;
        for k in 0..n {
            let start = Instant::now();
            let (report, _) = bench.tick(
                &mut instance,
                k + if task == TASK_ECHO { n } else { 0 },
                Some(1.0),
                task,
            );
            host += start.elapsed();
            assert!(report.staged <= 3);
        }
        println!(
            "{name}: {:?} per tick (latch + run + commit), {} on {}",
            host / n as u32,
            n,
            engine.triple()
        );
    }
    let n = 100_000u32;
    let start = Instant::now();
    for k in 0..n {
        bench
            .image
            .latch(
                3 * 20_000 + u64::from(k),
                joker_core::time::Instant::mono(u64::from(k)),
                bench.slots,
                &bench.registry,
            )
            .unwrap();
        instance
            .run(TASK_EMPTY, &mut bench.image, &bench.outputs)
            .unwrap();
        bench.image.end_tick().unwrap();
    }
    let per_call = start.elapsed() / n;
    println!(
        "empty on-cycle call (pack + call + check): {per_call:?} on {}",
        engine.triple()
    );
    assert!(per_call.as_micros() < 5);
}
