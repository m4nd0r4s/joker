#![no_std]
#![doc = include_str!("../README.md")]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod hash;
pub mod quality;
pub mod registry;
pub mod slot;
mod sync;
pub mod value;

pub use quality::{Quality, QualityCode, QualityReason};
pub use registry::{GroupId, Registry, RegistryBuilder, SignalId, SignalMeta};
pub use slot::{Collision, SeqExtender, Slot};
pub use value::{QualityError, Sample, ValueType};
