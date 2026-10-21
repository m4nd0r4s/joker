//! The tick path never allocates (AGENTS.md invariant 6): a counting global
//! allocator watches latch, alarms, derived signals, method expansion, the
//! full envelope, the commit and the trace writer.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use joker_core::alarm::{AlarmConfig, AlarmRuntime, Alarms, Severity};
use joker_core::class::{Classes, DerivedDef, InstanceDef, MethodDef, Stmt};
use joker_core::image::{MethodCall, Source};
use joker_core::output::{Bound, Budget, OnInterlock};
use joker_core::time::Instant;
use joker_core::trace::{SegmentWriter, TickRecord, TraceResult};
use joker_core::{CommitSink, OutputResult, Proposal, Quality, SignalId, Slot, WriterId};

struct LastResult(Option<OutputResult>);

impl CommitSink for LastResult {
    fn output(&mut self, r: &OutputResult) {
        self.0 = Some(*r);
    }
}

#[test]
fn a_full_tick_does_not_allocate() {
    let mut h = Harness::new(vec![app("optimizer:13", 2_000)], |c| {
        c.interlock = Some(program("x.f > 49 Hz && held(x.f < 52 Hz, 200ms)"));
        c.on_interlock = OnInterlock::Hold;
        c.requires_good = leak(vec![SignalId(0)]);
        c.grace_ns = 200 * MS;
        c.range = None;
        c.range_exprs = Some((
            Bound::Const(-100.0),
            Bound::Expr(program("if x.f > 50 Hz then 80 else 90")),
        ));
        c.deadband = Some(0.1);
        c.min_interval_ns = 100 * MS;
        c.max_actuations = Some(Budget {
            count: 50,
            window_ns: 60_000 * MS,
        });
    });
    h.image.set_method_buffer(leak(vec![MethodCall::EMPTY; 4]));
    h.image.set_read_sets(leak(vec![0; 2]), 2).unwrap();
    let group = h.registry.resolve_group("fast").unwrap();
    let extra: &'static [Slot] = leak((0..4).map(|_| Slot::new()).collect());
    let alarm_configs: &'static [AlarmConfig<'static>] = leak(vec![AlarmConfig::new(
        0,
        group,
        program("x.f > 50.5 Hz"),
        Severity::High,
    )]);
    let mut alarms = Alarms::new(alarm_configs, leak(vec![AlarmRuntime::EMPTY; 1])).unwrap();
    alarms
        .set_state_arena(leak(vec![0; alarms.state_words()]))
        .unwrap();
    let stmts: &'static [Stmt<'static>] = leak(vec![Stmt::Assign {
        target: h.out(),
        expr: method_program("own(x.p_set) + a0"),
    }]);
    let mut classes = Classes::new(
        leak(vec![InstanceDef {
            methods: leak(vec![MethodDef { params: 1, stmts }]),
        }]),
        leak(vec![DerivedDef {
            signal: SignalId(2),
            group,
            expr: program("held(x.f > 50 Hz, 300ms)"),
        }]),
        leak(vec![Proposal::EMPTY; 4]),
    );
    classes
        .set_state_arena(leak(vec![0; classes.state_words()]))
        .unwrap();
    let buffer = leak(vec![0u8; 1 << 20]);
    let mut segment = SegmentWriter::new(0, buffer).unwrap();
    let mut results = [TraceResult {
        signal: SignalId(0),
        outcome: 0,
        applied: 0,
    }; 1];
    let out = h.out();
    let mut tick = |k: u64, h: &mut Harness, segment: &mut SegmentWriter<'_>| {
        let f = 50.0 + ((k % 7) as f64) * 0.3;
        h.set_input(f, Quality::GOOD, k);
        h.image
            .latch(k, Instant::mono(k * PERIOD), h.slots, &h.registry)
            .unwrap();
        let mut sink = LastResult(None);
        alarms
            .evaluate(&h.image, extra, &mut h.outputs, &mut sink)
            .unwrap();
        classes.evaluate_derived(&h.image, extra).unwrap();
        h.image.mark_read(0, 0);
        h.image
            .stage_cycle(Proposal {
                writer: WriterId(0),
                signal: out,
                value: (k as f64).to_bits(),
                task: 0,
                ..Proposal::EMPTY
            })
            .unwrap();
        let mut call = MethodCall {
            writer: WriterId(0),
            n_args: 1,
            source: Source::Cycle,
            task: 1,
            ..MethodCall::EMPTY
        };
        call.args[0] = 0.5f64.to_bits();
        h.image.stage_method(call).unwrap();
        classes
            .expand(&mut h.image, h.slots, &h.outputs, &mut sink)
            .unwrap();
        h.outputs.commit(&h.image, h.slots, &mut sink).unwrap();
        results[0] = TraceResult::from(&sink.0.unwrap());
        segment
            .push(&TickRecord {
                index: k,
                at: Instant::mono(k * PERIOD),
                inputs: h.image.snapshot(),
                proposals: h.image.direct_proposals(),
                results: &results,
                methods: h.image.method_calls(),
            })
            .unwrap();
        h.image.end_tick().unwrap();
    };
    for k in 0..10 {
        tick(k, &mut h, &mut segment);
    }
    let info = allocation_counter::measure(|| {
        for k in 10..2_000 {
            tick(k, &mut h, &mut segment);
        }
    });
    assert_eq!(info.count_total, 0, "{info:?}");
}
