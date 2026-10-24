//! `joker_manifest::profile::parse`: loading a `.profile.toml` (ref/03
//! §2.2) into a `ProfileInfo` (AUR-414).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::path::Path;

use joker_manifest::context::{Direction, SignalType};
use joker_manifest::profile::parse;

fn ok(text: &str) -> joker_manifest::context::ProfileInfo {
    let report = parse(Path::new("acme/pcs.profile.toml"), text);
    assert!(
        report.profile.is_some(),
        "expected a profile, got errors:\n{}",
        report.render()
    );
    report.profile.unwrap()
}

const HEADER: &str = r#"
[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"
"#;

#[test]
fn a_minimal_profile_with_one_output_signal() {
    let p = ok(&format!(
        "{HEADER}[signals.p_set]\naddress = {{ at = 0, fc = 6 }}\ntype = \"u16\"\ndirection = \"output\"\n"
    ));
    let s = p.signals.get("p_set").expect("p_set");
    assert_eq!(s.ty, SignalType::Integer);
    assert_eq!(s.direction, Direction::Output);
}

#[test]
fn a_signal_keeps_its_raw_address_keys_and_type_name_for_the_driver() {
    let p = ok(&format!(
        "{HEADER}[signals.led]\nat = 0\nfc = 5\ntype = \"bool\"\ndirection = \"output\"\nlabel = \"LED\"\n"
    ));
    let s = p.signals.get("led").expect("led");
    assert_eq!(s.type_name.as_deref(), Some("bool"));
    assert!(
        s.raw.contains_key("at"),
        "{:?}",
        s.raw.keys().collect::<Vec<_>>()
    );
    assert!(s.raw.contains_key("fc"));
    assert!(
        !s.raw.contains_key("label"),
        "generic keys do not reach the driver"
    );
    assert!(!s.raw.contains_key("type"));
    assert!(!s.raw.contains_key("direction"));
}

/// A case: the code a malformed profile reports, the file and what it
/// shows.
type Case = (&'static str, String, &'static str);

fn cases() -> Vec<Case> {
    vec![
        ("J0142", "[signals]\n".to_owned(), "no `[profile]` section"),
        (
            "J0142",
            format!("{HEADER}[signals.p_set]\naddress = {{ at = 0, fc = 6 }}\n"),
            "signal without `type`",
        ),
        (
            "J0809",
            format!("{HEADER}[signals.p_set]\naddress = {{ at = 0 }}\ntype = \"nope\"\n"),
            "unknown raw type",
        ),
        (
            "J0815",
            format!(
                "{HEADER}[signals.p_ac]\naddress = {{ at = 0 }}\ntype = \"f32\"\nunit = \"bogus\"\n"
            ),
            "invalid unit",
        ),
        (
            "J0820",
            format!("{HEADER}[fallback]\ntimeout = \"soon\"\n"),
            "fallback timeout is not a duration",
        ),
        (
            "J0821",
            format!("{HEADER}[actions.reset]\nsteps = []\ntimeout = \"soon\"\n"),
            "action timeout is not a duration",
        ),
        (
            "J0821",
            format!(
                "{HEADER}[actions.set]\nsteps = [{{ write = {{ at = 1, type = \"u16\", value = \"$level\" }} }}]\n"
            ),
            "a step argument the action does not declare",
        ),
    ]
}

#[test]
fn every_case_reports_its_code() {
    for (code, text, what) in cases() {
        let report = parse(Path::new("x.profile.toml"), &text);
        assert!(
            report
                .diagnostics
                .sorted(&report.sources)
                .iter()
                .any(|d| d.code == code),
            "{code} ({what}) not reported; got {:?}\n{}",
            report.diagnostics.sorted(&report.sources),
            report.render()
        );
    }
}

#[test]
fn unit_on_a_signal() {
    let p = ok(&format!(
        "{HEADER}[signals.p_ac]\naddress = {{ at = 0, fc = 4 }}\ntype = \"f32\"\nunit = \"kW\"\n"
    ));
    let s = p.signals.get("p_ac").unwrap();
    assert_eq!(s.ty, SignalType::Float);
    assert!(s.unit.is_some());
}

#[test]
fn an_enum_signal_resolves_its_variants() {
    let p = ok(&format!(
        "{HEADER}[enums]\nPcsMode = {{ off = 0, standby = 1 }}\n[signals.mode]\naddress = {{ at = 0, fc = 3 }}\ntype = \"u16\"\nenum = \"PcsMode\"\n"
    ));
    let s = p.signals.get("mode").unwrap();
    match &s.ty {
        SignalType::Enum { name, variants } => {
            assert_eq!(name, "PcsMode");
            assert_eq!(variants, &vec!["off".to_owned(), "standby".to_owned()]);
        }
        other => panic!("expected an enum, got {other:?}"),
    }
}

#[test]
fn defaults_poll_and_timeout() {
    let p = ok(&format!(
        "{HEADER}[defaults]\npoll = \"200ms\"\ntimeout = \"1s\"\n"
    ));
    assert_eq!(p.poll, Some(joker_units::Duration::from_millis(200)));
    assert_eq!(p.timeout, Some(joker_units::Duration::from_secs(1)));
}

#[test]
fn fallback_presence_and_fixed_timeout() {
    let p = ok(&format!("{HEADER}[fallback]\ntimeout = \"2s\"\n"));
    assert!(p.fallback);
    assert!(p.configurable);
    assert_eq!(
        p.fallback_timeout,
        Some(joker_units::Duration::from_secs(2))
    );
}

#[test]
fn fallback_not_configurable() {
    let p = ok(&format!(
        "{HEADER}[fallback]\ntimeout = \"2s\"\nconfigurable = false\n"
    ));
    assert!(!p.configurable);
}

#[test]
fn no_fallback_section_means_no_fallback() {
    let p = ok(HEADER);
    assert!(!p.fallback);
}

#[test]
fn an_action_with_an_explicit_timeout() {
    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ wait = \"500ms\" }}]\ntimeout = \"9s\"\n"
    ));
    assert_eq!(
        p.actions.get("reset"),
        Some(&Some(joker_units::Duration::from_secs(9)))
    );
}

#[test]
fn an_action_default_timeout_sums_the_waits_plus_a_second_per_step() {
    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ write = {{ at = 1, value = 1 }} }}, {{ wait = \"500ms\" }}, {{ write = {{ at = 1, value = 0 }} }}]\n"
    ));
    // 3 steps x 1s + the 500ms wait.
    assert_eq!(
        p.actions.get("reset"),
        Some(&Some(joker_units::Duration::from_millis(3_500)))
    );
}

#[test]
fn a_write_step_keeps_its_raw_address_and_a_literal_value() {
    use joker_manifest::context::{ActionStep, ActionValue};
    use joker_manifest::value::Value;

    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ write = {{ at = 40300, fc = 6, value = 1 }} }}]\n"
    ));
    let steps = p.action_steps.get("reset").expect("reset steps");
    assert_eq!(steps.len(), 1);
    let ActionStep::Write {
        addr,
        type_name,
        value,
    } = &steps[0]
    else {
        panic!("expected a write step, got {:?}", steps[0]);
    };
    assert!(addr.contains_key("at"));
    assert!(addr.contains_key("fc"));
    assert!(
        !addr.contains_key("value"),
        "step-kind keys do not leak into the address"
    );
    assert!(type_name.is_none());
    assert_eq!(*value, ActionValue::Literal(Value::Integer(1)));
}

#[test]
fn a_write_step_with_an_arg_reference() {
    use joker_manifest::context::{ActionStep, ActionValue};

    let p = ok(&format!(
        "{HEADER}[actions.set_speed]\nargs = {{ level = \"f64\" }}\nsteps = [{{ write = {{ at = 1, type = \"u16\", value = \"$level\" }} }}]\n"
    ));
    let steps = p.action_steps.get("set_speed").expect("set_speed steps");
    let ActionStep::Write {
        type_name, value, ..
    } = &steps[0]
    else {
        panic!("expected a write step, got {:?}", steps[0]);
    };
    assert_eq!(type_name.as_deref(), Some("u16"));
    assert_eq!(*value, ActionValue::Arg("level".to_owned()));
}

#[test]
fn a_wait_step() {
    use joker_manifest::context::ActionStep;

    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ wait = \"500ms\" }}]\n"
    ));
    let steps = p.action_steps.get("reset").expect("reset steps");
    assert_eq!(
        steps[0],
        ActionStep::Wait(joker_units::Duration::from_millis(500))
    );
}

#[test]
fn a_read_step_with_into() {
    use joker_manifest::context::ActionStep;

    let p = ok(&format!(
        "{HEADER}[actions.get_status]\nresult = {{ running = \"bool\" }}\nsteps = [{{ read = {{ at = 2, type = \"bool\", into = \"running\" }} }}]\n"
    ));
    let steps = p.action_steps.get("get_status").expect("get_status steps");
    let ActionStep::Read {
        addr,
        type_name,
        into,
    } = &steps[0]
    else {
        panic!("expected a read step, got {:?}", steps[0]);
    };
    assert!(addr.contains_key("at"));
    assert_eq!(type_name.as_deref(), Some("bool"));
    assert_eq!(into, "running");
}

#[test]
fn an_until_step_with_equals_and_timeout() {
    use joker_manifest::context::ActionStep;
    use joker_manifest::value::Value;

    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ until = {{ at = 2, type = \"bool\", equals = true, timeout = \"2s\" }} }}]\n"
    ));
    let steps = p.action_steps.get("reset").expect("reset steps");
    let ActionStep::Until {
        addr,
        type_name,
        equals,
        timeout,
    } = &steps[0]
    else {
        panic!("expected an until step, got {:?}", steps[0]);
    };
    assert!(addr.contains_key("at"));
    assert_eq!(type_name.as_deref(), Some("bool"));
    assert_eq!(*equals, Value::Bool(true));
    assert_eq!(*timeout, joker_units::Duration::from_secs(2));
}

#[test]
fn a_fail_if_step_with_a_message() {
    use joker_manifest::context::ActionStep;
    use joker_manifest::value::Value;

    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ fail_if = {{ at = 3, type = \"bool\", equals = true, message = \"fault\" }} }}]\n"
    ));
    let steps = p.action_steps.get("reset").expect("reset steps");
    let ActionStep::FailIf {
        addr,
        type_name,
        equals,
        message,
    } = &steps[0]
    else {
        panic!("expected a fail_if step, got {:?}", steps[0]);
    };
    assert!(addr.contains_key("at"));
    assert_eq!(type_name.as_deref(), Some("bool"));
    assert_eq!(*equals, Value::Bool(true));
    assert_eq!(message, "fault");
}

#[test]
fn an_action_default_timeout_now_also_counts_until_budgets() {
    let p = ok(&format!(
        "{HEADER}[actions.reset]\nsteps = [{{ until = {{ at = 2, type = \"bool\", equals = true, timeout = \"2s\" }} }}]\n"
    ));
    // 1 step x 1s + the 2s until budget.
    assert_eq!(
        p.actions.get("reset"),
        Some(&Some(joker_units::Duration::from_secs(3)))
    );
}

#[test]
fn quirks_latency() {
    let p = ok(&format!(
        "{HEADER}\n[profile.quirks]\nlatency = \"15ms\"\nmax_regs = 125\n"
    ));
    assert_eq!(p.latency, Some(joker_units::Duration::from_millis(15)));
    assert!(p.quirks.contains_key("latency"));
    assert!(
        p.quirks.contains_key("max_regs"),
        "the raw table keeps every key"
    );
}

#[test]
fn an_action_keeps_its_args_result_long_and_description() {
    let p = ok(&format!(
        "{HEADER}[actions.calibrate]\nargs = {{ span = \"f64 %\", fast = \"bool\" }}\nresult = {{ offset = \"i64\" }}\nlong = true\ndescription = \"zero the sensor\"\nsteps = [{{ write = {{ at = 1, type = \"u16\", value = \"$span\" }} }}]\n"
    ));
    let decl = p.action_decls.get("calibrate").expect("calibrate");
    assert_eq!(decl.args.get("span").map(String::as_str), Some("f64 %"));
    assert_eq!(decl.args.get("fast").map(String::as_str), Some("bool"));
    assert_eq!(decl.result.get("offset").map(String::as_str), Some("i64"));
    assert!(decl.long);
    assert!(decl.native.is_none());
    assert_eq!(decl.description.as_deref(), Some("zero the sensor"));
}

#[test]
fn a_native_action_keeps_its_table_and_has_no_steps() {
    let p = ok(&format!(
        "{HEADER}[actions.identify]\nnative = {{ method = \"ns=2;s=Identify\" }}\ntimeout = \"2s\"\n"
    ));
    let decl = p.action_decls.get("identify").expect("identify");
    let native = decl.native.as_ref().expect("a native table");
    assert_eq!(
        native.get("method").and_then(|n| n.as_str()),
        Some("ns=2;s=Identify")
    );
    assert!(!p.action_steps.contains_key("identify"));
    assert!(!decl.long);
}

#[test]
fn implements_maps_class_actions_to_profile_actions() {
    let p = ok(&format!(
        "{HEADER}[actions.reset_fault]\nsteps = [{{ wait = \"10ms\" }}]\n[implements.\"std/pump@1\"]\nmap = {{ reset = \"actions.reset_fault\", stop = \"actions.missing\", running = \"status.running\" }}\n"
    ));
    assert_eq!(
        p.implemented_actions.get("reset").map(String::as_str),
        Some("reset_fault")
    );
    assert!(!p.implemented_actions.contains_key("stop"));
    assert!(!p.implemented_actions.contains_key("running"));
}

#[test]
fn enums_keep_their_variant_values() {
    let p = ok(&format!(
        "{HEADER}[enums]\nPcsMode = {{ off = 0, standby = 3 }}\n"
    ));
    let mode = p.enums.get("PcsMode").expect("PcsMode");
    assert_eq!(mode.get("off"), Some(&0));
    assert_eq!(mode.get("standby"), Some(&3));
}
