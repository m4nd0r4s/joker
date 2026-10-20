//! Evaluation: quality rules (docs/16 §3.2, §4), stateful functions and
//! windows (ADR 0034), and property tests that the VM never panics.
//!
//! The VM cannot allocate: `joker-expr` links no allocator unless its
//! `alloc` feature is on, and nothing in `eval` is behind that feature, so
//! an allocation would not compile. The property tests cover panics, on
//! valid and on corrupted state.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;

use joker_expr::{
    CheckOptions, CompileOptions, EnumId, Env, Inputs, Output, Program, Quality, QualityCode,
    Reading, Reason, SignalInfo, SignalType, Value, check, compile, eval,
};
use joker_units::{Duration, Unit};
use proptest::prelude::*;

const SIGNALS: &[(&str, u32, SignalType, Option<&str>)] = &[
    ("a.on", 1, SignalType::Bool, None),
    ("b.on", 2, SignalType::Bool, None),
    ("m.p", 3, SignalType::F64, Some("W")),
    ("m.p_kw", 4, SignalType::F64, Some("kW")),
    ("m.f", 5, SignalType::F64, Some("Hz")),
    ("t.c", 6, SignalType::F64, Some("°C")),
    ("n.u", 7, SignalType::U64, None),
    ("n.v", 8, SignalType::U64, None),
    ("n.i", 9, SignalType::I64, None),
    ("s.word", 10, SignalType::Bits, None),
    ("s.mode", 11, SignalType::Enum(EnumId(1)), None),
    ("x.r", 12, SignalType::F64, None),
    ("x.s", 13, SignalType::F64, None),
];

struct TestEnv;

impl Env for TestEnv {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        SIGNALS.iter().find(|s| s.0 == path).map(|s| SignalInfo {
            id: s.1,
            ty: s.2,
            unit: s.3.map(|u| Unit::parse(u).unwrap()),
        })
    }

    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)> {
        match (enum_name, variant) {
            ("Mode", "auto") => Some((EnumId(1), 0)),
            ("Mode", "manual") => Some((EnumId(1), 1)),
            _ => None,
        }
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo)) {
        let prefix = selector.trim_end_matches('*');
        for s in SIGNALS {
            if s.0.starts_with(prefix) && selector.ends_with('*') {
                found(self.signal(s.0).unwrap());
            }
        }
    }
}

#[derive(Default)]
struct World {
    readings: BTreeMap<u32, Reading>,
}

impl World {
    fn set(&mut self, signal: u32, value: Value, quality: Quality) {
        self.readings.insert(
            signal,
            Reading {
                raw: value.to_raw(),
                quality,
                acquired: 0,
            },
        );
    }
}

impl Inputs for World {
    fn read(&self, signal: u32) -> Reading {
        self.readings.get(&signal).copied().unwrap_or(Reading {
            raw: 0,
            quality: Quality::new(QualityCode::Bad, Reason::NOT_CONNECTED),
            acquired: 0,
        })
    }
}

fn program(src: &str, period_ms: u64) -> Program {
    let checked = check(src, &TestEnv, CheckOptions::default()).unwrap();
    compile(
        &checked,
        CompileOptions {
            period: Duration::from_millis(period_ms),
        },
    )
    .unwrap()
}

const GOOD: Quality = Quality::GOOD;
const UNC: Quality = Quality::new(QualityCode::Uncertain, Reason::STALE);
const BAD: Quality = Quality::new(QualityCode::Bad, Reason::COMM_FAILURE);

fn id(path: &str) -> u32 {
    SIGNALS.iter().find(|s| s.0 == path).unwrap().1
}

fn run(src: &str, set: &[(&str, Value, Quality)]) -> Output {
    let p = program(src, 100);
    let mut w = World::default();
    for (path, v, q) in set {
        w.set(id(path), *v, *q);
    }
    let mut state = vec![0u64; p.state_words()];
    eval(&p, &w, 0, &mut state).unwrap()
}

fn b(v: bool) -> Value {
    Value::Bool(v)
}

fn f(v: f64) -> Value {
    Value::F64(v)
}

#[test]
fn arithmetic_and_units() {
    let o = run(
        "m.p + m.p_kw",
        &[("m.p", f(500.0), GOOD), ("m.p_kw", f(2.0), GOOD)],
    );
    assert_eq!(o.get(), Ok(f(2500.0)));
    let o = run("m.p > 2 kW", &[("m.p", f(2500.0), GOOD)]);
    assert_eq!(o.get(), Ok(b(true)));
    let o = run("t.c - t.c", &[("t.c", f(20.0), GOOD)]);
    assert_eq!(o.get(), Ok(f(0.0)));
    let o = run("t.c > 293.15 K", &[("t.c", f(20.5), GOOD)]);
    assert_eq!(o.get(), Ok(b(true)));
    let o = run("clamp(x.r, 0, 1)", &[("x.r", f(3.0), GOOD)]);
    assert_eq!(o.get(), Ok(f(1.0)));
    let o = run(
        "sqrt(pow(x.r, 2) + pow(x.s, 2))",
        &[("x.r", f(3.0), GOOD), ("x.s", f(4.0), GOOD)],
    );
    assert_eq!(o.get(), Ok(f(5.0)));
    let o = run("bit(s.word, 3)", &[("s.word", Value::U64(0b1000), GOOD)]);
    assert_eq!(o.get(), Ok(b(true)));
    let o = run("s.mode == Mode.manual", &[("s.mode", Value::U64(1), GOOD)]);
    assert_eq!(o.get(), Ok(b(true)));
    let o = run(
        "min(x.r, x.s, 7)",
        &[("x.r", f(3.0), GOOD), ("x.s", f(4.0), GOOD)],
    );
    assert_eq!(o.get(), Ok(f(3.0)));
}

#[test]
fn integer_comparisons_are_exact() {
    let big = (1u64 << 53) + 1;
    let o = run(
        "n.u > n.v",
        &[
            ("n.u", Value::U64(big), GOOD),
            ("n.v", Value::U64(big - 1), GOOD),
        ],
    );
    assert_eq!(o.get(), Ok(b(true)));
    let o = run(
        "n.u == n.v",
        &[
            ("n.u", Value::U64(big), GOOD),
            ("n.v", Value::U64(big - 1), GOOD),
        ],
    );
    assert_eq!(o.get(), Ok(b(false)));
    let o = run("n.i < -3", &[("n.i", Value::I64(-4), GOOD)]);
    assert_eq!(o.get(), Ok(b(true)));
    let o = run(
        "n.u + 0 == n.v",
        &[
            ("n.u", Value::U64(big), GOOD),
            ("n.v", Value::U64(big - 1), GOOD),
        ],
    );
    assert_eq!(o.get(), Ok(b(true)));
}

#[test]
fn worst_quality_of_operands() {
    let o = run("x.r + x.s", &[("x.r", f(1.0), UNC), ("x.s", f(2.0), GOOD)]);
    assert_eq!(o.quality(), UNC);
    assert_eq!(o.get_uncertain(), Ok(f(3.0)));
    assert!(o.get().is_err());
    let o = run("x.r + x.s", &[("x.r", f(1.0), UNC), ("x.s", f(2.0), BAD)]);
    assert_eq!(o.quality(), BAD);
    assert!(o.get_uncertain().is_err());
}

#[test]
fn non_finite_is_bad_nan() {
    for (src, set) in [
        ("x.r / x.s", [("x.r", f(1.0), GOOD), ("x.s", f(0.0), GOOD)]),
        (
            "sqrt(x.r) + x.s",
            [("x.r", f(-1.0), GOOD), ("x.s", f(0.0), GOOD)],
        ),
        (
            "x.r + x.s",
            [("x.r", f(f64::NAN), GOOD), ("x.s", f(0.0), GOOD)],
        ),
        (
            "x.r + x.s",
            [("x.r", f(f64::INFINITY), GOOD), ("x.s", f(0.0), GOOD)],
        ),
    ] {
        assert_eq!(run(src, &set).quality(), Quality::BAD_NAN, "{src}");
    }
    let o = run(
        "x.r / x.s > 1",
        &[("x.r", f(0.0), GOOD), ("x.s", f(0.0), GOOD)],
    );
    assert_eq!(o.quality(), Quality::BAD_NAN);
}

#[test]
fn symmetric_short_circuit() {
    let cases = [
        ("a.on && b.on", false, GOOD, true, BAD, Some(false), GOOD),
        ("a.on && b.on", true, BAD, false, GOOD, Some(false), GOOD),
        ("a.on || b.on", true, GOOD, false, BAD, Some(true), GOOD),
        ("a.on || b.on", false, BAD, true, GOOD, Some(true), GOOD),
        ("a.on && b.on", true, GOOD, true, BAD, None, BAD),
        ("a.on && b.on", false, UNC, true, GOOD, None, UNC),
        ("a.on || b.on", false, GOOD, false, UNC, None, UNC),
        ("a.on && b.on", false, UNC, true, BAD, None, BAD),
    ];
    for (src, av, aq, bv, bq, want, wq) in cases {
        let o = run(src, &[("a.on", b(av), aq), ("b.on", b(bv), bq)]);
        assert_eq!(o.quality(), wq, "{src} {av} {aq} {bv} {bq}");
        if let Some(v) = want {
            assert_eq!(o.get(), Ok(b(v)));
        }
    }
}

#[test]
fn aggregations() {
    let set = [("a.on", b(false), GOOD), ("b.on", b(true), BAD)];
    let o = run("all(a.on, b.on)", &set);
    assert_eq!(o.get(), Ok(b(false)));
    let o = run("any(a.on, b.on)", &set);
    assert_eq!(o.quality(), BAD);
    let o = run(
        "any(a.on, b.on)",
        &[("a.on", b(true), GOOD), ("b.on", b(false), BAD)],
    );
    assert_eq!(o.get(), Ok(b(true)));
    let o = run(
        "count(a.on, b.on)",
        &[("a.on", b(true), GOOD), ("b.on", b(true), GOOD)],
    );
    assert_eq!(o.get(), Ok(Value::U64(2)));
    let o = run("count(a.on, b.on)", &set);
    assert_eq!(o.quality(), BAD);
    let o = run(
        "sum(m.p*)",
        &[("m.p", f(500.0), GOOD), ("m.p_kw", f(1.0), GOOD)],
    );
    assert_eq!(o.get(), Ok(f(1500.0)));
    let o = run(
        "maxs(m.p*)",
        &[("m.p", f(500.0), GOOD), ("m.p_kw", f(1.0), UNC)],
    );
    assert_eq!(o.get_uncertain(), Ok(f(1000.0)));
    assert_eq!(o.quality(), UNC);
}

#[test]
fn if_quality() {
    let set = |cq| {
        [
            ("a.on", b(true), cq),
            ("x.r", f(1.0), GOOD),
            ("x.s", f(2.0), BAD),
        ]
    };
    let o = run("if a.on then x.r else x.s", &set(GOOD));
    assert_eq!(o.get(), Ok(f(1.0)));
    let o = run("if a.on then x.r else x.s", &set(UNC));
    assert_eq!(o.quality(), UNC);
    assert_eq!(o.get_uncertain(), Ok(f(1.0)));
    let o = run("if a.on then x.r else x.s", &set(BAD));
    assert_eq!(o.quality(), BAD);
}

#[test]
fn meta_functions() {
    let p = program("good(x.r) && quality(x.s) == 1 && age(x.r) >= 2 s", 100);
    let mut w = World::default();
    w.readings.insert(
        id("x.r"),
        Reading {
            raw: 1f64.to_bits(),
            quality: GOOD,
            acquired: 1_000_000_000,
        },
    );
    w.set(id("x.s"), f(0.0), UNC);
    let mut state = vec![0; p.state_words()];
    let o = eval(&p, &w, 3_000_000_000, &mut state).unwrap();
    assert_eq!(o.get(), Ok(b(true)));
}

struct Timeline {
    program: Program,
    state: Vec<u64>,
    world: World,
    period_ns: u64,
    tick: u64,
}

impl Timeline {
    fn new(src: &str, period_ms: u64) -> Timeline {
        let program = program(src, period_ms);
        let state = vec![0; program.state_words()];
        Timeline {
            program,
            state,
            world: World::default(),
            period_ns: period_ms * 1_000_000,
            tick: 0,
        }
    }

    fn step(&mut self, path: &str, value: Value, quality: Quality) -> Output {
        self.world.set(id(path), value, quality);
        let now = self.tick * self.period_ns;
        self.tick += 1;
        eval(&self.program, &self.world, now, &mut self.state).unwrap()
    }
}

#[test]
fn held_needs_the_condition_for_the_whole_duration() {
    let mut t = Timeline::new("held(a.on, 300ms)", 100);
    let seq = [
        (true, GOOD, false),
        (true, GOOD, false),
        (true, GOOD, false),
        (true, GOOD, true),
        (true, UNC, true),
        (false, GOOD, false),
        (true, GOOD, false),
        (true, BAD, false),
        (true, GOOD, false),
        (true, GOOD, false),
        (true, GOOD, false),
        (true, GOOD, true),
    ];
    for (i, (v, q, want)) in seq.into_iter().enumerate() {
        let o = t.step("a.on", b(v), q);
        assert_eq!(
            o.get_uncertain().ok(),
            (q != BAD).then_some(b(want)),
            "tick {i}"
        );
    }
}

#[test]
fn edges_keep_state_across_bad() {
    let mut t = Timeline::new("rose(a.on)", 100);
    let seq = [
        (true, GOOD, Some(true)),
        (true, GOOD, Some(false)),
        (true, BAD, None),
        (true, GOOD, Some(false)),
        (false, GOOD, Some(false)),
        (true, UNC, Some(true)),
    ];
    for (i, (v, q, want)) in seq.into_iter().enumerate() {
        let o = t.step("a.on", b(v), q);
        assert_eq!(o.get_uncertain().ok(), want.map(b), "tick {i}");
    }
    let mut t = Timeline::new("fell(a.on)", 100);
    assert_eq!(t.step("a.on", b(false), GOOD).get(), Ok(b(false)));
    assert_eq!(t.step("a.on", b(true), GOOD).get(), Ok(b(false)));
    assert_eq!(t.step("a.on", b(false), GOOD).get(), Ok(b(true)));
}

#[test]
fn hysteresis() {
    let mut t = Timeline::new("hyst(x.r, 1, 2)", 100);
    let seq = [
        (1.5, false),
        (2.5, true),
        (1.5, true),
        (0.5, false),
        (1.5, false),
    ];
    for (v, want) in seq {
        assert_eq!(t.step("x.r", f(v), GOOD).get(), Ok(b(want)));
    }
    assert_eq!(t.step("x.r", f(2.5), BAD).quality(), BAD);
    assert_eq!(t.step("x.r", f(1.5), GOOD).get(), Ok(b(false)));
}

#[test]
fn window_warm_up_and_average() {
    let mut t = Timeline::new("avg(x.r, 1s)", 100);
    let o = t.step("x.r", f(1.0), GOOD);
    assert_eq!(o.quality(), Quality::UNCERTAIN_INITIAL);
    assert_eq!(o.get_uncertain(), Ok(f(1.0)));
    for _ in 0..8 {
        t.step("x.r", f(1.0), GOOD);
    }
    let o = t.step("x.r", f(11.0), GOOD);
    assert_eq!(o.get(), Ok(f(2.0)));
    let o = t.step("x.r", f(1.0), GOOD);
    assert_eq!(o.get(), Ok(f(2.0)));
}

#[test]
fn window_quality_follows_the_ten_percent_rule() {
    let mut t = Timeline::new("avg(x.r, 2s)", 100);
    for _ in 0..20 {
        t.step("x.r", f(4.0), GOOD);
    }
    let o = t.step("x.r", f(1e9), BAD);
    assert_eq!(o.quality(), GOOD);
    assert_eq!(o.get(), Ok(f(4.0)));
    let o = t.step("x.r", f(4.0), UNC);
    assert_eq!(o.quality(), UNC);
    let o = t.step("x.r", f(1e9), BAD);
    assert_eq!(o.quality(), BAD);
    for _ in 0..18 {
        t.step("x.r", f(4.0), GOOD);
    }
    let o = t.step("x.r", f(4.0), GOOD);
    assert_eq!(o.quality(), GOOD);
    assert_eq!(o.get(), Ok(f(4.0)));
}

#[test]
fn window_min_max_and_rate() {
    let values = [5.0, 3.0, 8.0, 1.0, 7.0, 2.0, 9.0, 4.0];
    let mut mn = Timeline::new("minw(x.r, 300ms)", 100);
    let mut mx = Timeline::new("maxw(x.r, 300ms)", 100);
    for (i, v) in values.iter().enumerate() {
        let lo = mn.step("x.r", f(*v), GOOD);
        let hi = mx.step("x.r", f(*v), GOOD);
        let window = &values[i.saturating_sub(2)..=i];
        let want_lo = window.iter().cloned().fold(f64::INFINITY, f64::min);
        let want_hi = window.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(lo.get_uncertain(), Ok(f(want_lo)), "min at {i}");
        assert_eq!(hi.get_uncertain(), Ok(f(want_hi)), "max at {i}");
    }
    let mut r = Timeline::new("rate(m.p, 1s)", 100);
    assert_eq!(r.step("m.p", f(0.0), GOOD).quality(), Quality::BAD_NAN);
    for i in 1..=10 {
        let o = r.step("m.p", f(i as f64 * 10.0), GOOD);
        assert!((o.get_uncertain().unwrap().as_f64() - 100.0).abs() < 1e-9);
    }
}

#[test]
fn every_sub_expression_runs_every_tick() {
    let mut t = Timeline::new("if a.on then avg(x.r, 300ms) else 0", 100);
    t.world.set(id("a.on"), b(false), GOOD);
    t.step("x.r", f(3.0), GOOD);
    t.step("x.r", f(3.0), GOOD);
    t.world.set(id("a.on"), b(true), GOOD);
    let o = t.step("x.r", f(6.0), GOOD);
    assert_eq!(o.get(), Ok(f(4.0)));
}

#[test]
fn state_arena_too_small() {
    let p = program("avg(x.r, 1s)", 100);
    let mut state = vec![0; p.state_words() - 1];
    assert!(eval(&p, &World::default(), 0, &mut state).is_err());
}

const EXPRS: &[&str] = &[
    "x.r + x.s * 2 - x.r / x.s",
    "x.r > x.s && a.on || !b.on",
    "if a.on then x.r else -x.s",
    "avg(x.r, 500ms) + minw(x.s, 300ms) - maxw(x.r, 1s) + rate(x.s, 200ms) * 1 s",
    "held(a.on, 250ms) || rose(b.on) && fell(a.on)",
    "hyst(x.r, -1, 1) == a.on",
    "sqrt(abs(x.r)) + pow(x.s, 2) + clamp(x.r, -5, 5)",
    "n.u > n.v || n.i < 0 || bit(s.word, 7)",
    "count(a.on, b.on, x.r > 0) > 1 && any(a.on, b.on) && all(a.on, x.s < 1)",
    "sum(x.r, x.s, 1) + maxs(x.r, x.s) + mins(x.r, x.s) + max(x.r, x.s, 0) + min(x.r, 1)",
    "s.mode == Mode.auto && good(x.r) && quality(x.s) < 2 && age(x.r) < 1 s",
    "t.c - t.c > 1 K || t.c + 1 K > 20 °C || m.p > 1 kW || m.f < 49.9 Hz",
];

fn quality_strategy() -> impl Strategy<Value = Quality> {
    (0u32..4, any::<u8>()).prop_map(|(c, r)| Quality::from_bits(c | (u32::from(r) << 8)))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn vm_never_panics(
        which in 0..EXPRS.len(),
        period_ms in 1u64..200,
        ticks in proptest::collection::vec(
            (proptest::collection::vec((any::<u64>(), quality_strategy()), SIGNALS.len()), 0u64..500_000_000),
            1..40,
        ),
    ) {
        let p = program(EXPRS[which], period_ms);
        let mut state = vec![0u64; p.state_words()];
        let mut now = 0u64;
        let mut w = World::default();
        for (readings, dt) in ticks {
            for ((raw, q), s) in readings.into_iter().zip(SIGNALS) {
                w.readings.insert(s.1, Reading { raw, quality: q, acquired: now / 2 });
            }
            now += dt;
            let out = eval(&p, &w, now, &mut state).unwrap();
            if let Ok(Value::F64(x)) = out.get_uncertain() {
                prop_assert!(x.is_finite());
            }
        }
    }

    #[test]
    fn vm_survives_garbage_state(
        which in 0..EXPRS.len(),
        garbage in proptest::collection::vec(any::<u64>(), 0..4096),
    ) {
        let p = program(EXPRS[which], 50);
        let mut state = garbage;
        state.resize(p.state_words().max(state.len()), u64::MAX);
        let w = World::default();
        let _ = eval(&p, &w, 123, &mut state);
    }
}
