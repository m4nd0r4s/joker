//! AUR-404, docs/01 §8 (M1): one device-fallback scenario, run against
//! every driver kind that has outputs, each through its real driver and
//! `jokerd::drivers::run_driver` over its in-memory double. An applied
//! output reaches the device; when the equipment loses it, the keepalive
//! puts it back without a new commit; when the supervisor forces safe, the
//! output's `safe` reaches the device without the group ticking.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use joker_core::Outcome;
use joker_core::image::{Proposal, Source};
use joker_core::time::{ClockQuality, Instant};
use joker_driver_gpio::{GpioDriver, GpioPlanner};
use joker_driver_modbus::{ModbusTcp, ModbusTcpPlanner};
use joker_driver_revpi::image::{Device, FakeImage};
use joker_driver_revpi::{RevPiDriver, RevPiPlanner};
use joker_driver_sysfs::{SysfsDriver, SysfsPlanner};
use joker_hal::link::{DevicePaths, TcpEndpoint};
use joker_hal::testing::{MemoryGpio, VirtualHost};
use joker_hal::{
    Driver, GpioChip, IoError, Link, OpenArgs, PlanRequest, PlanView, Planner, TcpConnect, TcpIo,
    TcpStream,
};
use joker_manifest::OsFileSystem;
use jokerd::cycle::{RunOptions, run};
use jokerd::drivers::{point_targets, run_driver};
use jokerd::load::{Loaded, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::VirtualPacer;
use jokerd::plan::Plan;
use jokerd::shared::Shared;
use jokerd::supervisor;

const MS: u64 = 1_000_000;

const PROJECT: &str = "[project]\nname = \"fallback\"\njoker = \"1.0\"\nmode = \"dev\"\n";

/// When the scenario's steps happen, on the driver's clock.
struct Timeline {
    keepalive: u64,
    lose: u64,
    check: u64,
    force: u64,
    end: u64,
}

const TIMELINE: Timeline = Timeline {
    keepalive: 50 * MS,
    lose: 100 * MS,
    check: 250 * MS,
    force: 300 * MS,
    end: 450 * MS,
};

/// What the device showed at each step of the scenario.
#[derive(Debug, PartialEq)]
struct Seen {
    applied: bool,
    lost: bool,
    renewed: bool,
    safe: bool,
}

/// The scenario's expectations: the applied `true` reached the device, the
/// loss took effect, the keepalive renewed `true`, and the forced `safe`
/// (`false`) reached the device.
const EXPECTED: Seen = Seen {
    applied: true,
    lost: true,
    renewed: true,
    safe: true,
};

fn start() -> Instant {
    Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }
}

fn loaded(manifest: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, Loaded) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("joker.toml"),
        format!("{PROJECT}{manifest}"),
    )
    .unwrap();
    for (path, text) in files {
        let file = dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let l = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!l.report.has_errors(), "{}", l.report.render());
    (dir, l)
}

/// Applies `true` to `output` through the envelope, in one tick.
fn apply_true(plan: &Plan, shared: &Shared, output: &str) {
    shared.inboxes[0]
        .push(Proposal {
            writer: plan.writer("user:manual:4").unwrap(),
            signal: plan.id(output).unwrap(),
            value: 1,
            source: Source::Async,
            ttl_ns: 3_600_000 * MS,
            ..Proposal::EMPTY
        })
        .unwrap();
    run(
        plan,
        shared,
        vec![Vec::new()],
        &mut VirtualPacer::new(start()),
        RunOptions {
            max_ticks: Some(1),
            ..RunOptions::default()
        },
    )
    .unwrap();
    let (_, meta, _) = shared
        .output(plan.id(output).unwrap())
        .unwrap()
        .read()
        .unwrap();
    assert_eq!(
        Outcome::from_code(meta.outcome, meta.detail).unwrap(),
        Outcome::Accepted
    );
}

/// Runs `driver` for device 0 of `plan` through the scenario: `level`
/// reads the output on the device, `lose` makes the equipment drop it.
#[allow(
    clippy::too_many_arguments,
    reason = "the driver and its plan pieces, plus the two probes into its double"
)]
fn scenario<D: Driver>(
    driver: D,
    request: PlanRequest<'_>,
    hal_plan: PlanView<'_>,
    plan: &Plan,
    shared: &Arc<Shared>,
    keepalive: Option<u64>,
    level: impl Fn() -> bool,
    lose: impl Fn(),
) -> Seen {
    let t = TIMELINE;
    let targets = point_targets(plan, 0);
    let host = Arc::new(VirtualHost::new(0));
    let clock = Arc::clone(&host);
    let groups = supervisor::group_boxes(plan, shared);
    let seen = Rc::new(Cell::new((false, false, false)));
    let step = Rc::new(Cell::new(0u8));
    let (seen_in, step_in, shared_in) = (Rc::clone(&seen), Rc::clone(&step), Arc::clone(shared));
    let probe = &level;
    host.run(run_driver(
        driver,
        &*host,
        request,
        hal_plan,
        plan.devices[0].timeout_ns,
        &targets,
        shared,
        0,
        keepalive,
        tokio::sync::mpsc::channel(1).1,
        move || {
            let now = clock.mono();
            let (mut applied, mut lost, mut renewed) = seen_in.get();
            match step_in.get() {
                0 if now >= t.lose => {
                    applied = probe();
                    lose();
                    lost = !probe();
                    step_in.set(1);
                }
                1 if now >= t.check => {
                    renewed = probe();
                    step_in.set(2);
                }
                2 if now >= t.force => {
                    supervisor::force_safe(&shared_in, &groups);
                    step_in.set(3);
                }
                _ => {}
            }
            seen_in.set((applied, lost, renewed));
            now >= t.end
        },
    ))
    .unwrap();
    let (applied, lost, renewed) = seen.get();
    Seen {
        applied,
        lost,
        renewed,
        safe: !level(),
    }
}

fn sysfs(keepalive: Option<u64>) -> Seen {
    let (_dir, l) = loaded(
        "[devices.pi]\ndriver = \"sysfs\"\n\n[devices.pi.signals]\n\
         act = { type = \"bool\", direction = \"output\", led = \"led0\" }\n\n\
         [outputs.\"pi.act\"]\nsafe = false\naccept_missing_fallback = \"indicator LED, no load\"\n",
        &[],
    );
    let plan = &l.plan;
    let shared = Arc::new(Shared::new(plan));
    apply_true(plan, &shared, "pi.act");

    let sys = tempfile::tempdir().unwrap();
    let led = sys.path().join("class/leds/led0");
    std::fs::create_dir_all(&led).unwrap();
    std::fs::write(led.join("max_brightness"), "1\n").unwrap();
    std::fs::write(led.join("brightness"), "0\n").unwrap();
    std::fs::write(led.join("trigger"), "[mmc0] none\n").unwrap();

    let owned = jokerd::bridge::owned_request(&plan.devices[0]);
    let views = owned.views();
    let hal_plan = SysfsPlanner::plan(&views.request()).unwrap();
    let grants = joker_driver_sysfs::grants(&views.request());
    let paths = DevicePaths::new(
        sys.path(),
        &grants.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let driver = SysfsDriver::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Paths(paths),
    )
    .unwrap();
    let brightness = led.join("brightness");
    scenario(
        driver,
        views.request(),
        hal_plan.view(),
        plan,
        &shared,
        keepalive,
        || std::fs::read_to_string(&brightness).unwrap().trim() != "0",
        || std::fs::write(&brightness, "0\n").unwrap(),
    )
}

fn gpio(keepalive: Option<u64>) -> Seen {
    let (_dir, l) = loaded(
        "[devices.relay]\ndriver = \"gpio\"\n\n[devices.relay.signals]\n\
         coil = { type = \"bool\", direction = \"output\", line = 17 }\n\n\
         [outputs.\"relay.coil\"]\nsafe = false\naccept_missing_fallback = \"bench relay\"\n",
        &[],
    );
    let plan = &l.plan;
    let shared = Arc::new(Shared::new(plan));
    apply_true(plan, &shared, "relay.coil");

    let owned = jokerd::bridge::owned_request(&plan.devices[0]);
    let views = owned.views();
    let hal_plan = GpioPlanner::plan(&views.request()).unwrap();
    let (lines, handle) = MemoryGpio::new();
    let driver = GpioDriver::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Gpio(GpioChip::new(lines)),
    )
    .unwrap();
    scenario(
        driver,
        views.request(),
        hal_plan.view(),
        plan,
        &shared,
        keepalive,
        || handle.level(17),
        || handle.drive(17, false),
    )
}

const REVPI_BOARD: &str = include_str!("../../../boards/revpi-connect-5.board.toml");

fn revpi(keepalive: Option<u64>) -> Seen {
    let (_dir, l) = loaded(
        "[target]\nboard = \"revpi-connect-5\"\n\n\
         [devices.io]\ndriver = \"revpi\"\npoll = \"50ms\"\n\n[devices.io.signals]\n\
         lamp = { type = \"bool\", direction = \"output\", var = \"O_1\" }\n\n\
         [outputs.\"io.lamp\"]\nsafe = false\naccept_missing_fallback = \"bench lamp\"\n",
        &[("boards/revpi-connect-5.board.toml", REVPI_BOARD)],
    );
    let plan = &l.plan;
    let shared = Arc::new(Shared::new(plan));
    apply_true(plan, &shared, "io.lamp");

    let image = FakeImage::new();
    image.module(Device {
        address: 32,
        module_type: 96,
        input: (11, 70),
        output: (81, 18),
        active: true,
        ..Device::default()
    });
    image.variable("O_1", 81, 0, 1);

    let owned = jokerd::bridge::owned_request(&plan.devices[0]);
    let views = owned.views();
    let hal_plan = RevPiPlanner::plan(&views.request()).unwrap();
    let driver = RevPiDriver::with_image(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Box::new(image.clone()),
    )
    .unwrap();
    scenario(
        driver,
        views.request(),
        hal_plan.view(),
        plan,
        &shared,
        keepalive,
        || image.byte(81) & 1 == 1,
        || image.set_bytes(81, &[0]),
    )
}

/// A Modbus TCP server with one coil, answering FC 5 by its echo.
#[derive(Debug, Default)]
struct Coil {
    on: bool,
}

struct CoilIo {
    coil: Arc<Mutex<Coil>>,
    reply: VecDeque<u8>,
}

impl TcpIo for CoilIo {
    fn poll_read(&mut self, _cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        if self.reply.is_empty() {
            return Poll::Pending;
        }
        let n = buf.len().min(self.reply.len());
        for (b, r) in buf.iter_mut().zip(self.reply.drain(..n)) {
            *b = r;
        }
        Poll::Ready(Ok(n))
    }

    fn poll_write(&mut self, _cx: &mut Context<'_>, frame: &[u8]) -> Poll<Result<usize, IoError>> {
        let pdu = &frame[7..];
        let reply: Vec<u8> = match pdu[0] {
            5 => {
                self.coil.lock().unwrap().on = pdu[3] == 0xFF;
                pdu.to_vec()
            }
            fc => vec![fc | 0x80, 1],
        };
        self.reply.extend(&frame[..4]);
        self.reply
            .extend(u16::try_from(reply.len() + 1).unwrap().to_be_bytes());
        self.reply.push_back(frame[6]);
        self.reply.extend(reply);
        Poll::Ready(Ok(frame.len()))
    }

    fn poll_flush(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        Poll::Ready(Ok(()))
    }
}

struct CoilServer(Arc<Mutex<Coil>>);

impl TcpConnect for CoilServer {
    fn poll_connect(&mut self, _cx: &mut Context<'_>) -> Poll<Result<TcpStream, IoError>> {
        Poll::Ready(Ok(TcpStream::new(CoilIo {
            coil: Arc::clone(&self.0),
            reply: VecDeque::new(),
        })))
    }
}

const RELAY_PROFILE: &str = r#"
schema = 1

[profile]
name    = "acme/relay"
version = "1.0.0"
driver  = "modbus"

[signals.run]
at        = 0
fc        = 5
type      = "bool"
direction = "output"

[fallback]
timeout = "250ms"
value   = false
"#;

fn modbus_tcp(keepalive: Option<u64>) -> Seen {
    let (_dir, l) = loaded(
        "[devices.relay]\ndriver = \"modbus-tcp\"\naddress = \"127.0.0.1:502\"\nunit_id = 1\n\
         profile = \"acme/relay@1\"\ntimeout = \"100ms\"\n\n\
         [outputs.\"relay.run\"]\nsafe = false\n",
        &[("profiles/acme/relay@1.profile.toml", RELAY_PROFILE)],
    );
    let plan = &l.plan;
    assert_eq!(
        plan.devices[0].fallback_keepalive_ns,
        Some(TIMELINE.keepalive),
        "the keepalive is a fifth of the profile's fallback timeout"
    );
    let shared = Arc::new(Shared::new(plan));
    apply_true(plan, &shared, "relay.run");

    let coil = Arc::new(Mutex::new(Coil::default()));
    let owned = jokerd::bridge::owned_request(&plan.devices[0]);
    let views = owned.views();
    let hal_plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let driver = ModbusTcp::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Tcp(TcpEndpoint::new(CoilServer(Arc::clone(&coil)))),
    )
    .unwrap();
    scenario(
        driver,
        views.request(),
        hal_plan.view(),
        plan,
        &shared,
        keepalive,
        || coil.lock().unwrap().on,
        || coil.lock().unwrap().on = false,
    )
}

#[test]
fn sysfs_follows_the_fallback_scenario() {
    assert_eq!(sysfs(Some(TIMELINE.keepalive)), EXPECTED);
}

#[test]
fn gpio_follows_the_fallback_scenario() {
    assert_eq!(gpio(Some(TIMELINE.keepalive)), EXPECTED);
}

#[test]
fn revpi_follows_the_fallback_scenario() {
    assert_eq!(revpi(Some(TIMELINE.keepalive)), EXPECTED);
}

#[test]
fn modbus_tcp_follows_the_fallback_scenario() {
    assert_eq!(modbus_tcp(Some(TIMELINE.keepalive)), EXPECTED);
}

#[test]
fn without_a_keepalive_a_lost_output_stays_lost() {
    for (kind, seen) in [
        ("sysfs", sysfs(None)),
        ("gpio", gpio(None)),
        ("revpi", revpi(None)),
        ("modbus-tcp", modbus_tcp(None)),
    ] {
        assert!(seen.applied && seen.lost, "{kind}: {seen:?}");
        assert!(!seen.renewed, "{kind}: {seen:?}");
    }
}
