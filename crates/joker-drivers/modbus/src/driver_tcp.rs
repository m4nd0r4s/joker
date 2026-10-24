//! The runtime half of `modbus-tcp`: one TCP connection per device,
//! reconnected on demand, MBAP-framed requests, and every failure mapped
//! to the batch failures of ref/06 §2.8 (ADR: same PDU/exception handling
//! as `modbus-rtu`, only the framing and the transport differ).

use std::vec::Vec;

use embedded_io_async::{Read, Write};
use joker_hal::types::{J0852, J0853};
use joker_hal::{
    BatchFailure, CommittedWrite, ConnectOut, Cx, Driver, DriverError, HealthOut, IoError, Link,
    MonoNs, OpenArgs, PointId, RawSample, RawSink, RawValue, ReadBatch, TcpEndpoint, WriteAcks,
    WriteKind, WriteStatus, code,
};

use crate::plan::{Address, Quirks, Request, check_point, span, unit_id};
use crate::tcp::{MAX_ADU, header_length, read_data_len, request, transaction_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Point {
    id: PointId,
    addr: Address,
    span: u16,
    output: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counters {
    requests: u64,
    timeouts: u64,
    reconnects: u64,
    exceptions: u64,
    disconnects: u64,
    dropped_samples: u64,
}

/// The runtime half of `modbus-tcp`.
#[derive(Debug)]
pub struct ModbusTcp {
    endpoint: TcpEndpoint,
    stream: Option<joker_hal::TcpStream>,
    unit: u8,
    quirks: Quirks,
    points: Vec<Point>,
    next_transaction: u16,
    last_request: Option<MonoNs>,
    counters: Counters,
}

/// Why one exchange failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fail {
    Timeout,
    Gone,
    Garbled,
    Exception(u8),
}

impl Fail {
    fn batch(self) -> BatchFailure {
        match self {
            Self::Timeout => BatchFailure::Timeout,
            Self::Gone => BatchFailure::NotConnected,
            Self::Garbled => BatchFailure::CommFailure("unexpected response"),
            Self::Exception(1) => BatchFailure::DeviceFault("exception 1: illegal function"),
            Self::Exception(2) => BatchFailure::DeviceFault("exception 2: illegal data address"),
            Self::Exception(3) => BatchFailure::DeviceFault("exception 3: illegal data value"),
            Self::Exception(_) => BatchFailure::DeviceFault("exception"),
        }
    }

    fn write(self) -> WriteStatus {
        match self {
            Self::Timeout => WriteStatus::Timeout,
            Self::Gone => WriteStatus::NotConnected,
            Self::Garbled => WriteStatus::CommFailure,
            Self::Exception(_) => WriteStatus::DeviceRejected,
        }
    }
}

fn io_fail(e: IoError) -> Fail {
    match e {
        IoError::Disconnected | IoError::Denied => Fail::Gone,
        _ => Fail::Garbled,
    }
}

fn read_fail(e: embedded_io_async::ReadExactError<IoError>) -> Fail {
    match e {
        embedded_io_async::ReadExactError::UnexpectedEof => Fail::Gone,
        embedded_io_async::ReadExactError::Other(e) => io_fail(e),
    }
}

impl ModbusTcp {
    fn point(&self, id: PointId) -> Option<Point> {
        self.points.iter().copied().find(|p| p.id == id)
    }

    fn transaction(&mut self) -> u16 {
        let t = self.next_transaction;
        self.next_transaction = self.next_transaction.wrapping_add(1);
        t
    }

    /// Sends `frame` and reads one reply into `out`. The reply starts with
    /// the 6-byte MBAP header (transaction, protocol and length, two bytes
    /// each); its length counts the unit byte plus the PDU (function code
    /// and the rest), and the PDU alone sizes the check of the reply. In
    /// `out`, byte 6 is the unit, byte 7 the function and the rest follows.
    async fn exchange(
        &mut self,
        cx: &Cx<'_>,
        frame: &[u8; 12],
        body_len: usize,
        transaction: u16,
        out: &mut [u8; MAX_ADU],
    ) -> Result<usize, Fail> {
        if self.stream.is_none() {
            self.counters.reconnects += 1;
            self.stream = Some(self.endpoint.connect().await.map_err(io_fail)?);
        }
        if let Some(last) = self.last_request
            && self.quirks.min_interval_ns > 0
        {
            cx.sleep_until(last + self.quirks.min_interval_ns).await;
        }
        self.last_request = Some(cx.now().mono_ns);
        self.counters.requests += 1;
        let io = async {
            let stream = self.stream.as_mut().ok_or(Fail::Gone)?;
            stream.write_all(frame).await.map_err(io_fail)?;
            stream.read_exact(&mut out[..6]).await.map_err(read_fail)?;
            let Some(got_transaction) = transaction_of(&out[..6]) else {
                return Err(Fail::Garbled);
            };
            if got_transaction != transaction {
                return Err(Fail::Garbled);
            }
            let Some(len) = header_length(&out[..6]) else {
                return Err(Fail::Garbled);
            };
            let tail_len = usize::from(len);
            let pdu_len = tail_len.saturating_sub(1);
            if pdu_len == 0 || 6 + tail_len > out.len() {
                return Err(Fail::Garbled);
            }
            stream
                .read_exact(&mut out[6..6 + tail_len])
                .await
                .map_err(read_fail)?;
            let function = frame[7];
            let got_function = out[7];
            if got_function == function | 0x80 {
                return Err(Fail::Exception(out[8]));
            }
            if got_function != function {
                return Err(Fail::Garbled);
            }
            if pdu_len != 1 + body_len {
                return Err(Fail::Garbled);
            }
            Ok(6 + tail_len)
        };
        let result = match cx.within(io).await {
            Ok(r) => r,
            Err(_) => Err(Fail::Timeout),
        };
        match result {
            Err(Fail::Timeout) => self.counters.timeouts += 1,
            Err(Fail::Exception(_)) => self.counters.exceptions += 1,
            Err(Fail::Gone) => {
                self.counters.disconnects += 1;
                self.stream = None;
            }
            _ => {}
        }
        result
    }
}

impl Driver for ModbusTcp {
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError> {
        let Link::Tcp(endpoint) = link else {
            return Err(DriverError::new(
                code!("J0873"),
                "modbus-tcp needs a TCP endpoint",
            ));
        };
        let device = args
            .device(0)
            .ok_or_else(|| DriverError::new(J0852, "a modbus-tcp instance has one device"))?;
        let unit = unit_id(device.config).map_err(|m| DriverError::new(J0852, m))?;
        let quirks =
            Quirks::parse(device.quirks).map_err(|m| DriverError::new(crate::plan::J0818, m))?;
        let mut points = Vec::new();
        for p in args.request().points {
            let addr = check_point(p).map_err(|d| DriverError::new(d.code, d.message))?;
            points.push(Point {
                id: p.id,
                addr,
                span: span(p),
                output: p.direction == joker_hal::Direction::Output,
            });
        }
        for b in args.plan().iter() {
            if Request::decode(b.private).is_none() {
                return Err(DriverError::new(J0853, "plan made by another driver"));
            }
        }
        Ok(Self {
            endpoint,
            stream: None,
            unit,
            quirks,
            points,
            next_transaction: 0,
            last_request: None,
            counters: Counters::default(),
        })
    }

    async fn connect(
        &mut self,
        _cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        self.stream = Some(
            self.endpoint
                .connect()
                .await
                .map_err(|_| DriverError::new(code!("J0876"), "could not connect"))?,
        );
        Ok(())
    }

    async fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let r = Request::decode(batch.private)
            .ok_or(BatchFailure::ConfigError("batch without a request"))?;
        let transaction = self.transaction();
        let frame = request(transaction, self.unit, r.fc, r.start, r.count);
        let data_len = read_data_len(r.fc, r.count);
        let mut buf = [0u8; MAX_ADU];
        let len = self
            .exchange(cx, &frame, 1 + data_len, transaction, &mut buf)
            .await
            .map_err(Fail::batch)?;
        let byte_count_at = 8;
        if usize::from(buf[byte_count_at]) != data_len || len != 9 + data_len {
            return Err(BatchFailure::CommFailure("wrong byte count"));
        }
        let data = &buf[byte_count_at + 1..byte_count_at + 1 + data_len];
        let now = cx.now().mono_ns;
        let mut words = [0u16; 125];
        if matches!(r.fc, 3 | 4) {
            for (i, w) in words.iter_mut().take(usize::from(r.count)).enumerate() {
                *w = u16::from_be_bytes([data[2 * i], data[2 * i + 1]]);
            }
        }
        for &id in batch.points {
            let Some(p) = self.point(id) else {
                continue;
            };
            let Some(offset) = p.addr.at.checked_sub(r.start).map(usize::from) else {
                continue;
            };
            let value = match r.fc {
                1 | 2 => data
                    .get(offset / 8)
                    .map(|byte| RawValue::Bool(byte >> (offset % 8) & 1 == 1)),
                _ => words
                    .get(offset..offset + usize::from(p.span))
                    .map(RawValue::Regs),
            };
            if let Some(value) = value
                && out
                    .push(RawSample {
                        ts_hw: Some(now),
                        ..RawSample::good(id, value)
                    })
                    .is_err()
            {
                self.counters.dropped_samples += 1;
            }
        }
        Ok(())
    }

    async fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) {
        let mut failed_group: Option<u32> = None;
        for (k, w) in writes.iter().enumerate() {
            if let (Some(g), Some(a)) = (failed_group, w.atomic)
                && a.group == g
            {
                acks.set(k, WriteStatus::Skipped, None);
                continue;
            }
            let status = match (self.point(w.point), w.value, w.kind) {
                (_, _, WriteKind::Relinquish) => WriteStatus::Unsupported,
                (Some(p), RawValue::Bool(on), _) if p.output && p.addr.fc == 5 => {
                    let transaction = self.transaction();
                    let frame = request(
                        transaction,
                        self.unit,
                        5,
                        p.addr.at,
                        if on { 0xFF00 } else { 0x0000 },
                    );
                    let mut buf = [0u8; MAX_ADU];
                    match self.exchange(cx, &frame, 4, transaction, &mut buf).await {
                        Ok(12) if buf[7..12] == frame[7..12] => WriteStatus::Ok,
                        Ok(_) => WriteStatus::CommFailure,
                        Err(f) => f.write(),
                    }
                }
                _ => WriteStatus::Unsupported,
            };
            if status != WriteStatus::Ok
                && let Some(a) = w.atomic
            {
                failed_group = Some(a.group);
            }
            acks.set(k, status, Some(cx.now().mono_ns));
        }
    }

    fn health(&self, out: &mut HealthOut<'_>) {
        let c = self.counters;
        out.counter("requests", c.requests);
        out.counter("timeouts", c.timeouts);
        out.counter("reconnects", c.reconnects);
        out.counter("exceptions", c.exceptions);
        out.counter("disconnects", c.disconnects);
        out.counter("dropped_samples", c.dropped_samples);
    }
}
