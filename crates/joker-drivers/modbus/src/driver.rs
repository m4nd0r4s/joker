//! The runtime half of `modbus-rtu`: one request per batch on the serial
//! line, coil writes with their echo, and every failure mapped to the batch
//! failures of ref/06 §2.8.
//!
//! The driver makes one attempt per call; retries are the core's. Before
//! each request it drops whatever the line holds, so a late answer to a
//! cancelled request is never taken for the next one.

use std::vec::Vec;

use joker_hal::types::{J0852, J0853};
use joker_hal::{
    BatchFailure, CommittedWrite, ConnectOut, Cx, Driver, DriverError, HealthOut, IoError, Link,
    MonoNs, OpenArgs, PointId, RawSample, RawSink, RawValue, ReadBatch, SerialPort, WriteAcks,
    WriteKind, WriteStatus, code,
};

use crate::plan::{Address, Quirks, Request, check_point, span, unit_id};
use crate::rtu::{MAX_ADU, crc_ok, read_data_len, request};

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
    crc_errors: u64,
    exceptions: u64,
    disconnects: u64,
    dropped_samples: u64,
}

/// The runtime half of `modbus-rtu`.
#[derive(Debug)]
pub struct ModbusRtu {
    port: SerialPort,
    unit: u8,
    quirks: Quirks,
    points: Vec<Point>,
    last_request: Option<MonoNs>,
    counters: Counters,
}

/// Why one exchange failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fail {
    Timeout,
    Gone,
    Crc,
    Garbled,
    Exception(u8),
}

impl Fail {
    fn batch(self) -> BatchFailure {
        match self {
            Self::Timeout => BatchFailure::Timeout,
            Self::Gone => BatchFailure::NotConnected,
            Self::Crc => BatchFailure::CommFailure("bad CRC"),
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
            Self::Crc | Self::Garbled => WriteStatus::CommFailure,
            Self::Exception(_) => WriteStatus::DeviceRejected,
        }
    }
}

impl ModbusRtu {
    fn point(&self, id: PointId) -> Option<Point> {
        self.points.iter().copied().find(|p| p.id == id)
    }

    async fn exchange(
        &mut self,
        cx: &Cx<'_>,
        frame: &[u8; 8],
        body_len: usize,
        out: &mut [u8; MAX_ADU],
    ) -> Result<usize, Fail> {
        if let Some(last) = self.last_request
            && self.quirks.min_interval_ns > 0
        {
            cx.sleep_until(last + self.quirks.min_interval_ns).await;
        }
        self.last_request = Some(cx.now().mono_ns);
        self.counters.requests += 1;
        let function = frame[1];
        let io = async {
            self.port.discard_input().map_err(|_| Fail::Gone)?;
            self.port.send(frame).await.map_err(io_fail)?;
            self.port
                .receive_exact(&mut out[..2])
                .await
                .map_err(io_fail)?;
            if out[1] == function | 0x80 {
                self.port
                    .receive_exact(&mut out[2..5])
                    .await
                    .map_err(io_fail)?;
                return if !crc_ok(&out[..5]) {
                    Err(Fail::Crc)
                } else if out[0] != frame[0] {
                    Err(Fail::Garbled)
                } else {
                    Err(Fail::Exception(out[2]))
                };
            }
            if out[1] != function {
                return Err(Fail::Garbled);
            }
            let len = 2 + body_len + 2;
            self.port
                .receive_exact(&mut out[2..len])
                .await
                .map_err(io_fail)?;
            if !crc_ok(&out[..len]) {
                return Err(Fail::Crc);
            }
            if out[0] != frame[0] {
                return Err(Fail::Garbled);
            }
            Ok(len)
        };
        let result = match cx.within(io).await {
            Ok(r) => r,
            Err(_) => Err(Fail::Timeout),
        };
        match result {
            Err(Fail::Timeout) => self.counters.timeouts += 1,
            Err(Fail::Crc) => self.counters.crc_errors += 1,
            Err(Fail::Exception(_)) => self.counters.exceptions += 1,
            Err(Fail::Gone) => self.counters.disconnects += 1,
            _ => {}
        }
        result
    }
}

fn io_fail(e: IoError) -> Fail {
    match e {
        IoError::Disconnected | IoError::Denied => Fail::Gone,
        _ => Fail::Garbled,
    }
}

impl Driver for ModbusRtu {
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError> {
        let Link::Serial(port) = link else {
            return Err(DriverError::new(
                code!("J0873"),
                "modbus-rtu needs a serial line",
            ));
        };
        let device = args
            .device(0)
            .ok_or_else(|| DriverError::new(J0852, "a modbus-rtu instance has one device"))?;
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
            port,
            unit,
            quirks,
            points,
            last_request: None,
            counters: Counters::default(),
        })
    }

    async fn connect(
        &mut self,
        _cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        self.port
            .discard_input()
            .map_err(|_| DriverError::new(code!("J0876"), "the serial port is gone"))
    }

    async fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let r = Request::decode(batch.private)
            .ok_or(BatchFailure::ConfigError("batch without a request"))?;
        let frame = request(self.unit, r.fc, r.start, r.count);
        let data_len = read_data_len(r.fc, r.count);
        let mut buf = [0u8; MAX_ADU];
        let len = self
            .exchange(cx, &frame, 1 + data_len, &mut buf)
            .await
            .map_err(Fail::batch)?;
        if usize::from(buf[2]) != data_len || len != 5 + data_len {
            return Err(BatchFailure::CommFailure("wrong byte count"));
        }
        let data = &buf[3..3 + data_len];
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
                    let frame = request(self.unit, 5, p.addr.at, if on { 0xFF00 } else { 0x0000 });
                    let mut buf = [0u8; MAX_ADU];
                    match self.exchange(cx, &frame, 4, &mut buf).await {
                        Ok(8) if buf[..6] == frame[..6] => WriteStatus::Ok,
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
        out.counter("crc_errors", c.crc_errors);
        out.counter("exceptions", c.exceptions);
        out.counter("disconnects", c.disconnects);
        out.counter("dropped_samples", c.dropped_samples);
    }
}
