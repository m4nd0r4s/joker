//! Type and unit checking (docs/16 §4, ref/01 §3.7) and the rendered
//! diagnostic of every code the checker produces.

#![allow(clippy::unwrap_used, clippy::panic)]

use joker_expr::{
    CheckOptions, Checked, Detail, Diagnostic, EnumId, Env, Repr, SignalInfo, SignalType, Ty, check,
};
use joker_units::{QuantityKind, Unit};

struct TestEnv;

fn sig(id: u32, ty: SignalType, unit: Option<&str>) -> SignalInfo {
    SignalInfo {
        id,
        ty,
        unit: unit.map(|u| Unit::parse(u).unwrap()),
    }
}

const SITE_MODE: EnumId = EnumId(1);
const PUMP_STATE: EnumId = EnumId(2);

fn signals() -> Vec<(&'static str, SignalInfo)> {
    vec![
        ("alarm.p1_hi.active", sig(1, SignalType::Bool, None)),
        ("alarm.p1_lo.active", sig(2, SignalType::Bool, None)),
        ("alarm.p2_hi.active", sig(3, SignalType::Bool, None)),
        ("bms.soc", sig(4, SignalType::F64, Some("%"))),
        ("bms.frac", sig(5, SignalType::F64, None)),
        ("bms.p_max_charge", sig(6, SignalType::F64, Some("kW"))),
        ("inv1.ac.p", sig(7, SignalType::F64, Some("kW"))),
        ("inv2.ac.p", sig(8, SignalType::F64, Some("W"))),
        ("meter.f", sig(9, SignalType::F64, Some("Hz"))),
        ("meter.p", sig(10, SignalType::F64, Some("W"))),
        ("meter.q", sig(11, SignalType::F64, Some("kvar"))),
        ("tank.t", sig(12, SignalType::F64, Some("°C"))),
        ("tank.t_k", sig(13, SignalType::F64, Some("K"))),
        ("tank.level", sig(14, SignalType::F64, Some("m"))),
        ("counter.n", sig(15, SignalType::U64, None)),
        ("counter.m", sig(16, SignalType::U64, None)),
        ("offset.i", sig(17, SignalType::I64, None)),
        ("status.word", sig(18, SignalType::Bits, None)),
        ("site.mode", sig(19, SignalType::Enum(SITE_MODE), None)),
        ("pump.state", sig(20, SignalType::Enum(PUMP_STATE), None)),
        ("dev.name", sig(21, SignalType::String, None)),
        ("pump.run", sig(22, SignalType::Bool, None)),
        ("pcs3.ac.p", sig(23, SignalType::F64, Some("kvar"))),
    ]
}

fn matches(selector: &str, path: &str) -> bool {
    let sel: Vec<&str> = selector.split('.').collect();
    let segs: Vec<&str> = path.split('.').collect();
    fn seg_match(p: &str, s: &str) -> bool {
        match p.split_once('*') {
            None => p == s,
            Some((pre, rest)) => {
                s.starts_with(pre)
                    && (0..=s.len() - pre.len()).any(|i| seg_match(rest, &s[pre.len() + i..]))
            }
        }
    }
    fn go(sel: &[&str], segs: &[&str]) -> bool {
        match (sel.first(), segs.first()) {
            (None, None) => true,
            (Some(&"**"), _) => (1..=segs.len()).any(|n| go(&sel[1..], &segs[n..])),
            (Some(p), Some(s)) => seg_match(p, s) && go(&sel[1..], &segs[1..]),
            _ => false,
        }
    }
    go(&sel, &segs)
}

impl Env for TestEnv {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        signals()
            .into_iter()
            .find(|(p, _)| *p == path)
            .map(|(_, s)| s)
    }

    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)> {
        match (enum_name, variant) {
            ("SiteMode", "auto") => Some((SITE_MODE, 0)),
            ("SiteMode", "maintenance") => Some((SITE_MODE, 1)),
            ("PumpState", "off") => Some((PUMP_STATE, 0)),
            ("PumpState", "run") => Some((PUMP_STATE, 1)),
            _ => None,
        }
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo)) {
        let mut all = signals();
        all.sort_by_key(|(p, _)| *p);
        for (p, s) in all {
            if matches(selector, p) {
                found(s);
            }
        }
    }
}

fn ok(src: &str) -> Checked {
    check(src, &TestEnv, CheckOptions::default()).unwrap_or_else(|e| {
        let mut out = String::new();
        e.render(src, "test", &mut out).unwrap();
        panic!("{out}")
    })
}

fn err(src: &str) -> Diagnostic {
    match check(src, &TestEnv, CheckOptions::default()) {
        Ok(_) => panic!("{src}: expected an error"),
        Err(e) => e,
    }
}

fn rendered(src: &str) -> String {
    let mut out = String::new();
    err(src).render(src, "expr", &mut out).unwrap();
    out
}

fn unit_of(c: &Checked) -> Unit {
    c.result_type().unit().unwrap()
}

#[test]
fn well_typed() {
    for src in [
        "meter.f > 49.8 Hz",
        "meter.p > 50 kW",
        "meter.p + inv1.ac.p < 1 MW",
        "bms.soc >= 5 %",
        "bms.frac >= 0.05",
        "clamp(bms.frac, 0, 1) > 0.5",
        "2 * meter.p > 1 kW",
        "site.mode == SiteMode.maintenance",
        "pump.state != PumpState.run || !pump.run",
        "any(alarm.p1_*.active)",
        "all(alarm.**) && count(alarm.*.active) > 1",
        "sum(inv*.ac.p) > 3 kW",
        "maxs(inv1.ac.p, inv2.ac.p) > 1 kW",
        "if pump.run then meter.p else 0 W",
        "held(meter.f > 50.2 Hz, 2s)",
        "avg(meter.p, 10s) > 5 kW && rate(meter.p, 5s) < 1 kW/s",
        "minw(meter.f, 1m) > 49 Hz && maxw(meter.f, 15m) < 51 Hz",
        "rose(pump.run) || fell(pump.run)",
        "hyst(tank.level, 1 m, 2 m)",
        "bit(status.word, 3)",
        "good(meter.p) && quality(meter.f) == 0 && age(meter.p) < 5 s",
        "tank.t > 20 °C",
        "tank.t > 293 K",
        "tank.t - tank.t > 2 K",
        "tank.t + 2 K > 25 °C",
        "tank.t - 2 K > 25 °C",
        "2 K + tank.t > 25 °C",
        "rate(tank.t, 1m) < 0.5 °C/min",
        "abs(meter.p) < 1 kW",
        "sqrt(meter.p * meter.p + meter.q * meter.q) < 100 kW",
        "pow(tank.level, 2) < 4 m2",
        "pow(bms.frac, 0.5) < 1",
        "counter.n > 3",
        "offset.i < -3",
        "min(meter.p, inv1.ac.p, inv2.ac.p) > 0 W",
        "-bms.p_max_charge < meter.p",
    ] {
        ok(src);
    }
}

#[test]
fn units_of_results() {
    assert_eq!(unit_of(&ok("meter.p + inv1.ac.p")).symbol(), "W");
    assert_eq!(
        unit_of(&ok("meter.p + inv1.ac.p")).kind(),
        Some(QuantityKind::ActivePower)
    );
    assert_eq!(unit_of(&ok("tank.t - tank.t")).symbol(), "K");
    assert_eq!(unit_of(&ok("tank.t + 1 K")).symbol(), "°C");
    assert_eq!(unit_of(&ok("rate(meter.p, 5s)")).symbol(), "W/s");
    assert_eq!(unit_of(&ok("rate(tank.t, 5s)")).symbol(), "K/s");
    assert_eq!(
        unit_of(&ok("rate(meter.q, 5s)")).kind(),
        Some(QuantityKind::ReactivePower)
    );
    assert_eq!(unit_of(&ok("meter.p / meter.f")).symbol(), "W/Hz");
    assert_eq!(unit_of(&ok("age(meter.p)")), Unit::SECOND);
    assert_eq!(unit_of(&ok("sum(inv1.ac.p, inv2.ac.p)")).symbol(), "kW");
    assert_eq!(unit_of(&ok("-bms.p_max_charge")).symbol(), "kW");
    assert_eq!(ok("pump.run").result_type(), Ty::Bool);
    assert_eq!(ok("site.mode").result_type(), Ty::Enum(SITE_MODE));
}

#[test]
fn conversions_are_annotated() {
    let c = ok("meter.p > 50 kW");
    let root = c.ast().root();
    let joker_expr::ast::Node::Binary { lhs, rhs, .. } = c.ast().node(root) else {
        panic!()
    };
    assert!(c.info(lhs).coerce.is_identity());
    assert_eq!(c.info(rhs).coerce.scale, 1000.0);

    let c = ok("50 kW < meter.p");
    let joker_expr::ast::Node::Binary { lhs, rhs, .. } = c.ast().node(c.ast().root()) else {
        panic!()
    };
    assert_eq!(c.info(lhs).coerce.scale, 1000.0);
    assert!(c.info(rhs).coerce.is_identity());

    let c = ok("tank.t > 293.15 K");
    let joker_expr::ast::Node::Binary { rhs, .. } = c.ast().node(c.ast().root()) else {
        panic!()
    };
    assert_eq!(c.info(rhs).coerce.offset, -273.15);
}

#[test]
fn integer_representations() {
    let c = ok("counter.n > counter.m");
    let joker_expr::ast::Node::Binary { lhs, rhs, .. } = c.ast().node(c.ast().root()) else {
        panic!()
    };
    assert!(matches!(
        c.info(lhs).ty,
        Ty::Number {
            repr: Repr::Unsigned,
            ..
        }
    ));
    assert!(matches!(
        c.info(rhs).ty,
        Ty::Number {
            repr: Repr::Unsigned,
            ..
        }
    ));
    let c = ok("offset.i < 3");
    let joker_expr::ast::Node::Binary { rhs, .. } = c.ast().node(c.ast().root()) else {
        panic!()
    };
    assert!(matches!(
        c.info(rhs).ty,
        Ty::Number {
            repr: Repr::Signed,
            ..
        }
    ));
    assert!(matches!(
        c.info(rhs).detail,
        Detail::Const(joker_expr::Const::Int(3))
    ));
}

#[test]
fn selectors_expand_in_path_order() {
    let c = ok("sum(inv*.ac.p)");
    let root = c.ast().root();
    let joker_expr::ast::Node::Call { first, count, .. } = c.ast().node(root) else {
        panic!()
    };
    let sel = c.ast().args(first, count)[0];
    let Detail::Selector { first, count } = c.info(sel).detail else {
        panic!()
    };
    let ids: Vec<(u32, f64)> = c
        .elements(first, count)
        .map(|e| (e.signal.id, e.coerce.scale))
        .collect();
    assert_eq!(ids, vec![(7, 1.0), (8, 0.001)]);
}

#[test]
fn own_only_in_methods() {
    let opts = CheckOptions {
        in_class_method: true,
    };
    assert!(check("own(meter.p) > 1 kW", &TestEnv, opts).is_ok());
    assert_eq!(err("own(meter.p) > 1 kW").code(), "J0306");
}

#[test]
fn result_expectations() {
    assert_eq!(ok("meter.p").expect_bool().unwrap_err().code(), "J0304");
    assert!(ok("meter.p > 1 W").expect_bool().is_ok());
    let conv = ok("-bms.p_max_charge")
        .expect_unit(&Unit::parse("W").unwrap())
        .unwrap();
    assert_eq!(conv.scale, 1000.0);
    assert_eq!(
        ok("meter.f")
            .expect_unit(&Unit::parse("kW").unwrap())
            .unwrap_err()
            .code(),
        "J0301"
    );
}

#[test]
fn codes() {
    for (src, code) in [
        ("meter.p > 50 Hz", "J0301"),
        ("meter.p > 50", "J0301"),
        ("meter.p + 5", "J0301"),
        ("bms.soc >= 0.05", "J0301"),
        ("bms.frac >= 5 %", "J0301"),
        ("clamp(meter.p, 0, 1) > 0 W", "J0301"),
        ("tank.t + tank.t > 1 K", "J0301"),
        ("tank.t * 2 > 1 °C", "J0301"),
        ("2 K - tank.t > 1 K", "J0301"),
        ("-tank.t < 0 °C", "J0301"),
        ("sum(tank.t, tank.t) > 0 °C", "J0301"),
        ("sqrt(tank.level) > 1", "J0301"),
        ("pow(tank.level, bms.frac) > 1", "J0301"),
        ("tank.t > 2 m", "J0301"),
        ("meter.p > 5 kvar", "J0108"),
        ("meter.p + meter.q > 0 W", "J0108"),
        ("sum(*.ac.p) > 0 W", "J0108"),
        ("meter.x > 1 W", "J0302"),
        ("site.mode == SiteMode.offline", "J0303"),
        ("site.mode == Mode.auto", "J0303"),
        ("site.mode == PumpState.run", "J0304"),
        ("pump.run + 1", "J0304"),
        ("meter.p && pump.run", "J0304"),
        ("!meter.p", "J0304"),
        ("if meter.p then 1 else 2", "J0304"),
        ("if pump.run then 1 W else false", "J0304"),
        ("site.mode < SiteMode.auto", "J0304"),
        ("pump.run == 1", "J0304"),
        ("dev.name == 1", "J0304"),
        ("any(meter.*)", "J0304"),
        ("bit(meter.p, 1)", "J0304"),
        ("avg(meter.p, 16m) > 1 W", "J0305"),
        ("rate(meter.p, 1h) > 1 W/s", "J0305"),
        ("avg(meter.p, 0s) > 1 W", "J0152"),
        ("avg(meter.p, 10 kW) > 1 W", "J0152"),
        ("avg(meter.p, 10) > 1 W", "J0152"),
        ("frobnicate(meter.p)", "J0307"),
        ("abs(meter.p, meter.f)", "J0307"),
        ("max(meter.p)", "J0307"),
        ("avg(meter.p, meter.f)", "J0307"),
        ("good(meter.p + 1 W)", "J0307"),
        ("bit(status.word, 64)", "J0307"),
        ("bit(status.word, counter.n)", "J0307"),
        ("any(nothing.*)", "J0155"),
        ("meter.p > 5 kWW", "J0109"),
        ("meter.p > 5 k°C", "J0109"),
        ("a $ b", "J0158"),
    ] {
        assert_eq!(err(src).code(), code, "{src}");
    }
}

#[test]
fn rendered_diagnostics() {
    let cases: &[(&str, &str)] = &[
        (
            "meter.p > 50 Hz",
            "error[J0301]: incompatible dimensions: `W` and `Hz`
 --> expr:1:11
  |
1 | meter.p > 50 Hz
  |           ^^^^^
",
        ),
        (
            "meter.p > 5 kvar",
            "error[J0108]: incompatible quantity kinds: `W` (active_power) and `kvar` (reactive_power)
 --> expr:1:11
  |
1 | meter.p > 5 kvar
  |           ^^^^^^
",
        ),
        (
            "meter.pp > 1 W",
            "error[J0302]: unknown signal
 --> expr:1:1
  |
1 | meter.pp > 1 W
  | ^^^^^^^^
",
        ),
        (
            "site.mode == SiteMode.offline",
            "error[J0303]: unknown enum variant
 --> expr:1:14
  |
1 | site.mode == SiteMode.offline
  |              ^^^^^^^^^^^^^^^^
",
        ),
        (
            "pump.run && meter.p",
            "error[J0304]: expected bool, found number in `W`
 --> expr:1:13
  |
1 | pump.run && meter.p
  |             ^^^^^^^
",
        ),
        (
            "avg(meter.p, 20m) > 1 kW",
            "error[J0305]: window above the maximum (15 min)
 --> expr:1:14
  |
1 | avg(meter.p, 20m) > 1 kW
  |              ^^^
",
        ),
        (
            "own(meter.p) > 1 kW",
            "error[J0306]: `own()` outside a class method
 --> expr:1:1
  |
1 | own(meter.p) > 1 kW
  | ^^^^^^^^^^^^
",
        ),
        (
            "maxx(meter.p, 1 W)",
            "error[J0307]: unknown function
 --> expr:1:1
  |
1 | maxx(meter.p, 1 W)
  | ^^^^
",
        ),
        (
            "tank.t + tank.t > 1 K",
            "error[J0301]: `°C` is an affine scale: only differences and shifts by a difference are allowed
 --> expr:1:1
  |
1 | tank.t + tank.t > 1 K
  | ^^^^^^^^^^^^^^^
",
        ),
        (
            "meter.p > 5 kWW",
            "error[J0109]: unit outside the catalogue
 --> expr:1:13
  |
1 | meter.p > 5 kWW
  |             ^^^
",
        ),
        (
            "meter.p > (1 W",
            "error[J0158]: expected `)`
 --> expr:1:15
  |
1 | meter.p > (1 W
  |               ^
",
        ),
        (
            "any(nothing.*)",
            "error[J0155]: selector matches no signal
 --> expr:1:5
  |
1 | any(nothing.*)
  |     ^^^^^^^^^
",
        ),
    ];
    for (src, want) in cases {
        assert_eq!(rendered(src), *want, "{src}");
    }
}

#[test]
fn multi_line_sources() {
    let src = "pump.run &&\n  meter.p > 5 Hz";
    let mut out = String::new();
    err(src).render(src, "alarms.x.when", &mut out).unwrap();
    assert_eq!(
        out,
        "error[J0301]: incompatible dimensions: `W` and `Hz`
 --> alarms.x.when:2:13
  |
2 |   meter.p > 5 Hz
  |             ^^^^
"
    );
}
