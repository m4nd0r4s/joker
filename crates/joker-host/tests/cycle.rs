//! End to end: the echo guest, compiled to a component, runs cycles in the
//! host against a joker-core process image, arbiter and envelope.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{Bench, GAIN, TASK_BAD, TASK_ECHO, TASK_EMPTY, TASK_PANIC, TASK_SPIN};
use joker_core::Outcome;
use joker_host::{AppComponent, CycleStatus, HostEngine, InstanceLimits, RuntimeMode};

const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;

fn setup() -> (HostEngine, AppComponent) {
    let engine = HostEngine::native().unwrap();
    let app = AppComponent::compile(&engine, &common::echo(), RuntimeMode::Prod).unwrap();
    (engine, app)
}

#[test]
fn one_cycle_reads_the_image_and_its_writes_reach_the_outputs() {
    let (engine, app) = setup();
    assert_eq!(app.meta().name, "echo");
    let mut bench = Bench::new();
    let mut instance = bench.instance(&engine, &app);
    let (report, log) = bench.tick(&mut instance, 0, Some(4.0), TASK_ECHO);
    assert_eq!(report.status, CycleStatus::Ok);
    assert_eq!(report.staged, 3);
    assert_eq!(f64::from_bits(bench.applied(bench.y)), 4.0 * GAIN + 2.0);
    assert_eq!(bench.applied(bench.nan), CANONICAL_NAN);
    assert!(log.writes.iter().all(|w| w.outcome == Outcome::Accepted));
    assert!(instance.notes().is_empty());
}

#[test]
fn a_bad_input_is_an_error_with_a_note_and_the_proposals_still_go_through() {
    let (engine, app) = setup();
    let mut bench = Bench::new();
    let mut instance = bench.instance(&engine, &app);
    let (report, _) = bench.tick(&mut instance, 0, None, TASK_ECHO);
    assert_eq!(report.status, CycleStatus::AppError);
    assert_eq!(instance.last_error(), Some(("J-E0101", "x is not good")));
    assert_eq!(instance.notes(), &[0]);
    assert_eq!(report.staged, 3);
    assert_eq!(f64::from_bits(bench.applied(bench.y)), 0.0);
}

#[test]
fn traps_drop_the_proposals_and_a_trapped_instance_is_recreated() {
    let (engine, app) = setup();
    let mut bench = Bench::new();
    let mut instance = bench.instance(&engine, &app);
    let (report, log) = bench.tick(&mut instance, 0, Some(1.0), TASK_BAD);
    assert_eq!(report.status, CycleStatus::Trap("J-E0907"));
    assert_eq!(report.staged, 0);
    assert!(log.writes.is_empty());
    assert!(!instance.is_poisoned());
    let (report, _) = bench.tick(&mut instance, 1, Some(1.0), TASK_PANIC);
    assert_eq!(report.status, CycleStatus::Trap("J-E0905"));
    assert!(instance.is_poisoned());
    let mut instance = bench.instance(&engine, &app);
    let (report, _) = bench.tick(&mut instance, 2, Some(1.0), TASK_EMPTY);
    assert_eq!((report.status, report.staged), (CycleStatus::Ok, 0));
}

#[test]
fn a_task_past_its_deadline_is_interrupted() {
    let (engine, app) = setup();
    let mut bench = Bench::new();
    let mut instance = bench
        .try_instance(&engine, &app, InstanceLimits::default())
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let ticker = {
        let (engine, stop) = (engine.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_micros(100));
                engine.increment_epoch();
            }
        })
    };
    let (report, _) = bench.tick(&mut instance, 0, Some(1.0), TASK_SPIN);
    stop.store(true, Ordering::Relaxed);
    ticker.join().unwrap();
    assert_eq!(report.status, CycleStatus::Trap("J-E0904"));
}

fn fnv(acc: &mut u64, word: u64) {
    for b in word.to_le_bytes() {
        *acc ^= u64::from(b);
        *acc = acc.wrapping_mul(0x0100_0000_01b3);
    }
}

fn run_trace() -> u64 {
    let (engine, app) = setup();
    let mut bench = Bench::new();
    let mut instance = bench.instance(&engine, &app);
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    for k in 0..200u64 {
        let x = (k % 7 != 3).then_some(k as f64 * 0.37 - 30.0);
        let (report, log) = bench.tick(&mut instance, k, x, TASK_ECHO);
        fnv(&mut digest, u64::from(report.staged));
        for w in &log.writes {
            fnv(&mut digest, u64::from(w.outcome.code()));
        }
        for s in [bench.y, bench.nan, bench.r] {
            fnv(&mut digest, bench.applied(s));
        }
    }
    digest
}

/// The same component gives the same bits on every architecture (NaN
/// canonicalization, the deterministic generator, the latch clock): CI runs
/// this on x86_64 and the bench on aarch64 against one recorded digest.
#[test]
fn the_outcomes_are_identical_on_every_architecture() {
    let digest = run_trace();
    assert_eq!(digest, run_trace());
    assert_eq!(digest, GOLDEN, "digest {digest:#018x}");
}

const GOLDEN: u64 = 0xe331_b313_19bb_a4f6;
