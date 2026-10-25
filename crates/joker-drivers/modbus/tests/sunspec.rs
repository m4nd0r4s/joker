//! SunSpec discovery (ADR 0077) through `connect` of `modbus-tcp` and
//! `modbus-rtu`, against a simulated SunSpec device: the marker, the model
//! chain, the identity of model 1, the effective map reported to the core,
//! read-only traffic, and the drift of the map against the one the lock
//! pins.

#![allow(clippy::unwrap_used)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use joker_driver_modbus::rtu::crc16;
use joker_driver_modbus::sunspec::{Image, common_body};
use joker_driver_modbus::{ModbusPlanner, ModbusRtu, ModbusTcp, ModbusTcpPlanner};
use joker_hal::cbor::{Cbor, Value};
use joker_hal::link::SerialInfo;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, BenchError, MemorySerial, Reported, VirtualHost};
use joker_hal::{
    IoError, Link, Planner, RawKind, SerialPort, TcpConnect, TcpEndpoint, TcpIo, TcpStream,
};

const MS: u64 = 1_000_000;

#[derive(Debug)]
struct Device {
    image: Image,
    log: Vec<Vec<u8>>,
}

impl Device {
    fn serve(&mut self, pdu: &[u8]) -> Vec<u8> {
        self.log.push(pdu.to_vec());
        let at = u16::from_be_bytes([pdu[1], pdu[2]]);
        let count = u16::from_be_bytes([pdu[3], pdu[4]]);
        match (pdu[0], self.image.read(at, count)) {
            (3, Some(words)) => {
                let mut out = vec![3, (2 * count) as u8];
                for w in words {
                    out.extend_from_slice(&w.to_be_bytes());
                }
                out
            }
            (3, None) => vec![0x83, 2],
            (fc, _) => vec![fc | 0x80, 1],
        }
    }
}

struct Mbap {
    device: Arc<Mutex<Device>>,
    reply: VecDeque<u8>,
}

impl TcpIo for Mbap {
    fn poll_read(&mut self, _cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        if self.reply.is_empty() {
            return Poll::Pending;
        }
        let mut n = 0;
        while n < buf.len() {
            let Some(b) = self.reply.pop_front() else {
                break;
            };
            buf[n] = b;
            n += 1;
        }
        Poll::Ready(Ok(n))
    }

    fn poll_write(&mut self, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        let pdu = self.device.lock().unwrap().serve(&buf[7..]);
        self.reply.extend(&buf[..4]);
        self.reply.extend((pdu.len() as u16 + 1).to_be_bytes());
        self.reply.push_back(buf[6]);
        self.reply.extend(pdu);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        Poll::Ready(Ok(()))
    }
}

struct Connector(Arc<Mutex<Device>>);

impl TcpConnect for Connector {
    fn poll_connect(&mut self, _cx: &mut Context<'_>) -> Poll<Result<TcpStream, IoError>> {
        Poll::Ready(Ok(TcpStream::new(Mbap {
            device: self.0.clone(),
            reply: VecDeque::new(),
        })))
    }
}

fn inverter() -> Vec<u16> {
    let mut body = vec![0u16; 50];
    body[12] = 4321;
    body[13] = (-1i16) as u16;
    body
}

fn image(base: u16, extra: bool) -> Image {
    let mut image = Image::new(base);
    image.model(1, &common_body("ACME", "PCS-125", "4.12.0", "A1B2C3"));
    if extra {
        image.model(120, &[0; 26]);
    }
    image.model(103, &inverter());
    image.model(203, &[0; 105]);
    image.end();
    image
}

fn device(image: Image) -> Arc<Mutex<Device>> {
    Arc::new(Mutex::new(Device {
        image,
        log: Vec::new(),
    }))
}

fn request(discovery: Value, pinned: Option<Vec<u8>>) -> OwnedRequest {
    let mut d = OwnedDevice::new(
        "pv",
        Value::map([("unit_id", Value::Uint(1))]).encode(),
        2_000 * MS,
    );
    d.quirks = Value::map([("discovery", discovery)]).encode();
    d.discovered = pinned;
    let addr = Value::map([("at", Value::Uint(40084)), ("fc", Value::Uint(3))]).encode();
    OwnedRequest {
        bus: None,
        devices: vec![d],
        points: vec![OwnedPoint::input(
            0,
            "pv.m103.w",
            RawKind::Regs,
            1,
            "i16",
            addr,
            1_000 * MS,
        )],
    }
}

fn sunspec() -> Value {
    Value::map([("kind", Value::text("sunspec"))])
}

fn tcp(device: &Arc<Mutex<Device>>, req: &OwnedRequest) -> Result<Vec<Reported>, BenchError> {
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let link = Link::Tcp(TcpEndpoint::new(Connector(device.clone())));
    let mut bench: Bench<'_, ModbusTcp> = Bench::open(
        Arc::new(VirtualHost::new(0)),
        views.request(),
        plan.view(),
        link,
    )
    .unwrap();
    bench.connect_reports()
}

fn models(map: &[u8]) -> Vec<(u64, u64, u64)> {
    Cbor::new(map)
        .get("models")
        .unwrap()
        .unwrap()
        .array()
        .unwrap()
        .map(|m| {
            let m = m.unwrap();
            let field = |k: &str| m.get(k).unwrap().unwrap().as_u64().unwrap();
            (field("id"), field("at"), field("len"))
        })
        .collect()
}

#[test]
fn connect_discovers_the_chain_and_reports_identity_and_map() {
    let d = device(image(40000, false));
    let reports = tcp(&d, &request(sunspec(), None)).unwrap();
    assert_eq!(reports.len(), 1);
    let r = &reports[0];
    assert_eq!(r.vendor.as_deref(), Some("ACME"));
    assert_eq!(r.model.as_deref(), Some("PCS-125"));
    assert_eq!(r.serial.as_deref(), Some("A1B2C3"));
    assert_eq!(r.firmware.as_deref(), Some("4.12.0"));
    let map = r.discovered.clone().unwrap();
    assert_eq!(
        models(&map),
        [(1, 40002, 66), (103, 40070, 50), (203, 40122, 105)]
    );
    let scales = Cbor::new(&map).get("scales").unwrap().unwrap();
    assert_eq!(
        scales.get("m103.w_sf").unwrap().unwrap().as_i64().unwrap(),
        -1
    );
    let log = &d.lock().unwrap().log;
    assert!(
        log.iter().all(|pdu| pdu[0] == 3),
        "discovery reads only, with FC 3: {log:?}"
    );
}

#[test]
fn the_marker_is_found_at_the_next_base() {
    let d = device(image(50000, false));
    let reports = tcp(&d, &request(sunspec(), None)).unwrap();
    assert_eq!(
        models(reports[0].discovered.as_ref().unwrap())[0],
        (1, 50002, 66)
    );
    let none = device(Image::default());
    match tcp(&none, &request(sunspec(), None)) {
        Err(BenchError::Driver(e)) => assert_eq!(e.code.as_str(), "J0876"),
        other => unreachable!("{other:?}"),
    }
}

#[test]
fn a_chain_that_differs_from_the_pinned_map_fails_connect_with_drift() {
    let d = device(image(40000, false));
    let pinned = tcp(&d, &request(sunspec(), None)).unwrap()[0]
        .discovered
        .clone()
        .unwrap();
    let same = device(image(40000, false));
    assert!(tcp(&same, &request(sunspec(), Some(pinned.clone()))).is_ok());
    let shifted = device(image(40000, true));
    match tcp(&shifted, &request(sunspec(), Some(pinned))) {
        Err(BenchError::Driver(e)) => assert_eq!(e.code.as_str(), "J0401"),
        other => unreachable!("a model inserted before 103 shifts it: {other:?}"),
    }
}

#[test]
fn a_bad_discovery_is_refused_when_the_driver_opens() {
    let req = request(Value::map([("kind", Value::text("canopen_lss"))]), None);
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let d = device(image(40000, false));
    let link = Link::Tcp(TcpEndpoint::new(Connector(d)));
    match Bench::<'_, ModbusTcp>::open(
        Arc::new(VirtualHost::new(0)),
        views.request(),
        plan.view(),
        link,
    ) {
        Err(BenchError::Driver(e)) => assert_eq!(e.code.as_str(), "J0825"),
        other => unreachable!("{:?}", other.map(|_| ())),
    }
}

#[test]
fn discovery_runs_over_rtu_too() {
    let host = Arc::new(VirtualHost::new(0));
    let d = device(image(40000, false));
    let s = d.clone();
    let (serial, _line) = MemorySerial::new(
        host.clone(),
        SerialInfo::new_8n1(115_200),
        Box::new(move |frame: &[u8], _now| {
            let mut out = vec![frame[0]];
            out.extend(s.lock().unwrap().serve(&frame[1..frame.len() - 2]));
            let crc = crc16(&out).to_le_bytes();
            out.extend_from_slice(&crc);
            out
        }),
    );
    let mut req = request(sunspec(), None);
    req.bus = Some(joker_driver_modbus::profile::serial_bus(
        "rtu",
        "/dev/ttyUSB0",
        115_200,
    ));
    let views = req.views();
    let plan = ModbusPlanner::plan(&views.request()).unwrap();
    let mut bench: Bench<'_, ModbusRtu> = Bench::open(
        host,
        views.request(),
        plan.view(),
        Link::Serial(SerialPort::new(serial)),
    )
    .unwrap();
    let reports = bench.connect_reports().unwrap();
    assert_eq!(reports[0].vendor.as_deref(), Some("ACME"));
    assert_eq!(models(reports[0].discovered.as_ref().unwrap()).len(), 3);
}
