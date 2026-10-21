//! End-to-end ticks through registry, slots, process image, arbiter and
//! envelope, driven by a virtual clock.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use joker_core::arbiter::RemoteWriter;
use joker_core::commit::{Cause, Durability, ManualChange, RecordKind};
use joker_core::image::RELINQUISH;
use joker_core::output::{ClockRequirement, OnRange};
use joker_core::{ClockQuality, Level, Outcome, Proposal, RejectReason, WriterId};

#[test]
fn docs16_rate_example() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |_| {});
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])),
        (Outcome::ClampedRate, kw(1.0), Some(13))
    );
    for k in 1..20 {
        assert_eq!(
            summary(&h.tick(k, &[])),
            (Outcome::None, kw(1.0 + k as f64), Some(13)),
            "tick {k}"
        );
    }
    assert_eq!(summary(&h.tick(20, &[])), (Outcome::None, kw(0.0), None));
    assert_eq!(summary(&h.tick(22, &[])), (Outcome::None, kw(0.0), None));
    let slot = h.slots[h.out().index()].try_read().unwrap();
    assert_eq!(slot.get(), Ok(0f64.to_bits()));
}

#[test]
fn first_applied_value_is_safe_and_inherited_across_generations() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| c.rate = None);
    assert_eq!(summary(&h.tick(0, &[])), (Outcome::None, kw(0.0), None));
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |_| {});
    assert!(h.outputs.inherit(h.out(), kw(30.0)));
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])),
        (Outcome::ClampedRate, kw(31.0), Some(13))
    );
    assert!(!h.outputs.inherit(h.out(), kw(0.0)));
}

#[test]
fn manual_write_overrides_the_app_and_expires() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000), manual(500)], |c| {
        c.rate = None
    });
    assert_eq!(
        summary(&h.tick(0, &[(0, 10.0, true)])),
        (Outcome::Accepted, kw(10.0), Some(13))
    );
    let log = h.tick(1, &[(1, 42.0, false), (0, 10.0, true)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(42.0), Some(4)));
    assert_eq!(log.writes[0].outcome, Outcome::Accepted);
    assert_eq!(
        log.writes[1].outcome,
        Outcome::Overridden(Level::MANUAL_DEFAULT)
    );
    for k in 2..6 {
        let log = h.tick(k, &[(0, 10.0, true)]);
        assert_eq!(
            summary(&log),
            (
                Outcome::Overridden(Level::MANUAL_DEFAULT),
                kw(42.0),
                Some(4)
            )
        );
    }
    assert_eq!(
        summary(&h.tick(6, &[(0, 10.0, true)])),
        (Outcome::Accepted, kw(10.0), Some(13))
    );
    assert_eq!(
        h.outputs.last_outcome(WriterId(0), h.out()),
        Some(Outcome::Accepted)
    );
}

#[test]
fn not_owner_and_wrong_type_are_rejected_without_renewal() {
    let mut h = Harness::new(
        vec![app("optimizer:13", 300), manual(10_000), manual(10_000)],
        |c| c.rate = None,
    );
    h.tick(0, &[(1, 5.0, false)]);
    let log = h.tick(1, &[(2, 6.0, false)]);
    assert_eq!(
        summary(&log),
        (Outcome::Rejected(RejectReason::NotOwner), kw(5.0), Some(4))
    );
    let log = h.tick(2, &[(0, f64::NAN, true)]);
    assert_eq!(
        log.writes[0].outcome,
        Outcome::Rejected(RejectReason::WrongType)
    );
    assert_eq!(
        summary(&log),
        (Outcome::Rejected(RejectReason::WrongType), kw(5.0), Some(4))
    );
    let release = Proposal {
        flags: RELINQUISH,
        ..proposal(1, h.out(), 0.0)
    };
    let log = h.stage_raw(3, &[release, proposal(2, h.out(), 7.0)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(7.0), Some(4)));
    assert_eq!(log.writes[0].outcome, Outcome::Accepted);
}

#[test]
fn trip_holds_safe_until_reset() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| {
        c.rate = None;
        c.on_range = OnRange::Trip;
    });
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])),
        (Outcome::Accepted, kw(50.0), Some(13))
    );
    let log = h.tick(1, &[(0, 150.0, true)]);
    assert_eq!(summary(&log), (Outcome::Tripped, kw(0.0), Some(13)));
    assert!(log.outputs[0].tripped);
    let trip = log
        .decisions
        .iter()
        .find(|d| d.kind == RecordKind::Output)
        .unwrap();
    assert_eq!(trip.durability, Durability::Immediate);
    let log = h.tick(2, &[(0, 50.0, true)]);
    assert_eq!(summary(&log), (Outcome::Tripped, kw(0.0), Some(13)));
    assert!(h.outputs.reset(h.out()));
    let log = h.tick(3, &[]);
    assert_eq!(summary(&log), (Outcome::None, kw(50.0), Some(13)));
    let reset = log
        .decisions
        .iter()
        .find(|d| d.kind == RecordKind::Output)
        .unwrap();
    assert_eq!(reset.cause, Cause::Reset);
    assert!(!h.outputs.reset(h.out()));
}

#[test]
fn reset_is_recorded_even_when_nothing_else_changes() {
    let mut h = Harness::new(vec![app("optimizer:13", 300)], |c| {
        c.rate = None;
        c.on_range = OnRange::Trip;
    });
    h.tick(0, &[(0, 150.0, true)]);
    for k in 1..6 {
        h.tick(k, &[]);
    }
    assert!(h.outputs.reset(h.out()));
    let log = h.tick(6, &[]);
    assert_eq!(summary(&log), (Outcome::None, kw(0.0), None));
    let reset: Vec<_> = log
        .decisions
        .iter()
        .filter(|d| d.kind == RecordKind::Output)
        .collect();
    assert_eq!(reset.len(), 1);
    assert_eq!(reset[0].cause, Cause::Reset);
    assert_eq!(reset[0].durability, Durability::Immediate);
}

#[test]
fn unsynced_clock_rejects_writers_that_require_it() {
    let mut w = app("optimizer:13", 2_000);
    w.requires_clock = ClockRequirement::Synced;
    let mut h = Harness::new(vec![w], |c| c.rate = None);
    h.tick(0, &[(0, 50.0, true)]);
    h.clock = ClockQuality::Unsynced;
    let log = h.tick(1, &[(0, 60.0, true)]);
    assert_eq!(
        summary(&log),
        (
            Outcome::Rejected(RejectReason::ClockUnsynced),
            kw(0.0),
            Some(13)
        )
    );
    h.clock = ClockQuality::Synced;
    assert_eq!(
        summary(&h.tick(2, &[])),
        (Outcome::None, kw(60.0), Some(13))
    );
}

#[test]
fn decision_records_are_written_on_change_with_repeat_counts() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| c.rate = None);
    let mut records = Vec::new();
    for k in 0..5 {
        records.extend(h.tick(k, &[(0, 10.0, true)]).decisions);
    }
    let outputs: Vec<_> = records
        .iter()
        .filter(|d| d.kind == RecordKind::Output)
        .collect();
    let writes: Vec<_> = records
        .iter()
        .filter(|d| d.kind == RecordKind::Write(WriterId(0)))
        .collect();
    assert_eq!(outputs.len(), 1);
    assert_eq!(writes.len(), 1);
    assert_eq!(outputs[0].previous, None);
    assert_eq!(outputs[0].durability, Durability::Immediate);
    assert_eq!(writes[0].durability, Durability::Batch);
    let log = h.tick(5, &[(0, 20.0, true)]);
    let change = log
        .decisions
        .iter()
        .find(|d| d.kind == RecordKind::Output)
        .unwrap();
    let previous = change.previous.unwrap();
    assert_eq!(
        (previous.first_tick, previous.last_tick, previous.repeats),
        (0, 4, 5)
    );
    assert_eq!(change.cause, Cause::Proposal(WriterId(0)));
    assert_eq!(change.durability, Durability::Batch);
    let mut quiet = Vec::new();
    for k in 6..30 {
        quiet.extend(h.tick(k, &[]).decisions);
    }
    let expiry: Vec<_> = quiet
        .iter()
        .filter(|d| d.kind == RecordKind::Output)
        .collect();
    assert_eq!(expiry.len(), 2);
    assert_eq!(expiry[0].outcome, Outcome::None);
    assert_eq!(expiry[1].cause, Cause::Expiry);
    assert_eq!(expiry[1].level, None);
    assert_eq!(expiry[1].durability, Durability::Immediate);
}

#[test]
fn earlier_proposals_of_a_writer_in_a_tick_are_overridden_by_its_last() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| c.rate = None);
    let log = h.tick(0, &[(0, 10.0, true), (0, 20.0, true)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(20.0), Some(13)));
    assert_eq!(
        log.writes[0].outcome,
        Outcome::Overridden(Level::new(13).unwrap())
    );
    assert_eq!(log.writes[1].outcome, Outcome::Accepted);
}

#[test]
fn takeover_and_forced_release_are_recorded_with_their_kind() {
    let mut h = Harness::new(
        vec![app("optimizer:13", 2_000), manual(60_000), manual(60_000)],
        |c| c.rate = None,
    );
    let log = h.tick(0, &[(1, 5.0, false)]);
    assert!(matches!(
        log.manual[0].change,
        ManualChange::Set { lease_until, .. } if lease_until == 60_000 * MS
    ));
    assert_eq!(
        h.outputs
            .occupant(h.out(), Level::MANUAL_DEFAULT)
            .map(|e| e.writer),
        Some(WriterId(1))
    );
    let [release, take] = Proposal::takeover(WriterId(1), proposal(2, h.out(), 7.0));
    let log = h.stage_raw(1, &[release, take]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(7.0), Some(4)));
    let kinds: Vec<_> = log
        .decisions
        .iter()
        .filter(|d| d.kind == RecordKind::Write(WriterId(1)))
        .map(|d| (d.cause, d.durability))
        .collect();
    assert_eq!(
        kinds,
        [(Cause::Takeover(WriterId(2)), Durability::Immediate)]
    );
    assert_eq!(log.manual.len(), 2);
    assert_eq!(
        log.manual[0].change,
        ManualChange::Cleared(Cause::Takeover(WriterId(2)))
    );
    assert_eq!(log.manual[1].writer, WriterId(2));
    let force = Proposal::forced_release(WriterId(2), h.out(), WriterId(1));
    let log = h.stage_raw(2, &[force]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(0.0), None));
    assert_eq!(
        log.manual[0].change,
        ManualChange::Cleared(Cause::ForcedRelease(WriterId(1)))
    );
}

#[test]
fn manual_expiry_is_recorded_and_persisted() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000), manual(300)], |c| {
        c.rate = None
    });
    h.tick(0, &[(1, 5.0, false)]);
    h.tick(1, &[]);
    h.tick(2, &[]);
    let log = h.tick(3, &[]);
    assert_eq!(summary(&log), (Outcome::None, kw(0.0), None));
    let expiry: Vec<_> = log
        .decisions
        .iter()
        .filter(|d| d.kind == RecordKind::Write(WriterId(1)))
        .collect();
    assert_eq!(expiry.len(), 1);
    assert_eq!(expiry[0].cause, Cause::Expiry);
    assert_eq!(expiry[0].previous.unwrap().repeats, 1);
    assert_eq!(log.manual[0].change, ManualChange::Cleared(Cause::Expiry));
}

#[test]
fn manual_levels_are_restored_before_the_first_tick() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000), manual(60_000)], |c| {
        c.rate = None
    });
    assert_eq!(
        h.outputs
            .restore_manual(h.out(), WriterId(1), 12.0f64.to_bits(), 250 * MS, 0),
        Ok(Level::MANUAL_DEFAULT)
    );
    assert!(
        h.outputs
            .restore_manual(h.out(), WriterId(0), 1, 1, 0)
            .is_err()
    );
    assert_eq!(summary(&h.tick(0, &[(0, 3.0, true)])).2, Some(4));
    assert_eq!(
        summary(&h.tick(2, &[(0, 3.0, true)])),
        (
            Outcome::Overridden(Level::MANUAL_DEFAULT),
            kw(12.0),
            Some(4)
        )
    );
    assert_eq!(
        summary(&h.tick(3, &[(0, 3.0, true)])),
        (Outcome::Accepted, kw(3.0), Some(13))
    );
    assert!(
        h.outputs
            .restore_manual(h.out(), WriterId(1), 1, 1, 0)
            .is_err()
    );
}

#[test]
fn remote_writers_propose_in_delegated_levels() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| {
        c.rate = None;
        c.remote_levels = 1 << 8;
    });
    let remote: &'static [RemoteWriter] = leak(vec![RemoteWriter {
        level: Level::new(9).unwrap(),
        lease_ns: 10_000 * MS,
        link_max_lease_ns: 500 * MS,
    }]);
    h.outputs.set_remote_writers(remote);
    let out = h.out();
    let p = |slot: u32, value: f64| joker_core::Proposal {
        writer: WriterId::remote(slot),
        signal: out,
        value: value.to_bits(),
        ..joker_core::Proposal::EMPTY
    };
    let first = p(0, 9.0);
    let log = h.stage_raw(0, &[first, proposal(0, h.out(), 1.0)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(9.0), Some(9)));
    let log = h.stage_raw(5, &[proposal(0, h.out(), 1.0)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(1.0), Some(13)));
    let log = h.stage_raw(6, &[p(1, 4.0)]);
    assert_eq!(
        log.writes[0].outcome,
        Outcome::Rejected(joker_core::RejectReason::NotOwner)
    );
}
