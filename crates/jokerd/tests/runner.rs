#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use joker_core::time::{ClockQuality, Instant};
use joker_core::trace::{Header, Item, Ticks, decompress};
use jokerd::cycle::GroupTasks;
use jokerd::decisions::{DecisionLog, SharedLog};
use jokerd::io::{DeviceIo, SimDevice};
use jokerd::load::instantiate;
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, SystemPacer, VirtualPacer};
use jokerd::runner::{Running, Start};
use jokerd::task::{CycleTask, TaskContext, TaskError};
use jokerd::trace_ring::{Cut, digest_bytes, lock};

const MS: u64 = 1_000_000;

fn log(dir: &tempfile::TempDir) -> SharedLog {
    Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ))
}

fn start(max_ticks: Option<u64>) -> Start {
    Start {
        generation: 7,
        inherit: Default::default(),
        retention: Duration::from_secs(600),
        max_ticks,
    }
}

fn virtual_run(ticks: u64) -> (Running, tempfile::TempDir) {
    let loaded = common::bench();
    let tasks = instantiate(&loaded.plan, &loaded.manifest, &NativeTasks).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pacer = Box::new(VirtualPacer::new(Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }));
    let running = Running::start(
        Arc::new(loaded.plan),
        tasks,
        pacer,
        log(&dir),
        start(Some(ticks)),
    )
    .unwrap();
    (running, dir)
}

#[test]
fn every_change_reaches_the_decision_log_file() {
    let (running, dir) = virtual_run(100);
    running.join().unwrap();
    let entries = DecisionLog::read_all(&dir.path().join("decisions.jsonl")).unwrap();
    let outputs: Vec<_> = entries.iter().filter(|e| e["record"] == "output").collect();
    assert_eq!(outputs.len(), 4);
    assert_eq!(outputs[0]["output"], "pi.act_led");
    assert_eq!(outputs[0]["applied"], true);
    assert_eq!(outputs[0]["outcome"], "accepted");
    assert_eq!(outputs[0]["level"], "control:12");
    assert_eq!(outputs[0]["gen"], 7);
    assert_eq!(outputs[1]["applied"], false);
    assert_eq!(outputs[1]["previous"]["repeats"], 25);
    assert_eq!(outputs[1]["cause"]["writer"], "app:luz");
    let seqs: Vec<u64> = entries.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, (0..entries.len() as u64).collect::<Vec<_>>());
    assert!(entries.iter().all(|e| e["record"] != "gap"));
}

#[test]
fn the_ring_cuts_a_trace_file() {
    let (running, _dir) = virtual_run(100);
    let plan = Arc::clone(&running.plan);
    let ring = Arc::clone(&running.ring);
    running.join().unwrap();
    let ring = lock(&ring);
    assert_eq!(ring.len(), 100);
    let (first, last) = ring.bounds().unwrap();
    assert_eq!(last - first, 99 * 20 * MS);
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
                from_mono_ns: first + 50 * 20 * MS,
                to_mono_ns: last,
            },
            None,
        )
        .unwrap();
    let (decoded, rest) = Header::decode(&bytes).unwrap();
    assert_eq!(decoded.registry_hash, plan.registry_hash);
    let mut ticks = Vec::new();
    for item in joker_core::trace::Items::new(rest) {
        let Item::Ticks {
            group,
            n_ticks,
            payload,
        } = item.unwrap()
        else {
            panic!()
        };
        assert_eq!(group, 0);
        let raw = decompress(payload).unwrap();
        for t in Ticks::new(&raw, n_ticks, plan.groups[0].ports.len()) {
            let t = t.unwrap();
            let results: Vec<_> = t.results().collect();
            assert_eq!(results.len(), 1);
            ticks.push((t.index, results[0].applied));
        }
    }
    assert_eq!(ticks.len(), 50);
    assert_eq!(ticks[0], (50, 1));
    assert_eq!(ticks[49], (99, 0));
}

#[test]
fn drivers_receive_only_committed_values() {
    let (running, _dir) = virtual_run(30);
    let plan = Arc::clone(&running.plan);
    let shared = Arc::clone(&running.shared);
    running.join().unwrap();
    let mut sim = SimDevice::new(DeviceIo::new(&plan, shared, Monotonic::start(), "pi"));
    let commands = sim.step();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].applied, joker_core::envelope::Applied::Value(0));
    assert!(sim.step().is_empty());
}

struct Slow;

impl CycleTask for Slow {
    fn run(&mut self, _cx: &mut TaskContext<'_, '_>) -> Result<(), TaskError> {
        std::thread::sleep(Duration::from_millis(45));
        Ok(())
    }
}

#[test]
fn real_time_ticks_and_overruns() {
    let loaded = common::bench();
    let dir = tempfile::tempdir().unwrap();
    let clock = Monotonic::start();
    let tasks = instantiate(&loaded.plan, &loaded.manifest, &NativeTasks).unwrap();
    let pacer = Box::new(SystemPacer::new(clock, ClockQuality::Unsynced));
    let running = Running::start(
        Arc::new(loaded.plan.clone()),
        tasks,
        pacer,
        log(&dir),
        start(Some(10)),
    )
    .unwrap();
    let shared = Arc::clone(&running.shared);
    let started = std::time::Instant::now();
    running.join().unwrap();
    assert!(started.elapsed() >= Duration::from_millis(170));
    assert_eq!(shared.groups[0].ticks.get(), 10);
    assert_eq!(shared.groups[0].overruns.get(), 0);

    let slow: Vec<GroupTasks> = vec![vec![(
        joker_core::WriterId(0),
        Box::new(Slow) as Box<dyn CycleTask>,
    )]];
    let pacer = Box::new(SystemPacer::new(clock, ClockQuality::Unsynced));
    let running = Running::start(
        Arc::new(loaded.plan),
        slow,
        pacer,
        log(&dir),
        start(Some(4)),
    )
    .unwrap();
    let shared = Arc::clone(&running.shared);
    running.join().unwrap();
    assert!(
        shared.groups[0].overruns.get() >= 3,
        "{}",
        shared.groups[0].overruns.get()
    );
}
