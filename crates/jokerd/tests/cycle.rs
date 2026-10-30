#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use joker_core::commit::RecordKind;
use joker_core::envelope::Applied;
use joker_core::time::{ClockQuality, Instant};
use jokerd::cycle::{RunOptions, run};
use jokerd::load::instantiate;
use jokerd::native::NativeTasks;
use jokerd::pacer::{Pacer, VirtualPacer};
use jokerd::shared::Shared;

const MS: u64 = 1_000_000;

fn start() -> Instant {
    Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }
}

#[test]
fn plan_of_the_bench() {
    let loaded = common::bench();
    let plan = &loaded.plan;
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.groups[0].name, "main");
    assert_eq!(plan.groups[0].period_ns, 20 * MS);
    assert_eq!(plan.groups[0].tasks.len(), 1);
    assert_eq!(plan.groups[0].ports, vec!["pi.act_led".to_owned()]);
    assert_eq!(plan.outputs.len(), 1);
    assert_eq!(plan.outputs[0].group, "main");
    assert_eq!(plan.writers[0].name, "app:light");
    assert!(plan.writer("user:manual:4").is_some());
    assert!(plan.id("pi.act_led").is_some());
}

#[test]
fn light_blinks_and_every_change_is_recorded() {
    let loaded = common::bench();
    let plan = &loaded.plan;
    let shared = Shared::new(plan);
    let tasks = instantiate(plan, &loaded.manifest, &NativeTasks).unwrap();
    let mut pacer = VirtualPacer::new(start());
    let exit = run(
        plan,
        &shared,
        tasks,
        &mut pacer,
        RunOptions {
            max_ticks: Some(100),
            ..RunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(exit.ticks, 100);
    assert_eq!(pacer.now().mono_ns, start().mono_ns + 99 * 20 * MS);
    let led = plan.id("pi.act_led").unwrap();
    let (applied, meta, tick) = shared.output(led).unwrap().read().unwrap();
    assert_eq!(tick, 99);
    assert_eq!(applied, 0);
    assert_eq!(meta.outcome, 1);
    assert_eq!(meta.level, 12);
    assert_eq!(exit.applied["pi.act_led"], Applied::Value(0));

    let mut output_records = Vec::new();
    while let Some(r) = shared.decisions.pop().map(|q| q.record) {
        if r.kind == RecordKind::Output {
            output_records.push(r);
        }
    }
    let ticks: Vec<u64> = output_records.iter().map(|r| r.tick).collect();
    assert_eq!(ticks, vec![0, 25, 50, 75]);
    assert_eq!(output_records[1].previous.unwrap().repeats, 25);
    assert_eq!(shared.decisions_dropped.get(), 0);

    let mut chunks = 0;
    while shared.trace_filled.pop().is_some() {
        chunks += 1;
    }
    assert_eq!(chunks, 100);
    assert_eq!(shared.groups[0].ticks.get(), 100);
    assert_eq!(shared.groups[0].overruns.get(), 0);
}

#[test]
fn every_write_record_carries_the_causal_set_of_its_task() {
    let loaded = common::bench();
    let plan = &loaded.plan;
    let shared = Shared::new(plan);
    let tasks = instantiate(plan, &loaded.manifest, &NativeTasks).unwrap();
    let mut pacer = VirtualPacer::new(start());
    run(
        plan,
        &shared,
        tasks,
        &mut pacer,
        RunOptions {
            max_ticks: Some(30),
            ..RunOptions::default()
        },
    )
    .unwrap();

    let mut causes = std::collections::BTreeMap::new();
    while let Some(c) = shared.causes.pop() {
        causes.insert(c.id, c);
    }
    let mut writes = 0;
    while let Some(q) = shared.decisions.pop() {
        if let RecordKind::Write(_) = q.record.kind {
            writes += 1;
            assert_ne!(
                q.causal, 0,
                "write at tick {} has no causal set",
                q.record.tick
            );
            let set = causes.remove(&q.causal).expect("its causal record");
            assert_eq!(set.omitted, 0);
        } else {
            assert_eq!(q.causal, 0);
        }
    }
    assert!(writes > 0);
    assert!(causes.is_empty());
    assert_eq!(shared.causes_dropped.get(), 0);
}

#[test]
fn a_new_generation_inherits_the_last_applied_value() {
    let loaded = common::bench();
    let plan = &loaded.plan;
    let shared = Shared::new(plan);
    let mut pacer = VirtualPacer::new(start());
    let mut inherit = std::collections::BTreeMap::new();
    inherit.insert("pi.act_led".to_owned(), Applied::Value(1));
    let tasks = vec![Vec::new()];
    let exit = run(
        plan,
        &shared,
        tasks,
        &mut pacer,
        RunOptions {
            inherit,
            max_ticks: Some(1),
            ..RunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(exit.applied["pi.act_led"], Applied::Value(0));
}
