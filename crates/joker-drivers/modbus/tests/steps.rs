//! Step programs (ADR 0058, ADR 0073) through `read-addr` and `write-addr`
//! of `modbus-tcp` and `modbus-rtu`, against an in-memory Modbus server
//! written from the application protocol (v1.1b3 §6): coils, discrete
//! inputs, holding and input registers, FC 1–6, 15 and 16.

#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use joker_driver_modbus::rtu::crc16;
use joker_driver_modbus::{ModbusPlanner, ModbusRtu, ModbusTcp, ModbusTcpPlanner};
use joker_hal::cbor::{Cbor, Value};
use joker_hal::interpreter::{ArgList, Failure, NoArgs, Scalar};
use joker_hal::link::SerialInfo;
use joker_hal::spec::owned::{OwnedBus, OwnedDevice, OwnedPoint, OwnedRequest, RequestViews};
use joker_hal::steps::{Step, StepValue};
use joker_hal::testing::{Bench, MemorySerial, VirtualHost};
use joker_hal::{
    BatchFailure, IoError, Link, Planner, RawKind, RawValue, SerialPort, TcpConnect, TcpEndpoint,
    TcpIo, TcpStream,
};
use joker_units::Duration;

const MS: u64 = 1_000_000;

#[derive(Debug, Default)]
struct Server {
    coils: BTreeMap<u16, bool>,
    inputs: BTreeMap<u16, bool>,
    holding: BTreeMap<u16, u16>,
    input_regs: BTreeMap<u16, u16>,
    faulty: Vec<u16>,
    effects: Vec<(u16, u16)>,
    silent: bool,
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
        match fc {
            1 | 2 => {
                let bits = if fc == 1 { &self.coils } else { &self.inputs };
                let mut data = vec![0u8; usize::from(b.div_ceil(8))];
                for (i, at) in span(b).enumerate() {
                    match bits.get(&at) {
                        Some(true) => data[i / 8] |= 1 << (i % 8),
                        Some(false) => {}
                        None => return vec![fc | 0x80, 2],
                    }
                }
                let mut out = vec![fc, data.len() as u8];
                out.extend(data);
                out
            }
            3 | 4 => {
                let regs = if fc == 3 {
                    &self.holding
                } else {
                    &self.input_regs
                };
                let mut out = vec![fc, (2 * b) as u8];
                for at in span(b) {
                    let Some(r) = regs.get(&at) else {
                        return vec![fc | 0x80, 2];
                    };
                    out.extend_from_slice(&r.to_be_bytes());
                }
                out
            }
            5 => {
                self.coils.insert(start, b == 0xFF00);
                pdu[..5].to_vec()
            }
            6 => {
                self.holding.insert(start, b);
                for (written, raised) in self.effects.clone() {
                    if written == start {
                        self.input_regs.insert(raised, 1);
                    }
                }
                pdu[..5].to_vec()
            }
            15 => {
                for (i, at) in span(b).enumerate() {
                    self.coils.insert(at, pdu[6 + i / 8] >> (i % 8) & 1 == 1);
                }
                pdu[..5].to_vec()
            }
            16 => {
                for (i, at) in span(b).enumerate() {
                    self.holding.insert(at, word(pdu, 6 + 2 * i));
                }
                pdu[..5].to_vec()
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
        let mut server = self.server.lock().unwrap();
        if server.silent {
            return Poll::Ready(Ok(buf.len()));
        }
        let pdu = server.serve(&buf[7..]);
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
    s.input_regs.insert(0, 7);
    s.input_regs.insert(10, 0);
    s.input_regs.insert(11, 1450);
    s.inputs.insert(3, true);
    s.holding.insert(20, 0b1_0000);
    s.holding.insert(30, 0);
    s.holding.insert(31, 0);
    Arc::new(Mutex::new(s))
}

fn addr(entries: &[(&str, Value)]) -> Vec<u8> {
    Value::Map(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
    .encode()
}

fn at(at: u16, fc: u8) -> Vec<u8> {
    addr(&[
        ("at", Value::Uint(u64::from(at))),
        ("fc", Value::Uint(u64::from(fc))),
    ])
}

fn request(bus: Option<OwnedBus>) -> OwnedRequest {
    OwnedRequest {
        bus,
        devices: vec![OwnedDevice::new(
            "pcs",
            Value::map([("unit_id", Value::Uint(1))]).encode(),
            200 * MS,
        )],
        points: vec![OwnedPoint::input(
            0,
            "pcs.state",
            RawKind::Regs,
            1,
            "u16",
            at(0, 4),
            100 * MS,
        )],
    }
}

fn tcp_bench<'r>(
    server: &Arc<Mutex<Server>>,
    views: &'r RequestViews<'r>,
    plan: &'r joker_hal::plan::Plan,
) -> Bench<'r, ModbusTcp> {
    let link = Link::Tcp(TcpEndpoint::new(Connector(server.clone())));
    let mut bench = Bench::open(
        Arc::new(VirtualHost::new(0)),
        views.request(),
        plan.view(),
        link,
    )
    .unwrap();
    bench.connect().unwrap();
    bench
}

#[test]
fn a_program_writes_waits_for_and_reads_registers_over_tcp() {
    let server = server();
    let req = request(None);
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let mut bench = tcp_bench(&server, &views, &plan);

    let reset = at(40, 6);
    let ready = at(10, 4);
    let speed = at(11, 4);
    let program = [
        Step::Write {
            addr: Cbor::new(&reset),
            type_name: "u16",
            value: StepValue::Arg("level"),
        },
        Step::Wait(Duration::from_millis(10)),
        Step::Until {
            addr: Cbor::new(&ready),
            type_name: "u16",
            equals: RawValue::Uint(1),
            timeout: Duration::from_millis(500),
        },
        Step::Read {
            addr: Cbor::new(&speed),
            type_name: "u16",
            into: "speed",
        },
    ];
    server.lock().unwrap().effects.push((40, 10));
    let args = [("level", Scalar::Uint(3))];
    let read = bench
        .steps(&program, &ArgList(&args), 2_000 * MS)
        .unwrap()
        .unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].0, "speed");
    assert_eq!(read[0].1.value, Scalar::Uint(1450));
    assert_eq!(server.lock().unwrap().holding.get(&40), Some(&3));
}

#[test]
fn types_orders_coils_and_bits_reach_the_wire_as_the_address_says() {
    let server = server();
    let req = request(None);
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let mut bench = tcp_bench(&server, &views, &plan);

    let little = addr(&[
        ("at", Value::Uint(30)),
        ("fc", Value::Uint(16)),
        ("word_order", Value::text("little")),
    ]);
    let little_read = addr(&[
        ("at", Value::Uint(30)),
        ("fc", Value::Uint(3)),
        ("word_order", Value::text("little")),
    ]);
    let big_read = at(30, 3);
    let coil = at(5, 5);
    let coils = at(6, 15);
    let coil_read = at(5, 1);
    let discrete = at(3, 2);
    let bit = addr(&[
        ("at", Value::Uint(20)),
        ("fc", Value::Uint(3)),
        ("bit", Value::Uint(4)),
    ]);
    let program = [
        Step::Write {
            addr: Cbor::new(&little),
            type_name: "f32",
            value: StepValue::Literal(RawValue::Float(2.5)),
        },
        Step::Read {
            addr: Cbor::new(&little_read),
            type_name: "f32",
            into: "f",
        },
        Step::Read {
            addr: Cbor::new(&big_read),
            type_name: "u32",
            into: "swapped",
        },
        Step::Write {
            addr: Cbor::new(&coil),
            type_name: "bool",
            value: StepValue::Literal(RawValue::Bool(true)),
        },
        Step::Write {
            addr: Cbor::new(&coils),
            type_name: "",
            value: StepValue::Literal(RawValue::Bool(true)),
        },
        Step::Read {
            addr: Cbor::new(&coil_read),
            type_name: "bool",
            into: "coil",
        },
        Step::Read {
            addr: Cbor::new(&discrete),
            type_name: "",
            into: "discrete",
        },
        Step::Read {
            addr: Cbor::new(&bit),
            type_name: "bool",
            into: "bit",
        },
    ];
    let read = bench.steps(&program, &NoArgs, 2_000 * MS).unwrap().unwrap();
    let bits = 2.5f32.to_bits();
    let values: Vec<_> = read.iter().map(|(k, r)| (*k, r.value)).collect();
    assert_eq!(
        values,
        [
            ("f", Scalar::Float(2.5)),
            ("swapped", Scalar::Uint(u64::from(bits.rotate_left(16)))),
            ("coil", Scalar::Bool(true)),
            ("discrete", Scalar::Bool(true)),
            ("bit", Scalar::Bool(true)),
        ]
    );
    let s = server.lock().unwrap();
    assert_eq!(s.holding.get(&30), Some(&(bits as u16)));
    assert_eq!(s.holding.get(&31), Some(&((bits >> 16) as u16)));
    assert_eq!(s.coils.get(&6), Some(&true));
    assert!(s.log.iter().any(|pdu| pdu[0] == 15));
}

#[test]
fn exceptions_silence_and_unfit_steps_end_the_program() {
    let server = server();
    server.lock().unwrap().faulty.push(99);
    let req = request(None);
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let mut bench = tcp_bench(&server, &views, &plan);

    let fault = at(99, 6);
    let write_fault = [Step::Write {
        addr: Cbor::new(&fault),
        type_name: "u16",
        value: StepValue::Literal(RawValue::Uint(1)),
    }];
    assert_eq!(
        bench.steps(&write_fault, &NoArgs, 2_000 * MS).unwrap(),
        Err(Failure::Device(BatchFailure::DeviceFault(
            "exception 4: server device failure"
        )))
    );

    let wide = at(40, 6);
    let too_wide = [Step::Write {
        addr: Cbor::new(&wide),
        type_name: "u32",
        value: StepValue::Literal(RawValue::Uint(1)),
    }];
    assert!(matches!(
        bench.steps(&too_wide, &NoArgs, 2_000 * MS).unwrap(),
        Err(Failure::Device(BatchFailure::ConfigError(_)))
    ));
    let range = [Step::Write {
        addr: Cbor::new(&wide),
        type_name: "u16",
        value: StepValue::Literal(RawValue::Uint(70_000)),
    }];
    assert!(matches!(
        bench.steps(&range, &NoArgs, 2_000 * MS).unwrap(),
        Err(Failure::Device(BatchFailure::ConfigError(_)) | Failure::Unsupported)
    ));

    server.lock().unwrap().silent = true;
    let state = at(0, 4);
    let read = [Step::Read {
        addr: Cbor::new(&state),
        type_name: "u16",
        into: "state",
    }];
    assert_eq!(
        bench.steps(&read, &NoArgs, 2_000 * MS).unwrap(),
        Err(Failure::Device(BatchFailure::Timeout))
    );
}

#[test]
fn the_same_program_runs_over_rtu() {
    let server = server();
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
    let bus = joker_driver_modbus::profile::serial_bus("rtu", "/dev/ttyUSB0", 115_200);
    let req = request(Some(bus));
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

    let reg = at(50, 16);
    let back = at(50, 3);
    let program = [
        Step::Write {
            addr: Cbor::new(&reg),
            type_name: "i32",
            value: StepValue::Literal(RawValue::Int(-5)),
        },
        Step::Until {
            addr: Cbor::new(&back),
            type_name: "i32",
            equals: RawValue::Int(-5),
            timeout: Duration::from_millis(100),
        },
        Step::FailIf {
            addr: Cbor::new(&back),
            type_name: "i32",
            equals: RawValue::Int(0),
            message: "never",
        },
    ];
    assert_eq!(
        bench.steps(&program, &NoArgs, 2_000 * MS).unwrap(),
        Ok(Vec::new())
    );
    let s = server.lock().unwrap();
    assert_eq!(s.holding.get(&50), Some(&0xFFFF));
    assert_eq!(s.holding.get(&51), Some(&0xFFFB));
}
