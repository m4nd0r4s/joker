//! The real-time part of a driver (capability `rt-cycle`, ref/06 §3.3,
//! §3.5): one bus cycle per call on the `rt/drv/<bus>` thread, synchronous,
//! without allocation, blocking or `await`.

use crate::commit::CommitRx;
use crate::driver::{Driver, RawSink};
use crate::types::{DriverError, MonoNs};

/// What one bus cycle may touch: the sample ring, the latest-wins boxes of
/// the bus's outputs (with `force_safe`) and the cycle's instant.
#[derive(Debug)]
pub struct RtIo<'a> {
    /// Samples, on the same ring as the io part.
    pub samples: RawSink<'a>,
    /// Committed writes of the bus's outputs.
    pub outputs: CommitRx<'a>,
    /// Monotonic instant of this cycle.
    pub now: MonoNs,
}

/// The result of one bus cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtStatus {
    /// The cycle ran.
    Ok,
    /// The cycle ran past its declared WCET.
    Overrun,
    /// The bus is in a fault; the io part reports the detail.
    Fault,
}

/// The part of a driver that runs on `rt/drv/<bus>`.
pub trait RtPart: Send + 'static {
    /// One bus cycle. Synchronous, no allocation, no blocking, no await,
    /// bounded WCET.
    fn cycle(&mut self, io: &mut RtIo<'_>) -> RtStatus;
}

/// A driver with an RT part.
pub trait RtCapable: Driver {
    /// The RT part.
    type Rt: RtPart;

    /// Called once after `connect`; the core moves the part to the RT thread.
    fn rt_part(&mut self) -> Result<Self::Rt, DriverError>;
}
