//! The arbiter of one output: 16 levels with leases (docs/01 §6, docs/16 §1,
//! ADR 0011).
//!
//! Each tick, the owner group first expires the leases that ended at its
//! latch (docs/16 A2), then takes the staged proposals in order (A4), then
//! asks for the active level (A5, A6). The envelope runs on the result.

use crate::image::{Proposal, WriterId};
use crate::output::{
    Band, ConfigError, DEFAULT_LEASE_PERIODS, Level, OutputConfig, SECOND_NS, WriterConfig,
    WriterKind,
};

/// What a level holds: the writer's last proposal and its lease.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LevelEntry {
    /// The writer occupying the level.
    pub writer: WriterId,
    /// Its last proposed value bits.
    pub value: u64,
    /// Monotonic instant at which the lease expires (docs/16 A2).
    pub lease_until: u64,
}

/// Why the arbiter refused a proposal. Neither renews a lease (docs/16 A3).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The writer may not write this output, or a manual level is held by
    /// another writer (docs/16 A13).
    NotOwner,
    /// The value does not serve the output's type (docs/16 E10).
    WrongType,
}

/// A remote writer of an MCU: one (link, level) pair (docs/16 A12). Its
/// [`WriterId`] is `WriterId::remote(slot)`, with `slot` its index in the
/// remote table.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RemoteWriter {
    /// The delegated level it proposes in.
    pub level: Level,
    /// The lease it asks for.
    pub lease_ns: u64,
    /// The link session's `max_lease`; the effective lease is the smaller.
    pub link_max_lease_ns: u64,
}

/// What the arbiter did with one proposal.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Intake {
    /// The proposal now occupies `level`, with a renewed lease.
    Placed(Level),
    /// The writer's level was freed (docs/01 §6 rule 3); `None` if it held
    /// none.
    Relinquished(Option<Level>),
    /// The proposal was refused.
    Refused(Refusal),
}

/// The arbiter state of one output.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutputArbiter {
    levels: [Option<LevelEntry>; Level::COUNT],
    period_ns: u64,
}

impl OutputArbiter {
    /// An arbiter with no active level, for an output whose owner group
    /// ticks every `period_ns`.
    pub const fn new(period_ns: u64) -> Self {
        Self {
            levels: [None; Level::COUNT],
            period_ns,
        }
    }

    /// The lease a writer gets on this output: its declared lease, or three
    /// periods of the owner group (docs/16 A9).
    pub fn lease_of(&self, writer: &WriterConfig) -> u64 {
        writer
            .lease_ns
            .unwrap_or(self.period_ns.saturating_mul(DEFAULT_LEASE_PERIODS))
    }

    /// Frees every level whose lease ended at `t_latch` (docs/16 A2) and
    /// returns a bit mask of the freed levels (bit `n-1` for level `n`).
    pub fn expire(&mut self, t_latch: u64) -> u16 {
        self.expire_each(t_latch, |_, _| {})
    }

    /// Like [`OutputArbiter::expire`], calling `freed` with each freed level
    /// and what it held, from the highest priority.
    pub fn expire_each(&mut self, t_latch: u64, mut freed: impl FnMut(Level, LevelEntry)) -> u16 {
        let mut mask = 0;
        for (i, level) in self.levels.iter_mut().enumerate() {
            if let Some(entry) = *level
                && t_latch >= entry.lease_until
            {
                *level = None;
                mask |= 1 << i;
                if let Some(l) = Level::new(i as u8 + 1) {
                    freed(l, entry);
                }
            }
        }
        mask
    }

    /// Puts back a level saved before a restart (docs/16 A14), before the
    /// first tick. It replaces whatever the level holds.
    pub fn restore(&mut self, level: Level, entry: LevelEntry) {
        self.levels[level.index()] = Some(entry);
    }

    /// Takes one proposal of a remote writer (docs/16 A12). The level
    /// belongs to the level and not to the writer: any remote writer may
    /// replace or relinquish it, so a new generation of the Linux node, which
    /// renumbers its origins, keeps the level. A level that is not delegated
    /// is `NotOwner`. The lease is the smaller of the writer's and the
    /// link's `max_lease`, and counts from this tick of the consumer.
    pub fn propose_remote(
        &mut self,
        output: &OutputConfig<'_>,
        remote: &RemoteWriter,
        proposal: &Proposal,
        t_tick: u64,
    ) -> Intake {
        if !output.delegates(remote.level) {
            return Intake::Refused(Refusal::NotOwner);
        }
        let slot = &mut self.levels[remote.level.index()];
        if slot.is_some_and(|e| !e.writer.is_remote()) {
            return Intake::Refused(Refusal::NotOwner);
        }
        if proposal.is_relinquish() {
            return Intake::Relinquished(slot.take().map(|_| remote.level));
        }
        if !output.accepts(proposal.value) {
            return Intake::Refused(Refusal::WrongType);
        }
        let lease = remote.lease_ns.min(remote.link_max_lease_ns);
        *slot = Some(LevelEntry {
            writer: proposal.writer,
            value: proposal.value,
            lease_until: t_tick.saturating_add(lease),
        });
        Intake::Placed(remote.level)
    }

    /// Takes one proposal at tick instant `t_tick`. `writers` is the writer
    /// table, indexed by `WriterId`. A later proposal of the same writer in
    /// the same tick replaces an earlier one (docs/16 A4).
    pub fn propose(
        &mut self,
        output: &OutputConfig<'_>,
        writers: &[WriterConfig],
        proposal: &Proposal,
        t_tick: u64,
    ) -> Intake {
        let Some(writer) = writers.get(proposal.writer.index()) else {
            return Intake::Refused(Refusal::NotOwner);
        };
        if !output.writers.allows(proposal.writer, writer.kind) {
            return Intake::Refused(Refusal::NotOwner);
        }
        let mut lease = self.lease_of(writer);
        let slot = &mut self.levels[writer.level.index()];
        if let Some(entry) = slot
            && entry.writer != proposal.writer
        {
            return Intake::Refused(Refusal::NotOwner);
        }
        if proposal.is_relinquish() {
            let held = slot.take().map(|_| writer.level);
            return Intake::Relinquished(held);
        }
        if !output.accepts(proposal.value) {
            return Intake::Refused(Refusal::WrongType);
        }
        if proposal.ttl_ns != 0 && writer.kind == WriterKind::Manual {
            lease = proposal.ttl_ns.min(output.max_manual_ns);
        }
        *slot = Some(LevelEntry {
            writer: proposal.writer,
            value: proposal.value,
            lease_until: t_tick.saturating_add(lease),
        });
        Intake::Placed(writer.level)
    }

    /// The active level of highest priority and what it holds (docs/16 A5);
    /// `None` means the output goes to `safe` (A6).
    pub fn active(&self) -> Option<(Level, LevelEntry)> {
        self.levels
            .iter()
            .enumerate()
            .find_map(|(i, entry)| entry.map(|e| (Level::new(i as u8 + 1), e)))
            .and_then(|(level, entry)| level.map(|l| (l, entry)))
    }

    /// What a level holds.
    pub fn level(&self, level: Level) -> Option<LevelEntry> {
        self.levels[level.index()]
    }

    /// The level held by `writer`, if any.
    pub fn level_of(&self, writer: WriterId) -> Option<Level> {
        self.levels
            .iter()
            .position(|e| e.is_some_and(|e| e.writer == writer))
            .and_then(|i| Level::new(i as u8 + 1))
    }
}

/// The first two non-manual writers that share a level on `output`
/// (`J0210`, docs/16 A5), in writer order.
pub fn find_level_conflict(
    output: &OutputConfig<'_>,
    writers: &[WriterConfig],
) -> Option<(WriterId, WriterId, Level)> {
    let mut seen: [Option<WriterId>; Level::COUNT] = [None; Level::COUNT];
    for (i, w) in writers.iter().enumerate() {
        let id = WriterId(i as u32);
        if w.kind == WriterKind::Manual || !output.writers.allows(id, w.kind) {
            continue;
        }
        match seen[w.level.index()] {
            Some(first) => return Some((first, id, w.level)),
            None => seen[w.level.index()] = Some(id),
        }
    }
    None
}

/// Checks the writer table against one output (docs/16 A5, A9, A10;
/// ref/01 §3.11): only people in the manual band (`J0252`, `J-E1104`), no
/// local writer on a level delegated to the link (`J0210`), no two non-manual
/// writers on one level (`J0210`), leases within `max_lease` (`J0213`), and a
/// TTL for every manual writer within `max_manual`.
pub fn check_writers(
    output: &OutputConfig<'_>,
    writers: &[WriterConfig],
    period_ns: u64,
) -> Result<(), ConfigError> {
    let err = |code, writer: Option<usize>| ConfigError {
        code,
        output: Some(output.signal),
        writer: writer.map(|w| WriterId(w as u32)),
    };
    let mut taken = [false; Level::COUNT];
    for (i, w) in writers.iter().enumerate() {
        if !output.writers.allows(WriterId(i as u32), w.kind) {
            continue;
        }
        let manual = w.kind == WriterKind::Manual;
        match (manual, w.level.band() == Band::Manual) {
            (false, true) => return Err(err("J0252", Some(i))),
            (true, false) => return Err(err("J-E1104", Some(i))),
            _ => {}
        }
        if manual {
            match w.lease_ns {
                None | Some(0) => return Err(err("J-E1101", Some(i))),
                Some(ttl) if ttl > output.max_manual_ns => return Err(err("J-E1102", Some(i))),
                Some(_) => {}
            }
            continue;
        }
        let lease = w
            .lease_ns
            .unwrap_or(period_ns.saturating_mul(DEFAULT_LEASE_PERIODS));
        if lease > output.max_lease_ns {
            return Err(err("J0213", Some(i)));
        }
        if output.delegates(w.level) || core::mem::replace(&mut taken[w.level.index()], true) {
            return Err(err("J0210", Some(i)));
        }
    }
    Ok(())
}

/// Checks the TTL of a manual write before it is staged (docs/16 A10,
/// ref/09 §5.6): mandatory (`J-E1101`), at least 1 s (`J-E1103`) and at most
/// `max_manual` (`J-E1102`).
pub fn check_manual_ttl(
    output: &OutputConfig<'_>,
    ttl_ns: Option<u64>,
) -> Result<u64, ConfigError> {
    let err = |code| ConfigError {
        code,
        output: Some(output.signal),
        writer: None,
    };
    match ttl_ns {
        None | Some(0) => Err(err("J-E1101")),
        Some(ttl) if ttl < SECOND_NS => Err(err("J-E1103")),
        Some(ttl) if ttl > output.max_manual_ns => Err(err("J-E1102")),
        Some(ttl) => Ok(ttl),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Proposal, RELINQUISH};
    use crate::output::{ClockRequirement, SafeValue, Writers};
    use crate::registry::{GroupId, SignalId};
    use crate::time::Instant;
    use crate::time::{Clock, VirtualClock};
    use crate::value::ValueType;

    const MS: u64 = 1_000_000;
    const PERIOD: u64 = 100 * MS;

    fn writer(kind: WriterKind, level: &str, lease_ms: Option<u64>) -> WriterConfig {
        WriterConfig {
            kind,
            level: Level::parse(level).unwrap(),
            lease_ns: lease_ms.map(|l| l * MS),
            requires_clock: ClockRequirement::Any,
        }
    }

    fn table() -> [WriterConfig; 4] {
        [
            writer(WriterKind::Cycle, "optimizer:13", Some(2_000)),
            writer(WriterKind::Cycle, "safety:1", None),
            writer(WriterKind::Manual, "manual:4", Some(60_000)),
            writer(WriterKind::Manual, "manual:4", Some(60_000)),
        ]
    }

    fn output() -> OutputConfig<'static> {
        OutputConfig::new(
            SignalId(0),
            ValueType::F64,
            GroupId(0),
            SafeValue::Value(0f64.to_bits()),
        )
    }

    fn p(writer: u32, value: f64) -> Proposal {
        Proposal {
            writer: WriterId(writer),
            value: value.to_bits(),
            ..Proposal::EMPTY
        }
    }

    fn active_value(arb: &OutputArbiter) -> Option<(u8, f64)> {
        arb.active()
            .map(|(l, e)| (l.get(), f64::from_bits(e.value)))
    }

    #[test]
    fn highest_active_level_wins_and_none_means_safe() {
        let (out, w) = (output(), table());
        let mut arb = OutputArbiter::new(PERIOD);
        assert_eq!(arb.active(), None);
        assert_eq!(
            arb.propose(&out, &w, &p(0, 5.0), 0),
            Intake::Placed(Level::new(13).unwrap())
        );
        assert_eq!(active_value(&arb), Some((13, 5.0)));
        assert_eq!(
            arb.propose(&out, &w, &p(1, 1.0), 0),
            Intake::Placed(Level::new(1).unwrap())
        );
        assert_eq!(active_value(&arb), Some((1, 1.0)));
    }

    #[test]
    fn lease_expires_at_latch_and_is_renewed_by_proposals() {
        let (out, w) = (output(), table());
        let mut arb = OutputArbiter::new(PERIOD);
        arb.propose(&out, &w, &p(0, 5.0), 0);
        assert_eq!(arb.expire(1_900 * MS), 0);
        arb.propose(&out, &w, &p(0, 6.0), 1_900 * MS);
        assert_eq!(arb.expire(2_000 * MS), 0);
        assert_eq!(arb.expire(3_900 * MS), 1 << 12);
        assert_eq!(arb.active(), None);
    }

    #[test]
    fn default_lease_is_three_periods() {
        let (out, w) = (output(), table());
        let mut arb = OutputArbiter::new(PERIOD);
        arb.propose(&out, &w, &p(1, 1.0), 0);
        assert_eq!(
            arb.level(Level::new(1).unwrap()).unwrap().lease_until,
            300 * MS
        );
        assert_eq!(arb.expire(299 * MS), 0);
        assert_eq!(arb.expire(300 * MS), 1);
    }

    #[test]
    fn last_proposal_of_a_writer_in_a_tick_wins() {
        let (out, w) = (output(), table());
        let mut arb = OutputArbiter::new(PERIOD);
        arb.propose(&out, &w, &p(0, 1.0), 0);
        arb.propose(&out, &w, &p(0, 2.0), 0);
        assert_eq!(active_value(&arb), Some((13, 2.0)));
    }

    #[test]
    fn wrong_type_and_not_owner_do_not_renew() {
        let mut out = output();
        let w = table();
        let mut arb = OutputArbiter::new(PERIOD);
        arb.propose(&out, &w, &p(0, 5.0), 0);
        assert_eq!(
            arb.propose(&out, &w, &p(0, f64::NAN), 1_000 * MS),
            Intake::Refused(Refusal::WrongType)
        );
        assert_eq!(
            arb.level(Level::new(13).unwrap()).unwrap().lease_until,
            2_000 * MS
        );
        assert_eq!(
            arb.propose(&out, &w, &p(9, 1.0), 0),
            Intake::Refused(Refusal::NotOwner)
        );
        let only = [WriterId(1)];
        out.writers = Writers::Only(&only);
        assert_eq!(
            arb.propose(&out, &w, &p(0, 1.0), 0),
            Intake::Refused(Refusal::NotOwner)
        );
        assert!(matches!(
            arb.propose(&out, &w, &p(2, 1.0), 0),
            Intake::Placed(_)
        ));
    }

    #[test]
    fn relinquish_frees_the_level_now() {
        let (out, w) = (output(), table());
        let mut arb = OutputArbiter::new(PERIOD);
        arb.propose(&out, &w, &p(0, 5.0), 0);
        let release = Proposal {
            flags: RELINQUISH,
            ..p(0, 0.0)
        };
        assert_eq!(
            arb.propose(&out, &w, &release, 0),
            Intake::Relinquished(Level::new(13))
        );
        assert_eq!(arb.active(), None);
        assert_eq!(
            arb.propose(&out, &w, &release, 0),
            Intake::Relinquished(None)
        );
    }

    #[test]
    fn manual_level_belongs_to_one_principal_until_released() {
        let (out, w) = (output(), table());
        let mut arb = OutputArbiter::new(PERIOD);
        arb.propose(&out, &w, &p(2, 7.0), 0);
        assert_eq!(
            arb.propose(&out, &w, &p(3, 8.0), 0),
            Intake::Refused(Refusal::NotOwner)
        );
        assert!(matches!(
            arb.propose(&out, &w, &p(2, 9.0), 0),
            Intake::Placed(_)
        ));
        let takeover = Proposal {
            flags: RELINQUISH,
            ..p(2, 0.0)
        };
        arb.propose(&out, &w, &takeover, 0);
        assert!(matches!(
            arb.propose(&out, &w, &p(3, 8.0), 0),
            Intake::Placed(_)
        ));
        assert_eq!(arb.level_of(WriterId(3)), Some(Level::MANUAL_DEFAULT));
    }

    #[test]
    fn manual_write_with_ttl_beats_the_app_and_expires_by_itself() {
        let (out, w) = (output(), table());
        let clock = VirtualClock::new(Instant::mono(0));
        let mut arb = OutputArbiter::new(PERIOD);
        let manual = Proposal {
            ttl_ns: 1_000 * MS,
            ..p(2, 42.0)
        };
        let mut seen = [0u8; 40];
        for (tick, seen) in seen.iter_mut().enumerate() {
            let now = clock.now().mono_ns;
            arb.expire(now);
            arb.propose(&out, &w, &p(0, 5.0), now);
            if tick == 10 {
                arb.propose(&out, &w, &manual, now);
            }
            *seen = arb.active().map_or(0, |(l, _)| l.get());
            clock.advance(PERIOD);
        }
        assert!(seen[..10].iter().all(|&l| l == 13));
        assert!(seen[10..20].iter().all(|&l| l == 4));
        assert!(seen[20..].iter().all(|&l| l == 13));
    }

    #[test]
    fn manual_ttl_is_bounded_by_max_manual() {
        let (mut out, w) = (output(), table());
        out.max_manual_ns = 500 * MS;
        let mut arb = OutputArbiter::new(PERIOD);
        let manual = Proposal {
            ttl_ns: 1_000 * MS,
            ..p(2, 1.0)
        };
        arb.propose(&out, &w, &manual, 0);
        assert_eq!(
            arb.level(Level::MANUAL_DEFAULT).unwrap().lease_until,
            500 * MS
        );
        assert_eq!(
            check_manual_ttl(&out, Some(1_000 * MS)).unwrap_err().code,
            "J-E1102"
        );
        assert_eq!(
            check_manual_ttl(&out, Some(10 * MS)).unwrap_err().code,
            "J-E1103"
        );
        assert_eq!(check_manual_ttl(&out, None).unwrap_err().code, "J-E1101");
        assert_eq!(
            check_manual_ttl(&out, Some(SECOND_NS / 2))
                .unwrap_err()
                .code,
            "J-E1103"
        );
    }

    #[test]
    fn writer_table_checks() {
        let out = output();
        assert_eq!(check_writers(&out, &table(), PERIOD), Ok(()));
        let mut w = table();
        w[1].level = Level::new(13).unwrap();
        assert_eq!(check_writers(&out, &w, PERIOD).unwrap_err().code, "J0210");
        let mut w = table();
        w[0].lease_ns = Some(16 * 60 * SECOND_NS);
        assert_eq!(check_writers(&out, &w, PERIOD).unwrap_err().code, "J0213");
        let mut w = table();
        w[0].level = Level::MANUAL_DEFAULT;
        assert_eq!(check_writers(&out, &w, PERIOD).unwrap_err().code, "J0252");
        let mut w = table();
        w[2].level = Level::new(9).unwrap();
        assert_eq!(check_writers(&out, &w, PERIOD).unwrap_err().code, "J-E1104");
        let mut w = table();
        w[2].lease_ns = None;
        assert_eq!(check_writers(&out, &w, PERIOD).unwrap_err().code, "J-E1101");
    }

    #[test]
    fn remote_writers_own_delegated_levels_by_level() {
        let mut out = output();
        let w = table();
        let mut arb = OutputArbiter::new(PERIOD);
        let remote = RemoteWriter {
            level: Level::new(9).unwrap(),
            lease_ns: 5_000 * MS,
            link_max_lease_ns: 1_000 * MS,
        };
        let from = |slot: u32, value: f64| Proposal {
            writer: WriterId::remote(slot),
            value: value.to_bits(),
            ..Proposal::EMPTY
        };
        assert_eq!(
            arb.propose_remote(&out, &remote, &from(0, 1.0), 0),
            Intake::Refused(Refusal::NotOwner)
        );
        out.remote_levels = 1 << 8;
        assert_eq!(
            arb.propose_remote(&out, &remote, &from(0, 1.0), 0),
            Intake::Placed(Level::new(9).unwrap())
        );
        assert_eq!(
            arb.level(Level::new(9).unwrap()).unwrap().lease_until,
            1_000 * MS
        );
        assert_eq!(
            arb.propose_remote(&out, &remote, &from(7, 2.0), 100 * MS),
            Intake::Placed(Level::new(9).unwrap())
        );
        assert_eq!(arb.level_of(WriterId::remote(7)), Level::new(9));
        assert_eq!(
            arb.propose_remote(&out, &remote, &from(0, f64::NAN), 0),
            Intake::Refused(Refusal::WrongType)
        );
        let release = Proposal {
            flags: RELINQUISH,
            ..from(3, 0.0)
        };
        assert_eq!(
            arb.propose_remote(&out, &remote, &release, 0),
            Intake::Relinquished(Level::new(9))
        );
        let mut w = w;
        w[0].level = Level::new(9).unwrap();
        assert_eq!(check_writers(&out, &w, PERIOD).unwrap_err().code, "J0210");
    }

    #[test]
    fn level_conflicts_name_both_writers() {
        let out = output();
        let mut w = table();
        assert_eq!(find_level_conflict(&out, &w), None);
        w[1].level = Level::new(13).unwrap();
        assert_eq!(
            find_level_conflict(&out, &w),
            Some((WriterId(0), WriterId(1), Level::new(13).unwrap()))
        );
    }

    #[test]
    fn expire_each_reports_what_was_freed() {
        let (out, w) = (output(), table());
        let mut arb = OutputArbiter::new(PERIOD);
        arb.propose(&out, &w, &p(0, 5.0), 0);
        arb.propose(&out, &w, &p(1, 1.0), 0);
        let mut seen = [0u8; 2];
        let mut n = 0;
        let mask = arb.expire_each(2_000 * MS, |level, entry| {
            seen[n] = level.get();
            assert_eq!(
                entry.lease_until,
                if level.get() == 1 {
                    300 * MS
                } else {
                    2_000 * MS
                }
            );
            n += 1;
        });
        assert_eq!((mask, n, seen), (1 | 1 << 12, 2, [1, 13]));
        arb.restore(
            Level::MANUAL_DEFAULT,
            LevelEntry {
                writer: WriterId(2),
                value: 3,
                lease_until: 9,
            },
        );
        assert_eq!(arb.level_of(WriterId(2)), Some(Level::MANUAL_DEFAULT));
    }
}
