//! What the cycle thread shares with the rest of the daemon for one
//! generation. Every structure is allocated before the first tick and is
//! lock-free: the cycle thread never blocks on it, and a full queue drops and
//! counts (AGENTS.md invariants 6 and 7).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;
use joker_core::commit::{DecisionRecord, WriteResult};
use joker_core::image::Proposal;
use joker_core::{Quality, SignalId, Slot, ValueType};
use joker_hal::{CommitBox, OutputBinding, RawEncoding};

use crate::plan::Plan;

/// Capacity of each group's async inbox (docs/01 §5).
pub const INBOX_CAPACITY: usize = 256;
/// Capacity of the staging buffer of each group.
pub const STAGING_CAPACITY: usize = 512;
/// Capacity of the decision record queue.
pub const DECISION_CAPACITY: usize = 16_384;
/// Capacity of the write result queue (tickets of async writes).
pub const RESULT_CAPACITY: usize = 4_096;
/// Capacity of each group's reset-request queue (`jk reset`, docs/16 E7).
pub const RESET_CAPACITY: usize = 16;
/// State exports in flight (apps per request).
pub const EXPORT_CAPACITY: usize = 256;
/// Trace buffers in flight between the cycle thread and the trace ring.
pub const TRACE_BUFFERS: usize = 512;

/// A request to end a trip (`jk reset`, docs/16 E7, ref/09 §5.7): pushed
/// by the API into the owning group's own queue, consumed by that
/// group's own next tick (same reasoning as [`Shared::inboxes`] for
/// proposals — the cycle thread is the only one that may call
/// `Outputs::reset`).
#[derive(Debug, Clone, Copy)]
pub struct ResetRequest {
    /// The output to reset.
    pub signal: SignalId,
    /// Matches the [`ResetOutcome`] this request resolves to.
    pub ticket: u32,
}

/// What ending a trip resolved to (ref/09 §5.7's `ResetResult.result`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetKind {
    /// The output left trip and stayed clear this tick.
    Cleared,
    /// The output left trip but the same condition tripped it again,
    /// in the same tick (docs/16 E7).
    Retripped,
    /// The request named an unknown signal, or one not currently
    /// tripped; resolved immediately, without waiting for a tick.
    NotTripped,
}

/// The resolution of one [`ResetRequest`], by its `ticket`.
#[derive(Debug, Clone, Copy)]
pub struct ResetOutcome {
    /// Matches the request's `ticket`.
    pub ticket: u32,
    /// The output that was reset.
    pub signal: SignalId,
    /// The result.
    pub kind: ResetKind,
}

/// A counter that the cycle thread increments and others read.
#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    /// Adds one.
    pub fn bump(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// Adds `n`.
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    /// Stores a value.
    pub fn set(&self, n: u64) {
        self.0.store(n, Ordering::Relaxed);
    }

    /// Stores the maximum of the current value and `n`.
    pub fn max(&self, n: u64) {
        self.0.fetch_max(n, Ordering::Relaxed);
    }

    /// The current value.
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Statistics of one cycle group (docs/01 §19).
#[derive(Debug, Default)]
pub struct GroupStats {
    /// Executed ticks.
    pub ticks: Counter,
    /// Last executed tick index.
    pub last_tick: Counter,
    /// Skipped ticks (`overrun = "skip"`).
    pub overruns: Counter,
    /// Largest start lateness seen, in ns.
    pub max_late_ns: Counter,
    /// Largest tick duration seen, in ns.
    pub max_duration_ns: Counter,
    /// Task calls that failed.
    pub task_errors: Counter,
    /// Proposals dropped by a full inbox or staging buffer.
    pub dropped_proposals: Counter,
}

/// One tick of one group, encoded as a trace segment of one tick.
#[derive(Debug)]
pub struct TraceChunk {
    /// Group index.
    pub group: u32,
    /// Tick index.
    pub index: u64,
    /// Latch instant of the tick, monotonic ns.
    pub at_mono_ns: u64,
    /// A segment holding exactly one tick (docs/16 §7).
    pub segment: Vec<u8>,
}

/// The state of one app's cycle instance, exported between two ticks for a
/// recording (docs/16 §7, ref/04 §4.12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateExport {
    /// Which request this answers.
    pub request: u64,
    /// The app.
    pub app: String,
    /// Group index of its cycle instance.
    pub group: u32,
    /// Index of the first tick after the export.
    pub first_seq: u64,
    /// 0 exported, 1 no `persist`, 2 failed.
    pub state: u8,
    /// The state blob.
    pub blob: Vec<u8>,
}

/// The status of one output after its last commit, readable by the API.
#[derive(Debug)]
pub struct OutputStatus {
    slot: Slot,
}

impl OutputStatus {
    fn new() -> Self {
        Self { slot: Slot::new() }
    }

    /// Publishes the status of a tick (cycle thread only: one writer).
    pub fn publish(&self, applied: u64, meta: OutputMeta, tick: u64, at_mono_ns: u64) {
        self.slot.write(
            applied,
            Quality::from_bits(meta.pack()),
            tick as i64,
            at_mono_ns,
        );
    }

    /// Reads the last published status.
    pub fn read(&self) -> Option<(u64, OutputMeta, u64)> {
        let s = self.slot.read(8).ok()?;
        if s.seq() == 0 {
            return None;
        }
        Some((
            s.transport_bits(),
            OutputMeta::unpack(s.quality().to_bits()),
            s.ts_src_raw() as u64,
        ))
    }
}

/// The packed meta-data of an output status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputMeta {
    /// Active level, 0 for none.
    pub level: u8,
    /// Outcome code (ref/00 §4.4).
    pub outcome: u8,
    /// Outcome detail: reject reason or overriding level.
    pub detail: u8,
    /// In trip.
    pub tripped: bool,
    /// The applied state is `relinquish`.
    pub relinquish: bool,
}

impl OutputMeta {
    fn pack(self) -> u32 {
        u32::from(self.level)
            | u32::from(self.outcome) << 8
            | u32::from(self.detail) << 16
            | u32::from(self.tripped) << 24
            | u32::from(self.relinquish) << 25
    }

    fn unpack(bits: u32) -> Self {
        Self {
            level: (bits & 0xFF) as u8,
            outcome: ((bits >> 8) & 0xFF) as u8,
            detail: ((bits >> 16) & 0xFF) as u8,
            tripped: bits & (1 << 24) != 0,
            relinquish: bits & (1 << 25) != 0,
        }
    }
}

/// The lock-free state shared by the cycle thread and the services.
#[derive(Debug)]
pub struct Shared {
    /// One slot per signal, indexed by `SignalId`.
    pub slots: Box<[Slot]>,
    /// One async inbox per group, by group index.
    pub inboxes: Box<[ArrayQueue<Proposal>]>,
    /// One reset-request queue per group, by group index (`jk reset`).
    pub resets: Box<[ArrayQueue<ResetRequest>]>,
    /// Resolutions of reset requests, by ticket.
    pub reset_results: ArrayQueue<ResetOutcome>,
    /// Reset resolutions dropped by a full queue.
    pub reset_results_dropped: Counter,
    /// Decision records for the decision log.
    pub decisions: ArrayQueue<DecisionRecord>,
    /// Decision records dropped by a full queue.
    pub decisions_dropped: Counter,
    /// Results of proposals that carry a ticket.
    pub results: ArrayQueue<WriteResult>,
    /// Results dropped by a full queue.
    pub results_dropped: Counter,
    /// Empty trace buffers for the cycle thread.
    pub trace_free: ArrayQueue<Vec<u8>>,
    /// Filled trace chunks for the trace ring.
    pub trace_filled: ArrayQueue<TraceChunk>,
    /// Ticks not traced for lack of a buffer.
    pub trace_dropped: Counter,
    /// Status per output, in the order of [`Shared::output_ids`].
    pub outputs: Box<[OutputStatus]>,
    /// The output signals, sorted.
    pub output_ids: Box<[SignalId]>,
    /// Statistics per group.
    pub groups: Box<[GroupStats]>,
    /// The latest state export request; 0 is none.
    pub export_request: AtomicU64,
    /// The answers to state export requests.
    pub exports: ArrayQueue<StateExport>,
    /// Asks the cycle thread to stop after the current tick.
    pub stop: AtomicBool,
    /// The cycle thread has started ticking.
    pub running: AtomicBool,
    /// Per real-driver device (`plan.devices` order): the output bindings
    /// of its own write points, for a `CommitTx`/`CommitRx` pair the cycle
    /// thread and that device's io task share (ref/06 §2.9). Empty for a
    /// device with no output points, or whose driver this build does not
    /// resolve a point shape for.
    pub commit_bindings: Vec<Vec<OutputBinding>>,
    /// The boxes matching [`Shared::commit_bindings`], one per device.
    pub commit_boxes: Vec<Vec<CommitBox>>,
}

fn raw_encoding_of(ty: ValueType) -> Option<RawEncoding> {
    match ty {
        ValueType::Bool => Some(RawEncoding::Bool),
        ValueType::I64 => Some(RawEncoding::Int),
        ValueType::U64 | ValueType::Enum | ValueType::Bits => Some(RawEncoding::Uint),
        ValueType::F64 | ValueType::F32 => Some(RawEncoding::Float),
        _ => None,
    }
}

fn commit_channels(plan: &Plan) -> (Vec<Vec<OutputBinding>>, Vec<Vec<CommitBox>>) {
    let mut bindings = Vec::with_capacity(plan.devices.len());
    let mut boxes = Vec::with_capacity(plan.devices.len());
    for device in &plan.devices {
        let mut device_bindings = Vec::new();
        let mut device_boxes = Vec::new();
        for (i, point) in device.points.iter().enumerate() {
            if point.direction != joker_hal::Direction::Output {
                continue;
            }
            let Some(signal) = plan.id(&point.path) else {
                continue;
            };
            let Some(sig_plan) = plan.signals.iter().find(|s| s.path == point.path) else {
                continue;
            };
            let Some(encoding) = raw_encoding_of(sig_plan.ty) else {
                continue;
            };
            let Ok(point_id) = u32::try_from(i) else {
                continue;
            };
            device_bindings.push(OutputBinding {
                signal,
                point: point_id,
                encoding,
            });
            device_boxes.push(CommitBox::new());
        }
        bindings.push(device_bindings);
        boxes.push(device_boxes);
    }
    (bindings, boxes)
}

impl Shared {
    /// Allocates everything a plan needs, before the first tick.
    pub fn new(plan: &Plan) -> Self {
        let slots: Box<[Slot]> = (0..plan.signals.len()).map(|_| Slot::new()).collect();
        let mut output_ids: Vec<SignalId> = plan
            .outputs
            .iter()
            .filter_map(|o| plan.id(&o.path))
            .collect();
        output_ids.sort();
        let trace_free = ArrayQueue::new(TRACE_BUFFERS);
        let trace_len = trace_buffer_len(plan);
        for _ in 0..TRACE_BUFFERS {
            let _ = trace_free.push(Vec::with_capacity(trace_len));
        }
        let (commit_bindings, commit_boxes) = commit_channels(plan);
        Self {
            slots,
            inboxes: (0..plan.groups.len())
                .map(|_| ArrayQueue::new(INBOX_CAPACITY))
                .collect(),
            resets: (0..plan.groups.len())
                .map(|_| ArrayQueue::new(RESET_CAPACITY))
                .collect(),
            reset_results: ArrayQueue::new(RESULT_CAPACITY),
            reset_results_dropped: Counter::default(),
            decisions: ArrayQueue::new(DECISION_CAPACITY),
            decisions_dropped: Counter::default(),
            results: ArrayQueue::new(RESULT_CAPACITY),
            results_dropped: Counter::default(),
            trace_free,
            trace_filled: ArrayQueue::new(TRACE_BUFFERS),
            trace_dropped: Counter::default(),
            outputs: output_ids.iter().map(|_| OutputStatus::new()).collect(),
            output_ids: output_ids.into_boxed_slice(),
            groups: plan.groups.iter().map(|_| GroupStats::default()).collect(),
            export_request: AtomicU64::new(0),
            exports: ArrayQueue::new(EXPORT_CAPACITY),
            stop: AtomicBool::new(false),
            running: AtomicBool::new(false),
            commit_bindings,
            commit_boxes,
        }
    }

    /// The status of an output.
    pub fn output(&self, signal: SignalId) -> Option<&OutputStatus> {
        let i = self.output_ids.binary_search(&signal).ok()?;
        self.outputs.get(i)
    }

    /// Asks the cycle thread to stop.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Asks every group to export its apps' state after its next tick and
    /// returns the request number.
    pub fn request_export(&self) -> u64 {
        self.export_request.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Whether a stop was requested.
    pub fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

/// The largest one-tick segment of any group of a plan.
pub fn trace_buffer_len(plan: &Plan) -> usize {
    use joker_core::trace::{
        INPUT_LEN, PROPOSAL_LEN, RESULT_LEN, SEGMENT_HEADER_LEN, TICK_FIXED_LEN,
    };
    let ports = plan.groups.iter().map(|g| g.ports.len()).max().unwrap_or(0);
    SEGMENT_HEADER_LEN
        + TICK_FIXED_LEN
        + ports * INPUT_LEN
        + 2
        + STAGING_CAPACITY * PROPOSAL_LEN
        + 2
        + plan.outputs.len() * RESULT_LEN
        + 4
}
