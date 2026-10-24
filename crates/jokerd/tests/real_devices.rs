//! `jokerd::real_devices::RealDevices`: a real manifest device, spawned
//! through the actual `Devices::start` dispatch, running the real
//! `joker_driver_sysfs::SysfsDriver` against a fake `/sys` tree.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use joker_manifest::OsFileSystem;

use jokerd::devices::Devices;
use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::real_devices::RealDevices;
use jokerd::shared::Shared;

struct FakeSys(PathBuf);

impl FakeSys {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "joker-real-devices-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let zone = dir.join("class/thermal/thermal_zone0");
        fs::create_dir_all(&zone).unwrap();
        fs::write(zone.join("temp"), "48312\n").unwrap();
        Self(dir)
    }
}

impl Drop for FakeSys {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const JOKER_TOML: &str = r#"
[project]
name  = "real-devices-test"
joker = "1.0"
mode  = "dev"

[devices.pi]
driver = "sysfs"

[devices.pi.signals]
cpu_temp = { type = "f64", direction = "input", thermal = "thermal_zone0", poll = "20ms" }
"#;

#[test]
fn a_real_sysfs_device_reaches_shared_slots() {
    let fake = FakeSys::new();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    assert_eq!(loaded.plan.devices.len(), 1);
    assert_eq!(loaded.plan.devices[0].driver, "sysfs");

    let plan = Arc::new(loaded.plan);
    let shared = Arc::new(Shared::new(&plan));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let devices = RealDevices::with_sysfs_root(runtime.handle().clone(), fake.0.to_str().unwrap());
    let set = devices.start(&plan, &shared);

    let temp_id = plan.id("pi.cpu_temp").unwrap();
    let mut settled = false;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(20));
        if let Ok(sample) = shared.slots[temp_id.index()].read(8)
            && sample.seq() != 0
        {
            settled = true;
            assert_eq!(f64::from_bits(sample.transport_bits()), 48.312);
            break;
        }
    }
    assert!(settled, "the sysfs driver never wrote a sample");

    set.stop();
    runtime.shutdown_timeout(Duration::from_secs(1));
}

const GPIO_JOKER_TOML: &str = r#"
[project]
name  = "real-devices-gpio-test"
joker = "1.0"
mode  = "dev"

[devices.hdr]
driver = "gpio"
chip   = "gpiochip0-that-does-not-exist-in-this-test"

[devices.hdr.signals]
line4 = { type = "bool", direction = "input", line = 4, poll = "20ms" }
"#;

#[test]
fn a_gpio_device_dispatches_and_registers_its_action_channel_even_without_the_chip() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), GPIO_JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    assert_eq!(loaded.plan.devices[0].driver, "gpio");

    let plan = Arc::new(loaded.plan);
    let shared = Arc::new(Shared::new(&plan));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let devices = RealDevices::new(runtime.handle().clone());
    let set = devices.start(&plan, &shared);

    let sender = set
        .action_sender("hdr")
        .expect("the dispatched gpio device registers its action channel");
    drop(sender);

    set.stop();
    runtime.shutdown_timeout(Duration::from_secs(1));
}

#[test]
fn a_device_whose_driver_cannot_be_set_up_reads_bad_config_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), GPIO_JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let plan = Arc::new(loaded.plan);
    let shared = Arc::new(Shared::new(&plan));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let set = RealDevices::new(runtime.handle().clone()).start(&plan, &shared);

    let line = plan.id("hdr.line4").unwrap();
    let mut failed = None;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(10));
        if let Ok(sample) = shared.slots[line.index()].read(8)
            && sample.seq() != 0
        {
            failed = Some(sample.quality());
            break;
        }
    }
    let quality = failed.expect("the failed setup reached the slot");
    assert_eq!(quality.code(), joker_core::QualityCode::Bad);
    assert_eq!(quality.reason(), joker_core::QualityReason::ConfigError);
    assert_eq!(
        shared.devices[0].link.get(),
        jokerd::real_devices::LINK_STOPPED
    );

    set.stop();
    runtime.shutdown_timeout(Duration::from_secs(1));
}

#[test]
fn the_dispatched_device_has_a_real_action_channel_and_an_undeclared_name_does_not() {
    let fake = FakeSys::new();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let plan = Arc::new(loaded.plan);
    let shared = Arc::new(Shared::new(&plan));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let devices = RealDevices::with_sysfs_root(runtime.handle().clone(), fake.0.to_str().unwrap());
    let set = devices.start(&plan, &shared);

    assert!(
        set.action_sender("not-a-real-device").is_none(),
        "an unknown device name must not resolve to a channel"
    );
    let sender = set
        .action_sender("pi")
        .expect("the dispatched sysfs device registers its action channel");

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let outcome = runtime.block_on(async {
        sender
            .send(jokerd::drivers::ActionCall {
                steps: Vec::new(),
                reply: reply_tx,
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), reply_rx)
            .await
            .expect("the driver task answered before the deadline")
            .unwrap()
    });
    assert_eq!(outcome, jokerd::drivers::ActionOutcome::Ok);

    set.stop();
    runtime.shutdown_timeout(Duration::from_secs(1));
}
