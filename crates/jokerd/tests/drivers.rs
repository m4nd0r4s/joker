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
    lines.drive(17, false);
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
                decode: jokerd::drivers::Decode::TYPED,
                acquisition: joker_manifest::context::Acquisition::NONE,
            })
        })
        .collect();
    assert_eq!(targets.len(), 1, "only `button` is an input");

    let host = Arc::new(VirtualHost::new(0));
    let stop_at = 200_000_000u64;
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

    assert_eq!(shared.commit_bindings[0].len(), 1);
}

#[test]
fn a_read_that_starts_failing_turns_the_slot_bad_instead_of_keeping_the_last_value() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    std::fs::create_dir_all(dir.path().join("apps/light")).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let device_plan = loaded.plan.devices[0].clone();
    let shared = Arc::new(Shared::new(&loaded.plan));
    let owned = jokerd::bridge::owned_request(&device_plan);
    let views = owned.views();
    let plan = GpioPlanner::plan(&views.request()).unwrap();

    let (chip, lines) = MemoryGpio::new();
    lines.drive(17, false);
    let driver = GpioDriver::open(
        &joker_hal::OpenArgs::new(views.request(), plan.view()),
        Link::Gpio(GpioChip::new(chip)),
    )
    .unwrap();
    let targets = jokerd::drivers::point_targets(&loaded.plan, 0);
    let button_id = loaded.plan.id("io.button").unwrap();

    let host = Arc::new(VirtualHost::new(0));
    let host_for_stop = Arc::clone(&host);
    let shared_for_stop = Arc::clone(&shared);
    let mut good_before_failure = false;
    host.run(run_driver(
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
        move || {
            let now = host_for_stop.mono();
            if now >= 120_000_000 && !good_before_failure {
                let before = shared_for_stop.slots[button_id.index()].read(8).unwrap();
                assert_eq!(before.get(), Ok(1), "the line read good before it failed");
                good_before_failure = true;
                lines.deny(17);
            }
            now >= 300_000_000
        },
    ))
    .unwrap();

    let sample = shared.slots[button_id.index()].read(8).unwrap();
    assert_eq!(sample.quality().code(), joker_core::QualityCode::Bad);
    assert_eq!(
        sample.quality().reason(),
        joker_core::QualityReason::ConfigError,
        "a denied line is the gpio driver's own bad(config_error)"
    );
    assert!(sample.get().is_err(), "a bad sample carries no value");
}

#[test]
fn a_value_that_does_not_convert_to_the_signal_type_is_bad_config_error() {
    use joker_hal::acquire::{OwnedRaw, Settled};
    use joker_hal::types::RawStatus;
    let target = PointTarget {
        point: 0,
        signal: joker_core::SignalId(0),
        ty: joker_core::ValueType::Bool,
        decode: jokerd::drivers::Decode::TYPED,
        acquisition: joker_manifest::context::Acquisition::NONE,
    };
    let settled = Settled {
        point: 0,
        value: OwnedRaw::Text("on".into()),
        status: RawStatus::OK,
        invalid: None,
        ts_hw: None,
    };
    let (bits, quality) = jokerd::drivers::settled_sample(&target, &settled);
    assert_eq!(bits, 0);
    assert_eq!(quality.code(), joker_core::QualityCode::Bad);
    assert_eq!(quality.reason(), joker_core::QualityReason::ConfigError);
}

#[test]
fn a_wide_read_reaches_its_wide_slot_on_success_and_on_failure() {
    use joker_driver_i2c::{I2cDriver, I2cPlanner};
    use joker_hal::testing::MemoryI2c;
    use joker_hal::{I2cBus, Link};

    const I2C_TOML: &str = r#"
[project]
name  = "drivers-i2c-test"
joker = "1.0"
mode  = "dev"

[buses.i2c1]
kind = "i2c"
dev  = "/dev/i2c-1"

[devices.sensor]
driver  = "i2c"
bus     = "i2c1"
address = 0x40

[devices.sensor.signals]
temp = { type = "bytes", direction = "input", reg = 0, len = 2, poll = "20ms" }
"#;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), I2C_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    let device_plan = loaded.plan.devices[0].clone();

    let shared = Arc::new(Shared::new(&loaded.plan));
    let owned = jokerd::bridge::owned_request(&device_plan);
    let views = owned.views();
    let plan = I2cPlanner::plan(&views.request()).unwrap();

    let (bus, handle) = MemoryI2c::new();
    handle.preload(0x40, &[0xAB, 0xCD]);
    let link = Link::I2c(I2cBus::new(bus));
    let driver = I2cDriver::open(
        &joker_hal::OpenArgs::new(views.request(), plan.view()),
        link,
    )
    .unwrap();

    let temp_id = loaded.plan.id("sensor.temp").unwrap();
    let targets = jokerd::drivers::point_targets(&loaded.plan, 0);

    let host = Arc::new(VirtualHost::new(0));
    let stop_at = 100_000_000u64;
    let host_for_stop = Arc::clone(&host);
    host.run(run_driver(
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
    ))
    .unwrap();

    let sample = shared.wide_slot(temp_id).unwrap().read(8).unwrap();
    assert!(
        sample.seq() > 0,
        "a successful wide read must advance past seq 0"
    );
    assert_eq!(sample.as_slice(), &[0xAB, 0xCD]);
    assert!(sample.quality().is_good());

    let host2 = Arc::new(VirtualHost::new(0));
    let (bus2, handle2) = MemoryI2c::new();
    handle2.preload(0x40, &[0xAB, 0xCD]);
    handle2.deny(0x40);
    let link2 = Link::I2c(I2cBus::new(bus2));
    let driver2 = I2cDriver::open(
        &joker_hal::OpenArgs::new(views.request(), plan.view()),
        link2,
    )
    .unwrap();
    let shared2 = Arc::new(Shared::new(&loaded.plan));
    let host2_for_stop = Arc::clone(&host2);
    host2
        .run(run_driver(
            driver2,
            &*host2,
            views.request(),
            plan.view(),
            2_000_000_000,
            &targets,
            &shared2,
            0,
            None,
            tokio::sync::mpsc::channel(1).1,
            move || host2_for_stop.mono() >= stop_at,
        ))
        .unwrap();
    let failed_sample = shared2.wide_slot(temp_id).unwrap().read(8).unwrap();
    assert!(
        failed_sample.seq() > 0,
        "a FAILED wide read must also advance past seq 0, with a bad quality"
    );
    assert_eq!(failed_sample.quality().code(), joker_core::QualityCode::Bad);
    assert!(
        failed_sample.as_slice().is_empty(),
        "a bad wide sample carries no bytes"
    );
    let scalar = shared2.slots[temp_id.index()].read(8).unwrap();
    assert_eq!(
        scalar.seq(),
        0,
        "the scalar slot of a wide signal is never written"
    );
}

fn wide_target(ty: joker_core::ValueType) -> PointTarget {
    PointTarget {
        point: 0,
        signal: joker_core::SignalId(0),
        ty,
        decode: jokerd::drivers::Decode::TYPED,
        acquisition: joker_manifest::context::Acquisition::NONE,
    }
}

fn settled(value: joker_hal::acquire::OwnedRaw) -> joker_hal::acquire::Settled {
    joker_hal::acquire::Settled {
        point: 0,
        value,
        status: joker_hal::types::RawStatus::OK,
        invalid: None,
        ts_hw: None,
    }
}

#[test]
fn a_wide_value_that_does_not_decode_is_bad_config_error() {
    use joker_core::{QualityCode, QualityReason, ValueType};
    use joker_hal::acquire::OwnedRaw;

    let config_error = |(bytes, quality): (Vec<u8>, joker_core::Quality)| {
        assert!(bytes.is_empty(), "{bytes:?}");
        assert_eq!(quality.code(), QualityCode::Bad);
        assert_eq!(quality.reason(), QualityReason::ConfigError);
    };
    let bytes = wide_target(ValueType::Bytes);
    config_error(jokerd::drivers::settled_wide(
        &bytes,
        &settled(OwnedRaw::Bytes(vec![0x55; 65])),
    ));
    config_error(jokerd::drivers::settled_wide(
        &bytes,
        &settled(OwnedRaw::Uint(7)),
    ));
    let string = wide_target(ValueType::String);
    config_error(jokerd::drivers::settled_wide(
        &string,
        &settled(OwnedRaw::Text("caf\u{e9}".into())),
    ));
    config_error(jokerd::drivers::settled_wide(
        &string,
        &settled(OwnedRaw::Bytes(vec![0xff, 0xfe])),
    ));

    let (ok, quality) =
        jokerd::drivers::settled_wide(&string, &settled(OwnedRaw::Text("ok".into())));
    assert_eq!(ok, b"ok");
    assert!(quality.is_good());
    let regs = PointTarget {
        decode: jokerd::drivers::Decode::of_point("string", &[]),
        ..wide_target(ValueType::String)
    };
    let (text, _) =
        jokerd::drivers::settled_wide(&regs, &settled(OwnedRaw::Regs(vec![0x4f4b, 0x2000])));
    assert_eq!(
        text, b"OK \0",
        "two bytes per register, big-endian, padding kept"
    );
}

#[test]
fn a_wide_signal_turns_bad_when_its_link_goes_down() {
    const I2C_TOML: &str = r#"
[project]
name  = "drivers-i2c-down"
joker = "1.0"
mode  = "dev"

[buses.i2c1]
kind = "i2c"
dev  = "/dev/i2c-1"

[devices.sensor]
driver  = "i2c"
bus     = "i2c1"
address = 0x40

[devices.sensor.signals]
temp = { type = "bytes", direction = "input", reg = 0, len = 2, poll = "20ms" }
"#;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), I2C_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let shared = Shared::new(&loaded.plan);
    let temp_id = loaded.plan.id("sensor.temp").unwrap();
    let targets = jokerd::drivers::point_targets(&loaded.plan, 0);
    shared.wide_slot(temp_id).unwrap().write(
        b"\x01\x02",
        joker_core::Quality::GOOD,
        joker_core::value::NO_TS_SRC,
        1,
    );

    jokerd::drivers::mark_bad(&shared, &targets, joker_core::QualityReason::CommFailure, 2);

    let sample = shared.wide_slot(temp_id).unwrap().read(8).unwrap();
    assert_eq!(sample.quality().code(), joker_core::QualityCode::Bad);
    assert_eq!(
        sample.quality().reason(),
        joker_core::QualityReason::CommFailure
    );
    assert!(
        sample.as_slice().is_empty(),
        "the last good value never stays behind a failure"
    );
    assert_eq!(shared.slots[temp_id.index()].read(8).unwrap().seq(), 0);
}
