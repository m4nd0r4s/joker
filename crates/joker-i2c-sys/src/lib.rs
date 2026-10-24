#![doc = include_str!("../README.md")]

#[cfg(unix)]
use std::os::fd::BorrowedFd;

/// `I2C_M_RD` (`<linux/i2c.h>`): the message reads from the chip.
pub const I2C_M_RD: u16 = 0x0001;

/// The highest 7-bit I²C address.
pub const MAX_ADDRESS: u8 = 0x7F;

/// The header of one `struct i2c_msg`, without its buffer pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    /// The 7-bit address.
    pub addr: u16,
    /// `0` for a write, [`I2C_M_RD`] for a read.
    pub flags: u16,
    /// The bytes of the buffer.
    pub len: u16,
}

/// The messages of one combined transfer to `addr`: a write of
/// `write_len` bytes, then a read of `read_len` bytes, each only when it
/// is not empty. An address above [`MAX_ADDRESS`] or a length above
/// `u16::MAX` is refused (`InvalidInput`).
pub fn messages(addr: u8, write_len: usize, read_len: usize) -> std::io::Result<Vec<Message>> {
    if addr > MAX_ADDRESS {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "an I2C address is 7 bits (0x00-0x7F)",
        ));
    }
    let length = |n: usize| {
        u16::try_from(n).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "an I2C message is at most 65535 bytes",
            )
        })
    };
    let mut out = Vec::with_capacity(2);
    if write_len > 0 {
        out.push(Message {
            addr: u16::from(addr),
            flags: 0,
            len: length(write_len)?,
        });
    }
    if read_len > 0 {
        out.push(Message {
            addr: u16::from(addr),
            flags: I2C_M_RD,
            len: length(read_len)?,
        });
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::messages;
    use std::os::fd::{AsRawFd, BorrowedFd};

    /// `I2C_RDWR` (`<linux/i2c-dev.h>`): a plain request number, not an
    /// `_IOWR` encoding. [`libc::Ioctl`] is `c_ulong` on glibc and `c_int`
    /// on musl, so the constant takes whichever the target has.
    const I2C_RDWR: libc::Ioctl = 0x0707 as libc::Ioctl;

    /// `struct i2c_msg` (`<linux/i2c.h>`): field order and types match the
    /// kernel's C declaration, so `#[repr(C)]` gives the kernel's layout.
    #[repr(C)]
    struct I2cMsg {
        addr: u16,
        flags: u16,
        len: u16,
        buf: *mut u8,
    }

    /// `struct i2c_rdwr_ioctl_data` (`<linux/i2c-dev.h>`).
    #[repr(C)]
    struct I2cRdwrData {
        msgs: *mut I2cMsg,
        nmsgs: u32,
    }

    /// See [`super::transfer`].
    ///
    /// Sound: `bus` is a borrowed, open descriptor for the whole call;
    /// every `buf` points into `write` or `read`, which outlive the call,
    /// with the `len` of that slice, and the kernel only reads the write
    /// buffer (its message has no `I2C_M_RD`) and only writes `len` bytes
    /// into the read buffer. `msgs` points at a local array that lives
    /// across the call.
    #[allow(unsafe_code)]
    pub(super) fn transfer(
        bus: BorrowedFd<'_>,
        addr: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> std::io::Result<()> {
        let headers = messages(addr, write.len(), read.len())?;
        if headers.is_empty() {
            return Ok(());
        }
        let mut write_buf = write.as_ptr().cast_mut();
        let mut read_buf = read.as_mut_ptr();
        let mut msgs: Vec<I2cMsg> = headers
            .iter()
            .map(|m| I2cMsg {
                addr: m.addr,
                flags: m.flags,
                len: m.len,
                buf: if m.flags & super::I2C_M_RD == 0 {
                    core::mem::replace(&mut write_buf, core::ptr::null_mut())
                } else {
                    core::mem::replace(&mut read_buf, core::ptr::null_mut())
                },
            })
            .collect();
        let mut data = I2cRdwrData {
            msgs: msgs.as_mut_ptr(),
            nmsgs: msgs.len() as u32,
        };
        let result = unsafe { libc::ioctl(bus.as_raw_fd(), I2C_RDWR, &mut data) };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod other {
    use super::messages;
    use std::os::fd::BorrowedFd;

    pub(super) fn transfer(
        _bus: BorrowedFd<'_>,
        addr: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> std::io::Result<()> {
        messages(addr, write.len(), read.len())?;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "i2c-dev exists only on Linux",
        ))
    }
}

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(all(unix, not(target_os = "linux")))]
use other as imp;

/// Writes `write` to the 7-bit address `addr` of the open `i2c-dev` bus
/// `bus`, then reads `read.len()` bytes back with a repeated start, in one
/// `I2C_RDWR` call; an empty side is left out, and an empty transfer does
/// nothing. A chip that does not acknowledge its address fails with the
/// kernel's error (`ENXIO` or `EREMOTEIO`). On a system other than Linux
/// it fails with `Unsupported`.
#[cfg(unix)]
pub fn transfer(
    bus: BorrowedFd<'_>,
    addr: u8,
    write: &[u8],
    read: &mut [u8],
) -> std::io::Result<()> {
    imp::transfer(bus, addr, write, read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_register_read_is_a_write_then_a_read_to_the_same_address() {
        assert_eq!(
            messages(0x48, 1, 2).unwrap(),
            vec![
                Message {
                    addr: 0x48,
                    flags: 0,
                    len: 1
                },
                Message {
                    addr: 0x48,
                    flags: I2C_M_RD,
                    len: 2
                },
            ]
        );
    }

    #[test]
    fn an_empty_side_is_left_out() {
        assert_eq!(messages(0x10, 3, 0).unwrap().len(), 1);
        assert_eq!(messages(0x10, 0, 3).unwrap()[0].flags, I2C_M_RD);
        assert!(messages(0x10, 0, 0).unwrap().is_empty());
    }

    #[test]
    fn an_eight_bit_address_or_an_oversized_buffer_is_refused() {
        let err = messages(0x80, 1, 1).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let err = messages(0x10, 70_000, 0).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn a_transfer_on_a_descriptor_that_is_not_an_i2c_bus_fails() {
        use std::os::fd::AsFd;
        let not_a_bus = std::fs::File::open("/dev/null").unwrap();
        let mut read = [0u8; 2];
        assert!(transfer(not_a_bus.as_fd(), 0x48, &[0], &mut read).is_err());
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    #[test]
    fn off_linux_a_transfer_fails_as_unsupported() {
        use std::os::fd::AsFd;
        let file = std::fs::File::open("/dev/null").unwrap();
        let err = transfer(file.as_fd(), 0x48, &[0], &mut [0u8; 1]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }
}
