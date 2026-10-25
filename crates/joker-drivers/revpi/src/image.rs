//! The seam between the driver and piControl: [`PiImage`], with the real
//! `/dev/piControl0` ([`PiControl`]) and an in-memory process image
//! ([`FakeImage`]).

use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::string::String;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::vec;
use std::vec::Vec;

use joker_hal::link::DevicePaths;
pub use joker_picontrol_sys::{Device, Variable};

/// The size of the piControl process image.
pub const IMAGE_SIZE: usize = 4096;

/// The device node of piControl, relative to `/dev`.
pub const NODE: &str = "piControl0";

/// What the driver asks of piControl. Every call is short: the kernel
/// copies memory it already holds.
pub trait PiImage: Send + core::fmt::Debug {
    /// Opens (or reopens, after a reset) the process image.
    fn open(&mut self) -> io::Result<()>;
    /// Where the variable `name` is.
    fn find(&mut self, name: &str) -> io::Result<Variable>;
    /// The modules piControl knows.
    fn modules(&mut self) -> io::Result<Vec<Device>>;
    /// Reads `buf.len()` bytes from `offset`.
    fn read(&mut self, offset: u16, buf: &mut [u8]) -> io::Result<()>;
    /// Writes `buf` at `offset`.
    fn write(&mut self, offset: u16, buf: &[u8]) -> io::Result<()>;
    /// Sets or clears one bit of one byte.
    fn set_bit(&mut self, address: u16, bit: u8, on: bool) -> io::Result<()>;
}

/// The real process image: `/dev/piControl0` under the granted paths.
#[derive(Debug)]
pub struct PiControl {
    paths: DevicePaths,
    file: Option<File>,
}

impl PiControl {
    /// piControl at [`NODE`] under `paths`.
    pub fn new(paths: DevicePaths) -> Self {
        Self { paths, file: None }
    }

    fn file(&self) -> io::Result<&File> {
        self.file
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "piControl is not open"))
    }
}

#[cfg(unix)]
impl PiImage for PiControl {
    fn open(&mut self) -> io::Result<()> {
        self.file = None;
        self.file = Some(self.paths.open_rw(NODE)?);
        Ok(())
    }

    fn find(&mut self, name: &str) -> io::Result<Variable> {
        use std::os::fd::AsFd;
        joker_picontrol_sys::find_variable(self.file()?.as_fd(), name)
    }

    fn modules(&mut self) -> io::Result<Vec<Device>> {
        use std::os::fd::AsFd;
        joker_picontrol_sys::device_list(self.file()?.as_fd())
    }

    fn read(&mut self, offset: u16, buf: &mut [u8]) -> io::Result<()> {
        use std::os::unix::fs::FileExt;
        self.file()?.read_exact_at(buf, u64::from(offset))
    }

    fn write(&mut self, offset: u16, buf: &[u8]) -> io::Result<()> {
        use std::os::unix::fs::FileExt;
        self.file()?.write_all_at(buf, u64::from(offset))
    }

    fn set_bit(&mut self, address: u16, bit: u8, on: bool) -> io::Result<()> {
        use std::os::fd::AsFd;
        joker_picontrol_sys::set_bit(self.file()?.as_fd(), address, bit, on)
    }
}

#[cfg(not(unix))]
fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "piControl exists only on Linux")
}

#[cfg(not(unix))]
impl PiImage for PiControl {
    fn open(&mut self) -> io::Result<()> {
        Err(unsupported())
    }

    fn find(&mut self, _name: &str) -> io::Result<Variable> {
        Err(unsupported())
    }

    fn modules(&mut self) -> io::Result<Vec<Device>> {
        Err(unsupported())
    }

    fn read(&mut self, _offset: u16, _buf: &mut [u8]) -> io::Result<()> {
        Err(unsupported())
    }

    fn write(&mut self, _offset: u16, _buf: &[u8]) -> io::Result<()> {
        Err(unsupported())
    }

    fn set_bit(&mut self, _address: u16, _bit: u8, _on: bool) -> io::Result<()> {
        Err(unsupported())
    }
}

#[derive(Debug)]
struct FakeState {
    bytes: Vec<u8>,
    variables: BTreeMap<String, Variable>,
    modules: Vec<Device>,
    open: bool,
    down: bool,
    writes: Vec<(u16, Vec<u8>)>,
}

/// An in-memory process image with PiCtory variables and modules. Clones
/// share the same image, so a test keeps one to drive and inspect what the
/// driver sees.
#[derive(Clone, Debug)]
pub struct FakeImage(Arc<Mutex<FakeState>>);

impl Default for FakeImage {
    fn default() -> Self {
        Self::new()
    }
}

fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "piControl is not available")
}

impl FakeImage {
    /// An empty image: no variables, no modules, all bytes 0.
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(FakeState {
            bytes: vec![0; IMAGE_SIZE],
            variables: BTreeMap::new(),
            modules: Vec::new(),
            open: false,
            down: false,
            writes: Vec::new(),
        })))
    }

    fn state(&self) -> MutexGuard<'_, FakeState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Declares the variable `name`.
    pub fn variable(&self, name: &str, address: u16, bit: u8, length: u16) -> &Self {
        self.state().variables.insert(
            String::from(name),
            Variable {
                address,
                bit,
                length,
            },
        );
        self
    }

    /// Removes the variable `name`, as a new PiCtory configuration would.
    pub fn forget(&self, name: &str) {
        self.state().variables.remove(name);
    }

    /// Declares a module.
    pub fn module(&self, device: Device) -> &Self {
        self.state().modules.push(device);
        self
    }

    /// Marks the module at `address` present or absent on the bus.
    pub fn set_active(&self, address: u8, active: bool) {
        for m in self
            .state()
            .modules
            .iter_mut()
            .filter(|m| m.address == address)
        {
            m.active = active;
        }
    }

    /// Moves the module at `address` to other process-image ranges, as a
    /// piControl reset with a new configuration does.
    pub fn relocate(&self, address: u8, input: (u16, u16), output: (u16, u16)) {
        for m in self
            .state()
            .modules
            .iter_mut()
            .filter(|m| m.address == address)
        {
            m.input = input;
            m.output = output;
        }
    }

    /// piControl stops answering (`down`) until it is back and reopened.
    pub fn set_down(&self, down: bool) {
        let mut s = self.state();
        s.down = down;
        if down {
            s.open = false;
        }
    }

    /// The byte at `address`.
    pub fn byte(&self, address: u16) -> u8 {
        self.state()
            .bytes
            .get(usize::from(address))
            .copied()
            .unwrap_or(0)
    }

    /// Sets the bytes from `address`, as a module's inputs change.
    pub fn set_bytes(&self, address: u16, bytes: &[u8]) {
        let mut s = self.state();
        let start = usize::from(address);
        if let Some(slot) = s.bytes.get_mut(start..start + bytes.len()) {
            slot.copy_from_slice(bytes);
        }
    }

    /// Every byte write and bit set the driver made, in order; a bit set is
    /// recorded as one byte `[bit | value << 7]` at its address.
    pub fn writes(&self) -> Vec<(u16, Vec<u8>)> {
        self.state().writes.clone()
    }

    fn ready(&self) -> io::Result<MutexGuard<'_, FakeState>> {
        let s = self.state();
        if s.down || !s.open {
            return Err(gone());
        }
        Ok(s)
    }
}

impl PiImage for FakeImage {
    fn open(&mut self) -> io::Result<()> {
        let mut s = self.state();
        if s.down {
            return Err(gone());
        }
        s.open = true;
        Ok(())
    }

    fn find(&mut self, name: &str) -> io::Result<Variable> {
        self.ready()?
            .variables
            .get(name)
            .copied()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such variable"))
    }

    fn modules(&mut self) -> io::Result<Vec<Device>> {
        Ok(self.ready()?.modules.clone())
    }

    fn read(&mut self, offset: u16, buf: &mut [u8]) -> io::Result<()> {
        let s = self.ready()?;
        let start = usize::from(offset);
        let src = s
            .bytes
            .get(start..start + buf.len())
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "past the image"))?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn write(&mut self, offset: u16, buf: &[u8]) -> io::Result<()> {
        let mut s = self.ready()?;
        let start = usize::from(offset);
        let dst = s
            .bytes
            .get_mut(start..start + buf.len())
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "past the image"))?;
        dst.copy_from_slice(buf);
        s.writes.push((offset, buf.to_vec()));
        Ok(())
    }

    fn set_bit(&mut self, address: u16, bit: u8, on: bool) -> io::Result<()> {
        let mut s = self.ready()?;
        let byte = s
            .bytes
            .get_mut(usize::from(address))
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "past the image"))?;
        let mask = 1u8 << (bit & 7);
        *byte = if on { *byte | mask } else { *byte & !mask };
        s.writes.push((address, vec![bit | (u8::from(on) << 7)]));
        Ok(())
    }
}
