#![doc = include_str!("../README.md")]

#[cfg(unix)]
use std::os::fd::{AsRawFd, BorrowedFd};

/// Asserts (`true`) or deasserts (`false`) the DTR and RTS lines of the
/// serial port `fd`; `None` leaves a line as it is.
///
/// The `ioctl` calls are sound: `fd` is a borrowed open descriptor for the
/// whole call, and the argument is a pointer to a local `c_int` that lives
/// across the call and that `TIOCMBIS`/`TIOCMBIC` only read.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn set_modem_lines(
    fd: BorrowedFd<'_>,
    dtr: Option<bool>,
    rts: Option<bool>,
) -> std::io::Result<()> {
    for (bit, on) in [(libc::TIOCM_DTR, dtr), (libc::TIOCM_RTS, rts)] {
        let Some(on) = on else {
            continue;
        };
        let request = if on { libc::TIOCMBIS } else { libc::TIOCMBIC };
        let bits: libc::c_int = bit;
        let result = unsafe { libc::ioctl(fd.as_raw_fd(), request, &bits as *const libc::c_int) };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    #[test]
    fn a_file_that_is_not_a_tty_is_refused() {
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(set_modem_lines(file.as_fd(), Some(false), None).is_err());
        assert!(set_modem_lines(file.as_fd(), None, None).is_ok());
    }
}
