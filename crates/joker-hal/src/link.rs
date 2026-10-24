//! The transport the host gives a driver instance (ref/06 §3.4). A tier N
//! driver never opens a socket, a device file or a serial port itself.
//!
//! M0 has three forms: no transport, a serial line (for Modbus RTU) and the
//! device paths granted by the manifest (for `sysfs`). The serial line is a
//! poll-based object, so that the host's port (tokio on Linux, a double in
//! tests, the Embassy UART later) hides behind one type, and it implements
//! the `embedded-io-async` traits on top.

#[cfg(feature = "alloc")]
use core::future::poll_fn;
use core::task::{Context, Poll};

use crate::cx::MaybeSend;

/// Errors of a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoError {
    /// The device went away (cable unplugged, port closed).
    Disconnected,
    /// The operation is not supported by this transport.
    Unsupported,
    /// The operating system refused the operation (permissions, busy).
    Denied,
    /// Any other failure.
    Other,
}

impl embedded_io::Error for IoError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            Self::Disconnected => embedded_io::ErrorKind::NotConnected,
            Self::Unsupported => embedded_io::ErrorKind::Unsupported,
            Self::Denied => embedded_io::ErrorKind::PermissionDenied,
            Self::Other => embedded_io::ErrorKind::Other,
        }
    }
}

impl core::fmt::Display for IoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Disconnected => "disconnected",
            Self::Unsupported => "unsupported",
            Self::Denied => "denied",
            Self::Other => "i/o error",
        })
    }
}

impl core::error::Error for IoError {}

/// Parity of a serial line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Parity {
    /// None.
    None,
    /// Even.
    Even,
    /// Odd.
    Odd,
}

/// Line settings of a serial port, from `[buses.<b>]` (ref/01 §5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SerialInfo {
    /// Baud rate.
    pub baud: u32,
    /// Data bits, 7 or 8.
    pub data_bits: u8,
    /// Parity.
    pub parity: Parity,
    /// Stop bits, 1 or 2.
    pub stop_bits: u8,
}

impl SerialInfo {
    /// `baud` 8N1.
    pub const fn new_8n1(baud: u32) -> Self {
        Self {
            baud,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
        }
    }

    /// Bits on the wire per character: start, data, parity and stop.
    pub const fn bits_per_char(&self) -> u32 {
        let parity = match self.parity {
            Parity::None => 0,
            _ => 1,
        };
        1 + self.data_bits as u32 + parity + self.stop_bits as u32
    }

    /// Wire time of `bytes` characters in ns.
    pub const fn wire_ns(&self, bytes: u32) -> u64 {
        if self.baud == 0 {
            return 0;
        }
        (bytes as u64) * (self.bits_per_char() as u64) * 1_000_000_000 / (self.baud as u64)
    }
}

/// The host side of a serial line. Implementations never block: they
/// return `Pending` and wake the task.
pub trait SerialIo: MaybeSend {
    /// Reads available bytes; `Ok(0)` only for an empty `buf`.
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>>;
    /// Writes some bytes.
    fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IoError>>;
    /// Waits until every written byte left the port.
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError>>;
    /// Drops every byte received and not read yet, such as a late answer to
    /// a cancelled request.
    fn discard_input(&mut self) -> Result<(), IoError>;
    /// The line settings.
    fn info(&self) -> SerialInfo;
}

/// A serial line given by the host.
#[cfg(feature = "alloc")]
pub struct SerialPort(alloc::boxed::Box<dyn SerialIo>);

#[cfg(feature = "alloc")]
impl core::fmt::Debug for SerialPort {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("SerialPort").field(&self.0.info()).finish()
    }
}

#[cfg(feature = "alloc")]
impl SerialPort {
    /// Wraps the host's port.
    pub fn new(io: impl SerialIo + 'static) -> Self {
        Self(alloc::boxed::Box::new(io))
    }

    /// The line settings.
    pub fn info(&self) -> SerialInfo {
        self.0.info()
    }

    /// Drops the bytes received and not read yet.
    pub fn discard_input(&mut self) -> Result<(), IoError> {
        self.0.discard_input()
    }

    /// Writes all of `buf` and flushes.
    pub async fn send(&mut self, buf: &[u8]) -> Result<(), IoError> {
        let mut done = 0;
        while done < buf.len() {
            let io = &mut self.0;
            let n = poll_fn(|cx| io.poll_write(cx, &buf[done..])).await?;
            if n == 0 {
                return Err(IoError::Disconnected);
            }
            done += n;
        }
        let io = &mut self.0;
        poll_fn(|cx| io.poll_flush(cx)).await
    }

    /// Reads exactly `buf.len()` bytes.
    pub async fn receive_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        let mut done = 0;
        while done < buf.len() {
            let io = &mut self.0;
            let n = poll_fn(|cx| io.poll_read(cx, &mut buf[done..])).await?;
            if n == 0 {
                return Err(IoError::Disconnected);
            }
            done += n;
        }
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl embedded_io::ErrorType for SerialPort {
    type Error = IoError;
}

#[cfg(feature = "alloc")]
impl embedded_io_async::Read for SerialPort {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let io = &mut self.0;
        poll_fn(|cx| io.poll_read(cx, buf)).await
    }
}

#[cfg(feature = "alloc")]
impl embedded_io_async::Write for SerialPort {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        let io = &mut self.0;
        poll_fn(|cx| io.poll_write(cx, buf)).await
    }

    async fn flush(&mut self) -> Result<(), IoError> {
        let io = &mut self.0;
        poll_fn(|cx| io.poll_flush(cx)).await
    }
}

/// The host side of a TCP connection, once connected (ref/06 §3.4,
/// `Link::Tcp`). Poll-based and object-safe for the same reason as
/// [`SerialIo`]: `embedded-io-async`'s traits use `async fn` and cannot be
/// boxed directly.
#[cfg(feature = "alloc")]
pub trait TcpIo: MaybeSend {
    /// Reads available bytes; `Ok(0)` only for an empty `buf`.
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>>;
    /// Writes some bytes.
    fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IoError>>;
    /// Waits until every written byte left the socket.
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError>>;
}

/// A connected TCP stream given by the host.
#[cfg(feature = "alloc")]
pub struct TcpStream(alloc::boxed::Box<dyn TcpIo>);

#[cfg(feature = "alloc")]
impl core::fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("TcpStream").finish_non_exhaustive()
    }
}

#[cfg(feature = "alloc")]
impl TcpStream {
    /// Wraps the host's connected socket.
    pub fn new(io: impl TcpIo + 'static) -> Self {
        Self(alloc::boxed::Box::new(io))
    }
}

#[cfg(feature = "alloc")]
impl embedded_io::ErrorType for TcpStream {
    type Error = IoError;
}

#[cfg(feature = "alloc")]
impl embedded_io_async::Read for TcpStream {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let io = &mut self.0;
        poll_fn(|cx| io.poll_read(cx, buf)).await
    }
}

#[cfg(feature = "alloc")]
impl embedded_io_async::Write for TcpStream {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        let io = &mut self.0;
        poll_fn(|cx| io.poll_write(cx, buf)).await
    }

    async fn flush(&mut self) -> Result<(), IoError> {
        let io = &mut self.0;
        poll_fn(|cx| io.poll_flush(cx)).await
    }
}

/// Connects (or reconnects) a TCP endpoint (ref/06 §3.4, `Link::Tcp`): the
/// driver never opens the socket itself, it asks the host for one. The
/// host decides the address (from the manifest's `address`/`[buses]`) and
/// any OS-level connect timeout; the driver bounds the wait with its own
/// `Cx::within`, the same way `modbus-rtu` bounds an exchange.
#[cfg(feature = "alloc")]
pub trait TcpConnect: MaybeSend {
    /// Connects (or reconnects after a disconnect).
    fn poll_connect(&mut self, cx: &mut Context<'_>) -> Poll<Result<TcpStream, IoError>>;
}

/// A TCP endpoint given by the host.
#[cfg(feature = "alloc")]
pub struct TcpEndpoint(alloc::boxed::Box<dyn TcpConnect>);

#[cfg(feature = "alloc")]
impl core::fmt::Debug for TcpEndpoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("TcpEndpoint").finish_non_exhaustive()
    }
}

#[cfg(feature = "alloc")]
impl TcpEndpoint {
    /// Wraps the host's connector.
    pub fn new(connector: impl TcpConnect + 'static) -> Self {
        Self(alloc::boxed::Box::new(connector))
    }

    /// Connects, or reconnects after a disconnect.
    pub async fn connect(&mut self) -> Result<TcpStream, IoError> {
        let connector = &mut self.0;
        poll_fn(|cx| connector.poll_connect(cx)).await
    }
}

/// The pull a host requests a digital line with (ref/03 §2.2.5 `bias`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioBias {
    /// `as-is` (default): leaves the line's current pull.
    AsIs,
    /// `pull-up`.
    PullUp,
    /// `pull-down`.
    PullDown,
    /// `disabled`.
    Disabled,
}

/// The output stage a host requests a digital line with (ref/03 §2.2.5
/// `drive`, outputs only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioDrive {
    /// `push-pull` (default).
    PushPull,
    /// `open-drain`.
    OpenDrain,
    /// `open-source`.
    OpenSource,
}

/// How a host requests one granted line before the driver opens: an
/// output carries the physical level it starts at, which is its declared
/// `safe` resolved against `active_low` (AGENTS.md invariant 5), so the
/// line never shows an undeclared level between the request and the
/// first committed write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpioLineConfig {
    /// The line offset on the chip.
    pub line: u32,
    /// `None` for an input; `Some(level)` for an output, at that physical
    /// level.
    pub output: Option<bool>,
    /// The requested pull.
    pub bias: GpioBias,
    /// The requested output stage.
    pub drive: GpioDrive,
}

/// The host side of a set of digital lines (ref/06 §3.4, `Link::Gpio`,
/// ADR 0047): a tier N driver never opens `/dev/gpiochipN` itself, it asks
/// the host for the lines the manifest granted. Synchronous: unlike a
/// socket or a serial port, a local chardev ioctl does not block.
#[cfg(feature = "alloc")]
pub trait GpioLines: MaybeSend {
    /// The level of `line`.
    fn get(&mut self, line: u32) -> Result<bool, IoError>;
    /// Drives `line` to `value` (outputs only).
    fn set(&mut self, line: u32, value: bool) -> Result<(), IoError>;
}

/// A set of digital lines given by the host.
#[cfg(feature = "alloc")]
pub struct GpioChip(alloc::boxed::Box<dyn GpioLines>);

#[cfg(feature = "alloc")]
impl core::fmt::Debug for GpioChip {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("GpioChip").finish_non_exhaustive()
    }
}

#[cfg(feature = "alloc")]
impl GpioChip {
    /// Wraps the host's lines.
    pub fn new(lines: impl GpioLines + 'static) -> Self {
        Self(alloc::boxed::Box::new(lines))
    }

    /// The level of `line`.
    pub fn get(&mut self, line: u32) -> Result<bool, IoError> {
        self.0.get(line)
    }

    /// Drives `line` to `value`.
    pub fn set(&mut self, line: u32, value: bool) -> Result<(), IoError> {
        self.0.set(line, value)
    }
}

/// The host side of an I²C bus (ref/06 §3.4, `Link::I2c`, ADR 0047): a
/// tier N driver never opens `/dev/i2c-N` itself. One combined transfer
/// (write, then read with a repeated start) covers the common
/// register-style chip protocol; synchronous, like [`GpioLines`].
#[cfg(feature = "alloc")]
pub trait I2cTransfers: MaybeSend {
    /// Writes `write` to the 7-bit address `addr`, then reads `read.len()`
    /// bytes back with a repeated start. `write` alone (empty `read`) is a
    /// plain write; `read` alone (empty `write`) a plain read.
    fn transfer(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), IoError>;
}

/// An I²C bus given by the host.
#[cfg(feature = "alloc")]
pub struct I2cBus(alloc::boxed::Box<dyn I2cTransfers>);

#[cfg(feature = "alloc")]
impl core::fmt::Debug for I2cBus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("I2cBus").finish_non_exhaustive()
    }
}

#[cfg(feature = "alloc")]
impl I2cBus {
    /// Wraps the host's bus.
    pub fn new(bus: impl I2cTransfers + 'static) -> Self {
        Self(alloc::boxed::Box::new(bus))
    }

    /// Writes `write` to `addr`, then reads `read.len()` bytes back.
    pub fn transfer(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), IoError> {
        self.0.transfer(addr, write, read)
    }
}

#[cfg(feature = "std")]
pub use paths::DevicePaths;

#[cfg(feature = "std")]
mod paths {
    use std::fs;
    use std::io;
    use std::path::{Component, Path, PathBuf};
    use std::string::String;
    use std::vec::Vec;

    /// Files granted to a tier N driver by the manifest (ref/06 §3.4,
    /// `Link::Paths`): a root and the relative prefixes under it that the
    /// driver may touch. The root is `/sys` for `sysfs` on a node, and a
    /// temporary directory in tests.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DevicePaths {
        root: PathBuf,
        granted: Vec<PathBuf>,
    }

    impl DevicePaths {
        /// Grants the relative `prefixes` under `root`.
        pub fn new(root: impl Into<PathBuf>, prefixes: &[&str]) -> Self {
            Self {
                root: root.into(),
                granted: prefixes.iter().map(PathBuf::from).collect(),
            }
        }

        /// The root.
        pub fn root(&self) -> &Path {
            &self.root
        }

        fn resolve(&self, relative: &str) -> io::Result<PathBuf> {
            let rel = Path::new(relative);
            let plain = rel.components().all(|c| matches!(c, Component::Normal(_)));
            if !plain || !self.granted.iter().any(|g| rel.starts_with(g)) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "path not granted to the driver",
                ));
            }
            Ok(self.root.join(rel))
        }

        /// Reads a whole file.
        pub fn read(&self, relative: &str) -> io::Result<String> {
            fs::read_to_string(self.resolve(relative)?)
        }

        /// Writes `contents` to an existing file in one write, as a sysfs
        /// attribute expects; never creates a file. The truncation is ignored
        /// by sysfs and makes a plain file behave like an attribute.
        pub fn write(&self, relative: &str, contents: &str) -> io::Result<()> {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(self.resolve(relative)?)?;
            file.write_all(contents.as_bytes())
        }

        /// True when the granted path exists.
        pub fn exists(&self, relative: &str) -> bool {
            self.resolve(relative).is_ok_and(|p| p.exists())
        }
    }
}

/// The transport of a driver instance.
#[derive(Debug)]
#[non_exhaustive]
pub enum Link {
    /// No transport.
    None,
    /// A serial line.
    #[cfg(feature = "alloc")]
    Serial(SerialPort),
    /// A TCP endpoint.
    #[cfg(feature = "alloc")]
    Tcp(TcpEndpoint),
    /// A set of digital lines (ADR 0047).
    #[cfg(feature = "alloc")]
    Gpio(GpioChip),
    /// An I²C bus (ADR 0047).
    #[cfg(feature = "alloc")]
    I2c(I2cBus),
    /// Device paths granted by the manifest (tier N only).
    #[cfg(feature = "std")]
    Paths(DevicePaths),
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn wire_time_of_8n1() {
        let info = SerialInfo::new_8n1(115_200);
        assert_eq!(info.bits_per_char(), 10);
        assert_eq!(info.wire_ns(8), 694_444);
        let even = SerialInfo {
            parity: Parity::Even,
            stop_bits: 2,
            ..info
        };
        assert_eq!(even.bits_per_char(), 12);
        assert_eq!(SerialInfo::new_8n1(0).wire_ns(8), 0);
    }

    #[test]
    fn device_paths_only_reach_granted_prefixes() {
        let dir = std::env::temp_dir().join(std::format!("joker-hal-paths-{}", std::process::id()));
        let leds = dir.join("class/leds/ACT");
        std::fs::create_dir_all(&leds).unwrap();
        std::fs::write(leds.join("brightness"), "0\n").unwrap();
        let paths = DevicePaths::new(&dir, &["class/leds/ACT"]);
        assert_eq!(paths.read("class/leds/ACT/brightness").unwrap(), "0\n");
        paths.write("class/leds/ACT/brightness", "1").unwrap();
        assert_eq!(paths.read("class/leds/ACT/brightness").unwrap(), "1");
        assert!(paths.read("class/leds/PWR/brightness").is_err());
        assert!(paths.read("class/leds/ACT/../PWR/brightness").is_err());
        assert!(paths.write("class/leds/ACT/new", "1").is_err());
        assert!(paths.exists("class/leds/ACT/brightness"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(all(test, feature = "testing"))]
#[allow(
    clippy::unwrap_used,
    reason = "test doubles fail the test on a broken fixture"
)]
mod tcp_tests {
    use alloc::collections::VecDeque;
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    use embedded_io_async::{Read, Write};

    use super::*;
    use crate::testing::VirtualHost;

    /// An always-ready in-memory socket: reads come from a canned buffer,
    /// writes are captured.
    #[derive(Clone, Default)]
    struct Fake {
        inbox: Arc<std::sync::Mutex<VecDeque<u8>>>,
        outbox: Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl TcpIo for Fake {
        fn poll_read(
            &mut self,
            _cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<Result<usize, IoError>> {
            let mut inbox = self.inbox.lock().unwrap();
            if inbox.is_empty() {
                return Poll::Ready(Err(IoError::Disconnected));
            }
            let mut n = 0;
            while n < buf.len() {
                let Some(b) = inbox.pop_front() else { break };
                buf[n] = b;
                n += 1;
            }
            Poll::Ready(Ok(n))
        }

        fn poll_write(
            &mut self,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, IoError>> {
            self.outbox.lock().unwrap().extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
            Poll::Ready(Ok(()))
        }
    }

    struct FakeConnector {
        stream: Option<Fake>,
        fail_first: bool,
    }

    impl TcpConnect for FakeConnector {
        fn poll_connect(&mut self, _cx: &mut Context<'_>) -> Poll<Result<TcpStream, IoError>> {
            if self.fail_first {
                self.fail_first = false;
                return Poll::Ready(Err(IoError::Disconnected));
            }
            Poll::Ready(Ok(TcpStream::new(self.stream.clone().unwrap_or_default())))
        }
    }

    #[test]
    fn connect_then_write_and_read() {
        let host = Arc::new(VirtualHost::new(0));
        let inbox = Arc::new(std::sync::Mutex::new(VecDeque::from(alloc::vec![
            b'o', b'k'
        ])));
        let fake = Fake {
            inbox,
            outbox: Arc::default(),
        };
        let mut endpoint = TcpEndpoint::new(FakeConnector {
            stream: Some(fake.clone()),
            fail_first: false,
        });
        let mut stream = host.run(endpoint.connect()).unwrap().unwrap();
        host.run(stream.write_all(b"hi")).unwrap().unwrap();
        assert_eq!(*fake.outbox.lock().unwrap(), b"hi");
        let mut buf = [0u8; 2];
        host.run(stream.read_exact(&mut buf)).unwrap().unwrap();
        assert_eq!(&buf, b"ok");
    }

    #[test]
    fn a_failed_connect_is_reported() {
        let host = Arc::new(VirtualHost::new(0));
        let mut endpoint = TcpEndpoint::new(FakeConnector {
            stream: None,
            fail_first: true,
        });
        let result = host.run(endpoint.connect()).unwrap();
        assert_eq!(result.err(), Some(IoError::Disconnected));
    }
}

#[cfg(all(test, feature = "testing"))]
#[allow(
    clippy::unwrap_used,
    reason = "test doubles fail the test on a broken fixture"
)]
mod gpio_tests {
    use alloc::collections::BTreeMap;
    use alloc::sync::Arc;

    use super::*;

    /// Lines held in memory: `get` returns whatever was last `set`,
    /// defaulting to `false`.
    #[derive(Clone, Default)]
    struct Fake {
        levels: Arc<std::sync::Mutex<BTreeMap<u32, bool>>>,
        deny: Option<u32>,
    }

    impl GpioLines for Fake {
        fn get(&mut self, line: u32) -> Result<bool, IoError> {
            if self.deny == Some(line) {
                return Err(IoError::Denied);
            }
            Ok(*self.levels.lock().unwrap().get(&line).unwrap_or(&false))
        }

        fn set(&mut self, line: u32, value: bool) -> Result<(), IoError> {
            if self.deny == Some(line) {
                return Err(IoError::Denied);
            }
            self.levels.lock().unwrap().insert(line, value);
            Ok(())
        }
    }

    #[test]
    fn set_then_get_reads_back_the_same_line() {
        let mut chip = GpioChip::new(Fake::default());
        assert!(!chip.get(17).unwrap());
        chip.set(17, true).unwrap();
        assert!(chip.get(17).unwrap());
        assert!(!chip.get(22).unwrap(), "a different line is unaffected");
    }

    #[test]
    fn a_denied_line_is_reported() {
        let mut chip = GpioChip::new(Fake {
            deny: Some(5),
            ..Fake::default()
        });
        assert_eq!(chip.get(5).err(), Some(IoError::Denied));
        assert_eq!(chip.set(5, true).err(), Some(IoError::Denied));
    }
}

#[cfg(all(test, feature = "testing"))]
#[allow(
    clippy::unwrap_used,
    reason = "test doubles fail the test on a broken fixture"
)]
mod i2c_tests {
    use alloc::collections::BTreeMap;
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    use super::*;

    /// One register file per 7-bit address; `transfer` with a one-byte
    /// `write` and a non-empty `read` is "set the register pointer, read
    /// back `read.len()` bytes from there".
    #[derive(Clone, Default)]
    struct Fake {
        chips: Arc<std::sync::Mutex<BTreeMap<u8, Vec<u8>>>>,
        deny: Option<u8>,
    }

    impl I2cTransfers for Fake {
        fn transfer(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), IoError> {
            if self.deny == Some(addr) {
                return Err(IoError::Denied);
            }
            let mut chips = self.chips.lock().unwrap();
            let regs = chips.entry(addr).or_default();
            if read.is_empty() {
                let at = write.first().copied().unwrap_or(0) as usize;
                for (i, &b) in write.iter().skip(1).enumerate() {
                    if regs.len() <= at + i {
                        regs.resize(at + i + 1, 0);
                    }
                    regs[at + i] = b;
                }
                return Ok(());
            }
            let at = write.first().copied().unwrap_or(0) as usize;
            for (i, out) in read.iter_mut().enumerate() {
                *out = regs.get(at + i).copied().unwrap_or(0);
            }
            Ok(())
        }
    }

    #[test]
    fn a_write_then_read_sees_the_written_bytes() {
        let mut bus = I2cBus::new(Fake::default());
        bus.transfer(0x40, &[0x00, 0xAB, 0xCD], &mut []).unwrap();
        let mut out = [0u8; 2];
        bus.transfer(0x40, &[0x00], &mut out).unwrap();
        assert_eq!(out, [0xAB, 0xCD]);
    }

    #[test]
    fn different_addresses_do_not_share_registers() {
        let mut bus = I2cBus::new(Fake::default());
        bus.transfer(0x40, &[0x00, 1], &mut []).unwrap();
        bus.transfer(0x41, &[0x00, 2], &mut []).unwrap();
        let mut out = [0u8; 1];
        bus.transfer(0x40, &[0x00], &mut out).unwrap();
        assert_eq!(out, [1]);
        bus.transfer(0x41, &[0x00], &mut out).unwrap();
        assert_eq!(out, [2]);
    }

    #[test]
    fn a_denied_address_is_reported() {
        let mut bus = I2cBus::new(Fake {
            deny: Some(0x50),
            ..Fake::default()
        });
        assert_eq!(
            bus.transfer(0x50, &[0x00], &mut [0u8; 1]).err(),
            Some(IoError::Denied)
        );
    }
}
