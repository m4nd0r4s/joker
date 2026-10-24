//! The tick of the cycle thread does not allocate (AGENTS.md invariant 6),
//! including when it admits trip resets (`jk reset`, docs/16 E7) and
//! resolves them at the commit of the same tick: every tick runs under
//! `assert_no_alloc`, which counts each call to the allocator made on the
//! cycle thread inside it. A full reset queue of requests for one output,
//! admitted in one tick, resolves every ticket, in request order.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use assert_no_alloc::{AllocDisabler, assert_no_alloc, reset_violation_count, violation_count};
use joker_core::time::{ClockQuality, Instant};
use joker_manifest::MemoryFileSystem;
use jokerd::cycle::{GroupTasks, RunOptions, run};
use jokerd::native::NativeTasks;
use jokerd::pacer::VirtualPacer;
use jokerd::shared::{RESET_CAPACITY, ResetKind, ResetRequest, Shared};

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

const MANIFEST: &str = "[project]\nname = \"no-alloc\"\njoker = \"1.0\"\nmode = \"dev\"\n\n\
    [outputs.\"pi.p_set\"]\nsafe = \"0 kW\"\nrange = [\"-100 kW\", \"100 kW\"]\non_range = \"trip\"\n\n\
    [devices.pi]\ndriver = \"sim\"\n\n\
    [devices.pi.signals]\np_set = { type = \"f64\", direction = \"output\", unit = \"kW\" }\n";

fn guarded(tick: &mut dyn FnMut()) {
    assert_no_alloc(tick);
}

#[test]
fn ticks_that_admit_and_resolve_resets_do_not_allocate() {
    let mut fs = MemoryFileSystem::default();
    fs.file("joker.toml", MANIFEST);
    let loaded = jokerd::load::load(&fs, Path::new("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let plan = loaded.plan;
    let shared = Shared::new(&plan);
    let signal = plan.id("pi.p_set").unwrap();
    let group = plan
        .groups
        .iter()
        .position(|g| g.name == plan.output("pi.p_set").unwrap().group)
        .unwrap();
    for ticket in 1..=RESET_CAPACITY as u32 {
        shared.resets[group]
            .push(ResetRequest { signal, ticket })
            .unwrap();
    }
    let tasks: Vec<GroupTasks> = plan.groups.iter().map(|_| Vec::new()).collect();
    let mut pacer = VirtualPacer::new(Instant {
        mono_ns: 1_000_000_000,
        utc_ns: 1_790_000_000_000_000_000,
        clock: ClockQuality::Synced,
    });
    reset_violation_count();
    run(
        &plan,
        &shared,
        tasks,
        &mut pacer,
        RunOptions {
            max_ticks: Some(20),
            initial_trips: vec![signal],
            tick_guard: Some(guarded),
            ..RunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(violation_count(), 0, "the tick allocated");

    let mut outcomes = Vec::new();
    while let Some(o) = shared.reset_results.pop() {
        outcomes.push(o);
    }
    let tickets: Vec<u32> = outcomes.iter().map(|o| o.ticket).collect();
    assert_eq!(tickets, (1..=RESET_CAPACITY as u32).collect::<Vec<_>>());
    assert!(
        outcomes.iter().all(|o| o.kind == ResetKind::Cleared),
        "{outcomes:?}"
    );
}

const EXPRESSIONS: &str = "[project]\nname = \"no-alloc-expr\"\njoker = \"1.0\"\nmode = \"dev\"\n\n\
    [outputs.\"pi.p_set\"]\nsafe = \"0 kW\"\ninterlock = \"pi.ready\"\nrequires_good = [\"pi.p_max\"]\n\
    range = [\"0 kW\", \"pi.p_max\"]\nreset_when = \"!alarm.high.active\"\n\n\
    [calc]\nmean = \"avg(pi.p_max, 1s)\"\n\n\
    [alarms.high]\nwhen = \"calc.mean > 4 kW\"\nseverity = \"high\"\nactions = { trip = [\"pi.p_set\"] }\n\n\
    [devices.pi]\ndriver = \"sim\"\n\n\
    [devices.pi.signals]\np_set = { type = \"f64\", direction = \"output\", unit = \"kW\" }\n\
    p_max = { type = \"f64\", direction = \"input\", unit = \"W\" }\n\
    ready = { type = \"bool\", direction = \"input\" }\n";

#[test]
fn ticks_that_evaluate_expressions_calc_and_alarms_do_not_allocate() {
    let mut fs = MemoryFileSystem::default();
    fs.file("joker.toml", EXPRESSIONS);
    let loaded = jokerd::load::load(&fs, Path::new("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let plan = loaded.plan;
    assert_eq!(plan.calcs.len(), 1);
    assert_eq!(plan.alarms.len(), 1);
    let shared = Shared::new(&plan);
    let good = joker_core::Quality::GOOD;
    let p_max = plan.id("pi.p_max").unwrap();
    shared.slots[p_max.index()].write(5_000.0f64.to_bits(), good, 0, 0);
    let ready = plan.id("pi.ready").unwrap();
    shared.slots[ready.index()].write(1, good, 0, 0);
    let tasks: Vec<GroupTasks> = plan.groups.iter().map(|_| Vec::new()).collect();
    let mut pacer = VirtualPacer::new(Instant {
        mono_ns: 1_000_000_000,
        utc_ns: 1_790_000_000_000_000_000,
        clock: ClockQuality::Synced,
    });
    reset_violation_count();
    run(
        &plan,
        &shared,
        tasks,
        &mut pacer,
        RunOptions {
            max_ticks: Some(60),
            tick_guard: Some(guarded),
            ..RunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(violation_count(), 0, "the tick allocated");
    let active = plan.id("alarm.high.active").unwrap();
    assert_eq!(
        shared.slots[active.index()]
            .read(8)
            .unwrap()
            .transport_bits(),
        1,
        "the alarm evaluated in the guarded ticks"
    );
}
