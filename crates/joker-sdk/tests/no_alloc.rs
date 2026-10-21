//! The cycle path does not allocate (AGENTS.md, invariant 6; ref/04 §4.13;
//! ADR 0056): 1,000 ticks of the track light app run under
//! `assert_no_alloc`, which counts every call to the allocator made on this
//! thread, separately for the core part of the tick (latch, commit, end of
//! tick) and for the `on-cycle` call.
//!
//! The guest side is proven by the light app itself: the global allocator of
//! `joker-sdk-sys` traps with `J-E0903` when a cycle task allocates, so every
//! tick returning `Ok` is a tick without guest allocation.
//!
//! The overhead per tick is printed; run it in release for numbers:
//!
//! ```sh
//! cargo test -p joker-sdk --release --test no_alloc -- --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use assert_no_alloc::{AllocDisabler, assert_no_alloc, reset_violation_count, violation_count};
use joker_core::ClockQuality;
use joker_host::CycleStatus;

use common::Counts;

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

const TICKS: u32 = 1_000;

/// Allocator calls that wasmtime 48.0.7 (the LTS line of ADR 0055, and also
/// 49 and 50) itself makes in one synchronous call
/// into a component when the component model async is enabled: it creates
/// and frees a task, a thread, a waitable set and a hash set of its
/// concurrency state (`enter_guest_sync_call`, `exit_guest_sync_call`). No
/// embedder setting avoids them while the engine accepts the world's async
/// exports (ADR 0056, decision 7). Everything else on the path must be zero.
const WASMTIME_SYNC_CALL: u32 = 8;

#[test]
fn a_thousand_ticks_of_the_light_app_do_not_allocate() {
    let (mut bench, mut instance) = common::light();
    let mut sink = Counts::default();
    let mut statuses = [CycleStatus::Ok; TICKS as usize];
    let (mut core, mut call) = (0u32, 0u32);
    let mut elapsed = Duration::ZERO;
    for (k, status) in (0..u64::from(TICKS)).zip(statuses.iter_mut()) {
        let at = joker_core::time::Instant {
            mono_ns: k * bench.period,
            utc_ns: 1_790_000_000_000_000_000,
            clock: ClockQuality::Synced,
        };
        let start = Instant::now();
        reset_violation_count();
        assert_no_alloc(|| {
            bench
                .image
                .latch(k, at, bench.slots, &bench.registry)
                .unwrap()
        });
        core += violation_count();
        reset_violation_count();
        let report = assert_no_alloc(|| instance.run(0, &mut bench.image, &bench.outputs));
        call += violation_count();
        reset_violation_count();
        assert_no_alloc(|| {
            bench
                .outputs
                .commit(&bench.image, bench.slots, &mut sink)
                .unwrap();
            bench.image.end_tick().unwrap();
        });
        core += violation_count();
        elapsed += start.elapsed();
        *status = report.unwrap().status;
    }
    println!(
        "light app over {TICKS} ticks: {core} allocator calls in the core, {call} in on-cycle \
         ({} per call, all wasmtime's), {:?} per tick (latch + on-cycle + commit)",
        call / TICKS,
        elapsed / TICKS
    );
    assert!(
        statuses.iter().all(|s| *s == CycleStatus::Ok),
        "{statuses:?}"
    );
    assert_eq!(
        (sink.writes, sink.accepted),
        (u64::from(TICKS), u64::from(TICKS))
    );
    assert_eq!(core, 0, "allocator calls in the core part of the tick");
    assert_eq!(
        call,
        WASMTIME_SYNC_CALL * TICKS,
        "allocator calls in on-cycle beyond wasmtime's own"
    );
}
