//! AUR-402, ADR 0078: a `revpi` device on the `revpi-connect-5` board runs
//! in jokerd. Its signals are PiCtory variables; the cycle loop applies
//! the outputs, the device's io task writes them through the real `revpi`
//! driver into a fake piControl process image, and reads the inputs back
//! into the shared slots.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use joker_core::image::{Proposal, Source};
use joker_core::time::{ClockQuality, Instant};
use joker_core::{Outcome, QualityCode, QualityReason};
use joker_driver_revpi::RevPiDriver;
use joker_driver_revpi::image::{Device, FakeImage};
use joker_hal::testing::VirtualHost;
use joker_hal::{OpenArgs, Planner};
use joker_manifest::OsFileSystem;
use jokerd::cycle::{RunOptions, run};
use jokerd::drivers::{point_targets, run_driver};
use jokerd::load::{Loaded, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::VirtualPacer;
use jokerd::shared::Shared;

const MS: u64 = 1_000_000;

const BOARD: &str = include_str!("../../../boards/revpi-connect-5.board.toml");

const MANIFEST: &str = r#"
[project]
name  = "revpi"
joker = "1.0"
mode  = "dev"

[target]
board = "revpi-connect-5"

[devices.io]
driver = "revpi"
poll   = "50ms"

[devices.io.signals]
start            = { type = "bool", var = "I_1" }
lamp             = { type = "bool", direction = "output", var = "O_1" }
level            = { type = "i64", unit = "mV", var = "InputValue_1" }
setpoint         = { type = "i64", unit = "1", direction = "output", var = "OutputValue_1" }
core_temperature = { type = "u64", unit = "\u00b0C" }

[outputs."io.lamp"]
safe                    = false
accept_missing_fallback = "bench lamp"

[outputs."io.setpoint"]
safe                    = 0
range                   = [-10000, 10000]
accept_missing_fallback = "bench analog output"
"#;

fn loaded() -> (tempfile::TempDir, Loaded) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), MANIFEST).unwrap();
    std::fs::create_dir_all(dir.path().join("boards")).unwrap();
    std::fs::write(dir.path().join("boards/revpi-connect-5.board.toml"), BOARD).unwrap();
    let l = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    (dir, l)
}

fn image() -> FakeImage {
    let image = FakeImage::new();
    image
        .module(Device {
            address: 0,
            module_type: 136,
            input: (0, 6),
            output: (6, 5),
            active: true,
            ..Device::default()
        })
        .module(Device {
            address: 32,
            module_type: 96,
            input: (11, 70),
            output: (81, 18),
            active: true,
            ..Device::default()
        })
        .module(Device {
            address: 33,
            module_type: 103,
            input: (99, 20),
            output: (119, 8),
            active: true,
            ..Device::default()
        });
    image
        .variable("core_temperature", 2, 0, 8)
        .variable("I_1", 11, 0, 1)
        .variable("O_1", 81, 0, 1)
        .variable("InputValue_1", 99, 0, 16)
        .variable("OutputValue_1", 119, 0, 16);
    image.set_bytes(2, &[47]);
    image.set_bytes(11, &[1]);
    image.set_bytes(99, &(-512i16).to_le_bytes());
    image
}

fn start() -> Instant {
    Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }
}

#[test]
fn a_revpi_device_writes_and_reads_the_process_image_through_jokerd() {
    let (_dir, l) = loaded();
    let plan = &l.plan;
    let shared = Arc::new(Shared::new(plan));
    for (path, bits) in [("io.lamp", 1u64), ("io.setpoint", (-300i64) as u64)] {
        shared.inboxes[0]
            .push(Proposal {
                writer: plan.writer("user:manual:4").unwrap(),
                signal: plan.id(path).unwrap(),
                value: bits,
                source: Source::Async,
                ttl_ns: 3_600_000 * MS,
                ..Proposal::EMPTY
            })
            .unwrap();
    }
    run(
        plan,
        &shared,
        vec![Vec::new()],
        &mut VirtualPacer::new(start()),
        RunOptions {
            max_ticks: Some(1),
            ..RunOptions::default()
        },
    )
    .unwrap();
    for path in ["io.lamp", "io.setpoint"] {
        let (_, meta, _) = shared
            .output(plan.id(path).unwrap())
            .unwrap()
            .read()
            .unwrap();
        assert_eq!(
            Outcome::from_code(meta.outcome, meta.detail).unwrap(),
            Outcome::Accepted,
            "{path}"
        );
    }

    let device = plan.devices[0].clone();
    assert_eq!(device.driver, "revpi");
    let owned = jokerd::bridge::owned_request(&device);
    let views = owned.views();
    let hal_plan = joker_driver_revpi::RevPiPlanner::plan(&views.request()).unwrap();
    let fake = image();
    let driver = RevPiDriver::with_image(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Box::new(fake.clone()),
    )
    .unwrap();
    let targets = point_targets(plan, 0);
    let host = Arc::new(VirtualHost::new(0));
    let clock = Arc::clone(&host);
    host.run(run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        device.timeout_ns,
        &targets,
        &shared,
        0,
        None,
        tokio::sync::mpsc::channel(1).1,
        move || clock.mono() >= 200 * MS,
    ))
    .unwrap();

    assert_eq!(fake.byte(81) & 1, 1, "the lamp's bit is set");
    assert_eq!(
        i16::from_le_bytes([fake.byte(119), fake.byte(120)]),
        -300,
        "the setpoint's word"
    );
    let value = |path: &str| {
        let s = shared.slots[plan.id(path).unwrap().index()]
            .read(8)
            .unwrap();
        let q = s.quality();
        assert_eq!(
            (q.code(), q.reason()),
            (QualityCode::Good, QualityReason::None),
            "{path}"
        );
        s.transport_bits()
    };
    assert_eq!(value("io.start"), 1);
    assert_eq!(value("io.level") as i64, -512);
    assert_eq!(
        value("io.core_temperature"),
        47,
        "without `var`, the variable has the signal's name"
    );
}

#[test]
fn a_revpi_device_with_a_signal_of_another_type_is_not_started() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("joker.toml"),
        MANIFEST.replace(
            "core_temperature = { type = \"u64\", unit = \"\\u00b0C\" }",
            "core_temperature = { type = \"f64\", unit = \"\\u00b0C\" }",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("boards")).unwrap();
    std::fs::write(dir.path().join("boards/revpi-connect-5.board.toml"), BOARD).unwrap();
    let l = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        l.plan.devices.iter().all(|d| d.driver != "revpi"),
        "a variable is bool, i64 or u64: the device is not started"
    );
}
