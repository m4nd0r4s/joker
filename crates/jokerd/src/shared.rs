//! What the cycle thread shares with the rest of the daemon for one
//! generation. Every structure is allocated before the first tick and is
//! lock-free: the cycle thread never blocks on it, and a full queue drops and
//! counts (AGENTS.md invariants 6 and 7).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;
use joker_core::commit::{DecisionRecord, WriteResult};
use joker_core::image::Proposal;
use joker_core::value::NO_TS_SRC;
use joker_core::{Quality, QualityCode, QualityReason, SignalId, Slot, ValueType};
use joker_hal::{CommitBox, OutputBinding, RawEncoding, WriteKind, WriteStatus};

use crate::plan::Plan;

/// Capacity of each group's async inbox (docs/01 §5).
pub const INBOX_CAPACITY: usize = 256;
/// Capacity of the staging buffer of each group.
pub const STAGING_CAPACITY: usize = 512;
/// Capacity of the decision record queue.
pub const DECISION_CAPACITY: usize = 16_384;
/// Capacity of the write result queue (tickets of async writes).
pub const RESULT_CAPACITY: usize = 4_096;
/// Capacity of the queue of failed write acks from the real drivers.
pub const WRITE_ACK_CAPACITY: usize = 1_024;
/// Capacity of each group's reset-request queue (`jk reset`, docs/16 E7).
pub const RESET_CAPACITY: usize = 16;
/// Capacity of each group's trip-injection queue (trips requested while
/// the generation runs; the persisted trips are restored before the first
/// tick through `RunOptions::initial_trips`, with no capacity, ADR 0061).
pub const TRIP_CAPACITY: usize = 16;
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

/// A request to put an output in trip while the generation runs, with no
/// reply: pushed into the owning group's own queue and consumed by that
/// group's next tick, the same reasoning as [`ResetRequest`] — the cycle
/// thread is the only one that may call `Outputs::trip`. The persisted
/// trips do not go through it (`RunOptions::initial_trips`, ADR 0061).
#[derive(Debug, Clone, Copy)]
pub struct TripRequest {
    /// The output to trip.
    pub signal: SignalId,
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
    /// Monotonic instant this group's tick last *started* (`rt/supervisor`,
    /// AUR-274): a hang anywhere in the cycle thread stops this from
    /// advancing, which is exactly the staleness the supervisor watches.
    pub last_tick_mono_ns: Counter,
}

/// What the io task of one real device reports (ref/06 §2.12, AGENTS.md
/// invariant 7), in `plan.devices` order.
#[derive(Debug, Default)]
pub struct DeviceStats {
    /// The effective link state, as the code of `joker_hal::LinkState`
    /// (`connecting` 0, `up` 1, `degraded` 2, `down` 3).
    pub link: Counter,
    /// Successful `connect` calls after the first one.
    pub reconnects: Counter,
    /// Samples the device's sample ring refused.
    pub dropped_samples: Counter,
    /// Samples that broke the rules of ref/06 §2.8 (`J0869`).
    pub invalid_samples: Counter,
}

/// A write a real driver did not confirm (ref/06 §2.9): what the decision
/// log records next to the commit that produced it (AGENTS.md invariant 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteAckRecord {
    /// The device, in `plan.devices` order.
    pub device: u32,
    /// The output that was written.
    pub output: SignalId,
    /// The write's sequence number on that device.
    pub seq: u64,
    /// Why it was written.
    pub kind: WriteKind,
    /// What the driver answered.
    pub status: WriteStatus,
    /// Monotonic instant of the answer.
    pub at_mono_ns: u64,
}

/// Whether the equipment confirmed the last write of an output (ref/06
/// §2.9), from an io task to the cycle thread, the only writer of the
/// output's slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Confirmation {
    /// The output.
    pub output: SignalId,
    /// `false` after a `device-rejected` ack: the output turns
    /// `uncertain(not_confirmed)`; `true` after a later `ok` ack.
    pub confirmed: bool,
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
    /// One trip-injection queue per group, by group index ([`TripRequest`]).
    pub trips: Box<[ArrayQueue<TripRequest>]>,
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
    /// When each output (same order as [`Shared::output_ids`]) last
    /// transitioned into trip, monotonic ns; 0 while clear (`GET
    /// /v1/trips`'s own `Trip.since`, ref/09 §5.7). Set by the cycle
    /// thread only, read by the API.
    pub trip_since: Box<[AtomicU64]>,
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
    /// Per device, matching [`Shared::commit_bindings`]: whether the
    /// device fallback covers that output, so its io task renews it with
    /// `keepalive` (ref/03 §2.2.11 `applies_to`).
    pub keepalive: Vec<Vec<bool>>,
    /// Per device (`plan.devices` order): its link and its counters.
    pub devices: Box<[DeviceStats]>,
    /// Writes the real drivers did not confirm, for the decision log.
    pub write_acks: ArrayQueue<WriteAckRecord>,
    /// Write acks dropped by a full queue.
    pub write_acks_dropped: Counter,
    /// Confirmations of output writes, for the cycle thread to apply to
    /// the outputs' slots.
    pub confirmations: ArrayQueue<Confirmation>,
    /// Confirmations dropped by a full queue.
    pub confirmations_dropped: Counter,
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

/// Per device: the output bindings, their boxes, and whether the device
/// fallback covers each output.
type CommitChannels = (Vec<Vec<OutputBinding>>, Vec<Vec<CommitBox>>, Vec<Vec<bool>>);

fn commit_channels(plan: &Plan) -> CommitChannels {
    let mut bindings = Vec::with_capacity(plan.devices.len());
    let mut boxes = Vec::with_capacity(plan.devices.len());
    let mut keepalive = Vec::with_capacity(plan.devices.len());
    for device in &plan.devices {
        let mut device_bindings = Vec::new();
        let mut device_boxes = Vec::new();
        let mut device_keepalive = Vec::new();
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
            let relative = point
                .path
                .strip_prefix(device.name.as_str())
                .and_then(|rest| rest.strip_prefix('.'))
                .unwrap_or(&point.path);
            device_keepalive.push(
                device
                    .fallback_outputs
                    .as_ref()
                    .is_none_or(|covered| covered.contains(relative)),
            );
        }
        bindings.push(device_bindings);
        boxes.push(device_boxes);
        keepalive.push(device_keepalive);
    }
    (bindings, boxes, keepalive)
}

impl Shared {
    /// Applies the pending [`Confirmation`]s to the outputs' slots (cycle
    /// thread only: the one writer of an output's slot). A rejected output
    /// keeps its applied value with `uncertain(not_confirmed)`; a confirmed
    /// one is `good` again. The core rewrites the slot `good` whenever the
    /// applied value changes, so a new value starts confirmed until its own
    /// ack says otherwise.
    pub fn apply_confirmations(&self, now_mono_ns: u64) {
        let not_confirmed = Quality::new(QualityCode::Uncertain, QualityReason::NotConfirmed);
        while let Some(c) = self.confirmations.pop() {
            let Some(slot) = self.slots.get(c.output.index()) else {
                continue;
            };
            let Ok(sample) = slot.read(8) else {
                continue;
            };
            let quality = sample.quality();
            let next = if c.confirmed {
                if quality != not_confirmed {
                    continue;
                }
                Quality::GOOD
            } else {
                not_confirmed
            };
            slot.write(sample.transport_bits(), next, NO_TS_SRC, now_mono_ns);
        }
    }

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
        let (commit_bindings, commit_boxes, keepalive) = commit_channels(plan);
        Self {
            slots,
            inboxes: (0..plan.groups.len())
                .map(|_| ArrayQueue::new(INBOX_CAPACITY))
                .collect(),
            resets: (0..plan.groups.len())
                .map(|_| ArrayQueue::new(RESET_CAPACITY))
                .collect(),
            trips: (0..plan.groups.len())
                .map(|_| ArrayQueue::new(TRIP_CAPACITY))
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
            trip_since: output_ids.iter().map(|_| AtomicU64::new(0)).collect(),
            output_ids: output_ids.into_boxed_slice(),
            groups: plan.groups.iter().map(|_| GroupStats::default()).collect(),
            export_request: AtomicU64::new(0),
            exports: ArrayQueue::new(EXPORT_CAPACITY),
            stop: AtomicBool::new(false),
            running: AtomicBool::new(false),
            commit_bindings,
            commit_boxes,
            keepalive,
            devices: plan
                .devices
                .iter()
                .map(|_| DeviceStats::default())
                .collect(),
            write_acks: ArrayQueue::new(WRITE_ACK_CAPACITY),
            write_acks_dropped: Counter::default(),
            confirmations: ArrayQueue::new(WRITE_ACK_CAPACITY),
            confirmations_dropped: Counter::default(),
        }
    }

    /// The status of an output.
    pub fn output(&self, signal: SignalId) -> Option<&OutputStatus> {
        let i = self.output_ids.binary_search(&signal).ok()?;
        self.outputs.get(i)
    }

    /// When `signal` last became tripped, monotonic ns; `None` while
    /// clear.
    pub fn trip_since(&self, signal: SignalId) -> Option<u64> {
        let i = self.output_ids.binary_search(&signal).ok()?;
        let since = self.trip_since.get(i)?.load(Ordering::Relaxed);
        (since != 0).then_some(since)
    }

    /// Sets (or clears, with 0) when `signal` became tripped (cycle
    /// thread only: one writer, same reasoning as [`OutputStatus::publish`]).
    pub fn set_trip_since(&self, signal: SignalId, since_mono_ns: u64) {
        if let Ok(i) = self.output_ids.binary_search(&signal)
            && let Some(cell) = self.trip_since.get(i)
        {
            cell.store(since_mono_ns, Ordering::Relaxed);
        }
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
