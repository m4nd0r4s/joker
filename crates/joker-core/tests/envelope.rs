//! The full envelope through the commit stage: permissive, quality with
//! grace, expression ranges, reset_when, deadband, min_interval and the
//! actuation budget (docs/01 §7, docs/16 E3–E8, ADR 0038).

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use joker_core::commit::{Cause, RecordKind};
use joker_core::output::{Bound, Budget, OnInterlock, OnRange};
use joker_core::quality::{QualityCode, QualityReason};
use joker_core::{Outcome, Quality, RejectReason};

fn bad() -> Quality {
    Quality::new(QualityCode::Bad, QualityReason::CommFailure)
}

fn uncertain() -> Quality {
    Quality::new(QualityCode::Uncertain, QualityReason::Stale)
}

#[test]
fn permissive_holds_or_goes_safe() {
    for (on_interlock, expected) in [(OnInterlock::Hold, 10.0), (OnInterlock::Safe, 0.0)] {
        let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| {
            c.rate = None;
            c.interlock = Some(program("x.f > 49.8 Hz && x.f < 50.2 Hz"));
            c.on_interlock = on_interlock;
        });
        h.set_input(50.0, Quality::GOOD, 0);
        assert_eq!(
            summary(&h.tick(0, &[(0, 10.0, true)])),
            (Outcome::Accepted, kw(10.0), Some(13))
        );
        h.set_input(50.3, Quality::GOOD, 1);
        assert_eq!(
            summary(&h.tick(1, &[(0, 20.0, true)])),
            (
                Outcome::Rejected(RejectReason::Interlock),
                kw(expected),
                Some(13)
            )
        );
        h.set_input(50.1, bad(), 2);
        assert_eq!(
            summary(&h.tick(2, &[(0, 20.0, true)])).0,
            Outcome::Rejected(RejectReason::Interlock)
        );
        h.set_input(50.1, Quality::GOOD, 3);
        assert_eq!(
            summary(&h.tick(3, &[(0, 20.0, true)])),
            (Outcome::Accepted, kw(20.0), Some(13))
        );
    }
}

#[test]
fn permit_uncertain_lets_uncertain_through() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| {
        c.rate = None;
        c.interlock = Some(program("x.f > 49.8 Hz"));
        c.permit_uncertain = true;
    });
    h.set_input(50.0, uncertain(), 0);
    assert_eq!(summary(&h.tick(0, &[(0, 10.0, true)])).0, Outcome::Accepted);
}

#[test]
fn requires_good_fails_after_grace() {
    let mut h = Harness::new(vec![app("optimizer:13", 5_000)], |c| {
        c.rate = None;
        c.requires_good = leak(vec![joker_core::SignalId(0)]);
        c.grace_ns = 200 * MS;
        c.on_interlock = OnInterlock::Hold;
    });
    h.set_input(50.0, Quality::GOOD, 0);
    assert_eq!(summary(&h.tick(0, &[(0, 10.0, true)])).0, Outcome::Accepted);
    assert_eq!(summary(&h.tick(1, &[(0, 11.0, true)])).0, Outcome::Accepted);
    h.set_input(50.0, bad(), 2);
    assert_eq!(summary(&h.tick(2, &[(0, 12.0, true)])).0, Outcome::Accepted);
    assert_eq!(summary(&h.tick(3, &[(0, 13.0, true)])).0, Outcome::Accepted);
    assert_eq!(
        summary(&h.tick(4, &[(0, 14.0, true)])),
        (Outcome::Rejected(RejectReason::Quality), kw(13.0), Some(13))
    );
    h.set_input(50.0, Quality::GOOD, 5);
    assert_eq!(
        summary(&h.tick(5, &[(0, 15.0, true)])),
        (Outcome::Accepted, kw(15.0), Some(13))
    );
}

#[test]
fn requires_good_without_grace_fails_at_once() {
    let mut h = Harness::new(vec![app("optimizer:13", 5_000)], |c| {
        c.rate = None;
        c.requires_good = leak(vec![joker_core::SignalId(0)]);
    });
    h.set_input(50.0, bad(), 0);
    assert_eq!(
        summary(&h.tick(0, &[(0, 10.0, true)])),
        (Outcome::Rejected(RejectReason::Quality), kw(0.0), Some(13))
    );
}

#[test]
fn expression_range_limits_follow_their_inputs() {
    let mut h = Harness::new(vec![app("optimizer:13", 5_000)], |c| {
        c.rate = None;
        c.range = None;
        c.range_exprs = Some((
            Bound::Const(-100.0),
            Bound::Expr(program("if x.f > 50 Hz then 10 else 20")),
        ));
    });
    h.set_input(49.0, Quality::GOOD, 0);
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])),
        (Outcome::ClampedRange, kw(20.0), Some(13))
    );
    h.set_input(51.0, Quality::GOOD, 1);
    assert_eq!(
        summary(&h.tick(1, &[(0, 50.0, true)])),
        (Outcome::ClampedRange, kw(10.0), Some(13))
    );
    h.set_input(51.0, bad(), 2);
    assert_eq!(
        summary(&h.tick(2, &[(0, 50.0, true)])),
        (
            Outcome::Rejected(RejectReason::Interlock),
            kw(0.0),
            Some(13)
        )
    );
}

#[test]
fn reset_when_ends_the_trip() {
    let mut h = Harness::new(vec![app("optimizer:13", 5_000)], |c| {
        c.rate = None;
        c.on_range = OnRange::Trip;
        c.reset_when = Some(program("x.f < 49 Hz"));
    });
    h.set_input(50.0, Quality::GOOD, 0);
    assert_eq!(summary(&h.tick(0, &[(0, 150.0, true)])).0, Outcome::Tripped);
    assert_eq!(summary(&h.tick(1, &[(0, 50.0, true)])).0, Outcome::Tripped);
    h.set_input(48.0, Quality::GOOD, 2);
    let log = h.tick(2, &[(0, 50.0, true)]);
    assert_eq!(summary(&log), (Outcome::Accepted, kw(50.0), Some(13)));
    assert!(
        log.decisions
            .iter()
            .any(|d| d.kind == RecordKind::Output && d.cause == Cause::Reset)
    );
}

#[test]
fn deadband_suppresses_strictly_smaller_changes() {
    let mut h = Harness::new(vec![app("optimizer:13", 5_000)], |c| {
        c.rate = None;
        c.deadband = Some(0.5);
    });
    assert_eq!(summary(&h.tick(0, &[(0, 10.0, true)])).0, Outcome::Accepted);
    assert_eq!(
        summary(&h.tick(1, &[(0, 10.25, true)])),
        (Outcome::Suppressed, kw(10.0), Some(13))
    );
    assert_eq!(
        summary(&h.tick(2, &[(0, 10.5, true)])),
        (Outcome::Accepted, kw(10.5), Some(13))
    );
}

#[test]
fn min_interval_counts_from_the_last_actuation() {
    let mut h = Harness::new(vec![app("optimizer:13", 5_000)], |c| {
        c.rate = None;
        c.min_interval_ns = 300 * MS;
    });
    assert_eq!(summary(&h.tick(0, &[(0, 10.0, true)])).0, Outcome::Accepted);
    assert_eq!(
        summary(&h.tick(1, &[(0, 11.0, true)])),
        (Outcome::Suppressed, kw(10.0), Some(13))
    );
    assert_eq!(
        summary(&h.tick(2, &[(0, 11.0, true)])).0,
        Outcome::Suppressed
    );
    assert_eq!(
        summary(&h.tick(3, &[(0, 11.0, true)])),
        (Outcome::Accepted, kw(11.0), Some(13))
    );
    assert_eq!(summary(&h.tick(4, &[(0, 11.0, true)])).0, Outcome::Accepted);
}

#[test]
fn budget_refuses_actuations_beyond_the_window() {
    let mut h = Harness::new(vec![app("optimizer:13", 5_000)], |c| {
        c.rate = None;
        c.max_actuations = Some(Budget {
            count: 2,
            window_ns: 1_000 * MS,
        });
        c.on_interlock = OnInterlock::Hold;
    });
    assert_eq!(summary(&h.tick(0, &[(0, 1.0, true)])).0, Outcome::Accepted);
    assert_eq!(summary(&h.tick(1, &[(0, 2.0, true)])).0, Outcome::Accepted);
    assert_eq!(
        summary(&h.tick(2, &[(0, 3.0, true)])),
        (Outcome::Rejected(RejectReason::Budget), kw(2.0), Some(13))
    );
    assert_eq!(summary(&h.tick(3, &[(0, 2.0, true)])).0, Outcome::Accepted);
    assert_eq!(
        summary(&h.tick(10, &[(0, 3.0, true)])),
        (Outcome::Accepted, kw(3.0), Some(13))
    );
}

#[test]
fn safe_is_never_blocked_by_step_6() {
    let mut h = Harness::new(vec![app("optimizer:13", 300)], |c| {
        c.rate = None;
        c.min_interval_ns = 10_000 * MS;
        c.deadband = Some(100.0);
        c.max_actuations = Some(Budget {
            count: 1,
            window_ns: 10_000 * MS,
        });
    });
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])).0,
        Outcome::Suppressed
    );
    let mut h = Harness::new(vec![app("optimizer:13", 300)], |c| {
        c.rate = None;
        c.min_interval_ns = 10_000 * MS;
        c.max_actuations = Some(Budget {
            count: 1,
            window_ns: 10_000 * MS,
        });
    });
    assert_eq!(
        summary(&h.tick(0, &[(0, 50.0, true)])),
        (Outcome::Accepted, kw(50.0), Some(13))
    );
    assert_eq!(summary(&h.tick(3, &[])), (Outcome::None, kw(0.0), None));
}
