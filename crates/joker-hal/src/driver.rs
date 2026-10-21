//! The runtime half of a driver (ref/06 §3.3): `no_std`, no allocation
//! required, every operation of §2.1 as an async method.

use core::future::Future;

use crate::cbor::Cbor;
use crate::commit::Sealed;
use crate::cx::{Cx, MaybeSend};
use crate::link::Link;
use crate::plan::{BatchSpec, PlanView};
use crate::spec::{DeviceSpec, PlanRequest};
use crate::types::{
    ActionFailure, AtomicRef, BatchFailure, BatchId, BatchKind, Detail, DeviceIx, DriverError,
    LinkState, MonoNs, PointId, RawKind, RawSample, RawValue, WriteKind, WriteStatus,
};

/// A batch at runtime, as handed to `read`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadBatch<'a> {
    /// Batch id.
    pub id: BatchId,
    /// Device.
    pub device: DeviceIx,
    /// Kind.
    pub kind: BatchKind,
    /// Points, increasing.
    pub points: &'a [PointId],
    /// Driver-private bytes of the batch.
    pub private: &'a [u8],
    /// Absolute monotonic deadline of this execution.
    pub deadline: MonoNs,
}

impl<'a> ReadBatch<'a> {
    /// The execution of `spec` with `deadline`.
    pub fn new(spec: &BatchSpec<'a>, deadline: MonoNs) -> Self {
        Self {
            id: spec.head.id,
            device: spec.head.device,
            kind: spec.head.kind,
            points: spec.points,
            private: spec.private,
            deadline,
        }
    }
}

/// A write that went through the arbiter and the envelope of its owner
/// group, converted to raw (ref/06 §2.9, ADR 0005).
///
/// It has a private field: outside this crate it is only obtained from a
/// [`CommitRx`](crate::commit::CommitRx), which builds it from the applied
/// values of a [`joker_core::Outputs`] or from the output's declared `safe`.
/// A driver therefore cannot be handed a value that did not go through the
/// envelope:
///
/// ```compile_fail
/// use joker_hal::{CommittedWrite, RawValue, WriteKind};
/// let forged = CommittedWrite {
///     seq: 1,
///     point: 0,
///     value: RawValue::Bool(true),
///     kind: WriteKind::Set,
///     at: None,
///     atomic: None,
/// };
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommittedWrite<'a> {
    /// Sequence number, unique per transmitter; the ack carries it back.
    pub seq: u64,
    /// The output point.
    pub point: PointId,
    /// The raw value; `None` for `relinquish`.
    pub value: RawValue<'a>,
    /// Why it is written.
    pub kind: WriteKind,
    /// Instant of a timed write, with `timed-write`.
    pub at: Option<MonoNs>,
    /// Position in an atomic group.
    pub atomic: Option<AtomicRef>,
    pub(crate) sealed: Sealed,
}

/// One acknowledgement slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AckSlot {
    seq: u64,
    status: Option<WriteStatus>,
    ts_hw: Option<MonoNs>,
}

/// The answer to one write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteAck {
    /// The write's sequence number.
    pub seq: u64,
    /// Status; an unset ack is `CommFailure` (ref/06 §3.3).
    pub status: WriteStatus,
    /// Hardware timestamp of the write.
    pub ts_hw: Option<MonoNs>,
}

/// One ack per write, in the same order and with the same `seq`.
#[derive(Debug)]
pub struct WriteAcks<'a> {
    slots: &'a mut [AckSlot],
}

impl<'a> WriteAcks<'a> {
    /// Acks for `writes` in `slots`; `None` if `slots` is shorter.
    pub fn new(writes: &[CommittedWrite<'_>], slots: &'a mut [AckSlot]) -> Option<Self> {
        let slots = slots.get_mut(..writes.len())?;
        for (slot, write) in slots.iter_mut().zip(writes) {
            *slot = AckSlot {
                seq: write.seq,
                status: None,
                ts_hw: None,
            };
        }
        Some(Self { slots })
    }

    /// Number of acks.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// True for no writes.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Sets the ack of write `index`; false when out of range.
    pub fn set(&mut self, index: usize, status: WriteStatus, ts_hw: Option<MonoNs>) -> bool {
        match self.slots.get_mut(index) {
            Some(slot) => {
                slot.status = Some(status);
                slot.ts_hw = ts_hw;
                true
            }
            None => false,
        }
    }

    /// Sets every ack not set yet.
    pub fn fill(&mut self, status: WriteStatus) {
        for slot in self.slots.iter_mut().filter(|s| s.status.is_none()) {
            slot.status = Some(status);
        }
    }

    /// The ack of write `index`.
    pub fn get(&self, index: usize) -> Option<WriteAck> {
        self.slots.get(index).map(|s| WriteAck {
            seq: s.seq,
            status: s.status.unwrap_or(WriteStatus::CommFailure),
            ts_hw: s.ts_hw,
        })
    }

    /// Every ack, in order.
    pub fn iter(&self) -> impl Iterator<Item = WriteAck> + '_ {
        (0..self.slots.len()).filter_map(|i| self.get(i))
    }
}

/// The sample ring is full: the sample was dropped and counted (ADR 0013).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// Where samples go: the device's SPSC ring on a node, a buffer in tests.
/// Never blocks.
pub trait SampleSink: MaybeSend {
    /// Copies `sample`, or refuses it when full.
    fn accept(&mut self, sample: &RawSample<'_>) -> Result<(), Full>;
}

/// The sample output of one `read`.
pub struct RawSink<'a> {
    sink: &'a mut dyn SampleSink,
    pushed: u32,
    dropped: u32,
}

impl core::fmt::Debug for RawSink<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawSink")
            .field("pushed", &self.pushed)
            .field("dropped", &self.dropped)
            .finish_non_exhaustive()
    }
}

impl<'a> RawSink<'a> {
    /// A sink over `sink`.
    pub fn new(sink: &'a mut dyn SampleSink) -> Self {
        Self {
            sink,
            pushed: 0,
            dropped: 0,
        }
    }

    /// Copies one sample; `Err(Full)` when the ring is full (counted).
    pub fn push(&mut self, sample: RawSample<'_>) -> Result<(), Full> {
        match self.sink.accept(&sample) {
            Ok(()) => {
                self.pushed += 1;
                Ok(())
            }
            Err(Full) => {
                self.dropped += 1;
                Err(Full)
            }
        }
    }

    /// Samples accepted in this call.
    pub fn pushed(&self) -> u32 {
        self.pushed
    }

    /// Samples dropped in this call.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }
}

/// Identity of a device read at `connect` (ref/06 §2.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DeviceInfo<'a> {
    /// Vendor.
    pub vendor: Option<&'a str>,
    /// Model.
    pub model: Option<&'a str>,
    /// Serial number.
    pub serial: Option<&'a str>,
    /// Firmware version.
    pub firmware: Option<&'a str>,
    /// Hardware revision.
    pub hw_revision: Option<&'a str>,
    /// SHA-256 of the device certificate.
    pub cert_sha256: Option<[u8; 32]>,
    /// Driver fields, in the descriptor's `discovered` schema.
    pub extra: Cbor<'a>,
}

/// Where `connect` reports device identities.
pub trait ConnectSink: MaybeSend {
    /// The identity and the discovered map of device `index`.
    fn device(&mut self, index: DeviceIx, info: &DeviceInfo<'_>, discovered: Option<Cbor<'_>>);
}

impl ConnectSink for () {
    fn device(&mut self, _: DeviceIx, _: &DeviceInfo<'_>, _: Option<Cbor<'_>>) {}
}

/// The output of `connect`.
pub struct ConnectOut<'a> {
    sink: &'a mut dyn ConnectSink,
}

impl core::fmt::Debug for ConnectOut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ConnectOut").finish_non_exhaustive()
    }
}

impl<'a> ConnectOut<'a> {
    /// An output over `sink`.
    pub fn new(sink: &'a mut dyn ConnectSink) -> Self {
        Self { sink }
    }

    /// Reports device `index`.
    pub fn device(&mut self, index: DeviceIx, info: &DeviceInfo<'_>, discovered: Option<Cbor<'_>>) {
        self.sink.device(index, info, discovered);
    }
}

/// Most health counters a driver reports (ref/06 §2.15).
pub const MAX_COUNTERS: usize = 64;

/// The output of `health` (ref/06 §2.12).
#[derive(Debug)]
pub struct HealthOut<'a> {
    link: Option<LinkState>,
    detail: Detail,
    counters: &'a mut [(&'static str, u64)],
    len: usize,
}

impl<'a> HealthOut<'a> {
    /// An output with room for `counters.len()` counters.
    pub fn new(counters: &'a mut [(&'static str, u64)]) -> Self {
        Self {
            link: None,
            detail: Detail::EMPTY,
            counters,
            len: 0,
        }
    }

    /// The link state as the driver sees it.
    pub fn link(&mut self, state: LinkState, detail: &str) {
        self.link = Some(state);
        self.detail = Detail::new(detail);
    }

    /// One counter, exported as `joker_driver_<name>{device}`; the excess
    /// ones are ignored.
    pub fn counter(&mut self, name: &'static str, value: u64) {
        if let Some(slot) = self
            .counters
            .get_mut(self.len)
            .filter(|_| self.len < MAX_COUNTERS)
        {
            *slot = (name, value);
            self.len += 1;
        }
    }

    /// The reported link state and detail.
    pub fn reported_link(&self) -> Option<(LinkState, &str)> {
        self.link.map(|l| (l, self.detail.as_str()))
    }

    /// The reported counters.
    pub fn counters(&self) -> &[(&'static str, u64)] {
        &self.counters[..self.len]
    }
}

/// A read by address (capability `raw-access`, ref/06 §2.10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AddrRead<'a> {
    /// Device.
    pub device: DeviceIx,
    /// Address, in the driver's `address` schema.
    pub addr: Cbor<'a>,
    /// Raw form expected.
    pub expect: RawKind,
    /// Width.
    pub width: u16,
    /// The profile's `type`.
    pub type_name: &'a str,
}

/// A write by address (capability `raw-access`, ref/06 §2.10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AddrWrite<'a> {
    /// Device.
    pub device: DeviceIx,
    /// Address, in the driver's `address` schema.
    pub addr: Cbor<'a>,
    /// The encoded value.
    pub value: RawValue<'a>,
    /// The profile's `type`.
    pub type_name: &'a str,
}

/// A native action invocation (ref/06 §2.10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionCall<'a> {
    /// Invocation id.
    pub call: u64,
    /// The action's `ActionSpec::id`.
    pub action: u32,
    /// Arguments, validated by the core.
    pub args: Cbor<'a>,
}

/// The result of a native action, as CBOR.
#[derive(Debug)]
pub struct ActionOut<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> ActionOut<'a> {
    /// An output into `buf`.
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    /// Sets the result record.
    pub fn result(&mut self, cbor: &[u8]) -> Result<(), Full> {
        let slot = self.buf.get_mut(..cbor.len()).ok_or(Full)?;
        slot.copy_from_slice(cbor);
        self.len = cbor.len();
        Ok(())
    }

    /// The result record.
    pub fn as_cbor(&self) -> Cbor<'_> {
        Cbor::new(&self.buf[..self.len])
    }
}

/// What `open` receives: the request and the plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenArgs<'a> {
    request: PlanRequest<'a>,
    plan: PlanView<'a>,
}

impl<'a> OpenArgs<'a> {
    /// The arguments of `open`.
    pub fn new(request: PlanRequest<'a>, plan: PlanView<'a>) -> Self {
        Self { request, plan }
    }

    /// The request.
    pub fn request(&self) -> &PlanRequest<'a> {
        &self.request
    }

    /// The plan.
    pub fn plan(&self) -> &PlanView<'a> {
        &self.plan
    }

    /// Device `index`.
    pub fn device(&self, index: DeviceIx) -> Option<&'a DeviceSpec<'a>> {
        self.request.device(index)
    }

    /// The configuration of device `index`.
    pub fn config(&self, index: DeviceIx) -> Option<Cbor<'a>> {
        self.request.device(index).map(|d| d.config)
    }
}

/// The runtime half of a driver. One call at a time per instance
/// (`&mut self`); any future may be dropped at an `await`.
pub trait Driver: Sized + MaybeSend + 'static {
    /// Builds the instance state. No I/O.
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError>;

    /// Connects and reports the identity of each device.
    fn connect(
        &mut self,
        cx: &mut Cx<'_>,
        out: &mut ConnectOut<'_>,
    ) -> impl Future<Output = Result<(), DriverError>> + MaybeSend;

    /// One execution of `batch`; one `out.push` per point. For `Push`
    /// batches: return as soon as at least one sample was pushed, or `Ok(())`
    /// at the deadline. Must be cancel-safe for `Push` batches.
    fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend;

    /// Capability `write`. One `acks.set(i, …)` per write; unset acks become
    /// `CommFailure`.
    fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) -> impl Future<Output = ()> + MaybeSend {
        async move {
            let _ = (cx, writes);
            acks.fill(WriteStatus::Unsupported);
        }
    }

    /// Capability `raw-access`.
    fn read_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrRead<'_>,
        out: &mut RawSink<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend {
        async move {
            let _ = (cx, req, out);
            Err(BatchFailure::ConfigError("raw-access not supported"))
        }
    }

    /// Capability `raw-access`.
    fn write_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrWrite<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend {
        async move {
            let _ = (cx, req);
            Err(BatchFailure::ConfigError("raw-access not supported"))
        }
    }

    /// Capability `native-actions`. The result record goes to `out` as CBOR.
    fn invoke(
        &mut self,
        cx: &mut Cx<'_>,
        call: &ActionCall<'_>,
        out: &mut ActionOut<'_>,
    ) -> impl Future<Output = Result<(), ActionFailure>> + MaybeSend {
        async move {
            let _ = (cx, call, out);
            Err(ActionFailure::Unsupported)
        }
    }

    /// Synchronous, cheap, no I/O.
    fn health(&self, out: &mut HealthOut<'_>) {
        let _ = out;
    }

    /// Must succeed.
    fn disconnect(&mut self, cx: &mut Cx<'_>) -> impl Future<Output = ()> + MaybeSend {
        async move {
            let _ = cx;
        }
    }
}

/// The planning half (ref/06 §3.3). Host side only: it needs `alloc`.
#[cfg(feature = "plan")]
pub trait Planner {
    /// Static descriptor (ref/06 §2.3).
    const DESCRIPTOR: &'static crate::descriptor::Descriptor;

    /// Pure: no I/O, no clock, no randomness, no global state. Same request,
    /// same plan.
    fn plan(
        req: &PlanRequest<'_>,
    ) -> Result<crate::plan::Plan, alloc::vec::Vec<crate::types::Diagnostic<'static>>>;
}
