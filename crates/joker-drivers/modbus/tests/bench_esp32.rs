//! `modbus-rtu` with the bench profile against an in-memory double of the
//! ESP32 bench firmware (docs/23 §4), written from the register table.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use joker_core::ValueType;
use joker_driver_modbus::plan::Request;
use joker_driver_modbus::profile::serial_bus;
use joker_driver_modbus::rtu::crc16;
use joker_driver_modbus::{ModbusPlanner, ModbusRtu, Profile};
use joker_hal::acquire::{OwnedRaw, Settled};
use joker_hal::link::SerialInfo;
use joker_hal::testing::{
    Bench, Event, LineFault, LineHandle, MemorySerial, OutputHarness, VirtualHost,
};
use joker_hal::{
    CommitBox, CommitRx, CommitTx, CommittedWrite, Link, Planner, RawEncoding, RawStatus, Reason,
    RxState, SerialPort, WriteStatus,
};

const MS: u64 = 1_000_000;
const S: u64 = 1_000 * MS;

#[derive(Debug, Default)]
struct Esp {
    led: bool,
    button: bool,
    touch: u16,
    rssi: i16,
    uptime: u32,
    last_request: u64,
    corrupt: bool,
}

impl Esp {
    fn tick(&mut self, now: u64) {
        if now.saturating_sub(self.last_request) >= 2 * S {
            self.led = false;
        }
    }

    fn serve(&mut self, frame: &[u8], now: u64) -> Vec<u8> {
        if frame.len() < 4
            || crc16(&frame[..frame.len() - 2]).to_le_bytes()
                != [frame[frame.len() - 2], frame[frame.len() - 1]]
        {
            return Vec::new();
        }
        if frame[0] != 1 {
            return Vec::new();
        }
        self.tick(now);
        self.last_request = now;
        let fc = frame[1];
        let a = u16::from_be_bytes([frame[2], frame[3]]);
        let b = u16::from_be_bytes([frame[4], frame[5]]);
        let size = match fc {
            1 | 5 => 1,
            2 => 1,
            3 => 1,
            4 => 4,
            _ => 0,
        };
        let mut pdu = vec![fc];
        if fc == 0 || fc > 5 {
            pdu = vec![fc | 0x80, 1];
        } else if fc == 5 {
            if a >= 1 || (b != 0xFF00 && b != 0) {
                pdu = vec![fc | 0x80, if a >= 1 { 2 } else { 3 }];
            } else {
                self.led = b == 0xFF00;
                pdu.extend_from_slice(&frame[2..6]);
            }
        } else if b == 0 || u32::from(a) + u32::from(b) > size {
            pdu = vec![fc | 0x80, 2];
        } else if fc <= 2 {
            let bit = if fc == 1 { self.led } else { self.button };
            pdu.extend_from_slice(&[1, u8::from(bit)]);
        } else {
            let regs: Vec<u16> = if fc == 3 {
                vec![1]
            } else {
                vec![
                    self.touch,
                    self.rssi as u16,
                    (self.uptime >> 16) as u16,
                    self.uptime as u16,
                ]
            };
            pdu.push((2 * b) as u8);
            for r in &regs[usize::from(a)..usize::from(a + b)] {
                pdu.extend_from_slice(&r.to_be_bytes());
            }
        }
        let mut out = vec![1];
        out.extend(pdu);
        let crc = crc16(&out).to_le_bytes();
        out.extend_from_slice(&crc);
        if self.corrupt {
            let n = out.len();
            out[n - 1] ^= 0xFF;
        }
        out
    }
}

fn profile() -> Profile {
    Profile::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/project"),
        "local/bench-esp32@1",
    )
    .unwrap()
}

struct Rig {
    esp: Arc<Mutex<Esp>>,
    line: LineHandle,
    host: Arc<VirtualHost>,
}

fn new_rig() -> (Rig, SerialPort) {
    let esp = Arc::new(Mutex::new(Esp {
        touch: 612,
        rssi: -67,
        uptime: 0x0001_0002,
        button: true,
        ..Esp::default()
    }));
    let host = Arc::new(VirtualHost::new(0));
    let e = esp.clone();
    let (serial, line) = MemorySerial::new(
        host.clone(),
        SerialInfo::new_8n1(115_200),
        Box::new(move |req, now| e.lock().unwrap().serve(req, now)),
    );
    (Rig { esp, line, host }, SerialPort::new(serial))
}

struct Writes {
    harness: OutputHarness,
    tx: CommitTx<'static>,
    rx: CommitRx<'static>,
}

impl Writes {
    fn new(point: u32) -> Self {
        let harness = OutputHarness::new(&[("esp.led", ValueType::Bool, 0, point)]).unwrap();
        let bindings: &'static [_] =
            Box::leak(harness.bindings(&[RawEncoding::Bool]).into_boxed_slice());
        let boxes: &'static [CommitBox] = Box::leak(Box::new([CommitBox::new()]));
        let tx = CommitTx::new(
            harness.outputs(),
            bindings,
            boxes,
            Box::leak(Box::new([None])),
        )
        .unwrap();
        let rx = CommitRx::new(bindings, boxes, Box::leak(Box::new([RxState::default()]))).unwrap();
        Self { harness, tx, rx }
    }

    fn set(&mut self, on: bool) -> Vec<CommittedWrite<'static>> {
        self.harness.manual("esp.led", u64::from(on));
        self.tx.publish(self.harness.outputs());
        let mut out = Vec::new();
        self.rx.drain_into(&mut out);
        out
    }
}

fn value(p: &Profile, name: &str, settled: &[Settled], ids: &[u32]) -> Option<f64> {
    let id = p.signals.iter().position(|s| s.name == name)? as u32;
    let k = ids.iter().position(|i| *i == id)?;
    p.signal(name)?.decode(&settled[k].value)
}

#[test]
fn the_bench_profile_plans_four_requests() {
    let p = profile();
    let req = p.request(
        "esp",
        1,
        S,
        100 * MS,
        serial_bus("esp32", "/dev/ttyUSB0", 115_200),
    );
    let views = req.views();
    let plan = ModbusPlanner::plan(&views.request()).unwrap();
    let view = plan.view();
    assert_eq!(joker_hal::plan::validate(&views.request(), &view), Ok(()));
    let requests: Vec<(u8, u16, u16, usize)> = view
        .iter()
        .map(|b| {
            let r = Request::decode(b.private).unwrap();
            (r.fc, r.start, r.count, b.points.len())
        })
        .collect();
    assert_eq!(
        requests,
        [(1, 0, 1, 1), (2, 0, 1, 1), (3, 0, 1, 1), (4, 0, 4, 3)]
    );
    assert!(view.iter().all(|b| b.head.cost_ns > 10 * MS));
}

#[test]
fn reads_decode_writes_echo_and_readback_follows() {
    let p = profile();
    let req = p.request(
        "esp",
        1,
        S,
        100 * MS,
        serial_bus("esp32", "/dev/ttyUSB0", 115_200),
    );
    let views = req.views();
    let plan = ModbusPlanner::plan(&views.request()).unwrap();
    let (rig, port) = new_rig();
    let mut bench: Bench<'_, ModbusRtu> = Bench::open(
        rig.host.clone(),
        views.request(),
        plan.view(),
        Link::Serial(port),
    )
    .unwrap();
    bench.connect().unwrap();
    let inputs = plan.view().batch(3).unwrap().points.to_vec();
    let s = bench.read(3).unwrap();
    assert!(s.iter().all(|x| x.status == RawStatus::OK));
    assert_eq!(value(&p, "touch", &s, &inputs), Some(612.0));
    assert_eq!(value(&p, "rssi", &s, &inputs), Some(-67.0));
    assert_eq!(value(&p, "uptime", &s, &inputs), Some(65_538.0));
    let s = bench.read(1).unwrap();
    assert_eq!(s[0].value, OwnedRaw::Bool(true));
    let fw = bench.read(2).unwrap();
    assert_eq!(fw[0].value, OwnedRaw::Regs(vec![1]));

    let led = p.signals.iter().position(|s| s.name == "led").unwrap() as u32;
    let mut writes = Writes::new(led);
    let w = writes.set(true);
    assert_eq!(bench.write(&w).unwrap()[0].status, WriteStatus::Ok);
    assert!(rig.esp.lock().unwrap().led);
    assert_eq!(bench.read(0).unwrap()[0].value, OwnedRaw::Bool(true));
    let written = rig.line.written();
    assert_eq!(
        written[written.len() - 16..written.len() - 8],
        joker_driver_modbus::rtu::request(1, 5, 0, 0xFF00)
    );
    assert_eq!(
        written[written.len() - 8..],
        joker_driver_modbus::rtu::request(1, 1, 0, 1)
    );
}

#[test]
fn unplugged_cable_gives_comm_failure_within_poll_plus_timeout() {
    let p = profile();
    let poll = 100 * MS;
    let timeout = S;
    let req = p.request(
        "esp",
        1,
        timeout,
        poll,
        serial_bus("esp32", "/dev/ttyUSB0", 115_200),
    );
    let views = req.views();
    let plan = ModbusPlanner::plan(&views.request()).unwrap();
    let (rig, port) = new_rig();
    let mut bench: Bench<'_, ModbusRtu> = Bench::open(
        rig.host.clone(),
        views.request(),
        plan.view(),
        Link::Serial(port),
    )
    .unwrap();
    bench.connect().unwrap();
    let led = p.signals.iter().position(|s| s.name == "led").unwrap() as u32;
    let mut writes = Writes::new(led);
    let mut pending = writes.set(true);
    let before = bench
        .run_until(3 * S, |_| std::mem::take(&mut pending))
        .unwrap();
    assert!(before.iter().all(|e| match e {
        Event::Read { points, .. } => points.iter().all(|s| s.status == RawStatus::OK),
        Event::Write { acks, .. } => acks.iter().all(|a| a.status == WriteStatus::Ok),
    }));
    assert!(rig.esp.lock().unwrap().led);

    let unplugged = rig.host.mono() + 37 * MS;
    rig.host.advance_to(unplugged);
    rig.line.set_fault(LineFault::Silent);
    let after = bench.run_until(unplugged + 5 * S, |_| Vec::new()).unwrap();
    let first_bad = after
        .iter()
        .find_map(|e| match e {
            Event::Read { at, points, .. }
                if points.iter().all(|s| {
                    s.status == RawStatus::bad(Reason::COMM_FAILURE) && s.value == OwnedRaw::None
                }) =>
            {
                Some(*at)
            }
            _ => None,
        })
        .unwrap();
    assert!(
        first_bad >= unplugged && first_bad + timeout <= unplugged + poll + timeout,
        "{first_bad} {unplugged}"
    );
    let mut esp = rig.esp.lock().unwrap();
    esp.tick(unplugged + 2 * S);
    assert!(
        !esp.led,
        "the firmware turns the LED off by itself after 2 s"
    );
}

#[test]
fn a_vanished_port_crc_errors_and_exceptions() {
    let p = profile();
    let req = p.request(
        "esp",
        1,
        S,
        100 * MS,
        serial_bus("esp32", "/dev/ttyUSB0", 115_200),
    );
    let views = req.views();
    let plan = ModbusPlanner::plan(&views.request()).unwrap();
    let (rig, port) = new_rig();
    let mut bench: Bench<'_, ModbusRtu> = Bench::open(
        rig.host.clone(),
        views.request(),
        plan.view(),
        Link::Serial(port),
    )
    .unwrap();
    bench.connect().unwrap();
    let comm = RawStatus::bad(Reason::COMM_FAILURE);

    rig.esp.lock().unwrap().corrupt = true;
    assert!(bench.read(3).unwrap().iter().all(|s| s.status == comm));
    rig.esp.lock().unwrap().corrupt = false;

    rig.line.inject(&[0xAA, 0x55, 0x01]);
    assert!(
        bench
            .read(3)
            .unwrap()
            .iter()
            .all(|s| s.status == RawStatus::OK)
    );

    let t = rig.host.mono();
    rig.line.set_fault(LineFault::Gone);
    assert!(bench.read(3).unwrap().iter().all(|s| s.status == comm));
    assert_eq!(rig.host.mono(), t);
    let led = p.signals.iter().position(|s| s.name == "led").unwrap() as u32;
    let mut writes = Writes::new(led);
    assert_eq!(
        bench.write(&writes.set(true)).unwrap()[0].status,
        WriteStatus::NotConnected
    );
    rig.line.set_fault(LineFault::None);

    let mut bad = req.clone();
    bad.points.clear();
    bad.points.push(joker_hal::spec::owned::OwnedPoint::input(
        0,
        "ghost",
        joker_hal::RawKind::Regs,
        1,
        "u16",
        joker_hal::cbor::Value::map([
            ("at", joker_hal::cbor::Value::Uint(9)),
            ("fc", joker_hal::cbor::Value::Uint(4)),
        ])
        .encode(),
        100 * MS,
    ));
    let views = bad.views();
    let plan = ModbusPlanner::plan(&views.request()).unwrap();
    let (rig, port) = new_rig();
    let mut bench: Bench<'_, ModbusRtu> = Bench::open(
        rig.host.clone(),
        views.request(),
        plan.view(),
        Link::Serial(port),
    )
    .unwrap();
    let s = bench.read(0).unwrap();
    assert_eq!(s[0].status, RawStatus::bad(Reason::DEVICE_FAULT));
}

#[test]
fn plan_refuses_what_m0_cannot_do() {
    let p = profile();
    let mut req = p.request(
        "esp",
        1,
        S,
        100 * MS,
        serial_bus("esp32", "/dev/ttyUSB0", 115_200),
    );
    req.devices[0].config =
        joker_hal::cbor::Value::map([("unit_id", joker_hal::cbor::Value::Uint(0))]).encode();
    let views = req.views();
    let errs = ModbusPlanner::plan(&views.request()).unwrap_err();
    assert_eq!(errs[0].code.as_str(), "J0852");
    let mut req = p.request(
        "esp",
        1,
        S,
        100 * MS,
        serial_bus("esp32", "/dev/ttyUSB0", 115_200),
    );
    req.bus = None;
    let views = req.views();
    assert_eq!(
        ModbusPlanner::plan(&views.request()).unwrap_err()[0]
            .code
            .as_str(),
        "J0171"
    );
}

#[test]
fn a_full_sample_ring_counts_every_dropped_sample() {
    let p = profile();
    let req = p.request(
        "esp",
        1,
        S,
        100 * MS,
        serial_bus("esp32", "/dev/ttyUSB0", 115_200),
    );
    let views = req.views();
    let plan = ModbusPlanner::plan(&views.request()).unwrap();
    let (rig, port) = new_rig();
    let mut bench: Bench<'_, ModbusRtu> = Bench::open(
        rig.host.clone(),
        views.request(),
        plan.view(),
        Link::Serial(port),
    )
    .unwrap();
    bench.connect().unwrap();
    let points = plan.view().batch(3).unwrap().points.len() as u64;
    assert_eq!(u64::from(bench.read_into_full_ring(3).unwrap()), points);
    assert!(bench.counters().contains(&("dropped_samples", points)));
}
