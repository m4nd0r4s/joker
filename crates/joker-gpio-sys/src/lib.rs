#![doc = include_str!("../README.md")]

#[cfg(unix)]
use std::os::fd::{BorrowedFd, OwnedFd};

/// The kernel's fixed consumer-label buffer size (`<linux/gpio.h>`).
pub const GPIO_MAX_NAME_SIZE: usize = 32;

const GPIOHANDLE_REQUEST_INPUT: u32 = 1 << 0;
const GPIOHANDLE_REQUEST_OUTPUT: u32 = 1 << 1;
const GPIOHANDLE_REQUEST_OPEN_DRAIN: u32 = 1 << 3;
const GPIOHANDLE_REQUEST_OPEN_SOURCE: u32 = 1 << 4;
const GPIOHANDLE_REQUEST_BIAS_PULL_UP: u32 = 1 << 5;
const GPIOHANDLE_REQUEST_BIAS_PULL_DOWN: u32 = 1 << 6;
const GPIOHANDLE_REQUEST_BIAS_DISABLE: u32 = 1 << 7;

/// The pull a line is requested with (ref/03 §2.2.5 `bias`). Anything but
/// [`LineBias::AsIs`] needs the v1 bias flags, which Linux added in 5.5;
/// an older kernel refuses the request (`EINVAL`) instead of ignoring it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineBias {
    /// Leaves the line's current pull.
    AsIs,
    /// `GPIOHANDLE_REQUEST_BIAS_PULL_UP`.
    PullUp,
    /// `GPIOHANDLE_REQUEST_BIAS_PULL_DOWN`.
    PullDown,
    /// `GPIOHANDLE_REQUEST_BIAS_DISABLE`.
    Disabled,
}

/// The output stage a line is requested with (ref/03 §2.2.5 `drive`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineDrive {
    /// Push-pull, the kernel default.
    PushPull,
    /// `GPIOHANDLE_REQUEST_OPEN_DRAIN`.
    OpenDrain,
    /// `GPIOHANDLE_REQUEST_OPEN_SOURCE`.
    OpenSource,
}

/// One line request. `output` is `None` for an input and, for an output,
/// the physical level the kernel drives the line to the moment the request
/// succeeds, before any write: the caller passes the declared safe state,
/// already resolved against `active_low`, so the line never shows a level
/// the manifest did not declare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRequest {
    /// The line offset on the chip.
    pub line: u32,
    /// `None` for an input; `Some(level)` for an output opened at `level`.
    pub output: Option<bool>,
    /// The requested pull.
    pub bias: LineBias,
    /// The requested output stage; only [`LineDrive::PushPull`] on an input.
    pub drive: LineDrive,
}

impl LineRequest {
    /// The `gpiohandle_request.flags` of this request. An input with a
    /// drive other than push-pull is refused (`InvalidInput`): the kernel
    /// would reject it too, and ref/03 §2.2.5 makes `drive` outputs only.
    pub fn flags(&self) -> std::io::Result<u32> {
        let direction = match self.output {
            Some(_) => GPIOHANDLE_REQUEST_OUTPUT,
            None => GPIOHANDLE_REQUEST_INPUT,
        };
        let drive = match (self.output, self.drive) {
            (_, LineDrive::PushPull) => 0,
            (Some(_), LineDrive::OpenDrain) => GPIOHANDLE_REQUEST_OPEN_DRAIN,
            (Some(_), LineDrive::OpenSource) => GPIOHANDLE_REQUEST_OPEN_SOURCE,
            (None, _) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "`drive` applies to output lines only",
                ));
            }
        };
        let bias = match self.bias {
            LineBias::AsIs => 0,
            LineBias::PullUp => GPIOHANDLE_REQUEST_BIAS_PULL_UP,
            LineBias::PullDown => GPIOHANDLE_REQUEST_BIAS_PULL_DOWN,
            LineBias::Disabled => GPIOHANDLE_REQUEST_BIAS_DISABLE,
        };
        Ok(direction | drive | bias)
    }

    /// The `gpiohandle_request.default_values[0]` of this request: the
    /// physical level of an output at request time, `0` for an input.
    pub fn default_value(&self) -> u8 {
        u8::from(self.output.unwrap_or(false))
    }
}

/// `label`, null-padded or truncated to the kernel's fixed
/// `consumer_label` buffer (`GPIO_MAX_NAME_SIZE` bytes, always
/// null-terminated: a 32-byte label is truncated to 31 bytes of content
/// so the last byte stays `0`).
pub fn consumer_label(label: &str) -> [u8; GPIO_MAX_NAME_SIZE] {
    let mut buf = [0u8; GPIO_MAX_NAME_SIZE];
    let bytes = label.as_bytes();
    let n = bytes.len().min(GPIO_MAX_NAME_SIZE - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    buf
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{GPIO_MAX_NAME_SIZE, LineRequest, consumer_label};
    use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

    /// Maximum number of lines one request can cover (`GPIOHANDLES_MAX`,
    /// `<linux/gpio.h>`); this crate always requests exactly one.
    const GPIOHANDLES_MAX: usize = 64;

    /// `struct gpiohandle_request` (`<linux/gpio.h>`): field order and
    /// types match the kernel's C declaration exactly, so `#[repr(C)]`
    /// gives this the same layout the kernel's own compiler produced.
    #[repr(C)]
    struct GpioHandleRequest {
        line_offsets: [u32; GPIOHANDLES_MAX],
        flags: u32,
        default_values: [u8; GPIOHANDLES_MAX],
        consumer_label: [u8; GPIO_MAX_NAME_SIZE],
        lines: u32,
        fd: i32,
    }

    /// `struct gpiohandle_data` (`<linux/gpio.h>`).
    #[repr(C)]
    struct GpioHandleData {
        values: [u8; GPIOHANDLES_MAX],
    }

    /// `_IOWR(ty, nr, size)` (`<asm-generic/ioctl.h>`): read-write
    /// direction, the driver type byte, the request number, and the
    /// payload size encoded together, exactly as the kernel's own macro
    /// does. Returns [`libc::Ioctl`] (not a fixed integer type): that alias
    /// is `c_ulong` on glibc but `c_int` on musl (the bench board's own
    /// libc), so hardcoding either one breaks the other target.
    const fn iowr(ty: u32, nr: u32, size: usize) -> libc::Ioctl {
        const IOC_READ: u32 = 2;
        const IOC_WRITE: u32 = 1;
        (((IOC_READ | IOC_WRITE) << 30) | (ty << 8) | nr | ((size as u32) << 16)) as libc::Ioctl
    }

    const GPIO_GET_LINEHANDLE_IOCTL: libc::Ioctl =
        iowr(0xB4, 0x03, core::mem::size_of::<GpioHandleRequest>());
    const GPIOHANDLE_GET_LINE_VALUES_IOCTL: libc::Ioctl =
        iowr(0xB4, 0x08, core::mem::size_of::<GpioHandleData>());
    const GPIOHANDLE_SET_LINE_VALUES_IOCTL: libc::Ioctl =
        iowr(0xB4, 0x09, core::mem::size_of::<GpioHandleData>());

    /// See [`super::request_line_handle`].
    ///
    /// Sound: `chip` is a borrowed, open descriptor for the whole call;
    /// `req` is a local, fully-initialized value the kernel only reads
    /// from and writes `lines`/`fd` back into, and the returned `fd` is
    /// only trusted as an owned descriptor after the kernel reports
    /// success.
    #[allow(unsafe_code)]
    pub(super) fn request_line_handle(
        chip: BorrowedFd<'_>,
        request: &LineRequest,
        consumer: &str,
    ) -> std::io::Result<OwnedFd> {
        let mut req = GpioHandleRequest {
            line_offsets: [0; GPIOHANDLES_MAX],
            flags: request.flags()?,
            default_values: [0; GPIOHANDLES_MAX],
            consumer_label: consumer_label(consumer),
            lines: 1,
            fd: -1,
        };
        req.line_offsets[0] = request.line;
        req.default_values[0] = request.default_value();
        let result = unsafe { libc::ioctl(chip.as_raw_fd(), GPIO_GET_LINEHANDLE_IOCTL, &mut req) };
        if result < 0 || req.fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { OwnedFd::from_raw_fd(req.fd) })
    }

    /// See [`super::get_line_value`].
    ///
    /// Sound: `handle` is a borrowed, open descriptor for the whole call,
    /// and `data` is a local value the kernel only writes into.
    #[allow(unsafe_code)]
    pub(super) fn get_line_value(handle: BorrowedFd<'_>) -> std::io::Result<bool> {
        let mut data = GpioHandleData {
            values: [0; GPIOHANDLES_MAX],
        };
        let result = unsafe {
            libc::ioctl(
                handle.as_raw_fd(),
                GPIOHANDLE_GET_LINE_VALUES_IOCTL,
                &mut data,
            )
        };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(data.values[0] != 0)
    }

    /// See [`super::set_line_value`].
    ///
    /// Sound: `handle` is a borrowed, open descriptor for the whole call,
    /// and `data` is a local value that lives across the call, which the
    /// kernel only reads.
    #[allow(unsafe_code)]
    pub(super) fn set_line_value(handle: BorrowedFd<'_>, value: bool) -> std::io::Result<()> {
        let mut data = GpioHandleData {
            values: [0; GPIOHANDLES_MAX],
        };
        data.values[0] = u8::from(value);
        let result =
            unsafe { libc::ioctl(handle.as_raw_fd(), GPIOHANDLE_SET_LINE_VALUES_IOCTL, &data) };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod other {
    use super::LineRequest;
    use std::os::fd::{BorrowedFd, OwnedFd};

    fn unsupported() -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "GPIO character devices exist only on Linux",
        )
    }

    pub(super) fn request_line_handle(
        _chip: BorrowedFd<'_>,
        request: &LineRequest,
        _consumer: &str,
    ) -> std::io::Result<OwnedFd> {
        request.flags()?;
        Err(unsupported())
    }

    pub(super) fn get_line_value(_handle: BorrowedFd<'_>) -> std::io::Result<bool> {
        Err(unsupported())
    }

    pub(super) fn set_line_value(_handle: BorrowedFd<'_>, _value: bool) -> std::io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(all(unix, not(target_os = "linux")))]
use other as imp;

/// Requests a handle for one line of an open `gpiochip` fd, with the
/// direction, initial level, bias and drive of `request`; the returned
/// descriptor is what [`get_line_value`]/[`set_line_value`] act on. On a
/// system other than Linux it fails with `Unsupported`.
#[cfg(unix)]
pub fn request_line_handle(
    chip: BorrowedFd<'_>,
    request: &LineRequest,
    consumer: &str,
) -> std::io::Result<OwnedFd> {
    imp::request_line_handle(chip, request, consumer)
}

/// Reads the current physical level of a handle from
/// [`request_line_handle`]. On a system other than Linux it fails with
/// `Unsupported`.
#[cfg(unix)]
pub fn get_line_value(handle: BorrowedFd<'_>) -> std::io::Result<bool> {
    imp::get_line_value(handle)
}

/// Drives a handle from [`request_line_handle`] to a physical level
/// (outputs only; the kernel rejects this on an input handle). On a system
/// other than Linux it fails with `Unsupported`.
#[cfg(unix)]
pub fn set_line_value(handle: BorrowedFd<'_>, value: bool) -> std::io::Result<()> {
    imp::set_line_value(handle, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(line: u32) -> LineRequest {
        LineRequest {
            line,
            output: None,
            bias: LineBias::AsIs,
            drive: LineDrive::PushPull,
        }
    }

    #[test]
    fn an_output_is_requested_at_its_given_level() {
        let low = LineRequest {
            output: Some(false),
            ..input(4)
        };
        let high = LineRequest {
            output: Some(true),
            ..input(4)
        };
        assert_eq!(low.flags().unwrap(), GPIOHANDLE_REQUEST_OUTPUT);
        assert_eq!(low.default_value(), 0);
        assert_eq!(high.flags().unwrap(), GPIOHANDLE_REQUEST_OUTPUT);
        assert_eq!(high.default_value(), 1);
        assert_eq!(input(4).flags().unwrap(), GPIOHANDLE_REQUEST_INPUT);
        assert_eq!(input(4).default_value(), 0);
    }

    #[test]
    fn bias_and_drive_become_request_flags() {
        let pulled = LineRequest {
            bias: LineBias::PullUp,
            ..input(1)
        };
        assert_eq!(
            pulled.flags().unwrap(),
            GPIOHANDLE_REQUEST_INPUT | GPIOHANDLE_REQUEST_BIAS_PULL_UP
        );
        for (bias, flag) in [
            (LineBias::PullDown, GPIOHANDLE_REQUEST_BIAS_PULL_DOWN),
            (LineBias::Disabled, GPIOHANDLE_REQUEST_BIAS_DISABLE),
        ] {
            assert_eq!(
                LineRequest { bias, ..input(1) }.flags().unwrap(),
                GPIOHANDLE_REQUEST_INPUT | flag
            );
        }
        for (drive, flag) in [
            (LineDrive::OpenDrain, GPIOHANDLE_REQUEST_OPEN_DRAIN),
            (LineDrive::OpenSource, GPIOHANDLE_REQUEST_OPEN_SOURCE),
        ] {
            let out = LineRequest {
                output: Some(true),
                drive,
                ..input(1)
            };
            assert_eq!(out.flags().unwrap(), GPIOHANDLE_REQUEST_OUTPUT | flag);
        }
    }

    #[test]
    fn a_drive_on_an_input_is_refused() {
        let bad = LineRequest {
            drive: LineDrive::OpenDrain,
            ..input(1)
        };
        let err = bad.flags().unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn a_consumer_label_longer_than_the_kernel_buffer_is_truncated_not_overflowed() {
        let long = "x".repeat(100);
        let label = consumer_label(&long);
        assert_eq!(label.len(), GPIO_MAX_NAME_SIZE);
        assert_eq!(
            label[GPIO_MAX_NAME_SIZE - 1],
            0,
            "null-terminated, not full of x"
        );
    }

    #[cfg(unix)]
    #[test]
    fn requesting_a_line_on_a_non_gpiochip_fd_is_refused() {
        use std::os::fd::AsFd;
        let not_a_chip = std::fs::File::open("/dev/null").unwrap();
        assert!(request_line_handle(not_a_chip.as_fd(), &input(0), "joker-test").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn getting_or_setting_a_value_on_a_non_handle_fd_is_refused() {
        use std::os::fd::AsFd;
        let not_a_handle = std::fs::File::open("/dev/null").unwrap();
        assert!(get_line_value(not_a_handle.as_fd()).is_err());
        assert!(set_line_value(not_a_handle.as_fd(), true).is_err());
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    #[test]
    fn off_linux_every_call_fails_as_unsupported() {
        use std::os::fd::AsFd;
        let file = std::fs::File::open("/dev/null").unwrap();
        let err = request_line_handle(file.as_fd(), &input(0), "joker-test").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        let err = get_line_value(file.as_fd()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }
}
