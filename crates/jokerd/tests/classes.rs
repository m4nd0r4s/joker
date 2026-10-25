//! Class instances on the node (ADR 0084): composite and implicit
//! instances planned from their contracts, queries as derived signals,
//! method records expanded in the cycle, recorded in the trace and
//! expanded again by the replay.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use joker_core::ClockQuality;
use joker_core::image::{MethodCall, NO_TASK, Source as ProposalSource};
use joker_core::registry::Direction;
use jokerd::decisions::DecisionLog;
use jokerd::load::{instantiate, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, Pacer, SystemPacer};
use jokerd::replay::replay;
use jokerd::runner::{Running, Start};
use jokerd::traces::Source;

const MANIFEST: &str = r#"
[project]
name  = "classes"
joker = "1.0"
mode  = "dev"

[cycles.main]
period = "20ms"

[outputs."pi.act_led"]
safe  = false
group = "main"

[devices.pi]
driver = "sim"

[devices.pi.signals]
act_led = { type = "bool", direction = "output" }
key     = { type = "bool", direction = "input" }

[devices.esp]
driver  = "modbus-tcp"
address = "127.0.0.1:1"
unit_id = 1
profile = "local/bench-esp32@1"

[outputs."esp.led"]
safe = false

[instances.led_pi]
class = "std/led@1"
map   = { on = "pi.act_led" }

[instances.key]
class = "std/button@1"
map   = { pressed = "pi.key" }
"#;

fn loaded() -> jokerd::load::Loaded {
    let fs = common::fs(&[("joker.toml", MANIFEST)]);
    load(&fs, std::path::Path::new("joker.toml"), &NativeTasks).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn composite_and_implicit_instances_are_planned_from_their_contracts() {
    let plan = loaded().plan;
    let names: Vec<&str> = plan.instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["key", "led_pi", "esp"]);
    let led = &plan.instances[1];
    assert_eq!(led.classes, ["std/led@1"]);
    assert_eq!(led.group.as_deref(), Some("main"));
    let methods: Vec<&str> = led.methods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(methods, ["off", "on", "set", "toggle"]);
    assert_eq!(led.methods[1].writes, ["pi.act_led"]);
    let esp = &plan.instances[2];
    assert!(esp.implicit);
    assert_eq!(esp.classes, ["std/button@1", "std/led@1"]);
    assert_eq!(esp.method("toggle").unwrap().1.writes, ["esp.led"]);
    for query in [
        "key.clicked",
        "key.long_hold",
        "esp.clicked",
        "esp.released",
    ] {
        let s = plan.signal(query).unwrap_or_else(|| panic!("{query}"));
        assert_eq!(s.direction, Direction::Derived);
        assert!(plan.calcs.iter().any(|c| c.path == query), "{query}");
    }
    let clicked = plan.calcs.iter().find(|c| c.path == "key.clicked").unwrap();
    assert_eq!(clicked.expr.source, "rose(key.pressed)");
}

#[test]
fn a_method_call_is_recorded_and_replays_with_zero_divergences() {
    let loaded = loaded();
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("d.jsonl")).unwrap(),
    ));
    let clock = Monotonic::start();
    let tasks = instantiate(&loaded.plan, &loaded.manifest, &NativeTasks).unwrap();
    let running = Running::start(
        Arc::new(loaded.plan.clone()),
        tasks,
        Box::new(SystemPacer::new(clock, ClockQuality::Unsynced)),
        log,
        Start {
            generation: 1,
            inherit: Default::default(),
            retention: Duration::from_secs(60),
            max_ticks: None,
            utc_origin_ns: 0,
            cpus: Vec::new(),
        },
    )
    .unwrap();
    let plan = &running.plan;
    let instance = plan
        .instances
        .iter()
        .position(|i| i.name == "led_pi")
        .unwrap();
    let (method, _) = plan.instances[instance].method("toggle").unwrap();
    let group = plan.groups.iter().position(|g| g.name == "main").unwrap();
    let writer = plan.writer("user:manual:4").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let from = SystemPacer::new(clock, ClockQuality::Unsynced)
        .now()
        .mono_ns;
    for _ in 0..3 {
        running.shared.method_inboxes[group]
            .push(MethodCall {
                writer,
                instance: instance as u32,
                method: method as u16,
                source: ProposalSource::Async,
                task: NO_TASK,
                ttl_ns: 60_000_000_000,
                ..MethodCall::EMPTY
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }
    let led = plan.id("pi.act_led").unwrap();
    let source = Source {
        plan: Arc::clone(&running.plan),
        shared: Arc::clone(&running.shared),
        ring: Arc::clone(&running.ring),
        generation: (1, loaded.plan.manifest_hash.clone()),
        utc_origin_ns: 0,
        clock: 2,
    };
    std::thread::sleep(Duration::from_millis(200));
    let to = SystemPacer::new(clock, ClockQuality::Unsynced)
        .now()
        .mono_ns;
    let bytes = source.cut(from, to, None).unwrap();
    let status = running.shared.output(led).unwrap().read().unwrap();
    running.stop().unwrap();
    assert_eq!(status.0, 1, "three toggles from safe leave the LED on");
    let report = replay(&loaded.plan, &loaded.manifest, &NativeTasks, &bytes).unwrap();
    assert!(report.ticks > 20, "{report:?}");
    assert!(report.identical(), "{:?}", report.divergences.first());
}
