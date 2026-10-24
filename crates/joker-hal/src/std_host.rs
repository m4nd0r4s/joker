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
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use joker_core::{Clock, ClockQuality, Instant};
use rustix::fs::{Mode, OFlags};
use rustix::termios::{
    ControlModes, OptionalActions, QueueSelector, SpecialCodeIndex, tcflush, tcgetattr, tcsetattr,
};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::cbor::Cbor;
use crate::cx::Host;
use crate::link::{
    GpioBias, GpioDrive, GpioLineConfig, GpioLines, I2cTransfers, IoError, Parity, SerialInfo,
    SerialIo,
};
use crate::types::{DriverError, J0852, MonoNs};

/// The node's current clock quality, shared between whoever evaluates it
/// and every clock that stamps instants with it. Reading it is one atomic
/// load, so a clock on the tick path can read it on every instant.
#[derive(Debug)]
pub struct ClockQualityCell(AtomicU8);

impl ClockQualityCell {
    /// A cell holding `quality`.
    pub const fn new(quality: ClockQuality) -> Self {
        Self(AtomicU8::new(quality as u8))
    }

    /// The quality held.
    pub fn get(&self) -> ClockQuality {
        ClockQuality::from_u8(self.0.load(Ordering::Relaxed)).unwrap_or(ClockQuality::Unsynced)
    }

    /// Replaces the quality held.
    pub fn set(&self, quality: ClockQuality) {
        self.0.store(quality as u8, Ordering::Relaxed);
    }
}

/// A host on tokio's clock. Monotonic time starts at 0 when the host is
/// created; UTC comes from the system clock, with the quality its
/// [`ClockQualityCell`] holds when each instant is read.
#[derive(Debug)]
pub struct TokioHost {
    start: tokio::time::Instant,
    utc_start_ns: i64,
    quality: Arc<ClockQualityCell>,
    sleeps: Mutex<BTreeMap<MonoNs, Pin<Box<tokio::time::Sleep>>>>,
}

impl TokioHost {
    /// A host starting now, with a fixed clock quality.
    pub fn new(quality: ClockQuality) -> Self {
        Self::tracking(Arc::new(ClockQualityCell::new(quality)))
    }

    /// A host starting now, whose instants carry the quality `quality`
    /// holds at the time they are read.
    pub fn tracking(quality: Arc<ClockQualityCell>) -> Self {
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
            clock: self.quality.get(),
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

/// A connected TCP socket on tokio's reactor.
#[derive(Debug)]
pub struct TokioTcp(tokio::net::TcpStream);

impl crate::link::TcpIo for TokioTcp {
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        let mut read_buf = tokio::io::ReadBuf::new(buf);
        match Pin::new(&mut self.0).poll_read(cx, &mut read_buf) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(read_buf.filled().len())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io_error(&e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        match Pin::new(&mut self.0).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => Poll::Ready(Ok(n)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io_error(&e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        match Pin::new(&mut self.0).poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io_error(&e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Connects to a fixed `host:port` on tokio's reactor, reconnecting on
/// every call (ref/06 §3.4, `Link::Tcp`). A host name is resolved again on
/// every call, so a device whose address changes in DNS is found on the
/// next reconnection.
pub struct TokioTcpConnector {
    addr: String,
    connecting: Option<Pin<Box<dyn Future<Output = io::Result<tokio::net::TcpStream>> + Send>>>,
}

impl core::fmt::Debug for TokioTcpConnector {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TokioTcpConnector")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl TokioTcpConnector {
    /// A connector for `addr`. Must run inside a tokio runtime.
    pub fn new(addr: std::net::SocketAddr) -> Self {
        Self::to(&std::format!("{addr}"))
    }

    /// A connector for `address`, `"<host>:<port>"` where the host is a
    /// name, an IPv4 literal or a bracketed IPv6 literal. Must run inside
    /// a tokio runtime.
    pub fn to(address: &str) -> Self {
        Self {
            addr: String::from(address),
            connecting: None,
        }
    }
}

impl crate::link::TcpConnect for TokioTcpConnector {
    fn poll_connect(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<crate::link::TcpStream, IoError>> {
        let fut = self
            .connecting
            .get_or_insert_with(|| Box::pin(tokio::net::TcpStream::connect(self.addr.clone())));
        match fut.as_mut().poll(cx) {
            Poll::Ready(Ok(stream)) => {
                self.connecting = None;
                Poll::Ready(Ok(crate::link::TcpStream::new(TokioTcp(stream))))
            }
            Poll::Ready(Err(e)) => {
                self.connecting = None;
                Poll::Ready(Err(io_error(&e)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A Linux `gpiochip`'s lines (`Link::Gpio`, ref/06 §3.4): chardev **v1**
/// (`<linux/gpio.h>`, `joker-gpio-sys`), not v2 — the bench board's
/// kernel (4.19) predates v2 (needs 5.10+). Every line it serves is
/// requested once at [`LinuxGpio::open`] time, each with its own fixed
/// direction (v1's `GPIO_GET_LINEHANDLE_IOCTL` fixes input/output per
/// request, unlike v2's per-line reconfiguration); a line the manifest
/// never granted is [`IoError::Unsupported`], the same answer a line on
/// a chip this build never opened would give.
pub struct LinuxGpio {
    handles: BTreeMap<u32, std::os::fd::OwnedFd>,
}

impl core::fmt::Debug for LinuxGpio {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LinuxGpio")
            .field(
                "lines",
                &self.handles.keys().collect::<alloc::vec::Vec<_>>(),
            )
            .finish()
    }
}

impl LinuxGpio {
    /// Opens `chip_path` (e.g. `/dev/gpiochip0`) and requests a handle
    /// for each line the manifest granted, with its direction, bias and
    /// drive; an output starts at the physical level its config carries
    /// (its declared `safe`), set by the kernel in the request itself.
    /// Fails the whole chip if the device or any one line's handle cannot
    /// be opened, a bias the kernel does not support (v1 bias flags need
    /// Linux 5.5+) included: a device the manifest expects known-good
    /// lines from is better refused here than left half-granted.
    pub fn open(chip_path: &Path, lines: &[GpioLineConfig]) -> io::Result<Self> {
        let chip = File::open(chip_path)?;
        let mut handles = BTreeMap::new();
        for config in lines {
            let handle =
                joker_gpio_sys::request_line_handle(chip.as_fd(), &line_request(config), "jokerd")?;
            handles.insert(config.line, handle);
        }
        Ok(Self { handles })
    }
}

/// The chardev request of one granted line.
fn line_request(config: &GpioLineConfig) -> joker_gpio_sys::LineRequest {
    joker_gpio_sys::LineRequest {
        line: config.line,
        output: config.output,
        bias: match config.bias {
            GpioBias::AsIs => joker_gpio_sys::LineBias::AsIs,
            GpioBias::PullUp => joker_gpio_sys::LineBias::PullUp,
            GpioBias::PullDown => joker_gpio_sys::LineBias::PullDown,
            GpioBias::Disabled => joker_gpio_sys::LineBias::Disabled,
        },
        drive: match config.drive {
            GpioDrive::PushPull => joker_gpio_sys::LineDrive::PushPull,
            GpioDrive::OpenDrain => joker_gpio_sys::LineDrive::OpenDrain,
            GpioDrive::OpenSource => joker_gpio_sys::LineDrive::OpenSource,
        },
    }
}

impl GpioLines for LinuxGpio {
    fn get(&mut self, line: u32) -> Result<bool, IoError> {
        let handle = self.handles.get(&line).ok_or(IoError::Unsupported)?;
        joker_gpio_sys::get_line_value(handle.as_fd()).map_err(|e| io_error(&e))
    }

    fn set(&mut self, line: u32, value: bool) -> Result<(), IoError> {
        let handle = self.handles.get(&line).ok_or(IoError::Unsupported)?;
        joker_gpio_sys::set_line_value(handle.as_fd(), value).map_err(|e| io_error(&e))
    }
}

/// A Linux `i2c-dev` bus (`Link::I2c`, ref/06 §3.4): `/dev/i2c-N`, the
/// `dev` of an `i2c` bus (ref/01 §5.6), opened read-write. Each transfer
/// is one `I2C_RDWR` call (`joker-i2c-sys`): the register write and the
/// data read reach the chip joined by a repeated start, and the kernel
/// serializes them with every other user of the adapter.
#[derive(Debug)]
pub struct LinuxI2c {
    bus: File,
}

impl LinuxI2c {
    /// Opens `dev` (e.g. `/dev/i2c-1`) for reading and writing. A path
    /// that is not an `i2c-dev` node opens, but its first transfer fails.
    pub fn open(dev: &Path) -> io::Result<Self> {
        let bus = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dev)?;
        Ok(Self { bus })
    }
}

impl I2cTransfers for LinuxI2c {
    fn transfer(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), IoError> {
        joker_i2c_sys::transfer(self.bus.as_fd(), addr, write, read).map_err(|e| i2c_error(&e))
    }
}

/// An `i2c-dev` failure as an [`IoError`]: a chip that does not
/// acknowledge (`ENXIO`, `EREMOTEIO`) or a bus timeout is a device fault
/// ([`IoError::Other`]), not a lost link; a refused open or an address
/// another driver owns (`EBUSY`) is [`IoError::Denied`].
fn i2c_error(e: &io::Error) -> IoError {
    let errno = e.raw_os_error();
    if errno.is_some_and(no_acknowledge) {
        return IoError::Other;
    }
    if errno == Some(rustix::io::Errno::BUSY.raw_os_error()) {
        return IoError::Denied;
    }
    io_error(e)
}

/// The errors of a chip that did not answer: `ENXIO`, `EREMOTEIO` (Linux
/// only) and `ETIMEDOUT`.
fn no_acknowledge(errno: i32) -> bool {
    #[cfg(target_os = "linux")]
    let remote = errno == rustix::io::Errno::REMOTEIO.raw_os_error();
    #[cfg(not(target_os = "linux"))]
    let remote = false;
    remote
        || errno == rustix::io::Errno::NXIO.raw_os_error()
        || errno == rustix::io::Errno::TIMEDOUT.raw_os_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cbor::Value;
    use crate::cx::Cx;
    use crate::link::SerialPort;
    use std::vec::Vec;

    #[tokio::test]
    async fn a_tracking_host_stamps_the_quality_its_cell_holds_now() {
        let cell = Arc::new(ClockQualityCell::new(ClockQuality::Synced));
        let host = TokioHost::tracking(Arc::clone(&cell));
        assert_eq!(host.now().clock, ClockQuality::Synced);
        cell.set(ClockQuality::Holdover);
        assert_eq!(host.now().clock, ClockQuality::Holdover);
        cell.set(ClockQuality::Unsynced);
        assert_eq!(host.now().clock, ClockQuality::Unsynced);
    }

    #[test]
    fn a_chip_that_does_not_answer_is_a_device_fault_and_a_busy_address_is_denied() {
        let nxio = io::Error::from_raw_os_error(rustix::io::Errno::NXIO.raw_os_error());
        assert_eq!(i2c_error(&nxio), IoError::Other);
        let busy = io::Error::from_raw_os_error(rustix::io::Errno::BUSY.raw_os_error());
        assert_eq!(i2c_error(&busy), IoError::Denied);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(i2c_error(&denied), IoError::Denied);
    }

    #[test]
    fn a_transfer_on_a_node_that_is_not_an_i2c_bus_fails() {
        let mut bus = LinuxI2c::open(Path::new("/dev/null")).unwrap();
        let mut read = [0u8; 2];
        assert!(bus.transfer(0x48, &[0], &mut read).is_err());
        assert!(LinuxI2c::open(Path::new("/nonexistent/i2c-9")).is_err());
    }

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

    #[tokio::test]
    async fn tcp_connects_and_carries_bytes_both_ways() {
        use crate::link::TcpEndpoint;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accept = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2];
            tokio::io::AsyncReadExt::read_exact(&mut socket, &mut buf)
                .await
                .unwrap();
            tokio::io::AsyncWriteExt::write_all(&mut socket, &[9, 9])
                .await
                .unwrap();
            buf
        });
        let mut endpoint = TcpEndpoint::new(TokioTcpConnector::new(addr));
        let mut stream = endpoint.connect().await.unwrap();
        use embedded_io_async::{Read, Write};
        stream.write_all(&[7, 8]).await.unwrap();
        let echoed = accept.await.unwrap();
        assert_eq!(echoed, [7, 8]);
        let mut buf = [0u8; 2];
        stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, [9, 9]);
    }

    fn input(line: u32) -> GpioLineConfig {
        GpioLineConfig {
            line,
            output: None,
            bias: GpioBias::AsIs,
            drive: GpioDrive::PushPull,
        }
    }

    #[tokio::test]
    async fn tcp_resolves_a_host_name() {
        use crate::link::TcpEndpoint;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accept = tokio::spawn(async move { listener.accept().await.is_ok() });
        let mut endpoint =
            TcpEndpoint::new(TokioTcpConnector::to(&std::format!("localhost:{port}")));
        assert!(endpoint.connect().await.is_ok());
        assert!(accept.await.unwrap());
    }

    #[test]
    fn opening_a_non_gpiochip_path_is_refused() {
        assert!(LinuxGpio::open(Path::new("/dev/null"), &[input(0)]).is_err());
    }

    #[test]
    fn a_line_config_becomes_the_same_chardev_request() {
        let config = GpioLineConfig {
            line: 17,
            output: Some(true),
            bias: GpioBias::PullDown,
            drive: GpioDrive::OpenDrain,
        };
        let request = line_request(&config);
        assert_eq!(request.line, 17);
        assert_eq!(request.output, Some(true));
        assert_eq!(request.default_value(), 1);
        assert_eq!(request.bias, joker_gpio_sys::LineBias::PullDown);
        assert_eq!(request.drive, joker_gpio_sys::LineDrive::OpenDrain);
        let request = line_request(&GpioLineConfig {
            bias: GpioBias::Disabled,
            drive: GpioDrive::OpenSource,
            output: Some(false),
            ..input(3)
        });
        assert_eq!(request.default_value(), 0);
        assert_eq!(request.bias, joker_gpio_sys::LineBias::Disabled);
        assert_eq!(request.drive, joker_gpio_sys::LineDrive::OpenSource);
        assert_eq!(
            line_request(&GpioLineConfig {
                bias: GpioBias::PullUp,
                ..input(3)
            })
            .bias,
            joker_gpio_sys::LineBias::PullUp
        );
    }

    #[test]
    fn a_line_outside_the_granted_set_is_unsupported() {
        let mut empty = LinuxGpio {
            handles: std::collections::BTreeMap::new(),
        };
        assert_eq!(empty.get(0), Err(IoError::Unsupported));
        assert_eq!(empty.set(0, true), Err(IoError::Unsupported));
    }
}
