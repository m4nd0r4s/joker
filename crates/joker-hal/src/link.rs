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
