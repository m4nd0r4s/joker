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
    let output = eval_bool(&program, &shared, 123).unwrap();
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
    let output = eval_bool(&program, &shared, 456).unwrap();
    assert_eq!(output.get().unwrap(), joker_expr::Value::Bool(false));
}

#[test]
fn an_unknown_path_fails_to_compile() {
    let loaded = common::bench();
    let plan = loaded.plan;
    assert!(compile_bool("nope.nope", &plan, joker_units::Duration::from_millis(20)).is_err());
}
