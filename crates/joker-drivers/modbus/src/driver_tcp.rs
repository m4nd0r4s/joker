//! The runtime half of `modbus-tcp`: one TCP connection per device,
//! reconnected on demand, MBAP-framed requests, output writes confirmed by
//! their echo (coils with FC 5 or 15, registers with FC 6 or 16, a
//! register bit by read-modify-write, ADR 0076), the reads and writes of a
//! step by address (`raw-access`, ADR 0073), and every failure mapped
//! to the batch failures of ref/06 §2.8 (ADR: same PDU/exception handling
//! as `modbus-rtu`, only the framing and the transport differ).

use std::vec::Vec;

use embedded_io_async::{Read, Write};
use joker_hal::types::{J0852, J0853};
use joker_hal::{
    AddrRead, AddrWrite, BatchFailure, CommittedWrite, ConnectOut, Cx, Driver, DriverError,
    HealthOut, IoError, Link, MonoNs, OpenArgs, PointId, RawSample, RawSink, RawValue, ReadBatch,
    TcpEndpoint, WriteAcks, WriteKind, WriteStatus, code,
};

use crate::access::{Access, OutputWrite, Pdu, echoes, register_write, with_bit};
use crate::plan::{Address, Quirks, Request, check_point, span, unit_id};
use crate::sunspec::{Discovery, Found, ReadError, Step, Walk, report};
use crate::tcp::{MAX_ADU, frame, header_length, read_data_len, request, transaction_of};

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
    discovery: Option<Discovery>,
    pinned: Option<Vec<u8>>,
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
            Self::Exception(4) => BatchFailure::DeviceFault("exception 4: server device failure"),
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
        frame: &[u8],
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

    /// Sends a write request and checks that the answer echoes it.
    async fn send_write(&mut self, cx: &Cx<'_>, pdu: &Pdu) -> Result<(), Fail> {
        let transaction = self.transaction();
        let (request, n) = frame(transaction, self.unit, pdu.as_bytes());
        let mut buf = [0u8; MAX_ADU];
        let len = self
            .exchange(cx, &request[..n], 4, transaction, &mut buf)
            .await?;
        if len == 12 && echoes(&buf[7..12], pdu.as_bytes()) {
            Ok(())
        } else {
            Err(Fail::Garbled)
        }
    }

    /// Reads the one holding register at `at` with FC 3.
    async fn read_holding(&mut self, cx: &Cx<'_>, at: u16) -> Result<u16, Fail> {
        let words = self.read_holding_regs(cx, at, 1).await?;
        words.first().copied().ok_or(Fail::Garbled)
    }

    /// Writes one output point and says how it went.
    async fn write_output(&mut self, cx: &Cx<'_>, p: Point, value: RawValue<'_>) -> WriteStatus {
        let op = match OutputWrite::of(&p.addr, p.span, value, self.quirks.single_fc16) {
            Ok(op) => op,
            Err(status) => return status,
        };
        let done = match op {
            OutputWrite::Direct(pdu) => self.send_write(cx, &pdu).await,
            OutputWrite::Bit { at, bit, on, fc16 } => match self.read_holding(cx, at).await {
                Ok(word) => {
                    self.send_write(cx, &register_write(at, with_bit(word, bit, on), fc16))
                        .await
                }
                Err(f) => Err(f),
            },
        };
        match done {
            Ok(()) => WriteStatus::Ok,
            Err(f) => f.write(),
        }
    }

    /// Reads `count` holding registers from `at` with FC 3.
    async fn read_holding_regs(
        &mut self,
        cx: &Cx<'_>,
        at: u16,
        count: u16,
    ) -> Result<Vec<u16>, Fail> {
        let transaction = self.transaction();
        let frame = request(transaction, self.unit, 3, at, count);
        let data_len = read_data_len(3, count);
        let mut buf = [0u8; MAX_ADU];
        let len = self
            .exchange(cx, &frame, 1 + data_len, transaction, &mut buf)
            .await?;
        if usize::from(buf[8]) != data_len || len != 9 + data_len {
            return Err(Fail::Garbled);
        }
        Ok(buf[9..9 + data_len]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect())
    }

    /// Walks the SunSpec chain (ADR 0077), with FC 3 reads only.
    async fn discover(&mut self, cx: &Cx<'_>, d: &Discovery) -> Result<Found, DriverError> {
        let mut walk = Walk::new(d);
        loop {
            match walk.step() {
                Step::Read { at, count } => {
                    let read = self.read_holding_regs(cx, at, count).await;
                    walk.feed(match &read {
                        Ok(words) => Ok(words.as_slice()),
                        Err(Fail::Exception(_)) => Err(ReadError::Exception),
                        Err(f) => Err(ReadError::Failed(f.batch())),
                    });
                }
                Step::Done => return Ok(walk.found()),
                Step::Failed(e) => return Err(DriverError::new(code!("J0876"), e.message())),
            }
        }
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
        let discovery =
            Discovery::parse(device.quirks).map_err(|m| DriverError::new(code!("J0825"), m))?;
        let pinned = device.discovered.map(|d| d.as_bytes().to_vec());
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
            discovery,
            pinned,
            next_transaction: 0,
            last_request: None,
            counters: Counters::default(),
        })
    }

    async fn connect(
        &mut self,
        cx: &mut Cx<'_>,
        out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        self.stream = Some(
            self.endpoint
                .connect()
                .await
                .map_err(|_| DriverError::new(code!("J0876"), "could not connect"))?,
        );
        if let Some(d) = self.discovery.clone() {
            let found = self.discover(cx, &d).await?;
            report(&found, self.pinned.as_deref(), out)?;
        }
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
            let status = match (self.point(w.point), w.kind) {
                (_, WriteKind::Relinquish) => WriteStatus::Unsupported,
                (Some(p), _) if p.output => self.write_output(cx, p, w.value).await,
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

    async fn read_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrRead<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let access = Access::parse(req.addr, req.type_name)?;
        let pdu = access.read()?;
        let transaction = self.transaction();
        let (request, n) = frame(transaction, self.unit, pdu.as_bytes());
        let data_len = access.read_data_len();
        let mut buf = [0u8; MAX_ADU];
        let len = self
            .exchange(cx, &request[..n], 1 + data_len, transaction, &mut buf)
            .await
            .map_err(Fail::batch)?;
        if usize::from(buf[8]) != data_len || len != 9 + data_len {
            return Err(BatchFailure::CommFailure("wrong byte count"));
        }
        let value = access.decode(&buf[9..9 + data_len])?;
        let sample = RawSample {
            ts_hw: Some(cx.now().mono_ns),
            ..RawSample::good(0, value)
        };
        if out.push(sample).is_err() {
            self.counters.dropped_samples += 1;
        }
        Ok(())
    }

    async fn write_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrWrite<'_>,
    ) -> Result<(), BatchFailure> {
        let access = Access::parse(req.addr, req.type_name)?;
        let pdu = access.write(req.value)?;
        self.send_write(cx, &pdu).await.map_err(Fail::batch)
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
