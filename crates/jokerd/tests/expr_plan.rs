//! AUR-416's pinning test: a `joker_expr::Program` compiled against a
//! jokerd `Plan` (`jokerd::expr_plan::compile_bool`) must read the
//! *same* signal a real write landed on in `Shared.slots` — proving
//! the id scheme used to compile an expression for real evaluation is
//! the one `Shared.slots` is actually indexed by, not
//! `joker-manifest::expr::Registry`'s own, separate enumeration.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use joker_core::Quality;
use joker_core::value::NO_TS_SRC;
use jokerd::expr_plan::{compile_bool, eval_bool};
use jokerd::shared::Shared;

#[test]
fn a_compiled_expression_reads_the_value_actually_written_to_the_plans_own_signal() {
    let loaded = common::bench();
    let plan = loaded.plan;
    let shared = Shared::new(&plan);

    let id = plan
        .id("pi.act_led")
        .expect("the bench manifest declares pi.act_led");
    shared.slots[id.index()].write(1, Quality::GOOD, NO_TS_SRC, 123);

    let program = compile_bool("pi.act_led", &plan, joker_units::Duration::from_millis(20))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let output = eval_bool(&program, &shared, 123, &mut Vec::new()).unwrap();
    assert_eq!(
        output.get().unwrap(),
        joker_expr::Value::Bool(true),
        "the program must read the bit this test actually wrote, at the id Plan::id() assigned"
    );
}

#[test]
fn a_false_write_is_read_as_false() {
    let loaded = common::bench();
    let plan = loaded.plan;
    let shared = Shared::new(&plan);

    let id = plan.id("pi.act_led").unwrap();
    shared.slots[id.index()].write(0, Quality::GOOD, NO_TS_SRC, 456);

    let program = compile_bool("pi.act_led", &plan, joker_units::Duration::from_millis(20))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let output = eval_bool(&program, &shared, 456, &mut Vec::new()).unwrap();
    assert_eq!(output.get().unwrap(), joker_expr::Value::Bool(false));
}

#[test]
fn an_unknown_path_fails_to_compile() {
    let loaded = common::bench();
    let plan = loaded.plan;
    assert!(compile_bool("nope.nope", &plan, joker_units::Duration::from_millis(20)).is_err());
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    dir
}

const ENUM_JOKER_TOML: &str = r#"
[project]
name  = "enum-permit"
joker = "1.0"
mode  = "dev"

[buses.line0]
kind = "serial"
port = "/dev/ttyUSB0"
baud = 9600

[devices.pcs]
driver  = "modbus-rtu"
bus     = "line0"
profile = "acme/pcs@1"
timeout = "1s"
unit_id = 1

[devices.pcs.actions.reset_fault]
permit = "pcs.state == PcsState.fault"
"#;

const ENUM_PROFILE_TOML: &str = r#"
schema = 1

[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"

[enums.PcsState]
off   = 0
run   = 1
fault = 2

[signals.state]
at   = 0
fc   = 4
type = "u16"
enum = "PcsState"

[actions.reset_fault]
steps = [
    { write = { at = 40300, fc = 6, type = "u16", value = 1 } },
]
"#;

fn enum_plan() -> (tempfile::TempDir, jokerd::plan::Plan) {
    let dir = project(&[
        ("joker.toml", ENUM_JOKER_TOML),
        ("profiles/acme/pcs@1.profile.toml", ENUM_PROFILE_TOML),
    ]);
    let loaded = jokerd::load::load(
        &joker_manifest::OsFileSystem,
        &dir.path().join("joker.toml"),
        &jokerd::native::NativeTasks,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    (dir, loaded.plan)
}

fn variant_index(plan: &jokerd::plan::Plan, path: &str, variant: &str) -> u64 {
    let signal = plan.signals.iter().find(|s| s.path == path).unwrap();
    signal.variants.iter().position(|v| v == variant).unwrap() as u64
}

#[test]
fn an_enum_permit_compiles_and_compares_the_enum_value() {
    let (_dir, plan) = enum_plan();
    let program = plan
        .permits
        .get(&("pcs".to_owned(), "reset_fault".to_owned()))
        .expect("an enum permit must compile against the plan (ADR 0060)");
    let shared = Shared::new(&plan);
    let id = plan.id("pcs.state").unwrap();

    let fault = variant_index(&plan, "pcs.state", "fault");
    shared.slots[id.index()].write(fault, Quality::GOOD, NO_TS_SRC, 10);
    let output = eval_bool(program, &shared, 10, &mut Vec::new()).unwrap();
    assert_eq!(output.get().unwrap(), joker_expr::Value::Bool(true));

    let run = variant_index(&plan, "pcs.state", "run");
    shared.slots[id.index()].write(run, Quality::GOOD, NO_TS_SRC, 20);
    let output = eval_bool(program, &shared, 20, &mut Vec::new()).unwrap();
    assert_eq!(output.get().unwrap(), joker_expr::Value::Bool(false));
}

#[test]
fn an_unknown_enum_variant_does_not_compile() {
    let (_dir, plan) = enum_plan();
    let err = compile_bool(
        "pcs.state == PcsState.exploded",
        &plan,
        joker_units::Duration::from_secs(1),
    )
    .unwrap_err();
    assert_eq!(err.code(), "J0303");
}

#[test]
fn a_windowed_permit_keeps_its_state_across_evaluations() {
    let loaded = common::bench();
    let plan = loaded.plan;
    let shared = Shared::new(&plan);
    let id = plan.id("pi.act_led").unwrap();
    let program = compile_bool(
        "held(pi.act_led, 1s)",
        &plan,
        joker_units::Duration::from_millis(100),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));

    let second = 1_000_000_000;
    let mut state = Vec::new();
    for t in 0..=20u64 {
        let now = t * second / 10;
        shared.slots[id.index()].write(1, Quality::GOOD, NO_TS_SRC, now);
        let output = eval_bool(&program, &shared, now, &mut state).unwrap();
        let expected = now >= second;
        assert_eq!(
            output.get().unwrap(),
            joker_expr::Value::Bool(expected),
            "at {now} ns"
        );
    }
    let fresh = eval_bool(&program, &shared, 2 * second, &mut Vec::new()).unwrap();
    assert_eq!(
        fresh.get().unwrap(),
        joker_expr::Value::Bool(false),
        "a fresh arena restarts the timer, which is what the persisted one avoids"
    );
}
