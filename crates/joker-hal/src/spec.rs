//! What the driver receives: the plan request compiled by the core from the
//! manifest, the lock and the profile (ref/06 §2.4–§2.5). The driver never
//! reads TOML.
//!
//! The views borrow: on Linux from the generation, on a Joker Micro from
//! flash. With `alloc`, [`owned`] holds the same records for hosts and tests.

use crate::cbor::Cbor;
use crate::types::{DeviceIx, Direction, PointId, Poll, RawKind, ReadClass};

/// Kind of a bus, with the codes of `bus-kind` in the CDDL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BusKind {
    /// No transport.
    None = 0,
    /// Ethernet.
    Ethernet = 1,
    /// Serial line.
    Serial = 2,
    /// CAN.
    Can = 3,
    /// EtherCAT.
    Ethercat = 4,
    /// I²C.
    I2c = 5,
    /// SPI.
    Spi = 6,
}

/// One point of a driver instance (ref/06 §2.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointSpec<'a> {
    /// Dense index in the instance, in canonical path order.
    pub id: PointId,
    /// Device of the instance.
    pub device: DeviceIx,
    /// Path relative to the device, for diagnostics only.
    pub path: &'a str,
    /// Direction.
    pub direction: Direction,
    /// Raw form the core expects (ref/06 §2.7).
    pub expect: RawKind,
    /// Registers, maximum bytes, or 0.
    pub width: u16,
    /// The profile's `type`, as is.
    pub type_name: &'a str,
    /// Address, in the driver's `address` schema.
    pub addr: Cbor<'a>,
    /// How the point is read.
    pub poll: Poll,
    /// Who needs it.
    pub class: ReadClass,
    /// Coherence group.
    pub coherent: Option<u32>,
    /// Debounce in ns, with the `debounce` capability.
    pub debounce_ns: u64,
    /// Output with timed writes.
    pub timed: bool,
}

/// A native action of a device (ref/06 §2.10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionSpec<'a> {
    /// Index.
    pub id: u32,
    /// Device.
    pub device: DeviceIx,
    /// Action name.
    pub name: &'a str,
    /// The profile's `native` table.
    pub native: Cbor<'a>,
    /// Deadline of an invocation.
    pub timeout_ns: u64,
    /// Long action, with progress.
    pub long: bool,
}

/// One device of the instance (ref/06 §2.4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceSpec<'a> {
    /// Index in the instance.
    pub index: DeviceIx,
    /// Device name in the manifest.
    pub name: &'a str,
    /// Driver keys of `[devices.<d>]`, in the descriptor's `config` schema.
    pub config: Cbor<'a>,
    /// Values of the device's `config = { … }` without `apply`.
    pub profile_config: Cbor<'a>,
    /// The profile's `quirks`.
    pub quirks: Cbor<'a>,
    /// Discovered map pinned in the lock.
    pub discovered: Option<Cbor<'a>>,
    /// Deadline of a call on this device, from `timeout`.
    pub timeout_ns: u64,
}

/// The bus of the instance (ref/06 §2.4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusSpec<'a> {
    /// Bus name.
    pub name: &'a str,
    /// Kind.
    pub kind: BusKind,
    /// All the keys of `[buses.<b>]`.
    pub params: Cbor<'a>,
}

/// The input of `plan` and of `open` (ref/06 §2.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanRequest<'a> {
    /// The bus, if any.
    pub bus: Option<BusSpec<'a>>,
    /// The devices.
    pub devices: &'a [DeviceSpec<'a>],
    /// The points, by `id`.
    pub points: &'a [PointSpec<'a>],
    /// The native actions.
    pub actions: &'a [ActionSpec<'a>],
}

impl<'a> PlanRequest<'a> {
    /// The point with `id`.
    pub fn point(&self, id: PointId) -> Option<&'a PointSpec<'a>> {
        self.points
            .get(id as usize)
            .filter(|p| p.id == id)
            .or_else(|| self.points.iter().find(|p| p.id == id))
    }

    /// The device with `index`.
    pub fn device(&self, index: DeviceIx) -> Option<&'a DeviceSpec<'a>> {
        self.devices.iter().find(|d| d.index == index)
    }
}

#[cfg(feature = "alloc")]
pub mod owned {
    //! Owned plan requests, for hosts and tests.

    use super::*;
    use alloc::string::String;
    use alloc::vec::Vec;

    /// An owned [`PointSpec`].
    #[derive(Clone, Debug, PartialEq)]
    pub struct OwnedPoint {
        /// See [`PointSpec::id`].
        pub id: PointId,
        /// See [`PointSpec::device`].
        pub device: DeviceIx,
        /// See [`PointSpec::path`].
        pub path: String,
        /// See [`PointSpec::direction`].
        pub direction: Direction,
        /// See [`PointSpec::expect`].
        pub expect: RawKind,
        /// See [`PointSpec::width`].
        pub width: u16,
        /// See [`PointSpec::type_name`].
        pub type_name: String,
        /// See [`PointSpec::addr`].
        pub addr: Vec<u8>,
        /// See [`PointSpec::poll`].
        pub poll: Poll,
        /// See [`PointSpec::class`].
        pub class: ReadClass,
        /// See [`PointSpec::coherent`].
        pub coherent: Option<u32>,
        /// See [`PointSpec::debounce_ns`].
        pub debounce_ns: u64,
        /// See [`PointSpec::timed`].
        pub timed: bool,
    }

    impl OwnedPoint {
        /// An input point with the given address, read every `period_ns`.
        pub fn input(
            id: PointId,
            path: &str,
            expect: RawKind,
            width: u16,
            type_name: &str,
            addr: Vec<u8>,
            period_ns: u64,
        ) -> Self {
            Self {
                id,
                device: 0,
                path: String::from(path),
                direction: Direction::Input,
                expect,
                width,
                type_name: String::from(type_name),
                addr,
                poll: Poll::Period(period_ns),
                class: ReadClass::Cycle,
                coherent: None,
                debounce_ns: 0,
                timed: false,
            }
        }

        /// An output point with the given address.
        pub fn output(
            id: PointId,
            path: &str,
            expect: RawKind,
            width: u16,
            type_name: &str,
            addr: Vec<u8>,
        ) -> Self {
            Self {
                direction: Direction::Output,
                poll: Poll::OnDemand,
                ..Self::input(id, path, expect, width, type_name, addr, 0)
            }
        }

        /// The borrowed view.
        pub fn view(&self) -> PointSpec<'_> {
            PointSpec {
                id: self.id,
                device: self.device,
                path: &self.path,
                direction: self.direction,
                expect: self.expect,
                width: self.width,
                type_name: &self.type_name,
                addr: Cbor::new(&self.addr),
                poll: self.poll,
                class: self.class,
                coherent: self.coherent,
                debounce_ns: self.debounce_ns,
                timed: self.timed,
            }
        }
    }

    /// An owned [`DeviceSpec`].
    #[derive(Clone, Debug, PartialEq)]
    pub struct OwnedDevice {
        /// See [`DeviceSpec::index`].
        pub index: DeviceIx,
        /// See [`DeviceSpec::name`].
        pub name: String,
        /// See [`DeviceSpec::config`].
        pub config: Vec<u8>,
        /// See [`DeviceSpec::profile_config`].
        pub profile_config: Vec<u8>,
        /// See [`DeviceSpec::quirks`].
        pub quirks: Vec<u8>,
        /// See [`DeviceSpec::discovered`].
        pub discovered: Option<Vec<u8>>,
        /// See [`DeviceSpec::timeout_ns`].
        pub timeout_ns: u64,
    }

    impl OwnedDevice {
        /// Device 0 named `name`, with `config`, empty quirks and `timeout_ns`.
        pub fn new(name: &str, config: Vec<u8>, timeout_ns: u64) -> Self {
            Self {
                index: 0,
                name: String::from(name),
                config,
                profile_config: Vec::from(Cbor::EMPTY_MAP.as_bytes()),
                quirks: Vec::from(Cbor::EMPTY_MAP.as_bytes()),
                discovered: None,
                timeout_ns,
            }
        }

        /// The borrowed view.
        pub fn view(&self) -> DeviceSpec<'_> {
            DeviceSpec {
                index: self.index,
                name: &self.name,
                config: Cbor::new(&self.config),
                profile_config: Cbor::new(&self.profile_config),
                quirks: Cbor::new(&self.quirks),
                discovered: self.discovered.as_deref().map(Cbor::new),
                timeout_ns: self.timeout_ns,
            }
        }
    }

    /// An owned [`BusSpec`].
    #[derive(Clone, Debug, PartialEq)]
    pub struct OwnedBus {
        /// See [`BusSpec::name`].
        pub name: String,
        /// See [`BusSpec::kind`].
        pub kind: BusKind,
        /// See [`BusSpec::params`].
        pub params: Vec<u8>,
    }

    impl OwnedBus {
        /// The borrowed view.
        pub fn view(&self) -> BusSpec<'_> {
            BusSpec {
                name: &self.name,
                kind: self.kind,
                params: Cbor::new(&self.params),
            }
        }
    }

    /// An owned plan request without actions.
    #[derive(Clone, Debug, PartialEq, Default)]
    pub struct OwnedRequest {
        /// The bus.
        pub bus: Option<OwnedBus>,
        /// The devices.
        pub devices: Vec<OwnedDevice>,
        /// The points.
        pub points: Vec<OwnedPoint>,
    }

    /// The borrowed records of an [`OwnedRequest`], from which a
    /// [`PlanRequest`] is taken with [`RequestViews::request`].
    #[derive(Clone, Debug)]
    pub struct RequestViews<'a> {
        bus: Option<BusSpec<'a>>,
        devices: Vec<DeviceSpec<'a>>,
        points: Vec<PointSpec<'a>>,
    }

    impl OwnedRequest {
        /// The borrowed records.
        pub fn views(&self) -> RequestViews<'_> {
            RequestViews {
                bus: self.bus.as_ref().map(OwnedBus::view),
                devices: self.devices.iter().map(OwnedDevice::view).collect(),
                points: self.points.iter().map(OwnedPoint::view).collect(),
            }
        }
    }

    impl<'a> RequestViews<'a> {
        /// The plan request.
        pub fn request(&self) -> PlanRequest<'_> {
            PlanRequest {
                bus: self.bus,
                devices: &self.devices,
                points: &self.points,
                actions: &[],
            }
        }
    }
}
