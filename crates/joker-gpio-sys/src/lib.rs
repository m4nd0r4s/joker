#![doc = include_str!("../README.md")]

#[cfg(unix)]
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

/// Maximum number of lines one request can cover (`GPIOHANDLES_MAX`,
/// `<linux/gpio.h>`); this crate always requests exactly one.
#[cfg(unix)]
const GPIOHANDLES_MAX: usize = 64;

/// The kernel's fixed consumer-label buffer size (`<linux/gpio.h>`).
#[cfg(unix)]
pub const GPIO_MAX_NAME_SIZE: usize = 32;

#[cfg(unix)]
const GPIOHANDLE_REQUEST_INPUT: u32 = 1 << 0;
#[cfg(unix)]
const GPIOHANDLE_REQUEST_OUTPUT: u32 = 1 << 1;

/// `struct gpiohandle_request` (`<linux/gpio.h>`): field order and types
/// match the kernel's C declaration exactly, so `#[repr(C)]` gives this
/// the same layout the kernel's own compiler produced.
#[cfg(unix)]
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
#[cfg(unix)]
#[repr(C)]
struct GpioHandleData {
    values: [u8; GPIOHANDLES_MAX],
}

/// `_IOWR(ty, nr, size)` (`<asm-generic/ioctl.h>`): read-write direction,
/// the driver type byte, the request number, and the payload size encoded
/// together, exactly as the kernel's own macro does. Returns
/// [`libc::Ioctl`] (not a fixed integer type): that alias is `c_ulong` on
/// glibc but `c_int` on musl (the bench board's own libc), so hardcoding
/// either one breaks the other target.
#[cfg(unix)]
const fn iowr(ty: u32, nr: u32, size: usize) -> libc::Ioctl {
    const IOC_READ: u32 = 2;
    const IOC_WRITE: u32 = 1;
    (((IOC_READ | IOC_WRITE) << 30) | (ty << 8) | nr | ((size as u32) << 16)) as libc::Ioctl
}

#[cfg(unix)]
const GPIO_GET_LINEHANDLE_IOCTL: libc::Ioctl =
    iowr(0xB4, 0x03, core::mem::size_of::<GpioHandleRequest>());
#[cfg(unix)]
const GPIOHANDLE_GET_LINE_VALUES_IOCTL: libc::Ioctl =
    iowr(0xB4, 0x08, core::mem::size_of::<GpioHandleData>());
#[cfg(unix)]
const GPIOHANDLE_SET_LINE_VALUES_IOCTL: libc::Ioctl =
    iowr(0xB4, 0x09, core::mem::size_of::<GpioHandleData>());

/// `label`, null-padded or truncated to the kernel's fixed
/// `consumer_label` buffer (`GPIO_MAX_NAME_SIZE` bytes, always
/// null-terminated: a 32-byte label is truncated to 31 bytes of content
/// so the last byte stays `0`).
#[cfg(unix)]
fn consumer_label(label: &str) -> [u8; GPIO_MAX_NAME_SIZE] {
    let mut buf = [0u8; GPIO_MAX_NAME_SIZE];
    let bytes = label.as_bytes();
    let n = bytes.len().min(GPIO_MAX_NAME_SIZE - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    buf
}

/// Requests a handle for one line of an open `gpiochip` fd: `output`
/// selects `GPIOHANDLE_REQUEST_OUTPUT` over `_INPUT`; the returned
/// descriptor is what [`get_line_value`]/[`set_line_value`] act on.
///
/// Sound: `chip` is a borrowed, open descriptor for the whole call; `req`
/// is a local, fully-initialized value the kernel only reads from and
/// writes `lines`/`fd` back into, and the returned `fd` is only trusted
/// as an owned descriptor after the kernel reports success.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn request_line_handle(
    chip: BorrowedFd<'_>,
    line: u32,
    output: bool,
    consumer: &str,
) -> std::io::Result<OwnedFd> {
    let mut req = GpioHandleRequest {
        line_offsets: [0; GPIOHANDLES_MAX],
        flags: if output {
            GPIOHANDLE_REQUEST_OUTPUT
        } else {
            GPIOHANDLE_REQUEST_INPUT
        },
        default_values: [0; GPIOHANDLES_MAX],
        consumer_label: consumer_label(consumer),
        lines: 1,
        fd: -1,
    };
    req.line_offsets[0] = line;
    let result = unsafe { libc::ioctl(chip.as_raw_fd(), GPIO_GET_LINEHANDLE_IOCTL, &mut req) };
    if result < 0 || req.fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(req.fd) })
}

/// Reads the current level of a handle from [`request_line_handle`].
///
/// Sound: `handle` is a borrowed, open descriptor for the whole call, and
/// `data` is a local value the kernel only writes into.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn get_line_value(handle: BorrowedFd<'_>) -> std::io::Result<bool> {
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

/// Drives a handle from [`request_line_handle`] (outputs only; the
/// kernel rejects this on an input handle).
///
/// Sound: `handle` is a borrowed, open descriptor for the whole call, and
/// `data` is a local value that lives across the call, which the kernel
/// only reads.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn set_line_value(handle: BorrowedFd<'_>, value: bool) -> std::io::Result<()> {
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    #[test]
    fn requesting_a_line_on_a_non_gpiochip_fd_is_refused() {
        let not_a_chip = std::fs::File::open("/dev/null").unwrap();
        assert!(request_line_handle(not_a_chip.as_fd(), 0, false, "joker-test").is_err());
    }

    #[test]
    fn getting_or_setting_a_value_on_a_non_handle_fd_is_refused() {
        let not_a_handle = std::fs::File::open("/dev/null").unwrap();
        assert!(get_line_value(not_a_handle.as_fd()).is_err());
        assert!(set_line_value(not_a_handle.as_fd(), true).is_err());
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
}
