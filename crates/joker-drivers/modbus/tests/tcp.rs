//! `modbus-tcp` against an in-memory double that speaks MBAP: one holding
//! register (read, fc 3) and one coil (write, fc 5).

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use joker_driver_modbus::tcp::transaction_of;
use joker_driver_modbus::{ModbusTcp, ModbusTcpPlanner};
use joker_hal::acquire::OwnedRaw;
use joker_hal::cbor::Value;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::{Bench, OutputHarness, VirtualHost};
use joker_hal::{
    CommitBox, CommitRx, CommitTx, CommittedWrite, IoError, Link, Planner, RawEncoding, RawKind,
    RawStatus, RxState, TcpConnect, TcpEndpoint, TcpIo, TcpStream, WriteStatus,
};

const MS: u64 = 1_000_000;

#[derive(Debug, Default)]
struct Device {
    holding0: u16,
    coil0: bool,
    refuse_connect: bool,
    garble_transaction: bool,
    silent: bool,
}

fn serve(d: &mut Device, frame: &[u8]) -> Vec<u8> {
    let transaction = transaction_of(frame).unwrap();
    let unit = frame[6];
    let fc = frame[7];
    let a = u16::from_be_bytes([frame[8], frame[9]]);
    let b = u16::from_be_bytes([frame[10], frame[11]]);
    let mut pdu = match (fc, a, b) {
        (3, 0, 1) => {
            let [h, l] = d.holding0.to_be_bytes();
            vec![3, 2, h, l]
        }
        (5, 0, v) if v == 0xFF00 || v == 0 => {
            d.coil0 = v == 0xFF00;
            vec![5, 0, 0, frame[10], frame[11]]
        }
        _ => vec![fc | 0x80, 2],
    };
    let mut out = vec![];
    let txn = (if d.garble_transaction {
        transaction.wrapping_add(1)
    } else {
        transaction
    })
    .to_be_bytes();
    out.extend_from_slice(&txn);
    out.extend_from_slice(&[0, 0]);
    let len = (1 + pdu.len()) as u16;
    out.extend_from_slice(&len.to_be_bytes());
    out.push(unit);
    out.append(&mut pdu);
    out
}

/// `poll_read` hands back whatever `poll_write` queued for it (`serve`
/// never waits, so the reply is always ready by the time it is read).
struct Double {
    device: Arc<Mutex<Device>>,
    reply: Arc<Mutex<std::collections::VecDeque<u8>>>,
}

impl TcpIo for Double {
    fn poll_read(&mut self, _cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        if self.device.lock().unwrap().silent {
            return Poll::Pending;
        }
        let mut reply = self.reply.lock().unwrap();
        if reply.is_empty() {
            return Poll::Ready(Err(IoError::Disconnected));
        }
        let mut n = 0;
        while n < buf.len() {
            let Some(b) = reply.pop_front() else { break };
            buf[n] = b;
            n += 1;
        }
        Poll::Ready(Ok(n))
    }

    fn poll_write(&mut self, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        let mut device = self.device.lock().unwrap();
        let reply = serve(&mut device, buf);
        self.reply.lock().unwrap().extend(reply);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        Poll::Ready(Ok(()))
    }
}

struct Connector {
    device: Arc<Mutex<Device>>,
}

impl TcpConnect for Connector {
    fn poll_connect(&mut self, _cx: &mut Context<'_>) -> Poll<Result<TcpStream, IoError>> {
        if self.device.lock().unwrap().refuse_connect {
            return Poll::Ready(Err(IoError::Disconnected));
        }
        Poll::Ready(Ok(TcpStream::new(Double {
            device: self.device.clone(),
            reply: Arc::new(Mutex::new(std::collections::VecDeque::new())),
        })))
    }
}

fn addr(at: u16, fc: u8) -> Vec<u8> {
    Value::map([
        ("at", Value::Uint(u64::from(at))),
        ("fc", Value::Uint(u64::from(fc))),
    ])
    .encode()
}

fn request() -> OwnedRequest {
    OwnedRequest {
        bus: None,
        devices: vec![OwnedDevice::new(
            "pcs",
            Value::map([("unit_id", Value::Uint(1))]).encode(),
            2_000 * MS,
        )],
        points: vec![
            OwnedPoint::input(0, "pcs.p_ac", RawKind::Regs, 1, "u16", addr(0, 3), 100 * MS),
            OwnedPoint::output(1, "pcs.apply", RawKind::Bool, 0, "bool", addr(0, 5)),
        ],
    }
}

struct Writes {
    harness: OutputHarness,
    tx: CommitTx<'static>,
    rx: CommitRx<'static>,
}

impl Writes {
    fn new() -> Self {
        let harness =
            OutputHarness::new(&[("pcs.apply", joker_core::ValueType::Bool, 0, 1)]).unwrap();
        let bindings: &'static [_] =
            Box::leak(harness.bindings(&[RawEncoding::Bool]).into_boxed_slice());
        let boxes: &'static [CommitBox] = Box::leak(Box::new([CommitBox::new()]));
        let last = Box::leak(Box::new([None]));
        let state = Box::leak(Box::new([RxState::default()]));
        let tx = CommitTx::new(harness.outputs(), bindings, boxes, last).unwrap();
        let rx = CommitRx::new(bindings, boxes, state).unwrap();
        Self { harness, tx, rx }
    }

    fn set(&mut self, on: bool) -> Vec<CommittedWrite<'static>> {
        self.harness.manual("pcs.apply", u64::from(on));
        self.tx.publish(self.harness.outputs());
        let mut out = Vec::new();
        self.rx.drain_into(&mut out);
        out
    }
}

#[test]
fn read_a_holding_register_and_write_a_coil() {
    let device = Arc::new(Mutex::new(Device {
        holding0: 42,
        coil0: false,
        refuse_connect: false,
        garble_transaction: false,
        silent: false,
    }));
    let req = request();
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let link = Link::Tcp(TcpEndpoint::new(Connector {
        device: device.clone(),
    }));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, ModbusTcp> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();

    let settled = bench.read(0).unwrap();
    assert_eq!(settled[0].status, RawStatus::OK);
    assert_eq!(settled[0].value, OwnedRaw::Regs(vec![42]));

    let mut writes = Writes::new();
    let w = writes.set(true);
    assert_eq!(bench.write(&w).unwrap()[0].status, WriteStatus::Ok);
    assert!(device.lock().unwrap().coil0);
}

#[test]
fn a_refused_connection_surfaces_as_not_connected() {
    let device = Arc::new(Mutex::new(Device {
        holding0: 0,
        coil0: false,
        refuse_connect: true,
        garble_transaction: false,
        silent: false,
    }));
    let req = request();
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let link = Link::Tcp(TcpEndpoint::new(Connector { device }));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, ModbusTcp> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    assert!(bench.connect().is_err());
}

#[test]
fn a_mismatched_transaction_id_is_rejected() {
    let device = Arc::new(Mutex::new(Device {
        holding0: 42,
        coil0: false,
        refuse_connect: false,
        garble_transaction: true,
        silent: false,
    }));
    let req = request();
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let link = Link::Tcp(TcpEndpoint::new(Connector { device }));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, ModbusTcp> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();
    let settled = bench.read(0).unwrap();
    assert_ne!(settled[0].status, RawStatus::OK);
}

#[test]
fn a_silent_connection_times_out_as_comm_failure() {
    let device = Arc::new(Mutex::new(Device {
        holding0: 42,
        coil0: false,
        refuse_connect: false,
        garble_transaction: false,
        silent: false,
    }));
    let req = request();
    let views = req.views();
    let plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let link = Link::Tcp(TcpEndpoint::new(Connector {
        device: device.clone(),
    }));
    let host = Arc::new(VirtualHost::new(0));
    let mut bench: Bench<'_, ModbusTcp> =
        Bench::open(host, views.request(), plan.view(), link).unwrap();
    bench.connect().unwrap();
    assert_eq!(bench.read(0).unwrap()[0].status, RawStatus::OK);

    device.lock().unwrap().silent = true;
    let settled = bench.read(0).unwrap();
    assert_eq!(
        settled[0].status,
        RawStatus::bad(joker_hal::Reason::COMM_FAILURE)
    );
}
