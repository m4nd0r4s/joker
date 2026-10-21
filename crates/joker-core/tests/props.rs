//! Property tests of the arbiter and envelope invariants (docs/16 §1–§2),
//! against a reference model of leases. `PROPTEST_CASES` sets the number of
//! cases: the default per commit, 10⁶ in the nightly job (docs/13 §5).

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use joker_core::output::{ClockRequirement, OnRange};
use joker_core::time::Instant;
use joker_core::{
    Applied, ClockQuality, Level, Outcome, Proposal, RejectReason, WriterConfig, WriterId,
    WriterKind,
};
use proptest::prelude::*;

#[derive(Clone, Debug)]
struct Env {
    low: f64,
    high: f64,
    safe: f64,
    rate: Option<f64>,
    on_range: OnRange,
}

#[derive(Clone, Debug)]
struct Step {
    gap: u64,
    clock: ClockQuality,
    reset: bool,
    proposals: Vec<(u32, f64)>,
}

fn env() -> impl Strategy<Value = Env> {
    (
        -200.0f64..0.0,
        1.0f64..200.0,
        0.0f64..1.0,
        prop::option::of(0.5f64..500.0),
        prop_oneof![
            Just(OnRange::Clamp),
            Just(OnRange::Reject),
            Just(OnRange::Trip)
        ],
    )
        .prop_map(|(low, high, t, rate, on_range)| Env {
            low,
            high,
            safe: low + t * (high - low),
            rate,
            on_range,
        })
}

fn value() -> impl Strategy<Value = f64> {
    prop_oneof![8 => -300.0f64..300.0, 1 => Just(f64::NAN), 1 => Just(f64::INFINITY)]
}

fn step() -> impl Strategy<Value = Step> {
    (
        prop_oneof![6 => Just(1u64), 1 => 2u64..30],
        prop_oneof![
            8 => Just(ClockQuality::Synced),
            1 => Just(ClockQuality::Holdover),
            1 => Just(ClockQuality::Unsynced)
        ],
        prop::bool::weighted(0.05),
        prop::collection::vec((0u32..4, value()), 0..4),
    )
        .prop_map(|(gap, clock, reset, proposals)| Step {
            gap,
            clock,
            reset,
            proposals,
        })
}

fn writers() -> Vec<WriterConfig> {
    let w = |kind, level: &str, lease_ms: u64, clock| WriterConfig {
        kind,
        level: Level::parse(level).unwrap(),
        lease_ns: Some(lease_ms * MS),
        requires_clock: clock,
    };
    vec![
        w(
            WriterKind::Cycle,
            "optimizer:13",
            400,
            ClockRequirement::Any,
        ),
        w(
            WriterKind::Cycle,
            "control:9",
            1_000,
            ClockRequirement::Synced,
        ),
        w(WriterKind::Cycle, "safety:1", 300, ClockRequirement::Any),
        w(WriterKind::Manual, "manual:4", 700, ClockRequirement::Any),
    ]
}

struct Model {
    lease_until: [Option<u64>; 4],
    value: [f64; 4],
}

impl Model {
    fn active(&self, writers: &[WriterConfig], now: u64) -> Option<(usize, Level)> {
        (0..4)
            .filter(|&w| self.lease_until[w].is_some_and(|t| now < t))
            .map(|w| (w, writers[w].level))
            .min_by_key(|&(_, l)| l)
    }
}

fn run(env: &Env, steps: &[Step]) -> Result<Vec<(u8, u64, Option<u8>)>, TestCaseError> {
    let ws = writers();
    let mut h = Harness::new(ws.clone(), |c| {
        c.range = Some((env.low, env.high));
        c.safe = joker_core::SafeValue::Value(env.safe.to_bits());
        c.rate = env.rate;
        c.on_range = env.on_range;
    });
    let out = h.out();
    let mut model = Model {
        lease_until: [None; 4],
        value: [0.0; 4],
    };
    let mut k = 0;
    let mut last = env.safe;
    let mut tripped = false;
    let mut history = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            k += step.gap;
        }
        let now = k * PERIOD;
        let dt = if i == 0 { PERIOD } else { step.gap * PERIOD };
        if step.reset && h.outputs.reset(out) {
            tripped = false;
        }
        let at = Instant {
            mono_ns: now,
            utc_ns: 0,
            clock: step.clock,
        };
        h.image.latch(k, at, h.slots, &h.registry).unwrap();
        for &(w, v) in &step.proposals {
            h.image
                .stage_cycle(Proposal {
                    writer: WriterId(w),
                    signal: out,
                    value: v.to_bits(),
                    ..Proposal::EMPTY
                })
                .unwrap();
            if v.is_finite() {
                model.lease_until[w as usize] = Some(now + ws[w as usize].lease_ns.unwrap());
                model.value[w as usize] = v;
            }
        }
        let mut log = Log::default();
        h.outputs.commit(&h.image, h.slots, &mut log).unwrap();
        h.image.end_tick().unwrap();
        let r = log.outputs[0];
        let Applied::Value(bits) = r.applied else {
            return Err(TestCaseError::fail("relinquish was never configured"));
        };
        let applied = f64::from_bits(bits);
        let expected = model.active(&ws, now);
        prop_assert_eq!(r.level, expected.map(|(_, l)| l), "lease model, tick {}", k);
        prop_assert!(
            applied >= env.low && applied <= env.high,
            "applied {} outside [{}, {}]",
            applied,
            env.low,
            env.high
        );
        let clock_refused = expected
            .is_some_and(|(w, _)| ws[w].requires_clock == ClockRequirement::Synced)
            && step.clock != ClockQuality::Synced;
        let out_of_range =
            expected.is_some_and(|(w, _)| !(env.low..=env.high).contains(&model.value[w]));
        if env.on_range == OnRange::Trip && out_of_range && !clock_refused {
            tripped = true;
        }
        prop_assert_eq!(r.tripped, tripped, "trip model, tick {}", k);
        if expected.is_none() || tripped || clock_refused {
            prop_assert_eq!(applied, env.safe, "safe expected, tick {}", k);
        } else if let Some(rate) = env.rate {
            let bound = rate * dt as f64 / 1e9;
            prop_assert!(
                (applied - last).abs() <= bound * (1.0 + 1e-9) + 1e-9,
                "rate: {} -> {} over {} ns",
                last,
                applied,
                dt
            );
        }
        if clock_refused && r.outcome != Outcome::None {
            prop_assert!(matches!(
                r.outcome,
                Outcome::Rejected(RejectReason::ClockUnsynced)
                    | Outcome::Rejected(RejectReason::WrongType)
                    | Outcome::Overridden(_)
                    | Outcome::Tripped
            ));
        }
        last = applied;
        history.push((r.outcome.code(), bits, r.level.map(|l| l.get())));
    }
    Ok(history)
}

proptest! {
    #[test]
    fn arbiter_and_envelope_invariants(env in env(), steps in prop::collection::vec(step(), 1..60)) {
        let first = run(&env, &steps)?;
        let second = run(&env, &steps)?;
        prop_assert_eq!(first, second);
    }
}
