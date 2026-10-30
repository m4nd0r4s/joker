#![no_std]
#![doc = include_str!("../README.md")]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod acquire;
pub mod cbor;
pub mod commit;
pub mod cx;
pub mod descriptor;
pub mod driver;
pub mod encode;
pub mod interpreter;
pub mod link;
pub mod mcu;
pub mod plan;
pub mod rt;
pub mod spec;
#[cfg(all(feature = "std", unix))]
pub mod std_host;
pub mod steps;
#[cfg(feature = "testing")]
pub mod testing;
pub mod types;

pub use cbor::Cbor;
pub use commit::{CommitBox, CommitRx, CommitTx, ForceSafe, OutputBinding, RawEncoding, RxState};
pub use cx::{Cx, Host, LogLevel, MaybeSend, MaybeSync};
pub use descriptor::{Capabilities, Capability, Descriptor, InstanceMode, Tier};
#[cfg(feature = "plan")]
pub use driver::Planner;
pub use driver::{
    AckSlot, ActionCall, ActionOut, AddrRead, AddrWrite, CommittedWrite, ConnectOut, ConnectSink,
    DeviceInfo, Driver, Full, HealthOut, OpenArgs, RawSink, ReadBatch, SampleSink, WriteAck,
    WriteAcks,
};
#[cfg(feature = "alloc")]
pub use link::SerialPort;
pub use link::{GpioBias, GpioDrive, GpioLineConfig, IoError, Link, SerialInfo, SerialIo};
#[cfg(feature = "alloc")]
pub use link::{
    GpioChip, GpioLines, I2cBus, I2cTransfers, TcpConnect, TcpEndpoint, TcpIo, TcpStream,
};
pub use mcu::AdcRead;
pub use plan::{BatchHead, BatchSpec, PlanView};
pub use rt::{RtCapable, RtIo, RtPart, RtStatus};
pub use spec::{ActionSpec, BusKind, BusSpec, DeviceSpec, PlanRequest, PointSpec};
pub use types::{
    ActionFailure, AtomicRef, BatchFailure, BatchId, BatchKind, Code, Detail, DeviceIx, Diagnostic,
    Direction, DriverError, LinkState, MonoNs, PointId, Poll, QualityCode, RawKind, RawSample,
    RawStatus, RawValue, ReadClass, Reason, WriteKind, WriteStatus,
};
