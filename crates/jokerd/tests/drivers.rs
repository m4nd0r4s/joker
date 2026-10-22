//! `jokerd::drivers::run_driver`: the io-domain loop against a real
//! `Driver` (here `joker_driver_gpio::GpioDriver` over an in-memory
//! `MemoryGpio`), writing into `Shared.slots` and draining a real
//! `CommitRx` for writes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;

use joker_driver_gpio::{GpioDriver, GpioPlanner};
use joker_hal::testing::{MemoryGpio, VirtualHost};
use joker_hal::{Driver, GpioChip, Link, Planner};
use joker_manifest::OsFileSystem;

use jokerd::drivers::{PointTarget, run_driver};
use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::shared::Shared;

const JOKER_TOML: &str = r#"
[project]
name  = "drivers-test"
joker = "1.0"
mode  = "dev"

[outputs."io.relay"]
safe = false

[cycles.main]
period = "20ms"

[apps.light]
path   = "apps/light"
bind   = { led = "io.relay" }
params = { half_period = "500 ms" }
cycles = { blink = "main" }

[devices.io]
driver = "gpio"
chip   = "gpiochip0"

[devices.io.signals]
button = { type = "bool", direction = "input", line = 17, active_low = true, poll = "50ms" }
relay  = { type = "bool", direction = "output", line = 22 }
"#;

#[test]
fn run_driver_reads_and_writes_through_the_real_commit_channel() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    std::fs::create_dir_all(dir.path().join("apps/light")).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    assert_eq!(loaded.plan.devices.len(), 1);
    let device_plan = loaded.plan.devices[0].clone();
    assert_eq!(device_plan.points.len(), 2);

    let shared = Arc::new(Shared::new(&loaded.plan));
    let owned = jokerd::bridge::owned_request(&device_plan);
    let views = owned.views();
    let plan = GpioPlanner::plan(&views.request()).unwrap();

    let (chip, lines) = MemoryGpio::new();
    lines.drive(17, false); // active_low: physical false -> logical true
    let link = Link::Gpio(GpioChip::new(chip));
    let driver = GpioDriver::open(
        &joker_hal::OpenArgs::new(views.request(), plan.view()),
        link,
    )
    .unwrap();

    let targets: Vec<PointTarget> = device_plan
        .points
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            if p.direction != joker_hal::Direction::Input {
                return None;
            }
            Some(PointTarget {
                point: i as u32,
                signal: loaded.plan.id(&p.path)?,
                ty: joker_core::ValueType::Bool,
            })
        })
        .collect();
    assert_eq!(targets.len(), 1, "only `button` is an input");

    let host = Arc::new(VirtualHost::new(0));
    let stop_at = 200_000_000u64; // 200 ms of virtual time
    let host_for_stop = Arc::clone(&host);
    let result = host.run(run_driver(
        driver,
        &*host,
        views.request(),
        plan.view(),
        2_000_000_000,
        &targets,
        &shared,
        0,
        None,
        tokio::sync::mpsc::channel(1).1,
        move || host_for_stop.mono() >= stop_at,
    ));
    assert!(result.is_ok(), "the loop must end via stop(), not stall");

    let button_id = loaded.plan.id("io.button").unwrap();
    let sample = shared.slots[button_id.index()].read(8).unwrap();
    assert_eq!(sample.transport_bits(), 1, "button settled to true");

    // No cycle thread runs in this test (that path is commit_channel.rs's
    // job); this only confirms Shared::new built the output binding this
    // driver's write-drain loop reads from.
    assert_eq!(shared.commit_bindings[0].len(), 1);
}
