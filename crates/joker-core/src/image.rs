//! The process image of one cycle group: latch, staging and the tick clock
//! (ADR 0004, docs/01 §3.4 and §4, ADR 0032).
//!
//! The image owns no memory: the caller hands in fixed storage once per
//! generation, and nothing on the tick path allocates, blocks or waits.

use crate::quality::{Quality, QualityCode, QualityReason};
use crate::registry::{GroupId, Registry, SignalId};
use crate::slot::{SeqExtender, Slot};
use crate::time::Instant;
use crate::value::Sample;

/// Writer number within a generation (ref/00 §5). Remote writers of an MCU
/// are `0x8000_0000 | slot` (docs/16 §7).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WriterId(pub u32);

impl WriterId {
    /// The bit that marks a remote writer of an MCU (docs/16 §7).
    pub const REMOTE: u32 = 0x8000_0000;

    /// The id as an index into dense tables.
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// A remote writer: a (link, level) pair of an MCU (docs/16 A12).
    pub const fn remote(slot: u32) -> Self {
        Self(Self::REMOTE | slot)
    }

    /// True for a remote writer.
    pub const fn is_remote(self) -> bool {
        self.0 & Self::REMOTE != 0
    }

    /// The slot of a remote writer, as an index into the remote table.
    pub const fn remote_slot(self) -> usize {
        (self.0 & !Self::REMOTE) as usize
    }
}

/// Why a proposal was staged, beyond the writer's own write (docs/16 A13,
/// ref/09 §5.6). Only a relinquish carries something other than
/// [`Intent::Write`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// The writer's own write or relinquish.
    Write,
    /// The occupant's level freed so that `by` can take it over.
    Takeover {
        /// The principal taking the level.
        by: WriterId,
    },
    /// The occupant's level freed by `by` with `force`.
    ForcedRelease {
        /// The principal releasing it.
        by: WriterId,
    },
}

/// Where a proposal was staged from (docs/01 §4).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Drained from the async inbox in step 2.
    Async,
    /// Written by a cycle task in step 4.
    Cycle,
}

/// Task number of a proposal or method record that no cycle task staged
/// (async, API, bridges).
pub const NO_TASK: u16 = u16::MAX;

/// The proposal flag that relinquishes the writer's level (ref/04 §3.3,
/// `WRITE` flag bit 0); the value is then 0 and ignored.
pub const RELINQUISH: u16 = 1;

/// A proposed write, as staged during a tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// Who proposes.
    pub writer: WriterId,
    /// The output.
    pub signal: SignalId,
    /// Value bits in the output's type and unit (ref/00 §4.1).
    pub value: u64,
    /// `WRITE` flags of ref/04 §3.3 ([`RELINQUISH`]).
    pub flags: u16,
    /// Step it was staged in.
    pub source: Source,
    /// Opaque tag returned with the result, for async write tickets.
    pub ticket: u32,
    /// TTL of a manual write in ns, which is its lease (ref/09 §5.6); 0 uses
    /// the writer's declared TTL. Ignored for other writers.
    pub ttl_ns: u64,
    /// Takeover or forced release of another writer's level.
    pub intent: Intent,
    /// The cycle task that staged it, for its causal set (ADR 0043), or
    /// [`NO_TASK`].
    pub task: u16,
}

impl Proposal {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        writer: WriterId(0),
        signal: SignalId(0),
        value: 0,
        flags: 0,
        source: Source::Cycle,
        ticket: 0,
        ttl_ns: 0,
        intent: Intent::Write,
        task: NO_TASK,
    };

    /// True when this proposal relinquishes the writer's level.
    pub const fn is_relinquish(&self) -> bool {
        self.flags & RELINQUISH != 0
    }

    /// The occupant's relinquish and the new proposal that a takeover stages,
    /// in this order, into the same inbox (docs/16 A13, ref/09 §5.6).
    pub const fn takeover(occupant: WriterId, new: Proposal) -> [Proposal; 2] {
        [
            Proposal {
                writer: occupant,
                flags: RELINQUISH,
                value: 0,
                intent: Intent::Takeover { by: new.writer },
                ..new
            },
            new,
        ]
    }

    /// The relinquish that a forced release by `by` stages for `occupant`.
    pub const fn forced_release(occupant: WriterId, signal: SignalId, by: WriterId) -> Proposal {
        Proposal {
            writer: occupant,
            signal,
            flags: RELINQUISH,
            source: Source::Async,
            intent: Intent::ForcedRelease { by },
            ..Proposal::EMPTY
        }
    }
}

/// Most arguments a method call carries.
pub const MAX_METHOD_ARGS: usize = 8;

/// A method record: a call of a class method by a writer (docs/17 §4.1),
/// expanded in step 5a (ADR 0041).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MethodCall {
    /// The caller, whose writes the expansion produces.
    pub writer: WriterId,
    /// Instance number.
    pub instance: u32,
    /// Method number within the instance.
    pub method: u16,
    /// Argument values, in each parameter's declared unit.
    pub args: [u64; MAX_METHOD_ARGS],
    /// Number of arguments.
    pub n_args: u8,
    /// Step it was staged in.
    pub source: Source,
    /// Opaque tag returned with the result.
    pub ticket: u32,
    /// The cycle task that staged it, or [`NO_TASK`].
    pub task: u16,
    /// The TTL of a manual caller (docs/16 A10), which every write the call
    /// expands to carries; 0 for any other caller (ADR 0084).
    pub ttl_ns: u64,
}

impl MethodCall {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        writer: WriterId(0),
        instance: 0,
        method: 0,
        args: [0; MAX_METHOD_ARGS],
        n_args: 0,
        source: Source::Cycle,
        ticket: 0,
        task: NO_TASK,
        ttl_ns: 0,
    };

    /// The arguments.
    pub fn args(&self) -> &[u64] {
        &self.args[..usize::from(self.n_args).min(MAX_METHOD_ARGS)]
    }
}

/// The time facts of one executed tick (ADR 0032).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TickInfo {
    /// Tick index `k`, with `t_k = t_0 + k·T` (docs/01 §4).
    pub index: u64,
    /// The latch instant, which is `now()` for every task of the tick.
    pub at: Instant,
    /// `dt()`: whole periods since the previous executed tick, in ns.
    pub dt_ns: u64,
    /// First executed tick of this group in this generation.
    pub first: bool,
    /// At least one tick was skipped right before this one.
    pub after_skip: bool,
}

/// The samples a cycle task read in one tick (M3, ADR 0043).
#[derive(Copy, Clone, Debug)]
pub struct CausalSet<'x> {
    ports: &'x [SignalId],
    snapshot: &'x [Sample],
    bits: &'x [u64],
}

impl<'x> CausalSet<'x> {
    /// The signals read, in `SignalId` order, with their latched samples.
    pub fn iter(&self) -> impl Iterator<Item = (SignalId, &'x Sample)> + 'x {
        let (ports, snapshot, bits) = (self.ports, self.snapshot, self.bits);
        (0..ports.len())
            .filter(move |&i| bits.get(i / 64).is_some_and(|w| w & (1 << (i % 64)) != 0))
            .map(move |i| (ports[i], &snapshot[i]))
    }

    /// Number of signals read.
    pub fn len(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// True when the task read nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Why an image call was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// The storage slices do not have matching lengths.
    StorageMismatch,
    /// The ports are not strictly increasing `SignalId`s.
    PortsNotSorted,
    /// A port is not a signal of the registry or has no slot.
    UnknownPort(SignalId),
    /// The period is zero.
    ZeroPeriod,
    /// A tick index that does not come after the previous one.
    TickNotAfterPrevious {
        /// The refused index.
        index: u64,
        /// The previous executed index.
        previous: u64,
    },
    /// The call does not belong to the current step of the tick.
    WrongPhase,
    /// The staging buffer is full; the proposal was dropped and counted.
    StagingFull,
}

/// Fixed storage for one [`ProcessImage`].
#[derive(Debug)]
pub struct ImageStorage<'a> {
    /// Signals the group's tasks read, as strictly increasing `SignalId`s.
    pub ports: &'a [SignalId],
    /// One sample per port.
    pub snapshot: &'a mut [Sample],
    /// One sequence extender per port.
    pub extenders: &'a mut [SeqExtender],
    /// Staging buffer; its length is the capacity.
    pub proposals: &'a mut [Proposal],
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Latched,
    Cycle,
}

/// The process image of one cycle group.
#[derive(Debug)]
pub struct ProcessImage<'a> {
    group: GroupId,
    period_ns: u64,
    read_retries: u32,
    storage: ImageStorage<'a>,
    staged: usize,
    expanded: usize,
    methods: &'a mut [MethodCall],
    n_methods: usize,
    reads: &'a mut [u64],
    tasks: usize,
    phase: Phase,
    last_index: Option<u64>,
    tick: Option<TickInfo>,
    collisions: u64,
    staging_drops: u64,
}

impl<'a> ProcessImage<'a> {
    /// Default extra read attempts when the latch collides with a writer.
    pub const DEFAULT_READ_RETRIES: u32 = 4;

    /// An image for `group`, ticking every `period_ns`. Every port starts as
    /// [`Sample::INITIAL`].
    pub fn new(
        group: GroupId,
        period_ns: u64,
        storage: ImageStorage<'a>,
    ) -> Result<Self, ImageError> {
        if period_ns == 0 {
            return Err(ImageError::ZeroPeriod);
        }
        if storage.snapshot.len() != storage.ports.len()
            || storage.extenders.len() != storage.ports.len()
        {
            return Err(ImageError::StorageMismatch);
        }
        if storage.ports.windows(2).any(|w| w[0] >= w[1]) {
            return Err(ImageError::PortsNotSorted);
        }
        storage.snapshot.fill(Sample::INITIAL);
        storage.extenders.fill(SeqExtender::for_slot());
        Ok(Self {
            group,
            period_ns,
            read_retries: Self::DEFAULT_READ_RETRIES,
            storage,
            staged: 0,
            expanded: 0,
            methods: &mut [],
            n_methods: 0,
            reads: &mut [],
            tasks: 0,
            phase: Phase::Idle,
            last_index: None,
            tick: None,
            collisions: 0,
            staging_drops: 0,
        })
    }

    /// Sets the extra read attempts of the latch; 0 on a single-core MCU
    /// (ref/08 §2.4.1).
    pub fn set_read_retries(&mut self, retries: u32) {
        self.read_retries = retries;
    }

    /// The group.
    pub fn group(&self) -> GroupId {
        self.group
    }

    /// Nominal period in ns.
    pub fn period_ns(&self) -> u64 {
        self.period_ns
    }

    /// The ports, in `SignalId` order (the trace order of docs/16 §7).
    pub fn ports(&self) -> &[SignalId] {
        self.storage.ports
    }

    /// The latched samples, one per port.
    pub fn snapshot(&self) -> &[Sample] {
        self.storage.snapshot
    }

    /// The latched sample of a port signal.
    pub fn sample(&self, signal: SignalId) -> Option<&Sample> {
        self.storage
            .ports
            .binary_search(&signal)
            .ok()
            .map(|i| &self.storage.snapshot[i])
    }

    /// The current tick, between [`ProcessImage::latch`] and
    /// [`ProcessImage::end_tick`], or the last one after it.
    pub fn tick(&self) -> Option<TickInfo> {
        self.tick
    }

    /// Latch collisions so far: reads that kept the previous sample.
    pub fn collisions(&self) -> u64 {
        self.collisions
    }

    /// Proposals dropped because the staging buffer was full.
    pub fn staging_drops(&self) -> u64 {
        self.staging_drops
    }

    /// Step 1: starts tick `index` at instant `at` and copies the slots of the
    /// ports into the snapshot (docs/01 §3.4). `slots` is indexed by
    /// `SignalId`. Staleness is computed here from `max_age` (docs/01 §3.3).
    pub fn latch(
        &mut self,
        index: u64,
        at: Instant,
        slots: &[Slot],
        registry: &Registry<'_>,
    ) -> Result<TickInfo, ImageError> {
        if let Some(previous) = self.last_index
            && index <= previous
        {
            return Err(ImageError::TickNotAfterPrevious { index, previous });
        }
        for (i, &port) in self.storage.ports.iter().enumerate() {
            let slot = slots
                .get(port.index())
                .ok_or(ImageError::UnknownPort(port))?;
            let meta = registry.signal(port).ok_or(ImageError::UnknownPort(port))?;
            match slot.read(self.read_retries) {
                Ok(sample) => {
                    let seq = self.storage.extenders[i].extend(sample.seq());
                    self.storage.snapshot[i] = sample.with_seq(seq);
                }
                Err(_) => self.collisions = self.collisions.saturating_add(1),
            }
            if let Some(max_age) = meta.max_age_ns {
                self.storage.snapshot[i] = age(self.storage.snapshot[i], at.mono_ns, max_age);
            }
        }
        let (dt_ns, first, after_skip) = match self.last_index {
            None => (self.period_ns, true, false),
            Some(previous) => {
                let elapsed = index - previous;
                (elapsed.saturating_mul(self.period_ns), false, elapsed > 1)
            }
        };
        let info = TickInfo {
            index,
            at,
            dt_ns,
            first,
            after_skip,
        };
        self.last_index = Some(index);
        self.tick = Some(info);
        self.staged = 0;
        self.expanded = 0;
        self.n_methods = 0;
        self.reads.fill(0);
        self.phase = Phase::Latched;
        Ok(info)
    }

    /// Step 2: stages a proposal drained from the async inbox. Only allowed
    /// before the first cycle proposal of the tick, so async proposals always
    /// come first (docs/16 A4).
    pub fn stage_async(&mut self, proposal: Proposal) -> Result<(), ImageError> {
        if self.expanded > 0 || self.phase != Phase::Latched {
            return Err(ImageError::WrongPhase);
        }
        self.push(Proposal {
            source: Source::Async,
            ..proposal
        })
    }

    /// Step 4: stages a proposal from a cycle task.
    pub fn stage_cycle(&mut self, proposal: Proposal) -> Result<(), ImageError> {
        if self.expanded > 0 || self.phase == Phase::Idle {
            return Err(ImageError::WrongPhase);
        }
        self.phase = Phase::Cycle;
        self.push(Proposal {
            source: Source::Cycle,
            ..proposal
        })
    }

    /// Hands in the read bitsets of `tasks` cycle tasks (ADR 0043): at least
    /// `tasks × ceil(ports / 64)` words.
    pub fn set_read_sets(&mut self, buffer: &'a mut [u64], tasks: usize) -> Result<(), ImageError> {
        if buffer.len() < tasks * self.words_per_task() {
            return Err(ImageError::StorageMismatch);
        }
        buffer.fill(0);
        self.reads = buffer;
        self.tasks = tasks;
        Ok(())
    }

    fn words_per_task(&self) -> usize {
        self.storage.ports.len().div_ceil(64)
    }

    /// Marks that cycle task `task` read port `port` (an index into
    /// [`ProcessImage::ports`]) in this tick. Returns false if out of range.
    pub fn mark_read(&mut self, task: u16, port: usize) -> bool {
        let words = self.words_per_task();
        if usize::from(task) >= self.tasks || port >= self.storage.ports.len() {
            return false;
        }
        self.reads[usize::from(task) * words + port / 64] |= 1 << (port % 64);
        true
    }

    /// Marks a read by signal. Returns false if the signal is not a port.
    pub fn mark_read_signal(&mut self, task: u16, signal: SignalId) -> bool {
        match self.storage.ports.binary_search(&signal) {
            Ok(port) => self.mark_read(task, port),
            Err(_) => false,
        }
    }

    /// The causal set of a cycle task in this tick: the ports it read, with
    /// their latched samples.
    pub fn read_set(&self, task: u16) -> Option<CausalSet<'_>> {
        let words = self.words_per_task();
        if usize::from(task) >= self.tasks {
            return None;
        }
        let start = usize::from(task) * words;
        Some(CausalSet {
            ports: self.storage.ports,
            snapshot: self.storage.snapshot,
            bits: &self.reads[start..start + words],
        })
    }

    /// Hands in the buffer for method records; its length is the capacity.
    pub fn set_method_buffer(&mut self, buffer: &'a mut [MethodCall]) {
        self.methods = buffer;
        self.n_methods = 0;
    }

    /// Stages a method record: an async one only before the first cycle
    /// proposal or record (step 2), a cycle one in step 4.
    pub fn stage_method(&mut self, call: MethodCall) -> Result<(), ImageError> {
        if self.expanded > 0 {
            return Err(ImageError::WrongPhase);
        }
        match (call.source, self.phase) {
            (_, Phase::Idle) | (Source::Async, Phase::Cycle) => return Err(ImageError::WrongPhase),
            (Source::Cycle, _) => self.phase = Phase::Cycle,
            (Source::Async, Phase::Latched) => {}
        }
        match self.methods.get_mut(self.n_methods) {
            Some(slot) => {
                *slot = call;
                self.n_methods += 1;
                Ok(())
            }
            None => {
                self.staging_drops = self.staging_drops.saturating_add(1);
                Err(ImageError::StagingFull)
            }
        }
    }

    /// The method records of this tick, in staging order.
    pub fn method_calls(&self) -> &[MethodCall] {
        &self.methods[..self.n_methods]
    }

    /// Step 5a: appends a write expanded from a method record, after every
    /// direct proposal (ADR 0041 §1).
    pub fn push_expanded(&mut self, proposal: Proposal) -> Result<(), ImageError> {
        if self.phase == Phase::Idle {
            return Err(ImageError::WrongPhase);
        }
        self.push(proposal)?;
        self.expanded += 1;
        Ok(())
    }

    /// The proposals staged directly (steps 2 and 4), without the writes
    /// expanded from method records: what the trace records.
    pub fn direct_proposals(&self) -> &[Proposal] {
        &self.storage.proposals[..self.staged - self.expanded]
    }

    fn push(&mut self, proposal: Proposal) -> Result<(), ImageError> {
        match self.storage.proposals.get_mut(self.staged) {
            Some(slot) => {
                *slot = proposal;
                self.staged += 1;
                Ok(())
            }
            None => {
                self.staging_drops = self.staging_drops.saturating_add(1);
                Err(ImageError::StagingFull)
            }
        }
    }

    /// The proposals of this tick, in order: async first, then cycle tasks in
    /// their declared order, then the writes expanded from method records
    /// (steps 5–7 consume them).
    pub fn proposals(&self) -> &[Proposal] {
        &self.storage.proposals[..self.staged]
    }

    /// Ends the tick after the commit; staging is cleared at the next latch.
    pub fn end_tick(&mut self) -> Result<(), ImageError> {
        if self.phase == Phase::Idle {
            return Err(ImageError::WrongPhase);
        }
        self.phase = Phase::Idle;
        Ok(())
    }
}

fn age(sample: Sample, now_ns: u64, max_age_ns: u64) -> Sample {
    let quality = sample.quality();
    if sample.seq() == 0 || quality.is_bad() {
        return sample;
    }
    let age = now_ns.saturating_sub(sample.ts_acq());
    let aged = if age > max_age_ns.saturating_mul(2) {
        Quality::with_flags(QualityCode::Bad, QualityReason::Stale, quality.flags())
    } else if age > max_age_ns && quality.is_good() {
        Quality::with_flags(
            QualityCode::Uncertain,
            QualityReason::Stale,
            quality.flags(),
        )
    } else {
        return sample;
    };
    Sample::new(
        sample.transport_bits(),
        aged,
        sample.ts_src_raw(),
        sample.ts_acq(),
        sample.seq(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{Alias, GroupMeta, RegistryBuilder, SignalMeta, UnitRef};
    use crate::value::{NO_TS_SRC, ValueType};

    const MS: u64 = 1_000_000;

    fn registry<'a>(
        s: &'a mut [SignalMeta<'a>],
        a: &'a mut [Alias<'a>],
        g: &'a mut [GroupMeta<'a>],
    ) -> Registry<'a> {
        let mut b = RegistryBuilder::new(s, a, g);
        b.group(GroupMeta {
            name: "fast",
            period_ns: 100 * MS,
        })
        .unwrap();
        b.signal(SignalMeta::input("x.a", ValueType::F64, UnitRef("kW")))
            .unwrap();
        let mut aged = SignalMeta::input("x.b", ValueType::F64, UnitRef("kW"));
        aged.max_age_ns = Some(100 * MS);
        b.signal(aged).unwrap();
        b.signal(SignalMeta::output(
            "x.o",
            ValueType::F64,
            UnitRef("kW"),
            "fast",
        ))
        .unwrap();
        b.build().unwrap()
    }

    fn proposal(value: u64) -> Proposal {
        Proposal {
            writer: WriterId(1),
            signal: SignalId(2),
            value,
            ..Proposal::EMPTY
        }
    }

    #[test]
    fn latch_copies_slots_and_keeps_them_for_the_tick() {
        let (mut s, mut a, mut g) = (
            [SignalMeta::EMPTY; 3],
            [Alias::EMPTY; 0],
            [GroupMeta::EMPTY; 1],
        );
        let reg = registry(&mut s, &mut a, &mut g);
        let slots: [Slot; 3] = core::array::from_fn(|_| Slot::new());
        let ports = [SignalId(0), SignalId(1)];
        let mut snap = [Sample::INITIAL; 2];
        let mut ext = [SeqExtender::for_slot(); 2];
        let mut props = [Proposal::EMPTY; 2];
        let mut img = ProcessImage::new(
            GroupId(0),
            100 * MS,
            ImageStorage {
                ports: &ports,
                snapshot: &mut snap,
                extenders: &mut ext,
                proposals: &mut props,
            },
        )
        .unwrap();
        assert_eq!(img.sample(SignalId(0)), Some(&Sample::INITIAL));
        slots[0].write(7, Quality::GOOD, NO_TS_SRC, 5 * MS);
        let info = img.latch(0, Instant::mono(10 * MS), &slots, &reg).unwrap();
        assert!(info.first);
        assert_eq!(info.dt_ns, 100 * MS);
        slots[0].write(8, Quality::GOOD, NO_TS_SRC, 6 * MS);
        assert_eq!(img.sample(SignalId(0)).unwrap().get(), Ok(7));
        assert_eq!(img.sample(SignalId(0)).unwrap().seq(), 1);
        assert_eq!(img.sample(SignalId(1)), Some(&Sample::INITIAL));
        img.end_tick().unwrap();
        img.latch(1, Instant::mono(110 * MS), &slots, &reg).unwrap();
        assert_eq!(img.sample(SignalId(0)).unwrap().get(), Ok(8));
        assert_eq!(img.sample(SignalId(0)).unwrap().seq(), 2);
    }

    #[test]
    fn staging_orders_async_before_cycle_and_clears_each_tick() {
        let (mut s, mut a, mut g) = (
            [SignalMeta::EMPTY; 3],
            [Alias::EMPTY; 0],
            [GroupMeta::EMPTY; 1],
        );
        let reg = registry(&mut s, &mut a, &mut g);
        let slots: [Slot; 3] = core::array::from_fn(|_| Slot::new());
        let mut snap: [Sample; 0] = [];
        let mut ext: [SeqExtender; 0] = [];
        let mut props = [Proposal::EMPTY; 2];
        let mut img = ProcessImage::new(
            GroupId(0),
            100 * MS,
            ImageStorage {
                ports: &[],
                snapshot: &mut snap,
                extenders: &mut ext,
                proposals: &mut props,
            },
        )
        .unwrap();
        assert_eq!(img.stage_cycle(proposal(1)), Err(ImageError::WrongPhase));
        img.latch(0, Instant::mono(0), &slots, &reg).unwrap();
        img.stage_async(proposal(1)).unwrap();
        img.stage_cycle(proposal(2)).unwrap();
        assert_eq!(img.stage_async(proposal(3)), Err(ImageError::WrongPhase));
        assert_eq!(img.stage_cycle(proposal(4)), Err(ImageError::StagingFull));
        assert_eq!(img.staging_drops(), 1);
        let staged = img.proposals();
        assert_eq!(staged.len(), 2);
        assert_eq!((staged[0].value, staged[0].source), (1, Source::Async));
        assert_eq!((staged[1].value, staged[1].source), (2, Source::Cycle));
        img.end_tick().unwrap();
        img.latch(1, Instant::mono(100 * MS), &slots, &reg).unwrap();
        assert!(img.proposals().is_empty());
    }

    #[test]
    fn dt_counts_whole_periods_after_a_skip() {
        let (mut s, mut a, mut g) = (
            [SignalMeta::EMPTY; 3],
            [Alias::EMPTY; 0],
            [GroupMeta::EMPTY; 1],
        );
        let reg = registry(&mut s, &mut a, &mut g);
        let slots: [Slot; 3] = core::array::from_fn(|_| Slot::new());
        let mut snap: [Sample; 0] = [];
        let mut ext: [SeqExtender; 0] = [];
        let mut props: [Proposal; 0] = [];
        let mut img = ProcessImage::new(
            GroupId(0),
            100 * MS,
            ImageStorage {
                ports: &[],
                snapshot: &mut snap,
                extenders: &mut ext,
                proposals: &mut props,
            },
        )
        .unwrap();
        let t0 = img.latch(5, Instant::mono(500 * MS), &slots, &reg).unwrap();
        assert_eq!((t0.dt_ns, t0.first, t0.after_skip), (100 * MS, true, false));
        img.end_tick().unwrap();
        let t1 = img.latch(6, Instant::mono(603 * MS), &slots, &reg).unwrap();
        assert_eq!(
            (t1.dt_ns, t1.first, t1.after_skip),
            (100 * MS, false, false)
        );
        img.end_tick().unwrap();
        let t2 = img.latch(8, Instant::mono(800 * MS), &slots, &reg).unwrap();
        assert_eq!((t2.dt_ns, t2.first, t2.after_skip), (200 * MS, false, true));
        img.end_tick().unwrap();
        assert_eq!(
            img.latch(8, Instant::mono(900 * MS), &slots, &reg),
            Err(ImageError::TickNotAfterPrevious {
                index: 8,
                previous: 8
            })
        );
    }

    #[test]
    fn staleness_is_computed_at_latch() {
        let (mut s, mut a, mut g) = (
            [SignalMeta::EMPTY; 3],
            [Alias::EMPTY; 0],
            [GroupMeta::EMPTY; 1],
        );
        let reg = registry(&mut s, &mut a, &mut g);
        let slots: [Slot; 3] = core::array::from_fn(|_| Slot::new());
        let ports = [SignalId(1)];
        let mut snap = [Sample::INITIAL; 1];
        let mut ext = [SeqExtender::for_slot(); 1];
        let mut props: [Proposal; 0] = [];
        let mut img = ProcessImage::new(
            GroupId(0),
            100 * MS,
            ImageStorage {
                ports: &ports,
                snapshot: &mut snap,
                extenders: &mut ext,
                proposals: &mut props,
            },
        )
        .unwrap();
        slots[1].write(1, Quality::GOOD, NO_TS_SRC, 0);
        let mut check = |index: u64, now: u64, code: QualityCode, reason: QualityReason| {
            img.latch(index, Instant::mono(now), &slots, &reg).unwrap();
            let q = img.sample(SignalId(1)).unwrap().quality();
            assert_eq!((q.code(), q.reason()), (code, reason));
            img.end_tick().unwrap();
        };
        check(0, 100 * MS, QualityCode::Good, QualityReason::None);
        check(1, 101 * MS, QualityCode::Uncertain, QualityReason::Stale);
        check(2, 201 * MS, QualityCode::Bad, QualityReason::Stale);
        slots[1].write(2, Quality::GOOD, NO_TS_SRC, 250 * MS);
        check(3, 300 * MS, QualityCode::Good, QualityReason::None);
    }

    #[test]
    fn rejects_bad_storage() {
        let mut snap = [Sample::INITIAL; 1];
        let mut ext = [SeqExtender::for_slot(); 2];
        let mut props: [Proposal; 0] = [];
        let ports = [SignalId(0), SignalId(1)];
        let err = ProcessImage::new(
            GroupId(0),
            1,
            ImageStorage {
                ports: &ports,
                snapshot: &mut snap,
                extenders: &mut ext,
                proposals: &mut props,
            },
        )
        .unwrap_err();
        assert_eq!(err, ImageError::StorageMismatch);
        let mut snap = [Sample::INITIAL; 2];
        let ports = [SignalId(1), SignalId(1)];
        let err = ProcessImage::new(
            GroupId(0),
            1,
            ImageStorage {
                ports: &ports,
                snapshot: &mut snap,
                extenders: &mut ext,
                proposals: &mut props,
            },
        )
        .unwrap_err();
        assert_eq!(err, ImageError::PortsNotSorted);
    }
}
