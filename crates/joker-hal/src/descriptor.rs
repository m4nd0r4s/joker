//! The static driver descriptor (ref/06 §2.3), compiled into the binary for
//! tier N.
//!
//! M0 carries the identity, protocols, buses, instance mode and capabilities.
//! The schemas of §2.3.1 (`config`, `address`, `quirks`, `native`,
//! `discovered`) are validated by each driver's `plan` until the schema
//! language lands (ADR 0047).

use crate::spec::BusKind;

/// Tier of a driver (`tier` in the CDDL).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Tier {
    /// Native Rust, compiled into `jokerd`.
    N = 0,
    /// Wasm component.
    W = 1,
    /// External process over JDP.
    X = 2,
}

/// Whether an instance serves one device or a whole bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum InstanceMode {
    /// One instance per device.
    Device = 0,
    /// One instance per bus.
    Bus = 1,
}

/// A capability of ref/06 §2.13.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Capability {
    /// `write`.
    Write = 0,
    /// `push`.
    Push = 1,
    /// `raw-access`.
    RawAccess = 2,
    /// `native-actions`.
    NativeActions = 3,
    /// `discover`.
    Discover = 4,
    /// `scan`.
    Scan = 5,
    /// `coherent`.
    Coherent = 6,
    /// `atomic-request`.
    AtomicRequest = 7,
    /// `timed-write`.
    TimedWrite = 8,
    /// `relinquish`.
    Relinquish = 9,
    /// `debounce`.
    Debounce = 10,
    /// `hw-timestamps`.
    HwTimestamps = 11,
    /// `rt-cycle`.
    RtCycle = 12,
}

impl Capability {
    /// The name used in the descriptor.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Push => "push",
            Self::RawAccess => "raw-access",
            Self::NativeActions => "native-actions",
            Self::Discover => "discover",
            Self::Scan => "scan",
            Self::Coherent => "coherent",
            Self::AtomicRequest => "atomic-request",
            Self::TimedWrite => "timed-write",
            Self::Relinquish => "relinquish",
            Self::Debounce => "debounce",
            Self::HwTimestamps => "hw-timestamps",
            Self::RtCycle => "rt-cycle",
        }
    }
}

/// A set of capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Capabilities(u32);

impl Capabilities {
    /// No capability.
    pub const NONE: Self = Self(0);

    /// The set of `list`.
    pub const fn of(list: &[Capability]) -> Self {
        let mut bits = 0;
        let mut i = 0;
        while i < list.len() {
            bits |= 1 << list[i] as u32;
            i += 1;
        }
        Self(bits)
    }

    /// True when `cap` is in the set.
    pub const fn has(self, cap: Capability) -> bool {
        self.0 & (1 << cap as u32) != 0
    }
}

/// The static descriptor of a driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    /// Driver name, `[a-z][a-z0-9-]{0,31}`.
    pub name: &'static str,
    /// SemVer.
    pub version: &'static str,
    /// Tier.
    pub tier: Tier,
    /// Profile families or `x:`/`w:` names served.
    pub protocols: &'static [&'static str],
    /// Accepted bus kinds; `None` for no transport.
    pub buses: &'static [BusKind],
    /// Instance mode.
    pub instance: InstanceMode,
    /// Capabilities.
    pub capabilities: Capabilities,
    /// Interface version, `joker-hal@x.y` for tier N.
    pub interface: &'static str,
    /// Compiles `no_std` for Joker Micro.
    pub micro: bool,
    /// SPDX license.
    pub license: &'static str,
    /// For humans.
    pub description: &'static str,
}

/// The interface version of this crate, for [`Descriptor::interface`].
pub const INTERFACE: &str = "joker-hal@0.1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_sets() {
        let caps = Capabilities::of(&[Capability::Write, Capability::RtCycle]);
        assert!(caps.has(Capability::Write));
        assert!(caps.has(Capability::RtCycle));
        assert!(!caps.has(Capability::Push));
        assert!(!Capabilities::NONE.has(Capability::Write));
        assert_eq!(Capability::RawAccess.name(), "raw-access");
    }
}
