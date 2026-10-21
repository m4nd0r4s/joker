//! Class methods and queries in the core: expansion in step 5a, own(),
//! nested calls, all-or-nothing calls, and derived signals (docs/17 §4,
//! ADR 0041).

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use joker_core::class::{Classes, DerivedDef, InstanceDef, MethodDef, MethodResult, Stmt};
use joker_core::image::{CausalSet, MethodCall, Source};
use joker_core::quality::{QualityCode, QualityReason};
use joker_core::time::Instant;
use joker_core::trace::{Item, Items, SegmentWriter, TickRecord, Ticks, TraceResult};
use joker_core::{
    CommitSink, DecisionRecord, Outcome, OutputResult, Proposal, Quality, SignalId, Slot, WriterId,
};

#[derive(Default)]
struct Sink {
    outputs: Vec<OutputResult>,
    methods: Vec<MethodResult>,
    causal: Vec<(WriterId, Vec<(SignalId, u64)>)>,
}

impl CommitSink for Sink {
    fn output(&mut self, r: &OutputResult) {
        self.outputs.push(*r);
    }
    fn method(&mut self, r: &MethodResult) {
        self.methods.push(*r);
    }
    fn causal(&mut self, r: &DecisionRecord, inputs: &CausalSet<'_>) {
        let joker_core::commit::RecordKind::Write(writer) = r.kind else {
            return;
        };
        let read = inputs.iter().map(|(s, sample)| (s, sample.seq())).collect();
        self.causal.push((writer, read));
    }
}

const OUT: SignalId = SignalId(1);
const QUERY: SignalId = SignalId(2);

struct Rig {
    h: Harness,
    classes: Classes<'static>,
    extra: &'static [Slot],
}

fn rig() -> Rig {
    let mut h = Harness::new(vec![app("optimizer:13", 60_000)], |c| {
        c.rate = None;
        c.range = None;
    });
    h.image.set_method_buffer(leak(vec![MethodCall::EMPTY; 8]));
    h.image.set_read_sets(leak(vec![0; 2]), 2).unwrap();
    let set: &'static [Stmt<'static>] = leak(vec![Stmt::Assign {
        target: OUT,
        expr: method_program("a0"),
    }]);
    let bump: &'static [Stmt<'static>] = leak(vec![Stmt::Assign {
        target: OUT,
        expr: method_program("own(x.p_set) + 1 kW"),
    }]);
    let guarded: &'static [Stmt<'static>] = leak(vec![Stmt::Assign {
        target: OUT,
        expr: method_program("if x.f > 50 Hz then 5 kW else 6 kW"),
    }]);
    let child = InstanceDef {
        methods: leak(vec![
            MethodDef {
                params: 1,
                stmts: set,
            },
            MethodDef {
                params: 0,
                stmts: bump,
            },
            MethodDef {
                params: 0,
                stmts: guarded,
            },
        ]),
    };
    let nested: &'static [&'static joker_expr::Program] = leak(vec![method_program("a0 * 2")]);
    let parent = InstanceDef {
        methods: leak(vec![MethodDef {
            params: 1,
            stmts: leak(vec![Stmt::Call {
                instance: 0,
                method: 0,
                args: nested,
            }]),
        }]),
    };
    let derived = leak(vec![DerivedDef {
        signal: QUERY,
        group: h.registry.resolve_group("fast").unwrap(),
        expr: program("held(x.f > 50 Hz, 200ms)"),
    }]);
    let mut classes = Classes::new(
        leak(vec![child, parent]),
        derived,
        leak(vec![Proposal::EMPTY; 8]),
    );
    classes
        .set_state_arena(leak(vec![0; classes.state_words()]))
        .unwrap();
    Rig {
        h,
        classes,
        extra: leak((0..3).map(|_| Slot::new()).collect()),
    }
}

fn call(instance: u32, method: u16, args: &[f64]) -> MethodCall {
    let mut c = MethodCall {
        writer: WriterId(0),
        instance,
        method,
        n_args: args.len() as u8,
        source: Source::Cycle,
        task: 0,
        ..MethodCall::EMPTY
    };
    for (slot, a) in c.args.iter_mut().zip(args) {
        *slot = a.to_bits();
    }
    c
}

impl Rig {
    fn tick(&mut self, k: u64, f: f64, quality: Quality, calls: &[MethodCall]) -> Sink {
        self.record(k, f, quality, calls, &[], None)
    }

    fn record(
        &mut self,
        k: u64,
        f: f64,
        quality: Quality,
        calls: &[MethodCall],
        direct: &[Proposal],
        segment: Option<&mut SegmentWriter<'_>>,
    ) -> Sink {
        let mut sink = Sink::default();
        self.h.set_input(f, quality, k);
        self.h
            .image
            .latch(k, Instant::mono(k * PERIOD), self.h.slots, &self.h.registry)
            .unwrap();
        self.classes
            .evaluate_derived(&self.h.image, self.extra)
            .unwrap();
        self.h.image.mark_read(0, 0);
        for p in direct {
            self.h.image.stage_cycle(*p).unwrap();
        }
        for c in calls {
            self.h.image.stage_method(*c).unwrap();
        }
        self.classes
            .expand(&mut self.h.image, self.h.slots, &self.h.outputs, &mut sink)
            .unwrap();
        self.h
            .outputs
            .commit(&self.h.image, self.h.slots, &mut sink)
            .unwrap();
        if let Some(segment) = segment {
            let results: Vec<TraceResult> = sink.outputs.iter().map(TraceResult::from).collect();
            segment
                .push(&TickRecord {
                    index: k,
                    at: Instant::mono(k * PERIOD),
                    inputs: self.h.image.snapshot(),
                    proposals: self.h.image.direct_proposals(),
                    results: &results,
                    methods: self.h.image.method_calls(),
                })
                .unwrap();
        }
        self.h.image.end_tick().unwrap();
        sink
    }

    fn query(&self) -> (u64, QualityCode) {
        let s = self.extra[QUERY.index()].try_read().unwrap();
        (s.transport_bits(), s.quality().code())
    }
}

fn applied(s: &Sink) -> (Outcome, f64) {
    let o = s.outputs[0];
    (o.outcome, f64::from_bits(o.applied.bits()))
}

#[test]
fn a_method_expands_into_a_write_of_the_caller() {
    let mut r = rig();
    let s = r.tick(0, 50.0, Quality::GOOD, &[call(0, 0, &[7.0])]);
    assert_eq!(applied(&s), (Outcome::Accepted, 7.0));
    assert_eq!(s.methods[0].writes, 1);
    assert_eq!(s.methods[0].error, None);
    assert_eq!(s.outputs[0].level.map(|l| l.get()), Some(13));
}

#[test]
fn own_sees_earlier_writes_of_the_tick_then_the_level() {
    let mut r = rig();
    let s = r.tick(0, 50.0, Quality::GOOD, &[call(0, 1, &[]), call(0, 1, &[])]);
    assert_eq!(applied(&s), (Outcome::Accepted, 2.0));
    let s = r.tick(1, 50.0, Quality::GOOD, &[call(0, 1, &[])]);
    assert_eq!(applied(&s), (Outcome::Accepted, 3.0));
}

#[test]
fn nested_calls_expand_in_place() {
    let mut r = rig();
    let s = r.tick(0, 50.0, Quality::GOOD, &[call(1, 0, &[4.0])]);
    assert_eq!(applied(&s), (Outcome::Accepted, 8.0));
    assert!(r.h.image.direct_proposals().is_empty());
    assert_eq!(r.h.image.proposals().len(), 1);
}

#[test]
fn a_bad_value_expands_the_call_to_nothing() {
    let mut r = rig();
    r.tick(0, 50.0, Quality::GOOD, &[call(0, 0, &[1.0])]);
    let bad = Quality::new(QualityCode::Bad, QualityReason::CommFailure);
    let s = r.tick(1, 51.0, bad, &[call(0, 2, &[])]);
    assert_eq!(s.methods[0].error, Some("J-E0504"));
    assert_eq!(s.methods[0].writes, 0);
    assert_eq!(applied(&s), (Outcome::None, 1.0));
}

#[test]
fn unknown_methods_and_wrong_arguments_are_refused() {
    let mut r = rig();
    let s = r.tick(
        0,
        50.0,
        Quality::GOOD,
        &[call(0, 9, &[]), call(0, 0, &[]), call(5, 0, &[])],
    );
    let errors: Vec<_> = s.methods.iter().map(|m| m.error).collect();
    assert_eq!(errors, [Some("J-E0505"), Some("J-E0504"), Some("J-E0907")]);
}

#[test]
fn queries_are_derived_signals_with_state() {
    let mut r = rig();
    r.tick(0, 51.0, Quality::GOOD, &[]);
    assert_eq!(r.query(), (0, QualityCode::Good));
    r.tick(2, 51.0, Quality::GOOD, &[]);
    assert_eq!(r.query(), (1, QualityCode::Good));
    let bad = Quality::new(QualityCode::Bad, QualityReason::CommFailure);
    r.tick(3, 51.0, bad, &[]);
    assert_eq!(r.query().1, QualityCode::Bad);
}

#[test]
fn writes_carry_the_causal_set_of_their_task() {
    let mut r = rig();
    let s = r.tick(0, 50.0, Quality::GOOD, &[call(0, 0, &[7.0])]);
    assert_eq!(s.causal, [(WriterId(0), vec![(SignalId(0), 1)])]);
    let direct = Proposal {
        writer: WriterId(0),
        signal: OUT,
        value: 3.0f64.to_bits(),
        task: 1,
        ..Proposal::EMPTY
    };
    let s = r.record(1, 50.0, Quality::GOOD, &[], &[direct], None);
    assert_eq!(s.causal, [(WriterId(0), vec![])]);
}

const GOLDEN: &str = "tests/golden/methods.jktrace";

fn script(k: u64) -> Vec<MethodCall> {
    match k {
        0 => vec![call(0, 0, &[7.0])],
        1 => vec![call(0, 1, &[]), call(0, 1, &[])],
        2 => vec![call(1, 0, &[1.5])],
        4 => vec![call(0, 2, &[]), call(0, 9, &[])],
        _ => vec![],
    }
}

fn record_methods() -> Vec<u8> {
    let mut r = rig();
    let mut buf = vec![0u8; 16 * 1024];
    let mut segment = SegmentWriter::new(0, &mut buf).unwrap();
    for k in 0..6 {
        let f = if k == 4 { 51.0 } else { 50.0 };
        r.record(k, f, Quality::GOOD, &script(k), &[], Some(&mut segment));
    }
    segment.finish().to_vec()
}

#[test]
fn method_records_replay_to_the_same_bytes() {
    let recorded = record_methods();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(GOLDEN, &recorded).unwrap();
    }
    assert_eq!(recorded, std::fs::read(GOLDEN).unwrap());
    let Some(Ok(Item::Ticks {
        n_ticks, payload, ..
    })) = Items::new(&recorded).next()
    else {
        panic!("no segment");
    };
    let mut r = rig();
    let mut buf = vec![0u8; 16 * 1024];
    let mut segment = SegmentWriter::new(0, &mut buf).unwrap();
    for tick in Ticks::new(payload, n_ticks, 1) {
        let tick = tick.unwrap();
        let input = tick.inputs().next().unwrap();
        let calls: Vec<MethodCall> = tick
            .method_calls()
            .map(|c| {
                let c = c.unwrap();
                MethodCall { task: 0, ..c }
            })
            .collect();
        let direct: Vec<Proposal> = tick.proposals().map(|p| p.to_proposal()).collect();
        r.record(
            tick.index,
            f64::from_bits(input.value),
            input.quality,
            &calls,
            &direct,
            Some(&mut segment),
        );
    }
    assert_eq!(segment.finish(), recorded.as_slice());
}
