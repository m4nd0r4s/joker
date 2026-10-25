#![doc = include_str!("../README.md")]

#[cfg(unix)]
use std::os::fd::BorrowedFd;

/// The longest variable name piControl stores, without its NUL.
pub const MAX_NAME: usize = 31;

/// The most modules [`device_list`] returns.
pub const MAX_DEVICES: usize = 255;

/// Where a PiCtory variable is in the process image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variable {
    /// The byte, from the start of the process image.
    pub address: u16,
    /// The bit within the byte, 0–7, for a variable of one bit.
    pub bit: u8,
    /// The length in bits: 1, 8, 16 or 32.
    pub length: u16,
}

/// One module piControl knows (`SDeviceInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Device {
    /// The module's address in the configuration.
    pub address: u8,
    /// Its serial number.
    pub serial: u32,
    /// Its type (`i16uModuleType`).
    pub module_type: u16,
    /// The first byte of its inputs, and how many.
    pub input: (u16, u16),
    /// The first byte of its outputs, and how many.
    pub output: (u16, u16),
    /// The first byte of its configuration, and how many.
    pub config: (u16, u16),
    /// `i8uModuleState`.
    pub state: u8,
    /// `i8uActive`: the module answers on the bus.
    pub active: bool,
}

impl Device {
    /// True when `address` is in one of the module's ranges.
    pub fn holds(&self, address: u16) -> bool {
        [self.input, self.output, self.config]
            .iter()
            .any(|(start, len)| {
                *len > 0
                    && address >= *start
                    && u32::from(address) < u32::from(*start) + u32::from(*len)
            })
    }
}

/// The name field of `SPIVariable`: at most [`MAX_NAME`] bytes, NUL
/// padded. An empty name, a longer one or one with a NUL is refused
/// (`InvalidInput`).
pub fn name_field(name: &str) -> std::io::Result<[u8; 32]> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_NAME || bytes.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a piControl variable name is 1 to 31 bytes without NUL",
        ));
    }
    let mut out = [0u8; 32];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Device, MAX_DEVICES, Variable, name_field};
    use std::os::fd::{AsRawFd, BorrowedFd};

    const fn io(nr: u32) -> libc::Ioctl {
        (((b'K' as u32) << 8) | nr) as libc::Ioctl
    }

    /// `KB_GET_DEVICE_INFO_LIST`, `_IO('K', 13)`.
    const KB_GET_DEVICE_INFO_LIST: libc::Ioctl = io(13);
    /// `KB_SET_VALUE`, `_IO('K', 16)`.
    const KB_SET_VALUE: libc::Ioctl = io(16);
    /// `KB_FIND_VARIABLE`, `_IO('K', 17)`.
    const KB_FIND_VARIABLE: libc::Ioctl = io(17);

    /// `SPIVariable`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SpiVariable {
        name: [u8; 32],
        address: u16,
        bit: u8,
        length: u16,
    }

    /// `SPIValue`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SpiValue {
        address: u16,
        bit: u8,
        value: u8,
    }

    /// `SDeviceInfo`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SDeviceInfo {
        address: u8,
        serial: u32,
        module_type: u16,
        hw_revision: u16,
        sw_major: u16,
        sw_minor: u16,
        svn_revision: u32,
        input_length: u16,
        input_offset: u16,
        output_length: u16,
        output_offset: u16,
        config_length: u16,
        config_offset: u16,
        state: u8,
        active: u8,
        reserve: [u8; 30],
    }

    const _: () = assert!(core::mem::size_of::<SpiVariable>() == 38);
    const _: () = assert!(core::mem::size_of::<SpiValue>() == 4);
    const _: () = assert!(core::mem::size_of::<SDeviceInfo>() == 64);

    const EMPTY_INFO: SDeviceInfo = SDeviceInfo {
        address: 0,
        serial: 0,
        module_type: 0,
        hw_revision: 0,
        sw_major: 0,
        sw_minor: 0,
        svn_revision: 0,
        input_length: 0,
        input_offset: 0,
        output_length: 0,
        output_offset: 0,
        config_length: 0,
        config_offset: 0,
        state: 0,
        active: 0,
        reserve: [0; 30],
    };

    /// See [`super::find_variable`].
    ///
    /// Sound: `fd` is a borrowed, open descriptor for the whole call, and
    /// the kernel reads and writes exactly one `SPIVariable`, which `var`
    /// is, with the layout of the C declaration.
    #[allow(unsafe_code)]
    pub(super) fn find_variable(fd: BorrowedFd<'_>, name: &str) -> std::io::Result<Variable> {
        let mut var = SpiVariable {
            name: name_field(name)?,
            address: 0,
            bit: 0,
            length: 0,
        };
        let r = unsafe { libc::ioctl(fd.as_raw_fd(), KB_FIND_VARIABLE, &mut var) };
        if r < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Variable {
            address: var.address,
            bit: var.bit,
            length: var.length,
        })
    }

    /// See [`super::device_list`].
    ///
    /// Sound: `fd` is a borrowed, open descriptor for the whole call; the
    /// kernel writes at most `REV_PI_DEV_CNT_MAX` (64) records into the
    /// array, which has room for [`MAX_DEVICES`], and returns how many it
    /// wrote.
    #[allow(unsafe_code)]
    pub(super) fn device_list(fd: BorrowedFd<'_>) -> std::io::Result<Vec<Device>> {
        let mut infos = vec![EMPTY_INFO; MAX_DEVICES];
        let r = unsafe { libc::ioctl(fd.as_raw_fd(), KB_GET_DEVICE_INFO_LIST, infos.as_mut_ptr()) };
        if r < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let n = usize::try_from(r).unwrap_or(0).min(MAX_DEVICES);
        Ok(infos[..n]
            .iter()
            .map(|i| Device {
                address: i.address,
                serial: i.serial,
                module_type: i.module_type,
                input: (i.input_offset, i.input_length),
                output: (i.output_offset, i.output_length),
                config: (i.config_offset, i.config_length),
                state: i.state,
                active: i.active != 0,
            })
            .collect())
    }

    /// See [`super::set_bit`].
    ///
    /// Sound: `fd` is a borrowed, open descriptor for the whole call, and
    /// the kernel reads exactly one `SPIValue`, which `value` is.
    #[allow(unsafe_code)]
    pub(super) fn set_bit(
        fd: BorrowedFd<'_>,
        address: u16,
        bit: u8,
        on: bool,
    ) -> std::io::Result<()> {
        let mut value = SpiValue {
            address,
            bit,
            value: u8::from(on),
        };
        let r = unsafe { libc::ioctl(fd.as_raw_fd(), KB_SET_VALUE, &mut value) };
        if r < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod other {
    use super::{Device, Variable, name_field};
    use std::os::fd::BorrowedFd;

    fn unsupported() -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "piControl exists only on Linux",
        )
    }

    pub(super) fn find_variable(_fd: BorrowedFd<'_>, name: &str) -> std::io::Result<Variable> {
        name_field(name)?;
        Err(unsupported())
    }

    pub(super) fn device_list(_fd: BorrowedFd<'_>) -> std::io::Result<Vec<Device>> {
        Err(unsupported())
    }

    pub(super) fn set_bit(
        _fd: BorrowedFd<'_>,
        _address: u16,
        _bit: u8,
        _on: bool,
    ) -> std::io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(all(unix, not(target_os = "linux")))]
use other as imp;

/// `KB_FIND_VARIABLE`: where the PiCtory variable `name` is in the process
/// image of the open `/dev/piControl0` `fd`. An unknown name fails with
/// the kernel's error.
#[cfg(unix)]
pub fn find_variable(fd: BorrowedFd<'_>, name: &str) -> std::io::Result<Variable> {
    imp::find_variable(fd, name)
}

/// `KB_GET_DEVICE_INFO_LIST`: the modules piControl knows.
#[cfg(unix)]
pub fn device_list(fd: BorrowedFd<'_>) -> std::io::Result<Vec<Device>> {
    imp::device_list(fd)
}

/// `KB_SET_VALUE`: sets (`on`) or clears bit `bit` (0–7) of byte
/// `address` of the process image, in the kernel.
#[cfg(unix)]
pub fn set_bit(fd: BorrowedFd<'_>, address: u16, bit: u8, on: bool) -> std::io::Result<()> {
    if bit > 7 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a bit of a byte is 0-7",
        ));
    }
    imp::set_bit(fd, address, bit, on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_one_to_31_bytes_padded_with_nul() {
        let f = name_field("O_1").unwrap();
        assert_eq!(&f[..4], b"O_1\0");
        assert!(f[3..].iter().all(|b| *b == 0));
        assert!(name_field(&"x".repeat(31)).is_ok());
        assert_eq!(
            name_field(&"x".repeat(32)).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(name_field("").is_err());
        assert!(name_field("a\0b").is_err());
    }

    #[test]
    fn a_module_holds_the_bytes_of_its_ranges() {
        let dio = Device {
            input: (11, 70),
            output: (81, 18),
            config: (99, 0),
            active: true,
            ..Device::default()
        };
        assert!(dio.holds(11) && dio.holds(80) && dio.holds(81) && dio.holds(98));
        assert!(!dio.holds(10) && !dio.holds(99));
    }

    #[cfg(unix)]
    #[test]
    fn calls_on_a_descriptor_that_is_not_picontrol_fail() {
        use std::os::fd::AsFd;
        let not_pi = std::fs::File::open("/dev/null").unwrap();
        assert!(find_variable(not_pi.as_fd(), "O_1").is_err());
        assert!(device_list(not_pi.as_fd()).is_err());
        assert!(set_bit(not_pi.as_fd(), 0, 0, true).is_err());
        assert_eq!(
            set_bit(not_pi.as_fd(), 0, 8, true).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }
}
