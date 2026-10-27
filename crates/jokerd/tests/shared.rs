//! `jokerd::shared::Shared`'s wide-slot table.

#![allow(clippy::unwrap_used)]

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
