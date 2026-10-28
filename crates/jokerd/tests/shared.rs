//! `jokerd::shared::Shared`'s wide-slot table.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use joker_manifest::OsFileSystem;

use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::shared::Shared;

const JOKER_TOML: &str = r#"
[project]
name  = "shared-wide-test"
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
temp = { type = "bytes", direction = "input", reg = 0, len = 2, poll = "50ms" }
"#;

/// An output guarded by `requires_good` on a `bytes` signal (ADR 0062
/// decision 7, ADR 0088).
const GUARDED_TOML: &str = r#"
[project]
name  = "shared-wide-guard"
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
temp = { type = "bytes", direction = "input", reg = 0, len = 2, poll = "50ms" }

[devices.pi]
driver = "sim"

[devices.pi.signals]
led = { type = "bool", direction = "output" }

[outputs."pi.led"]
safe          = false
requires_good = ["sensor.temp"]
"#;

/// One tick with a manual `true` on `pi.led`, after `quality` reached
/// the wide slot of `sensor.temp` (or nothing, with `None`).
fn guarded_outcome(quality: Option<joker_core::Quality>) -> joker_core::Outcome {
    use joker_core::image::{Proposal, Source};
    use joker_core::time::{ClockQuality, Instant};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), GUARDED_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    let plan = &loaded.plan;
    let shared = Shared::new(plan);
    let temp = plan.id("sensor.temp").unwrap();
    if let Some(q) = quality {
        let bytes: &[u8] = if q.is_good() { &[0xAB, 0xCD] } else { &[] };
        shared.write_wide(temp, bytes, q, 999_000_000);
    }
    shared.inboxes[0]
        .push(Proposal {
            writer: plan.writer("user:manual:4").unwrap(),
            signal: plan.id("pi.led").unwrap(),
            value: 1,
            source: Source::Async,
            ttl_ns: 3_600_000_000_000,
            ..Proposal::EMPTY
        })
        .unwrap();
    jokerd::cycle::run(
        plan,
        &shared,
        vec![Vec::new()],
        &mut jokerd::pacer::VirtualPacer::new(Instant {
            mono_ns: 1_000_000_000,
            utc_ns: 1_760_000_000_000_000_000,
            clock: ClockQuality::Synced,
        }),
        jokerd::cycle::RunOptions {
            max_ticks: Some(1),
            ..jokerd::cycle::RunOptions::default()
        },
    )
    .unwrap();
    let (_, meta, _) = shared
        .output(plan.id("pi.led").unwrap())
        .unwrap()
        .read()
        .unwrap();
    joker_core::Outcome::from_code(meta.outcome, meta.detail).unwrap()
}

#[test]
fn requires_good_on_a_wide_signal_follows_its_wide_slot() {
    use joker_core::{Outcome, Quality, QualityCode, QualityReason};
    assert_eq!(
        guarded_outcome(Some(Quality::new(QualityCode::Good, QualityReason::None))),
        Outcome::Accepted,
        "a good wide sample satisfies requires_good"
    );
    assert_ne!(
        guarded_outcome(Some(Quality::new(
            QualityCode::Bad,
            QualityReason::DeviceFault
        ))),
        Outcome::Accepted,
        "a bad wide sample does not"
    );
    assert_ne!(
        guarded_outcome(None),
        Outcome::Accepted,
        "nor does a wide signal never read"
    );
}

#[test]
fn a_wide_signal_gets_a_wide_slot_and_a_scalar_signal_does_not() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());

    let shared = Arc::new(Shared::new(&loaded.plan));
    let temp_id = loaded.plan.id("sensor.temp").unwrap();
    assert!(
        shared.wide_slot(temp_id).is_some(),
        "a bytes signal must resolve to a WideSlot"
    );

    let out_of_range = joker_core::SignalId(temp_id.0 + 1);
    assert!(
        shared.wide_slot(out_of_range).is_none(),
        "an id past the end of the plan resolves to no wide slot"
    );
}

#[test]
fn a_wide_slot_starts_uninitialized_and_advances_after_a_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let shared = Shared::new(&loaded.plan);
    let temp_id = loaded.plan.id("sensor.temp").unwrap();
    let slot = shared.wide_slot(temp_id).unwrap();
    assert_eq!(slot.read(0).unwrap().seq(), 0);
    slot.write(
        b"hi",
        joker_core::Quality::GOOD,
        joker_core::value::NO_TS_SRC,
        1,
    );
    assert_eq!(slot.read(0).unwrap().seq(), 1);
}

#[test]
fn an_alarm_message_shows_a_wide_signal_from_its_wide_slot() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let shared = Shared::new(&loaded.plan);
    let template = "fault on {sensor.temp}";
    assert_eq!(
        jokerd::alarms::interpolate(template, &loaded.plan, &shared),
        "fault on ?"
    );

    let id = loaded.plan.id("sensor.temp").unwrap();
    shared.wide_slot(id).unwrap().write(
        &[0xAB, 0xCD],
        joker_core::Quality::GOOD,
        joker_core::value::NO_TS_SRC,
        1,
    );
    assert_eq!(
        jokerd::alarms::interpolate(template, &loaded.plan, &shared),
        "fault on q80="
    );

    let bad = joker_core::Quality::new(
        joker_core::QualityCode::Bad,
        joker_core::QualityReason::CommFailure,
    );
    shared
        .wide_slot(id)
        .unwrap()
        .write(&[], bad, joker_core::value::NO_TS_SRC, 2);
    assert_eq!(
        jokerd::alarms::interpolate(template, &loaded.plan, &shared),
        "fault on ?"
    );
}
