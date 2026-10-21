//! Host pieces for Linux (feature `std`): a tokio clock for [`Cx`](crate::cx::Cx)
//! and the serial transport of a `serial` bus (ref/01 §5.6).
//!
//! The port is opened non-blocking and driven by tokio's reactor, so a read
//! that waits for the equipment never blocks the `io` thread.

use core::future::Future;
use core::task::{Context, Poll};

use std::boxed::Box;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::string::String;
use std::sync::{Mutex, PoisonError};

use joker_core::{Clock, ClockQuality, Instant};
use rustix::fs::{Mode, OFlags};
use rustix::termios::{
    ControlModes, OptionalActions, QueueSelector, SpecialCodeIndex, tcflush, tcgetattr, tcsetattr,
};
use tokio::io::unix::AsyncFd;

use crate::cbor::Cbor;
use crate::cx::Host;
use crate::link::{IoError, Parity, SerialInfo, SerialIo};
use crate::types::{DriverError, J0852, MonoNs};

/// A host on tokio's clock. Monotonic time starts at 0 when the host is
/// created; UTC comes from the system clock with the quality given.
#[derive(Debug)]
pub struct TokioHost {
    start: tokio::time::Instant,
    utc_start_ns: i64,
    quality: ClockQuality,
    sleeps: Mutex<BTreeMap<MonoNs, Pin<Box<tokio::time::Sleep>>>>,
}

impl TokioHost {
    /// A host starting now, with the clock quality the node computed.
    pub fn new(quality: ClockQuality) -> Self {
        let utc_start_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
        Self {
            start: tokio::time::Instant::now(),
            utc_start_ns,
            quality,
            sleeps: Mutex::new(BTreeMap::new()),
        }
    }

    fn mono(&self) -> MonoNs {
        u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

impl Clock for TokioHost {
    fn now(&self) -> Instant {
        let mono_ns = self.mono();
        Instant {
            mono_ns,
            utc_ns: self
                .utc_start_ns
                .saturating_add(i64::try_from(mono_ns).unwrap_or(i64::MAX)),
            clock: self.quality,
        }
    }
}

impl Host for TokioHost {
    fn poll_sleep(&self, at: MonoNs, cx: &mut Context<'_>) -> Poll<()> {
        let now = self.mono();
        let mut sleeps = self.sleeps.lock().unwrap_or_else(PoisonError::into_inner);
        sleeps.retain(|&t, _| t > now);
        if at <= now {
            return Poll::Ready(());
        }
        let start = self.start;
        let sleep = sleeps.entry(at).or_insert_with(|| {
            Box::pin(tokio::time::sleep_until(
                start + std::time::Duration::from_nanos(at),
            ))
        });
        match sleep.as_mut().poll(cx) {
            Poll::Ready(()) => {
                sleeps.remove(&at);
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn log(&self, level: crate::cx::LogLevel, message: &str) {
        let _ = (level, message);
    }
}

/// State of a modem control line while the port is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineControl {
    /// Do not touch the line.
    Keep,
    /// Deasserted; the default, so that opening the port does not reset an
    /// ESP32 board through its auto-reset circuit.
    Low,
    /// Asserted.
    High,
}

impl LineControl {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "keep" => Self::Keep,
            "low" => Self::Low,
            "high" => Self::High,
            _ => return None,
        })
    }

    fn level(self) -> Option<bool> {
        match self {
            Self::Keep => None,
            Self::Low => Some(false),
            Self::High => Some(true),
        }
    }
}

/// The keys of a `serial` bus (ref/01 §5.6) that the transport uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerialConfig {
    /// Device path.
    pub port: PathBuf,
    /// Line settings.
    pub info: SerialInfo,
    /// DTR while open.
    pub dtr: LineControl,
    /// RTS while open.
    pub rts: LineControl,
}

fn bad(detail: &str) -> DriverError {
    DriverError::new(J0852, detail)
}

impl SerialConfig {
    /// Reads the bus `params` (deterministic CBOR of `[buses.<b>]`).
    pub fn from_params(params: Cbor<'_>) -> Result<Self, DriverError> {
        let get = |key| params.get(key).map_err(|_| bad("bus params are not a map"));
        let port = get("port")?
            .ok_or_else(|| bad("serial bus without `port`"))?
            .as_str()
            .map_err(|_| bad("`port` must be a path"))?;
        let baud = get("baud")?
            .ok_or_else(|| bad("serial bus without `baud`"))?
            .as_u64()
            .ok()
            .and_then(|b| u32::try_from(b).ok())
            .filter(|b| (300..=4_000_000).contains(b))
            .ok_or_else(|| bad("`baud` must be 300–4000000"))?;
        let text = |key, default: &'static str| -> Result<String, DriverError> {
            match get(key)? {
                None => Ok(String::from(default)),
                Some(v) => v
                    .as_str()
                    .map(String::from)
                    .map_err(|_| bad("expected a string")),
            }
        };
        let int = |key, default: u64| -> Result<u64, DriverError> {
            match get(key)? {
                None => Ok(default),
                Some(v) => v.as_u64().map_err(|_| bad("expected an integer")),
            }
        };
        let parity = match text("parity", "none")?.as_str() {
            "none" => Parity::None,
            "even" => Parity::Even,
            "odd" => Parity::Odd,
            _ => return Err(bad("`parity` must be none, even or odd")),
        };
        let stop_bits = match int("stop", 1)? {
            1 => 1,
            2 => 2,
            _ => return Err(bad("`stop` must be 1 or 2")),
        };
        let data_bits = match int("data_bits", 8)? {
            7 => 7,
            8 => 8,
            _ => return Err(bad("`data_bits` must be 7 or 8")),
        };
        let line = |key| -> Result<LineControl, DriverError> {
            LineControl::parse(&text(key, "low")?)
                .ok_or_else(|| bad("`dtr`/`rts` must be keep, low or high"))
        };
        Ok(Self {
            port: PathBuf::from(port),
            info: SerialInfo {
                baud,
                data_bits,
                parity,
                stop_bits,
            },
            dtr: line("dtr")?,
            rts: line("rts")?,
        })
    }
}

fn io_error(e: &io::Error) -> IoError {
    match e.kind() {
        io::ErrorKind::PermissionDenied => IoError::Denied,
        io::ErrorKind::Unsupported => IoError::Unsupported,
        io::ErrorKind::NotFound
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::NotConnected
        | io::ErrorKind::UnexpectedEof => IoError::Disconnected,
        _ if e.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error())
            || e.raw_os_error() == Some(rustix::io::Errno::NXIO.raw_os_error()) =>
        {
            IoError::Disconnected
        }
        _ => IoError::Other,
    }
}

/// A serial port on tokio's reactor.
#[derive(Debug)]
pub struct TokioSerial {
    fd: AsyncFd<File>,
    info: SerialInfo,
}

impl TokioSerial {
    /// Opens and configures the port: raw mode, the line settings, and the
    /// modem lines. Must run inside a tokio runtime.
    ///
    /// The reactor registration uses `AsyncFd::new`, deprecated in tokio
    /// 1.54 because a generic `T` could report a different descriptor over
    /// time; a `File` owns its descriptor and always reports the same one,
    /// which is the condition `AsyncFd::register` asks for.
    #[allow(deprecated)]
    pub fn open(config: &SerialConfig) -> io::Result<Self> {
        let fd = rustix::fs::open(
            Path::new(&config.port),
            OFlags::RDWR | OFlags::NOCTTY | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let mut t = tcgetattr(&fd)?;
        t.make_raw();
        t.set_speed(config.info.baud)?;
        let mut c = t.control_modes;
        c.remove(
            ControlModes::CSIZE
                | ControlModes::PARENB
                | ControlModes::PARODD
                | ControlModes::CSTOPB,
        );
        c.insert(ControlModes::CLOCAL | ControlModes::CREAD);
        c.insert(if config.info.data_bits == 7 {
            ControlModes::CS7
        } else {
            ControlModes::CS8
        });
        match config.info.parity {
            Parity::None => {}
            Parity::Even => c.insert(ControlModes::PARENB),
            Parity::Odd => c.insert(ControlModes::PARENB | ControlModes::PARODD),
        }
        if config.info.stop_bits == 2 {
            c.insert(ControlModes::CSTOPB);
        }
        t.control_modes = c;
        t.special_codes[SpecialCodeIndex::VMIN] = 1;
        t.special_codes[SpecialCodeIndex::VTIME] = 0;
        tcsetattr(&fd, OptionalActions::Now, &t)?;
        joker_serial_sys::set_modem_lines(fd.as_fd(), config.dtr.level(), config.rts.level())?;
        tcflush(&fd, QueueSelector::IOFlush)?;
        Ok(Self {
            fd: AsyncFd::new(File::from(fd))?,
            info: config.info,
        })
    }
}

impl SerialIo for TokioSerial {
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        loop {
            let mut guard = match self.fd.poll_read_ready(cx) {
                Poll::Ready(Ok(g)) => g,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(io_error(&e))),
                Poll::Pending => return Poll::Pending,
            };
            match guard.try_io(|inner| (&mut inner.get_ref()).read(buf)) {
                Ok(Ok(0)) if !buf.is_empty() => return Poll::Ready(Err(IoError::Disconnected)),
                Ok(Ok(n)) => return Poll::Ready(Ok(n)),
                Ok(Err(e)) => return Poll::Ready(Err(io_error(&e))),
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        loop {
            let mut guard = match self.fd.poll_write_ready(cx) {
                Poll::Ready(Ok(g)) => g,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(io_error(&e))),
                Poll::Pending => return Poll::Pending,
            };
            match guard.try_io(|inner| (&mut inner.get_ref()).write(buf)) {
                Ok(Ok(n)) => return Poll::Ready(Ok(n)),
                Ok(Err(e)) => return Poll::Ready(Err(io_error(&e))),
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        Poll::Ready(Ok(()))
    }

    fn discard_input(&mut self) -> Result<(), IoError> {
        tcflush(self.fd.get_ref(), QueueSelector::IFlush).map_err(|e| io_error(&e.into()))
    }

    fn info(&self) -> SerialInfo {
        self.info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cbor::Value;
    use crate::cx::Cx;
    use crate::link::SerialPort;
    use std::vec::Vec;

    #[test]
    fn bus_params_follow_ref01() {
        let params = Value::map([
            ("kind", Value::text("serial")),
            ("port", Value::text("/dev/ttyUSB0")),
            ("baud", Value::Uint(115_200)),
        ])
        .encode();
        let c = SerialConfig::from_params(Cbor::new(&params)).unwrap();
        assert_eq!(c.info, SerialInfo::new_8n1(115_200));
        assert_eq!((c.dtr, c.rts), (LineControl::Low, LineControl::Low));
        let params = Value::map([
            ("port", Value::text("/dev/ttyUSB0")),
            ("baud", Value::Uint(9600)),
            ("parity", Value::text("even")),
            ("stop", Value::Uint(2)),
            ("dtr", Value::text("keep")),
            ("rts", Value::text("high")),
        ])
        .encode();
        let c = SerialConfig::from_params(Cbor::new(&params)).unwrap();
        assert_eq!((c.info.parity, c.info.stop_bits), (Parity::Even, 2));
        assert_eq!((c.dtr, c.rts), (LineControl::Keep, LineControl::High));
        for bad in [
            Value::map([("baud", Value::Uint(9600))]),
            Value::map([("port", Value::text("/x")), ("baud", Value::Uint(10))]),
            Value::map([
                ("port", Value::text("/x")),
                ("baud", Value::Uint(9600)),
                ("dtr", Value::text("on")),
            ]),
        ] {
            assert_eq!(
                SerialConfig::from_params(Cbor::new(&bad.encode()))
                    .unwrap_err()
                    .code,
                J0852
            );
        }
    }

    #[tokio::test]
    async fn a_pty_carries_bytes_both_ways() {
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let name = ptsname(&master, Vec::new()).unwrap();
        let config = SerialConfig {
            port: PathBuf::from(name.to_str().unwrap()),
            info: SerialInfo::new_8n1(115_200),
            dtr: LineControl::Keep,
            rts: LineControl::Keep,
        };
        let mut port = SerialPort::new(TokioSerial::open(&config).unwrap());
        let mut master = File::from(master);
        let host = TokioHost::new(ClockQuality::Unsynced);
        let cx = Cx::new(&host, host.now().mono_ns + 2_000_000_000);
        master.write_all(&[1, 2, 3]).unwrap();
        let mut buf = [0u8; 3];
        cx.within(port.receive_exact(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(buf, [1, 2, 3]);
        port.send(&[4, 5]).await.unwrap();
        let mut back = [0u8; 2];
        master.read_exact(&mut back).unwrap();
        assert_eq!(back, [4, 5]);
        let short = Cx::new(&host, host.now().mono_ns + 20_000_000);
        let r = short.within(port.receive_exact(&mut buf)).await;
        assert!(r.is_err(), "{r:?} {buf:?}");
        assert!(host.now().mono_ns >= 20_000_000);
    }
}
