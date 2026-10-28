#![no_std]
#![doc = include_str!("../README.md")]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod alarm;
pub mod arbiter;
pub mod class;
pub mod commit;
pub mod envelope;
pub mod hash;
pub mod image;
pub mod outcome;
pub mod output;
pub mod quality;
pub mod registry;
pub mod slot;
mod sync;
pub mod time;
pub mod trace;
pub mod value;
pub mod wide_slot;

pub use arbiter::{Intake, OutputArbiter, Refusal};
pub use commit::{
    CommitSink, DecisionRecord, IntakeSlot, OutputResult, Outputs, OutputsStorage, WriteResult,
};
pub use envelope::Applied;
pub use image::{ImageStorage, ProcessImage, Proposal, TickInfo, WriterId};
pub use outcome::{Outcome, RejectReason};
pub use output::{Level, OutputConfig, SafeValue, WriterConfig, WriterKind};
pub use quality::{Quality, QualityCode, QualityReason};
pub use registry::{GroupId, Registry, RegistryBuilder, SignalId, SignalMeta};
pub use slot::{Collision, SeqExtender, Slot};
pub use time::{Clock, ClockQuality, Instant, VirtualClock};
pub use value::{QualityError, Sample, ValueType};
pub use wide_slot::{WIDE_SLOT_CAPACITY, WideSample, WideSlot};
