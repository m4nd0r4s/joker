#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;

use super::report::{Report, Status};
use super::{Known, run_file};
use crate::guard::NoAlloc;

const PATH: &str = "conformance/envelope/rate.conformance.toml";

const DOCS16_EXAMPLE: &str = r#"# conformance/envelope/rate.conformance.toml
schema = 1

[suite]
kind   = "core"
id     = "envelope.rate"
spec   = ["01 §7 step 5", "16 A7", "16 E2"]
period = "100ms"

[[outputs]]
path  = "x.p_set"
type  = "f64"
unit  = "kW"
range = ["-100 kW", "100 kW"]
rate  = "10 kW/s"
safe  = "0 kW"

[[writers]]
id    = "a"
level = "optimizer:13"
lease = "2s"
kind  = "cycle"

[[case]]
id = "clamp_then_safe"
do = [{ at = "0ms", propose = "x.p_set", writer = "a", value = "50 kW" }]
expect = [
  { at = "0ms",    output = "x.p_set", outcome = "clamped_rate", equals = "1 kW", level = "optimizer:13" },
  { at = "2200ms", output = "x.p_set", outcome = "none",         equals = "0 kW", level = "none" },
]
"#;

fn suite(text: &str) -> super::report::SuiteReport {
    run_file(Path::new(PATH), text, &[], &NoAlloc)
}

fn statuses(text: &str) -> Vec<Status> {
    suite(text).cases.iter().map(|c| c.status).collect()
}

fn suite_codes(text: &str) -> Vec<&'static str> {
    suite(text).diagnostics.iter().map(|d| d.code).collect()
}

fn case_codes(text: &str) -> Vec<&'static str> {
    suite(text)
        .cases
        .iter()
        .flat_map(|c| c.diagnostics.iter().map(|d| d.code))
        .collect()
}

fn with_case(case: &str) -> String {
    let head = DOCS16_EXAMPLE.split("[[case]]").next().unwrap();
    format!("{head}[[case]]\n{case}\n")
}

#[test]
fn docs16_example_passes() {
    let report = suite(DOCS16_EXAMPLE);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert_eq!(report.cases.len(), 1);
    assert_eq!(report.cases[0].id, "envelope.rate.clamp_then_safe");
    assert_eq!(
        report.cases[0].status,
        Status::Pass,
        "{:?}",
        report.cases[0].failures
    );
    assert_eq!(report.cases[0].ticks, 23);
}

#[test]
fn ref03_example_with_an_input_passes() {
    let text = DOCS16_EXAMPLE
        .replace(
            "[[outputs]]",
            "[[signals]]\npath = \"x.f\"\ntype = \"f64\"\nunit = \"Hz\"\n\n[[outputs]]",
        )
        .replace(
            "do = [{",
            "do = [{ at = \"0ms\", input = \"x.f\", value = \"50 Hz\" }, {",
        );
    assert_eq!(statuses(&text), [Status::Pass]);
}

#[test]
fn display_forms_are_exact_synonyms() {
    let text = DOCS16_EXAMPLE
        .replace("\"clamped_rate\"", "\"Clamped(Rate)\"")
        .replace("outcome = \"none\"", "outcome = \"None\"");
    assert_eq!(statuses(&text), [Status::Pass]);
}

#[test]
fn a_wrong_expectation_fails_with_the_tick_and_values() {
    let text = DOCS16_EXAMPLE.replace("equals = \"1 kW\"", "equals = \"2 kW\"");
    let report = suite(&text);
    let case = &report.cases[0];
    assert_eq!(case.status, Status::Fail);
    assert_eq!(case.failures.len(), 1);
    let f = &case.failures[0];
    assert_eq!(
        (f.tick, f.at.as_str(), f.subject.as_str()),
        (0, "0ms", "x.p_set")
    );
    assert!(f.expected.contains("equals 2 kW"), "{}", f.expected);
    assert!(f.actual.contains("applied 1 kW"), "{}", f.actual);
}

#[test]
fn the_sequence_digest_is_deterministic() {
    let a = suite(DOCS16_EXAMPLE).cases[0].digest;
    let b = suite(DOCS16_EXAMPLE).cases[0].digest;
    assert!(a.is_some());
    assert_eq!(a, b);
    let other = DOCS16_EXAMPLE.replace("value = \"50 kW\"", "value = \"10 kW\"");
    assert_ne!(a, suite(&other).cases[0].digest);
}

#[test]
fn schema_must_be_first_and_one() {
    let text = DOCS16_EXAMPLE.replace("schema = 1", "joker = \"1.0\"\nschema = 1");
    assert!(suite_codes(&text).contains(&"J0800"));
    assert!(suite_codes(&DOCS16_EXAMPLE.replace("schema = 1", "schema = 2")).contains(&"J0801"));
    let newer = DOCS16_EXAMPLE.replace("schema = 1", "schema = 1\njoker = \"1.1\"");
    assert_eq!(suite_codes(&newer), ["J0802"]);
    let same = DOCS16_EXAMPLE.replace("schema = 1", "schema = 1\njoker = \"1.0\"");
    assert!(suite_codes(&same).is_empty());
}

#[test]
fn the_file_type_comes_from_the_suffix_and_the_suite_table() {
    let report = run_file(
        Path::new("conformance/envelope/rate.toml"),
        DOCS16_EXAMPLE,
        &[],
        &NoAlloc,
    );
    assert_eq!(report.diagnostics[0].code, "J0803");
    let no_suite = DOCS16_EXAMPLE.replace("[suite]", "[x_suite]");
    assert!(suite_codes(&no_suite).contains(&"J0803"));
}

#[test]
fn kind_must_match_the_location() {
    let text = DOCS16_EXAMPLE.replace("kind   = \"core\"", "kind   = \"class\"");
    assert!(suite_codes(&text).contains(&"J0845"));
    let driver = run_file(
        Path::new("conformance/drivers/rate.conformance.toml"),
        DOCS16_EXAMPLE,
        &[],
        &NoAlloc,
    );
    assert_eq!(driver.diagnostics[0].code, "J0845");
}

#[test]
fn ids_follow_the_grammar_and_are_unique() {
    let bad = DOCS16_EXAMPLE.replace("id = \"clamp_then_safe\"", "id = \"Clamp-Then\"");
    assert_eq!(case_codes(&bad), ["J0805"]);
    let twice = format!(
        "{DOCS16_EXAMPLE}\n[[case]]\nid = \"clamp_then_safe\"\nexpect = [{{ at = \"0ms\", output = \"x.p_set\", level = \"none\" }}]\n"
    );
    assert_eq!(suite_codes(&twice), ["J0805"]);
    let suite_id = DOCS16_EXAMPLE.replace("\"envelope.rate\"", "\"envelope..rate\"");
    assert_eq!(suite_codes(&suite_id), ["J0805"]);
}

#[test]
fn malformed_literals_are_j0806() {
    for (from, to) in [
        ("outcome = \"clamped_rate\"", "outcome = \"clamped(rate)\""),
        ("outcome = \"clamped_rate\"", "outcome = \"CLAMPED_RATE\""),
        ("level = \"optimizer:13\" }", "level = \"optimizer:12\" }"),
        ("at = \"2200ms\"", "at = \"2.2 seconds\""),
    ] {
        let text = DOCS16_EXAMPLE.replacen(from, to, 1);
        assert_eq!(case_codes(&text), ["J0806"], "{to}");
    }
    let period = DOCS16_EXAMPLE.replace("period = \"100ms\"", "period = \"100\"");
    assert_eq!(suite_codes(&period), ["J0806"]);
}

#[test]
fn unknown_and_reserved_keys() {
    let unknown =
        DOCS16_EXAMPLE.replace("kind  = \"cycle\"", "kind  = \"cycle\"\ncolour = \"red\"");
    assert_eq!(suite_codes(&unknown), ["J0140"]);
    let ignored = DOCS16_EXAMPLE.replace("kind  = \"cycle\"", "kind  = \"cycle\"\nx_note = \"ok\"");
    assert!(suite_codes(&ignored).is_empty());
    let reserved = DOCS16_EXAMPLE.replace("kind  = \"cycle\"", "kind  = \"cycle\"\n_note = \"no\"");
    assert_eq!(suite_codes(&reserved), ["J0141"]);
    let sim_only = DOCS16_EXAMPLE.replace(
        "id = \"clamp_then_safe\"",
        "id = \"clamp_then_safe\"\nsim_only = true",
    );
    assert_eq!(case_codes(&sim_only), ["J0140"]);
    let proposal = DOCS16_EXAMPLE.replace("kind  = \"cycle\"", "kind  = \"proposal\"");
    assert_eq!(suite_codes(&proposal), ["J0807"]);
    let metric = with_case(
        "id = \"m\"\nexpect = [{ at = \"0ms\", metric = \"x\", output = \"x.p_set\", level = \"none\" }]",
    );
    assert!(case_codes(&metric).contains(&"J0807"));
}

#[test]
fn predicates_must_be_present_and_compatible() {
    let none = with_case("id = \"p\"\nexpect = [{ at = \"0ms\", output = \"x.p_set\" }]");
    assert_eq!(case_codes(&none), ["J0840"]);
    let tolerance_only = with_case(
        "id = \"p\"\nexpect = [{ at = \"0ms\", output = \"x.p_set\", tolerance = \"1 kW\" }]",
    );
    assert_eq!(case_codes(&tolerance_only), ["J0840"]);
    let both = with_case(
        "id = \"p\"\nexpect = [{ at = \"0ms\", output = \"x.p_set\", equals = \"0 kW\", min = \"0 kW\" }]",
    );
    assert_eq!(case_codes(&both), ["J0840"]);
    let outcome_on_signal = with_case(
        "id = \"p\"\nexpect = [{ at = \"0ms\", signal = \"x.p_set\", outcome = \"none\" }]",
    );
    assert_eq!(case_codes(&outcome_on_signal), ["J0840"]);
}

#[test]
fn instants_are_aligned_with_the_period() {
    let text = DOCS16_EXAMPLE.replace("at = \"2200ms\"", "at = \"2250ms\"");
    assert_eq!(case_codes(&text), ["J0843"]);
    let during = with_case(
        "id = \"d\"\nexpect = [{ during = \"0ms .. 150ms\", output = \"x.p_set\", level = \"none\" }]",
    );
    assert_eq!(case_codes(&during), ["J0843"]);
}

#[test]
fn unknown_references_are_j0844() {
    let writer = DOCS16_EXAMPLE.replace("writer = \"a\"", "writer = \"b\"");
    assert_eq!(case_codes(&writer), ["J0844"]);
    let output = DOCS16_EXAMPLE.replace("propose = \"x.p_set\"", "propose = \"x.q_set\"");
    assert_eq!(case_codes(&output), ["J0844"]);
    let signal = with_case(
        "id = \"s\"\nexpect = [{ at = \"0ms\", signal = \"x.nope\", quality = \"good\" }]",
    );
    assert_eq!(case_codes(&signal), ["J0844"]);
}

#[test]
fn core_config_errors_reach_the_report() {
    let safe = DOCS16_EXAMPLE.replace("safe  = \"0 kW\"", "safe  = \"500 kW\"");
    let report = suite(&safe);
    assert_eq!(report.cases[0].status, Status::Error);
    assert_eq!(report.cases[0].diagnostics[0].code, "J0203");
    let missing = DOCS16_EXAMPLE.replace("safe  = \"0 kW\"\n", "");
    assert_eq!(suite_codes(&missing), ["J0201"]);
}

#[test]
fn features_outside_m0_are_skipped_with_their_paragraph() {
    let text = DOCS16_EXAMPLE.replace(
        "safe  = \"0 kW\"",
        "safe  = \"0 kW\"\ninterlock = \"x.f > 1 Hz\"",
    );
    let report = suite(&text);
    let case = &report.cases[0];
    assert_eq!(case.status, Status::Skip);
    assert_eq!(case.missing[0].name, "interlock");
    assert_eq!(case.missing[0].spec, "01 §7 step 2");
}

#[test]
fn known_failures_gate_the_run() {
    let failing = DOCS16_EXAMPLE.replace("equals = \"1 kW\"", "equals = \"2 kW\"");
    let known = |id: &str| {
        BTreeMap::from([(
            id.to_string(),
            Known {
                reason: "triaged".to_string(),
                issue: None,
            },
        )])
    };
    let none = BTreeMap::new();
    assert!(!Report::new(vec![suite(&failing)], &none, true).passed());
    let listed = Report::new(
        vec![suite(&failing)],
        &known("envelope.rate.clamp_then_safe"),
        true,
    );
    assert!(listed.passed());
    assert_eq!(listed.totals().known, 1);
    let stale = Report::new(
        vec![suite(DOCS16_EXAMPLE)],
        &known("envelope.rate.clamp_then_safe"),
        true,
    );
    assert!(!stale.passed());
    assert_eq!(stale.stale, ["envelope.rate.clamp_then_safe"]);
    let gone = Report::new(
        vec![suite(DOCS16_EXAMPLE)],
        &known("envelope.rate.gone"),
        true,
    );
    assert_eq!(gone.stale, ["envelope.rate.gone"]);
    let filtered = Report::new(
        vec![suite(DOCS16_EXAMPLE)],
        &known("envelope.rate.gone"),
        false,
    );
    assert!(filtered.passed());
}

#[test]
fn the_json_report_carries_counts_and_digests() {
    let report = Report::new(vec![suite(DOCS16_EXAMPLE)], &BTreeMap::new(), true);
    let json: serde_json::Value = serde_json::from_str(&report.json()).unwrap();
    assert_eq!(json["schema"], 1);
    assert_eq!(json["passed"], true);
    assert_eq!(json["totals"]["pass"], 1);
    let case = &json["suites"][0]["cases"][0];
    assert_eq!(case["status"], "pass");
    assert!(
        case["sequence_digest"]
            .as_str()
            .unwrap()
            .starts_with("fnv1a64:")
    );
}
