#![no_std]
#![doc = include_str!("../README.md")]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod hash;
pub mod image;
pub mod quality;
pub mod registry;
pub mod slot;
mod sync;
pub mod time;
pub mod value;

pub use image::{ImageStorage, ProcessImage, Proposal, TickInfo, WriterId};
pub use quality::{Quality, QualityCode, QualityReason};
pub use registry::{GroupId, Registry, RegistryBuilder, SignalId, SignalMeta};
pub use slot::{Collision, SeqExtender, Slot};
pub use time::{Clock, ClockQuality, Instant, VirtualClock};
pub use value::{QualityError, Sample, ValueType};
