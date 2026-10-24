//! Action steps on the catalog drivers (AUR-426, ADR 0073): invocations
//! admitted by the daemon's own controller and invocation hub run on the
//! device's real io task, through the real `modbus-tcp`, `sysfs` and
//! `gpio` drivers, against a Modbus TCP server on loopback, a fake `/sys`
//! tree and in-memory lines.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use joker_core::ClockQuality;
use joker_hal::acquire::OwnedRaw;
use joker_hal::cbor::Value as Cbor;
use joker_hal::testing::{MemoryGpio, VirtualHost};
use joker_hal::{GpioChip, Link, OpenArgs, Planner};
use joker_manifest::OsFileSystem;
use jokerd::action_exec::{ActionProgram, FailureKind};
use jokerd::actions::{MAX_WAIT, Request, Role};
use jokerd::bridge::{DeviceStep, DeviceStepValue, owned_request};
use jokerd::control::{Config, Controller};
use jokerd::decisions::DecisionLog;
use jokerd::devices::Devices;
use jokerd::drivers::{ActionCall, ActionOutcome, point_targets, run_driver};
use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, SystemPacer};
use jokerd::real_devices::RealDevices;
use jokerd::shared::Shared;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Default)]
struct Registers {
    holding: BTreeMap<u16, u16>,
    input: BTreeMap<u16, u16>,
    faulty: Vec<u16>,
}

fn word(pdu: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([pdu[at], pdu[at + 1]])
}

impl Registers {
    fn serve(&mut self, pdu: &[u8]) -> Vec<u8> {
        let (fc, start, n) = (pdu[0], word(pdu, 1), word(pdu, 3));
        if self.faulty.contains(&start) {
            return vec![fc | 0x80, 4];
        }
        match fc {
            3 | 4 => {
                let regs = if fc == 3 { &self.holding } else { &self.input };
                let mut out = vec![fc, (2 * n) as u8];
                for at in start..start + n {
                    out.extend_from_slice(&regs.get(&at).copied().unwrap_or(0).to_be_bytes());
                }
                out
            }
            6 => {
                self.holding.insert(start, n);
                if start == 100 {
                    self.input.insert(10, 1);
                }
                pdu[..5].to_vec()
            }
            16 => {
                for i in 0..n {
                    self.holding
                        .insert(start + i, word(pdu, 6 + 2 * usize::from(i)));
                }
                pdu[..5].to_vec()
            }
            _ => vec![fc | 0x80, 1],
        }
    }
}

async fn serve_modbus(listener: tokio::net::TcpListener, registers: Arc<Mutex<Registers>>) {
    while let Ok((mut stream, _)) = listener.accept().await {
        let registers = registers.clone();
        tokio::spawn(async move {
            let mut header = [0u8; 7];
            while stream.read_exact(&mut header).await.is_ok() {
                let len = usize::from(u16::from_be_bytes([header[4], header[5]]));
                let mut pdu = vec![0u8; len.saturating_sub(1)];
                if stream.read_exact(&mut pdu).await.is_err() {
                    return;
                }
                let reply = registers.lock().unwrap().serve(&pdu);
                let mut out = header[..4].to_vec();
                out.extend_from_slice(&(reply.len() as u16 + 1).to_be_bytes());
                out.push(header[6]);
                out.extend(reply);
                if stream.write_all(&out).await.is_err() {
                    return;
                }
            }
        });
    }
}

const PROFILE: &str = r#"
[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"

[signals.state]
at   = 0
fc   = 4
type = "u16"

[actions.start]
args   = { level = "u64" }
result = { speed = "u16", power = "f32" }
steps  = [
    { write = { at = 100, fc = 6, type = "u16", value = "$level" } },
    { until = { at = 10, fc = 4, type = "u16", equals = 1, timeout = "500ms" } },
    { read = { at = 11, fc = 4, type = "u16", into = "speed" } },
    { write = { at = 200, fc = 16, type = "f32", word_order = "little", value = 2.5 } },
    { read = { at = 200, fc = 3, type = "f32", word_order = "little", into = "power" } },
]

[actions.faulty]
steps = [{ write = { at = 999, fc = 6, type = "u16", value = 1 } }]
"#;

fn manifest(port: u16) -> String {
    format!(
        r#"
[project]
name  = "driver-actions"
joker = "1.0"
mode  = "dev"

[devices.pcs]
driver  = "modbus-tcp"
address = "127.0.0.1:{port}"
unit_id = 1
profile = "acme/pcs@1"
poll    = "20ms"
timeout = "200ms"
"#
    )
}

fn request(key: &str, args: Map<String, Value>) -> Request {
    Request {
        key: key.to_owned(),
        args,
        timeout_ns: None,
        reason: None,
        wait: true,
        principal: "user:test".into(),
        role: Some(Role::Operator),
    }
}

fn controller(dir: &Path, devices: Arc<dyn Devices>) -> Controller {
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.join("decisions.jsonl")).unwrap(),
    ));
    let clock = Monotonic::start();
    Controller::new(
        dir,
        log,
        Arc::new(NativeTasks),
        devices,
        Box::new(move || Box::new(SystemPacer::new(clock, ClockQuality::Unsynced))),
        Config {
            project_root: dir.to_path_buf(),
            ..Config::default()
        },
    )
    .unwrap()
}

#[test]
fn a_modbus_tcp_action_runs_its_steps_on_the_real_driver() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let registers = Arc::new(Mutex::new(Registers::default()));
    registers.lock().unwrap().input.insert(11, 1450);
    registers.lock().unwrap().faulty.push(999);
    runtime.spawn(serve_modbus(listener, registers.clone()));

    let dir = tempfile::tempdir().unwrap();
    let profiles = dir.path().join("profiles/acme");
    std::fs::create_dir_all(&profiles).unwrap();
    std::fs::write(profiles.join("pcs@1.profile.toml"), PROFILE).unwrap();
    let mut controller = controller(
        dir.path(),
        Arc::new(RealDevices::new(runtime.handle().clone())),
    );
    let g = controller
        .propose(&manifest(port), "", None, false)
        .unwrap_or_else(|e| panic!("{e}: {:?}", e.errors));
    controller
        .activate(g.number, None, None)
        .unwrap_or_else(|e| panic!("{e}: {:?}", e.errors));
    let hub = controller.actions();
    let controller = Arc::new(Mutex::new(controller));

    let mut args = Map::new();
    args.insert("level".into(), json!(3));
    let answer = jokerd::actions::invoke(&controller, &hub, "pcs", "start", request("k1", args))
        .unwrap_or_else(|r| panic!("{r:?}"));
    let started = hub.wait(&answer.id, MAX_WAIT).unwrap().json(false);
    assert_eq!(started["state"], "succeeded", "{started}");
    assert_eq!(started["result"], json!({ "speed": 1450, "power": 2.5 }));
    {
        let r = registers.lock().unwrap();
        assert_eq!(r.holding.get(&100), Some(&3));
        let bits = 2.5f32.to_bits();
        assert_eq!(r.holding.get(&200), Some(&(bits as u16)));
        assert_eq!(r.holding.get(&201), Some(&((bits >> 16) as u16)));
    }

    let answer = jokerd::actions::invoke(
        &controller,
        &hub,
        "pcs",
        "faulty",
        request("k2", Map::new()),
    )
    .unwrap_or_else(|r| panic!("{r:?}"));
    let faulty = hub.wait(&answer.id, MAX_WAIT).unwrap().json(false);
    assert_eq!(faulty["state"], "failed", "{faulty}");
    assert_eq!(faulty["error"]["kind"], "device");
    assert_eq!(faulty["error"]["code"], "J-E1407");

    jokerd::daemon::lock_controller(&controller).shutdown();
    runtime.shutdown_timeout(Duration::from_secs(2));
}

fn literal_write(addr: Vec<u8>, type_name: &str, value: OwnedRaw) -> DeviceStep {
    DeviceStep::Write {
        addr,
        type_name: Some(type_name.to_owned()),
        value: DeviceStepValue::Literal(value),
    }
}

fn read(addr: Vec<u8>, type_name: &str, into: &str) -> DeviceStep {
    DeviceStep::Read {
        addr,
        type_name: Some(type_name.to_owned()),
        into: into.to_owned(),
    }
}

fn call(steps: Vec<DeviceStep>) -> (ActionCall, tokio::sync::oneshot::Receiver<ActionOutcome>) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    (
        ActionCall::new(ActionProgram::Steps(steps), 1_000_000_000, tx),
        rx,
    )
}

const SYSFS_TOML: &str = r#"
[project]
name  = "driver-actions-sysfs"
joker = "1.0"
mode  = "dev"

[outputs."pi.act"]
safe = false

[devices.pi]
driver = "sysfs"

[devices.pi.signals]
act      = { type = "bool", direction = "output", led = "ACT" }
cpu_temp = { type = "f64", direction = "input", thermal = "thermal_zone0", poll = "20ms" }
"#;

#[test]
fn a_sysfs_step_program_runs_on_the_device_io_task() {
    let sys = tempfile::tempdir().unwrap();
    let led = sys.path().join("class/leds/ACT");
    let zone = sys.path().join("class/thermal/thermal_zone0");
    std::fs::create_dir_all(&led).unwrap();
    std::fs::create_dir_all(&zone).unwrap();
    std::fs::write(led.join("brightness"), "0\n").unwrap();
    std::fs::write(led.join("max_brightness"), "255\n").unwrap();
    std::fs::write(led.join("trigger"), "[none] timer\n").unwrap();
    std::fs::write(zone.join("temp"), "48312\n").unwrap();

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), SYSFS_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    let plan = Arc::new(loaded.plan);
    let shared = Arc::new(Shared::new(&plan));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let devices =
        RealDevices::with_sysfs_root(runtime.handle().clone(), sys.path().to_str().unwrap());
    let set = devices.start(&plan, &shared);
    let sender = set.action_sender("pi").unwrap();

    let led_addr = Cbor::map([("led", Cbor::text("ACT"))]).encode();
    let zone_addr = Cbor::map([("thermal", Cbor::text("thermal_zone0"))]).encode();
    let (c, rx) = call(vec![
        literal_write(led_addr.clone(), "bool", OwnedRaw::Bool(true)),
        read(led_addr, "u16", "level"),
        read(zone_addr, "f64", "temp"),
    ]);
    let outcome = runtime.block_on(async {
        sender.send(c).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .unwrap()
            .unwrap()
    });
    assert_eq!(
        outcome,
        ActionOutcome::Ok(json!({ "level": 255, "temp": 48.312 }))
    );
    assert_eq!(
        std::fs::read_to_string(led.join("brightness")).unwrap(),
        "255"
    );

    set.stop();
    runtime.shutdown_timeout(Duration::from_secs(1));
}

const GPIO_TOML: &str = r#"
[project]
name  = "driver-actions-gpio"
joker = "1.0"
mode  = "dev"

[outputs."hdr.relay"]
safe = false

[devices.hdr]
driver = "gpio"
chip   = "gpiochip0"

[devices.hdr.signals]
button = { type = "bool", direction = "input", line = 17, active_low = true, poll = "20ms" }
relay  = { type = "bool", direction = "output", line = 22 }
"#;

#[test]
fn a_gpio_step_program_runs_on_the_device_io_task() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), GPIO_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    let plan = Arc::new(loaded.plan);
    let shared = Arc::new(Shared::new(&plan));
    let device = plan.devices[0].clone();
    let owned = owned_request(&device);
    let views = owned.views();
    let hal_plan = joker_driver_gpio::GpioPlanner::plan(&views.request()).unwrap();
    let (gpio, lines) = MemoryGpio::new();
    lines.drive(17, false);
    let driver = <joker_driver_gpio::GpioDriver as joker_hal::Driver>::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Gpio(GpioChip::new(gpio)),
    )
    .unwrap();

    let relay = Cbor::map([("line", Cbor::Uint(22))]).encode();
    let button = Cbor::map([("line", Cbor::Uint(17)), ("active_low", Cbor::Bool(true))]).encode();
    let (pressed, pressed_rx) = call(vec![
        DeviceStep::FailIf {
            addr: button.clone(),
            type_name: Some("bool".into()),
            equals: OwnedRaw::Bool(false),
            message: "not pressed".into(),
        },
        literal_write(relay.clone(), "bool", OwnedRaw::Bool(true)),
        read(relay, "bool", "relay"),
    ]);
    let (to_input, to_input_rx) = call(vec![literal_write(button, "bool", OwnedRaw::Bool(true))]);
    let (action_tx, action_rx) = tokio::sync::mpsc::channel(4);
    action_tx.try_send(pressed).unwrap();
    action_tx.try_send(to_input).unwrap();

    let host = Arc::new(VirtualHost::new(0));
    let targets = point_targets(&plan, 0);
    let stop_host = Arc::clone(&host);
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
        action_rx,
        move || stop_host.mono() >= 100_000_000,
    ))
    .unwrap();

    let mut pressed_rx = pressed_rx;
    let mut to_input_rx = to_input_rx;
    assert_eq!(
        pressed_rx.try_recv().unwrap(),
        ActionOutcome::Ok(json!({ "relay": true }))
    );
    assert!(lines.level(22));
    let ActionOutcome::Failed { kind, .. } = to_input_rx.try_recv().unwrap() else {
        panic!("writing an input line succeeded");
    };
    assert_eq!(kind, FailureKind::Unsupported);
}
