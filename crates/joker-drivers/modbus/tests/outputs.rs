//! Output writes of `modbus-tcp` and `modbus-rtu` (ADR 0076) against an
//! in-memory Modbus server written from the application protocol (v1.1b3
//! §6): registers with FC 6 and 16, coils with FC 5 and 15, a `bool` in a
//! register bit by an FC 3 read followed by the write of the register, the
//! `single_write = "fc16"` quirk, and every write confirmed by its echo.
//! The committed writes come out of a real envelope and the output stage's
//! conversion (`joker_hal::encode`), as they do in jokerd.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use joker_core::ValueType;
use joker_driver_modbus::rtu::crc16;
use joker_driver_modbus::{ModbusPlanner, ModbusRtu, ModbusTcp, ModbusTcpPlanner};
use joker_hal::cbor::Value;
use joker_hal::encode::{OutputCodec, RawNumber, RegLayout};
use joker_hal::link::SerialInfo;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, MemorySerial, OutputHarness, VirtualHost};
use joker_hal::{
    CommitBox, CommitRx, CommitTx, CommittedWrite, IoError, Link, Planner, RawEncoding, RawKind,
    RxState, SerialPort, TcpConnect, TcpEndpoint, TcpIo, TcpStream, WriteStatus,
};

const MS: u64 = 1_000_000;

#[derive(Debug, Default)]
struct Server {
    coils: BTreeMap<u16, bool>,
    holding: BTreeMap<u16, u16>,
    faulty: Vec<u16>,
    bad_echo: bool,
    log: Vec<Vec<u8>>,
}

fn word(pdu: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([pdu[at], pdu[at + 1]])
}

impl Server {
    fn serve(&mut self, pdu: &[u8]) -> Vec<u8> {
        self.log.push(pdu.to_vec());
        let fc = pdu[0];
        let start = word(pdu, 1);
        let b = word(pdu, 3);
        if self.faulty.contains(&start) {
            return vec![fc | 0x80, 4];
        }
        let span = |n: u16| start..start.saturating_add(n);
        let mut echo = pdu[..5].to_vec();
        if self.bad_echo {
            echo[4] ^= 1;
        }
        match fc {
            3 => {
                let mut out = vec![fc, (2 * b) as u8];
                for at in span(b) {
                    let Some(r) = self.holding.get(&at) else {
                        return vec![fc | 0x80, 2];
                    };
                    out.extend_from_slice(&r.to_be_bytes());
                }
                out
            }
            5 => {
                self.coils.insert(start, b == 0xFF00);
                echo
            }
            6 => {
                self.holding.insert(start, b);
                echo
            }
            15 => {
                for (i, at) in span(b).enumerate() {
                    self.coils.insert(at, pdu[6 + i / 8] >> (i % 8) & 1 == 1);
                }
                echo
            }
            16 => {
                for (i, at) in span(b).enumerate() {
                    self.holding.insert(at, word(pdu, 6 + 2 * i));
                }
                echo
            }
            _ => vec![fc | 0x80, 1],
        }
    }
}

struct Mbap {
    server: Arc<Mutex<Server>>,
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
        let pdu = self.server.lock().unwrap().serve(&buf[7..]);
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

struct Connector(Arc<Mutex<Server>>);

impl TcpConnect for Connector {
    fn poll_connect(&mut self, _cx: &mut Context<'_>) -> Poll<Result<TcpStream, IoError>> {
        Poll::Ready(Ok(TcpStream::new(Mbap {
            server: self.0.clone(),
            reply: VecDeque::new(),
        })))
    }
}

fn server() -> Arc<Mutex<Server>> {
    let mut s = Server::default();
    s.holding.insert(20, 0x8001);
    Arc::new(Mutex::new(s))
}

fn addr(entries: &[(&str, u64)]) -> Vec<u8> {
    Value::Map(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Value::Uint(*v)))
            .collect(),
    )
    .encode()
}

const P: u32 = 0;
const Q: u32 = 1;
const ENABLE: u32 = 2;
const STANDBY: u32 = 3;
const COIL: u32 = 4;

fn request(quirks: Option<Value>) -> OwnedRequest {
    let mut device = OwnedDevice::new(
        "pcs",
        Value::map([("unit_id", Value::Uint(1))]).encode(),
        200 * MS,
    );
    if let Some(q) = quirks {
        device.quirks = q.encode();
    }
    OwnedRequest {
        bus: None,
        devices: vec![device],
        points: vec![
            OwnedPoint::output(
                P,
                "pcs.p",
                RawKind::Regs,
                1,
                "i16",
                addr(&[("at", 10), ("fc", 6)]),
            ),
            OwnedPoint::output(
                Q,
                "pcs.q",
                RawKind::Regs,
                2,
                "u32",
                addr(&[("at", 11), ("fc", 16)]),
            ),
            OwnedPoint::output(
                ENABLE,
                "pcs.enable",
                RawKind::Regs,
                1,
                "bool",
                addr(&[("at", 20), ("fc", 6), ("bit", 4)]),
            ),
            OwnedPoint::output(
                STANDBY,
                "pcs.standby",
                RawKind::Regs,
                1,
                "bool",
                addr(&[("at", 20), ("fc", 16), ("bit", 15)]),
            ),
            OwnedPoint::output(
                COIL,
                "pcs.coil",
                RawKind::Bool,
                0,
                "bool",
                addr(&[("at", 3), ("fc", 15)]),
            ),
        ],
    }
}

/// The committed write of `bits` on one output, out of a real envelope,
/// converted by the output stage with `codec` when there is one.
fn committed(
    ty: ValueType,
    point: u32,
    bits: u64,
    codec: Option<OutputCodec<'static>>,
) -> CommittedWrite<'static> {
    let mut harness = OutputHarness::new(&[("pcs.out", ty, 0, point)]).unwrap();
    let encoding = if ty == ValueType::Bool {
        RawEncoding::Bool
    } else {
        RawEncoding::Float
    };
    let bindings: &'static [_] = Box::leak(harness.bindings(&[encoding]).into_boxed_slice());
    let boxes: &'static [CommitBox] = Box::leak(Box::new([CommitBox::new()]));
    let last = Box::leak(Box::new([None]));
    let state = Box::leak(Box::new([RxState::default()]));
    let mut tx = CommitTx::new(harness.outputs(), bindings, boxes, last).unwrap();
    let mut rx = CommitRx::new(bindings, boxes, state).unwrap();
    harness.manual("pcs.out", bits);
    tx.publish(harness.outputs());
    let mut out = Vec::new();
    rx.drain_into(&mut out);
    let w = out.pop().unwrap();
    match codec {
        None => w,
        Some(c) => {
            let words: &'static mut [u16; 4] = Box::leak(Box::new([0; 4]));
            c.encode(&w, words).unwrap()
        }
    }
}

fn codec(raw: RawNumber, scale: f64, words: u8) -> OutputCodec<'static> {
    OutputCodec {
        invert: false,
        enum_raw: &[],
        scale,
        offset: 0.0,
        raw,
        regs: Some(RegLayout {
            words,
            words_little: false,
            bytes_little: false,
        }),
    }
}

fn writes() -> Vec<CommittedWrite<'static>> {
    vec![
        committed(
            ValueType::F64,
            P,
            (-1.0f64).to_bits(),
            Some(codec(RawNumber::Signed(16), 0.1, 1)),
        ),
        committed(
            ValueType::F64,
            Q,
            65_538.0f64.to_bits(),
            Some(codec(RawNumber::Unsigned(32), 1.0, 2)),
        ),
        committed(ValueType::Bool, ENABLE, 1, None),
        committed(ValueType::Bool, STANDBY, 0, None),
        committed(ValueType::Bool, COIL, 1, None),
    ]
}

fn tcp(server: &Arc<Mutex<Server>>, req: &OwnedRequest) -> Vec<WriteStatus> {
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let link = Link::Tcp(TcpEndpoint::new(Connector(server.clone())));
    let mut bench: Bench<'_, ModbusTcp> = Bench::open(
        Arc::new(VirtualHost::new(0)),
        views.request(),
        plan.view(),
        link,
    )
    .unwrap();
    bench.connect().unwrap();
    bench
        .write(&writes())
        .unwrap()
        .iter()
        .map(|a| a.status)
        .collect()
}

fn rtu(server: &Arc<Mutex<Server>>, req: &mut OwnedRequest) -> Vec<WriteStatus> {
    let host = Arc::new(VirtualHost::new(0));
    let s = server.clone();
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
    bench.connect().unwrap();
    bench
        .write(&writes())
        .unwrap()
        .iter()
        .map(|a| a.status)
        .collect()
}

fn assert_written(server: &Arc<Mutex<Server>>, statuses: &[WriteStatus]) {
    assert!(
        statuses.iter().all(|s| *s == WriteStatus::Ok),
        "{statuses:?}"
    );
    let s = server.lock().unwrap();
    assert_eq!(s.holding.get(&10), Some(&0xFFF6), "-1.0 / 0.1 = -10");
    assert_eq!(
        (s.holding.get(&11), s.holding.get(&12)),
        (Some(&1), Some(&2)),
        "65538, most significant word first"
    );
    assert_eq!(
        s.holding.get(&20),
        Some(&0x0011),
        "bit 4 set, bit 15 cleared, bit 0 kept"
    );
    assert_eq!(s.coils.get(&3), Some(&true));
    assert_eq!(
        s.log,
        vec![
            vec![6, 0, 10, 0xFF, 0xF6],
            vec![16, 0, 11, 0, 2, 4, 0, 1, 0, 2],
            vec![3, 0, 20, 0, 1],
            vec![6, 0, 20, 0x80, 0x11],
            vec![3, 0, 20, 0, 1],
            vec![16, 0, 20, 0, 1, 2, 0x00, 0x11],
            vec![15, 0, 3, 0, 1, 1, 1],
        ]
    );
}

#[test]
fn registers_bits_and_coils_reach_the_wire_over_tcp() {
    let server = server();
    let statuses = tcp(&server, &request(None));
    assert_written(&server, &statuses);
}

#[test]
fn registers_bits_and_coils_reach_the_wire_over_rtu() {
    let server = server();
    let statuses = rtu(&server, &mut request(None));
    assert_written(&server, &statuses);
}

#[test]
fn single_write_fc16_writes_one_register_with_fc_16() {
    let server = server();
    let quirks = Value::map([("single_write", Value::text("fc16"))]);
    let statuses = tcp(&server, &request(Some(quirks)));
    assert!(
        statuses.iter().all(|s| *s == WriteStatus::Ok),
        "{statuses:?}"
    );
    let s = server.lock().unwrap();
    assert_eq!(s.log[0], vec![16, 0, 10, 0, 1, 2, 0xFF, 0xF6]);
    assert_eq!(s.log[3], vec![16, 0, 20, 0, 1, 2, 0x80, 0x11]);
}

#[test]
fn a_wrong_echo_is_not_a_confirmation() {
    let server = server();
    server.lock().unwrap().bad_echo = true;
    let statuses = tcp(&server, &request(None));
    assert!(
        statuses.iter().all(|s| *s == WriteStatus::CommFailure),
        "{statuses:?}"
    );
    let server = self::server();
    server.lock().unwrap().bad_echo = true;
    let statuses = rtu(&server, &mut request(None));
    assert!(
        statuses.iter().all(|s| *s == WriteStatus::CommFailure),
        "{statuses:?}"
    );
}

#[test]
fn a_bit_whose_register_cannot_be_read_is_not_written() {
    let server = server();
    server.lock().unwrap().faulty.push(20);
    let statuses = tcp(&server, &request(None));
    assert_eq!(
        statuses,
        vec![
            WriteStatus::Ok,
            WriteStatus::Ok,
            WriteStatus::DeviceRejected,
            WriteStatus::DeviceRejected,
            WriteStatus::Ok,
        ]
    );
    let s = server.lock().unwrap();
    assert_eq!(s.holding.get(&20), Some(&0x8001), "untouched");
    assert!(
        s.log
            .iter()
            .all(|pdu| !(matches!(pdu[0], 6 | 16) && pdu[2] == 20)),
        "no write of register 20: {:?}",
        s.log
    );
}

#[test]
fn len_gives_the_registers_of_a_string_or_bytes_point_and_only_of_one() {
    let one = |type_name: &str, width: u16, entries: &[(&str, u64)], output: bool| {
        let point = if output {
            OwnedPoint::output(0, "m.x", RawKind::Regs, width, type_name, addr(entries))
        } else {
            OwnedPoint::input(
                0,
                "m.x",
                RawKind::Regs,
                width,
                type_name,
                addr(entries),
                100 * MS,
            )
        };
        let req = OwnedRequest {
            bus: None,
            devices: vec![OwnedDevice::new(
                "m",
                Value::map([("unit_id", Value::Uint(1))]).encode(),
                200 * MS,
            )],
            points: vec![point],
        };
        ModbusTcpPlanner::plan(&req.views().request())
            .err()
            .map(|e| e[0].code.as_str().to_owned())
    };
    let at3 = |len: Option<u64>| {
        let mut e = vec![("at", 0), ("fc", 3)];
        e.extend(len.map(|n| ("len", n)));
        e
    };
    assert_eq!(one("string", 4, &at3(Some(4)), false), None);
    assert_eq!(one("bytes", 32, &at3(Some(32)), false), None);
    assert_eq!(
        one("string", 4, &at3(None), false).as_deref(),
        Some("J0809"),
        "no len"
    );
    assert_eq!(
        one("bytes", 4, &at3(Some(2)), false).as_deref(),
        Some("J0809"),
        "len ≠ width"
    );
    assert_eq!(
        one("u16", 1, &at3(Some(1)), false).as_deref(),
        Some("J0809"),
        "len on a scalar"
    );
    assert_eq!(
        one("bytes", 33, &at3(Some(33)), false).as_deref(),
        Some("J0853"),
        "over 64 B"
    );
    assert_eq!(
        one("string", 2, &[("at", 0), ("fc", 16), ("len", 2)], true).as_deref(),
        Some("J0809"),
        "read only"
    );
}

#[test]
fn the_planner_admits_register_outputs_and_refuses_fc_6_on_two_registers() {
    let req = request(None);
    assert!(ModbusTcpPlanner::plan(&req.views().request()).is_ok());
    let mut wide = request(None);
    wide.points[Q as usize].addr = addr(&[("at", 11), ("fc", 6)]);
    let errors = ModbusTcpPlanner::plan(&wide.views().request()).unwrap_err();
    assert_eq!(errors[0].code.as_str(), "J0809", "{errors:?}");
    let mut input_fc = request(None);
    input_fc.points[P as usize].addr = addr(&[("at", 10), ("fc", 3)]);
    let errors = ModbusTcpPlanner::plan(&input_fc.views().request()).unwrap_err();
    assert_eq!(errors[0].code.as_str(), "J0810", "{errors:?}");
    let bad_quirk = request(Some(Value::map([("single_write", Value::text("fc5"))])));
    assert!(ModbusTcpPlanner::plan(&bad_quirk.views().request()).is_err());
}
