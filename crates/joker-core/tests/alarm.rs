//! Alarms in step 3 of the tick: delays, acknowledgement, Unknown as an
//! overlay, suppression, shelving and trip actions (docs/02 §11, ADR 0040).

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use joker_core::alarm::{
    AlarmCause, AlarmConfig, AlarmEvent, AlarmRuntime, AlarmState, Alarms, Severity,
};
use joker_core::commit::{Cause, RecordKind};
use joker_core::quality::{QualityCode, QualityReason};
use joker_core::time::Instant;
use joker_core::{
    CommitSink, DecisionRecord, Outcome, OutputResult, Quality, SignalId, Slot, WriteResult,
};

#[derive(Default)]
struct Sink {
    events: Vec<AlarmEvent>,
    outputs: Vec<OutputResult>,
    decisions: Vec<DecisionRecord>,
}

impl CommitSink for Sink {
    fn alarm(&mut self, e: &AlarmEvent) {
        self.events.push(*e);
    }
    fn output(&mut self, r: &OutputResult) {
        self.outputs.push(*r);
    }
    fn decision(&mut self, r: &DecisionRecord) {
        self.decisions.push(*r);
    }
    fn write(&mut self, _: &WriteResult) {}
}

const STATE: SignalId = SignalId(2);
const ACTIVE: SignalId = SignalId(3);

struct Rig {
    h: Harness,
    alarms: Alarms<'static>,
    extra: &'static [Slot],
}

fn rig(configure: impl FnOnce(&mut AlarmConfig<'static>)) -> Rig {
    let h = Harness::new(vec![app("optimizer:13", 60_000)], |c| c.rate = None);
    let mut config = AlarmConfig::new(
        7,
        h.registry.resolve_group("fast").unwrap(),
        program("x.f > 50.2 Hz"),
        Severity::High,
    );
    config.state_signal = Some(STATE);
    config.active_signal = Some(ACTIVE);
    configure(&mut config);
    let configs: &'static [AlarmConfig<'static>] = leak(vec![config]);
    let mut alarms = Alarms::new(configs, leak(vec![AlarmRuntime::EMPTY; 1])).unwrap();
    alarms
        .set_state_arena(leak(vec![0; alarms.state_words()]))
        .unwrap();
    Rig {
        h,
        alarms,
        extra: leak((0..4).map(|_| Slot::new()).collect()),
    }
}

impl Rig {
    fn tick(&mut self, k: u64, f: f64, quality: Quality, propose: Option<f64>) -> Sink {
        let mut sink = Sink::default();
        self.h.set_input(f, quality, k);
        self.h
            .image
            .latch(k, Instant::mono(k * PERIOD), self.h.slots, &self.h.registry)
            .unwrap();
        self.alarms
            .evaluate(&self.h.image, self.extra, &mut self.h.outputs, &mut sink)
            .unwrap();
        if let Some(v) = propose {
            self.h
                .image
                .stage_cycle(proposal(0, self.h.out(), v))
                .unwrap();
        }
        self.h
            .outputs
            .commit(&self.h.image, self.h.slots, &mut sink)
            .unwrap();
        self.h.image.end_tick().unwrap();
        sink
    }

    fn state(&self) -> AlarmState {
        self.alarms.state(7).unwrap().0
    }

    fn active_signal(&self) -> (u64, QualityCode) {
        let s = self.extra[ACTIVE.index()].try_read().unwrap();
        (s.transport_bits(), s.quality().code())
    }
}

fn bad() -> Quality {
    Quality::new(QualityCode::Bad, QualityReason::CommFailure)
}

#[test]
fn delays_and_acknowledgement() {
    let mut r = rig(|c| {
        c.delay_on_ns = 200 * MS;
        c.delay_off_ns = 100 * MS;
    });
    r.tick(0, 50.0, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::Normal);
    r.tick(1, 50.5, Quality::GOOD, None);
    r.tick(2, 50.5, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::Normal);
    let s = r.tick(3, 50.5, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::ActiveUnacked);
    assert_eq!(s.events[0].to, AlarmState::ActiveUnacked);
    assert_eq!(r.active_signal(), (1, QualityCode::Good));
    assert_eq!(
        r.extra[STATE.index()].try_read().unwrap().transport_bits(),
        1
    );
    r.tick(4, 50.0, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::ActiveUnacked);
    r.tick(5, 50.0, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::RtnUnacked);
    let mut sink = Sink::default();
    assert!(r.alarms.ack(7, 0, &mut sink).unwrap());
    assert_eq!(r.state(), AlarmState::Normal);
    assert_eq!(sink.events[0].cause, AlarmCause::Ack);
    assert!(!r.alarms.ack(7, 0, &mut sink).unwrap());
}

#[test]
fn rtn_unacked_goes_back_to_active() {
    let mut r = rig(|_| {});
    r.tick(0, 50.5, Quality::GOOD, None);
    r.tick(1, 50.0, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::RtnUnacked);
    r.tick(2, 50.5, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::ActiveUnacked);
}

#[test]
fn unknown_is_an_overlay() {
    let mut r = rig(|c| c.delay_on_ns = 100 * MS);
    r.tick(0, 50.5, Quality::GOOD, None);
    r.tick(1, 50.5, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::ActiveUnacked);
    let s = r.tick(2, 50.5, bad(), None);
    assert_eq!(r.state(), AlarmState::Unknown);
    assert_eq!(s.events[0].to, AlarmState::Unknown);
    assert_eq!(r.active_signal(), (1, QualityCode::Bad));
    let mut sink = Sink::default();
    assert!(r.alarms.ack(7, 0, &mut sink).unwrap());
    assert_eq!(r.state(), AlarmState::Unknown);
    let s = r.tick(3, 50.5, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::ActiveAcked);
    assert_eq!(s.events.len(), 1);
    assert_eq!(
        (s.events[0].from, s.events[0].to),
        (AlarmState::Unknown, AlarmState::ActiveAcked)
    );
    assert_eq!(r.active_signal(), (1, QualityCode::Good));
}

#[test]
fn unknown_from_normal_restarts_delay_on() {
    let mut r = rig(|c| c.delay_on_ns = 100 * MS);
    r.tick(0, 50.5, Quality::GOOD, None);
    r.tick(1, 50.5, bad(), None);
    r.tick(2, 50.5, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::Normal);
    r.tick(3, 50.5, Quality::GOOD, None);
    assert_eq!(r.state(), AlarmState::ActiveUnacked);
}

#[test]
fn uncertain_evaluates_with_the_flag() {
    let mut r = rig(|_| {});
    let s = r.tick(
        0,
        50.5,
        Quality::new(QualityCode::Uncertain, QualityReason::Stale),
        None,
    );
    assert_eq!(r.state(), AlarmState::ActiveUnacked);
    assert!(s.events[0].uncertain);
}

#[test]
fn trip_action_trips_in_the_same_tick_and_after_reset() {
    let out = SignalId(1);
    let mut r = rig(|c| c.trip = leak(vec![out]));
    let s = r.tick(0, 50.0, Quality::GOOD, Some(10.0));
    assert_eq!(s.outputs[0].outcome, Outcome::Accepted);
    let s = r.tick(1, 50.5, Quality::GOOD, Some(10.0));
    assert_eq!(s.outputs[0].outcome, Outcome::Tripped);
    assert!(s.outputs[0].tripped);
    assert!(
        s.decisions
            .iter()
            .any(|d| d.kind == RecordKind::Output && d.cause == Cause::Trip)
    );
    assert!(r.h.outputs.reset(out));
    let s = r.tick(2, 50.5, Quality::GOOD, Some(10.0));
    assert_eq!(s.outputs[0].outcome, Outcome::Accepted);
    let s = r.tick(3, 50.5, Quality::GOOD, Some(10.0));
    assert_eq!(s.outputs[0].outcome, Outcome::Tripped);
    r.tick(4, 50.0, Quality::GOOD, Some(10.0));
    assert!(r.h.outputs.reset(out));
    let s = r.tick(5, 50.0, Quality::GOOD, Some(10.0));
    assert_eq!(s.outputs[0].outcome, Outcome::Accepted);
}

#[test]
fn suppression_blocks_activation_and_trips() {
    let out = SignalId(1);
    let mut r = rig(|c| {
        c.trip = leak(vec![out]);
        c.suppress_when = Some(program("x.f > 55 Hz"));
    });
    let s = r.tick(0, 56.0, Quality::GOOD, Some(10.0));
    assert_eq!(r.state(), AlarmState::Normal);
    assert_eq!(s.outputs[0].outcome, Outcome::Accepted);
    r.tick(1, 51.0, Quality::GOOD, Some(10.0));
    assert_eq!(r.state(), AlarmState::ActiveUnacked);
}

#[test]
fn shelving_hides_but_never_disables_trips() {
    let out = SignalId(1);
    let mut r = rig(|c| {
        c.trip = leak(vec![out]);
        c.max_shelve_ns = 1_000 * MS;
    });
    let mut sink = Sink::default();
    assert!(r.alarms.shelve(7, 2_000 * MS, 0, &mut sink).is_err());
    r.alarms.shelve(7, 300 * MS, 0, &mut sink).unwrap();
    assert_eq!(sink.events[0].cause, AlarmCause::Shelve);
    let s = r.tick(1, 50.5, Quality::GOOD, Some(10.0));
    assert!(s.events[0].shelved);
    assert_eq!(s.outputs[0].outcome, Outcome::Tripped);
    let s = r.tick(3, 50.5, Quality::GOOD, None);
    assert_eq!(s.events[0].cause, AlarmCause::ShelveExpired);
    assert!(!r.alarms.state(7).unwrap().1);
    assert!(r.alarms.unshelve(7, 0, &mut sink).is_err());
}
