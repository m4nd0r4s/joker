//! `GET /metrics` (ADR 0071): the exposition is parsed back with the rules
//! of the Prometheus text format 0.0.4 (one `HELP` and one `TYPE` per
//! family, before its samples; contiguous families; valid names, labels and
//! values; no duplicate series), and its numbers are those of `Metrics`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{http_get, start};
use joker_api::ws::Limits;
use serde_json::{Value, json};

/// A parsed exposition: family name to type, and series to value.
#[derive(Debug, Default)]
struct Parsed {
    types: BTreeMap<String, String>,
    series: BTreeMap<String, f64>,
}

impl Parsed {
    fn value(&self, series: &str) -> f64 {
        *self
            .series
            .get(series)
            .unwrap_or_else(|| panic!("no series `{series}` in {:#?}", self.series.keys()))
    }
}

fn valid_name(name: &str, colon: bool) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || (colon && c == ':'))
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || (colon && c == ':'))
}

/// Splits `name{labels} value` and checks the labels, returning the
/// series in a canonical form (`name{a="x",b="y"}`) and the value.
fn sample(line: &str) -> (String, String, f64) {
    let (name, rest) = match line.find(['{', ' ']) {
        Some(i) => line.split_at(i),
        None => panic!("no value in `{line}`"),
    };
    assert!(valid_name(name, true), "bad metric name in `{line}`");
    let mut labels = BTreeMap::new();
    let rest = if let Some(body) = rest.strip_prefix('{') {
        let mut i = 0;
        loop {
            if body[i..].starts_with('}') {
                i += 1;
                break;
            }
            let eq = body[i..].find('=').unwrap() + i;
            let label = &body[i..eq];
            assert!(valid_name(label, false), "bad label name in `{line}`");
            assert!(
                body[eq + 1..].starts_with('"'),
                "unquoted label in `{line}`"
            );
            let mut value = String::new();
            i = eq + 2;
            loop {
                let c = body[i..].chars().next().unwrap();
                i += c.len_utf8();
                match c {
                    '\\' => {
                        let next = body[i..].chars().next().unwrap();
                        i += next.len_utf8();
                        value.push(match next {
                            '\\' => '\\',
                            '"' => '"',
                            'n' => '\n',
                            other => panic!("bad escape `\\{other}` in `{line}`"),
                        });
                    }
                    '"' => break,
                    '\n' => panic!("raw newline in a label of `{line}`"),
                    c => value.push(c),
                }
            }
            assert!(
                labels.insert(label.to_owned(), value).is_none(),
                "repeated label in `{line}`"
            );
            if body[i..].starts_with(',') {
                i += 1;
            }
        }
        &body[i..]
    } else {
        rest
    };
    let value = rest.strip_prefix(' ').expect("one space before the value");
    let value = match value {
        "NaN" => f64::NAN,
        "+Inf" => f64::INFINITY,
        "-Inf" => f64::NEG_INFINITY,
        v => v
            .parse()
            .unwrap_or_else(|_| panic!("bad value in `{line}`")),
    };
    let canonical = if labels.is_empty() {
        name.to_owned()
    } else {
        let inner: Vec<String> = labels.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
        format!("{name}{{{}}}", inner.join(","))
    };
    (name.to_owned(), canonical, value)
}

fn family_of(sample: &str, types: &BTreeMap<String, String>) -> String {
    if types.contains_key(sample) {
        return sample.to_owned();
    }
    for suffix in ["_sum", "_count", "_bucket"] {
        if let Some(base) = sample.strip_suffix(suffix)
            && types.contains_key(base)
        {
            return base.to_owned();
        }
    }
    panic!("`{sample}` has no `# TYPE` before it")
}

fn parse(text: &str) -> Parsed {
    assert!(text.ends_with('\n'), "the exposition ends with a newline");
    let mut parsed = Parsed::default();
    let mut helped = BTreeSet::new();
    let mut closed = BTreeSet::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        assert!(!line.is_empty(), "empty line in the exposition");
        if let Some(rest) = line.strip_prefix("# HELP ") {
            let (name, help) = rest.split_once(' ').unwrap();
            assert!(!help.is_empty(), "empty HELP for `{name}`");
            assert!(helped.insert(name.to_owned()), "two HELP for `{name}`");
            if let Some(c) = current.replace(name.to_owned()) {
                closed.insert(c);
            }
            assert!(!closed.contains(name), "family `{name}` is not contiguous");
        } else if let Some(rest) = line.strip_prefix("# TYPE ") {
            let (name, kind) = rest.split_once(' ').unwrap();
            assert!(
                ["counter", "gauge", "summary", "histogram", "untyped"].contains(&kind),
                "bad type in `{line}`"
            );
            assert_eq!(current.as_deref(), Some(name), "TYPE without its HELP");
            assert!(
                parsed
                    .types
                    .insert(name.to_owned(), kind.to_owned())
                    .is_none(),
                "two TYPE for `{name}`"
            );
        } else {
            assert!(!line.starts_with('#'), "unknown comment `{line}`");
            let (name, series, value) = sample(line);
            let family = family_of(&name, &parsed.types);
            assert_eq!(
                current.as_deref(),
                Some(family.as_str()),
                "`{line}` is outside its family"
            );
            if parsed.types[&family] == "counter" {
                assert!(name.ends_with("_total"), "counter `{name}` without _total");
            }
            assert!(
                parsed.series.insert(series, value).is_none(),
                "duplicate series `{line}`"
            );
        }
    }
    parsed
}

fn full_metrics() -> Value {
    json!({
        "at": "2026-10-24T10:00:00Z",
        "groups": [
            {"group": "fast", "ticks": 1200, "overruns": 3, "consecutive_overruns": 0,
             "jitter": {"p50": "12us", "p99": "80us", "p999": "95us", "max": "1.5ms"},
             "duration": {"max": "300us"}},
            {"group": "slow", "ticks": 12, "overruns": 0}
        ],
        "drivers": [
            {"device": "inverter \"A\"\\1", "link": "up", "reconnects": 2, "dropped_samples": 5,
             "invalid_samples": 1, "timeouts": 4, "bytes_rx": 1000, "bytes_tx": 200,
             "coalescing": 0.75, "latency": {"p99": "4ms", "max": "9ms"}},
            {"device": "meter", "link": "down", "reconnects": 0}
        ],
        "envelope": [{"output": "pcs.p_set", "accepted": 10, "clamped": 2, "rejected": 1}],
        "arbiter": {"manual_active": 1, "leases_expiring": 0},
        "apps": [{"app": "ctl", "traps": 0, "restarts": 1, "memory": "12 MiB", "cpu_async": "2.5s"}],
        "store": {"queue": 3, "dropped": 0, "disk_used": "1 GiB"},
        "system": {"temp": "61 °C", "throttled": false, "memory_used": "300 MiB", "disk_free": "2 GB"},
        "runtime": {"dropped": {"decisions": 4, "logs": 0}},
        "ml": null
    })
}

#[test]
fn every_field_of_metrics_maps_to_its_family_and_labels() {
    let text = joker_api::prometheus::render(&full_metrics());
    let p = parse(&text);
    assert_eq!(p.value(r#"joker_cycle_ticks_total{group="fast"}"#), 1200.0);
    assert_eq!(p.value(r#"joker_cycle_overruns_total{group="slow"}"#), 0.0);
    assert_eq!(p.types["joker_cycle_jitter_seconds"], "summary");
    assert_eq!(
        p.value(r#"joker_cycle_jitter_seconds{group="fast",quantile="0.999"}"#),
        95e-6
    );
    assert_eq!(
        p.value(r#"joker_cycle_jitter_max_seconds{group="fast"}"#),
        0.0015
    );
    assert_eq!(
        p.value(r#"joker_cycle_duration_max_seconds{group="fast"}"#),
        0.0003
    );
    let device = r#"device="inverter \"A\"\\1""#;
    assert_eq!(
        p.value(&format!(
            r#"joker_driver_link_state{{{device},state="up"}}"#
        )),
        1.0
    );
    assert_eq!(
        p.value(&format!(
            r#"joker_driver_link_state{{{device},state="down"}}"#
        )),
        0.0
    );
    assert_eq!(
        p.value(r#"joker_driver_link_state{device="meter",state="down"}"#),
        1.0
    );
    assert_eq!(
        p.value(&format!(
            r#"joker_driver_dropped_samples_total{{{device}}}"#
        )),
        5.0
    );
    assert_eq!(
        p.value(&format!(
            r#"joker_driver_latency_seconds{{{device},quantile="0.99"}}"#
        )),
        0.004
    );
    assert_eq!(
        p.value(r#"joker_envelope_results_total{output="pcs.p_set",result="clamped"}"#),
        2.0
    );
    assert_eq!(
        p.value(r#"joker_app_memory_bytes{app="ctl"}"#),
        12.0 * 1_048_576.0
    );
    assert_eq!(
        p.value(r#"joker_app_cpu_async_seconds_total{app="ctl"}"#),
        2.5
    );
    assert_eq!(p.value("joker_store_disk_used_bytes"), 1_073_741_824.0);
    assert_eq!(p.value("joker_system_temperature_celsius"), 61.0);
    assert_eq!(p.value("joker_system_throttled"), 0.0);
    assert_eq!(p.value("joker_system_disk_free_bytes"), 2e9);
    assert_eq!(p.value("joker_arbiter_manual_active"), 1.0);
    assert_eq!(p.types["joker_runtime_dropped_total"], "counter");
    assert_eq!(
        p.value(r#"joker_runtime_dropped_total{queue="decisions"}"#),
        4.0
    );
    assert_eq!(p.value(r#"joker_runtime_dropped_total{queue="logs"}"#), 0.0);
}

#[test]
fn absent_fields_are_left_out_rather_than_reported_as_zero() {
    let text = joker_api::prometheus::render(&json!({
        "at": "now",
        "groups": [{"group": "g", "ticks": 5, "jitter": {"max": "1ms"}}],
    }));
    let p = parse(&text);
    assert_eq!(p.value(r#"joker_cycle_ticks_total{group="g"}"#), 5.0);
    assert!(!text.contains("joker_cycle_overruns_total"), "{text}");
    assert!(!text.contains("joker_cycle_jitter_seconds "), "{text}");
    assert!(
        !p.types.contains_key("joker_cycle_jitter_seconds"),
        "{text}"
    );
    assert!(!text.contains("joker_driver"), "{text}");
    assert!(!text.contains("joker_system"), "{text}");
    let empty = joker_api::prometheus::render(&json!({"at": "now"}));
    assert!(empty.is_empty(), "{empty}");
}

#[test]
fn the_same_snapshot_renders_the_same_bytes() {
    let m = full_metrics();
    assert_eq!(
        joker_api::prometheus::render(&m),
        joker_api::prometheus::render(&m)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_metrics_serves_the_exposition_with_the_api_counters() {
    let f = start(Limits::default()).await;
    *f.stub.metrics.lock().unwrap() = Some(full_metrics());
    let (status, content_type, body) = http_get(&f.socket, "/metrics").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(content_type, "text/plain; version=0.0.4; charset=utf-8");
    let p = parse(&body);
    assert_eq!(p.value(r#"joker_cycle_ticks_total{group="fast"}"#), 1200.0);
    assert_eq!(p.value("joker_api_connections"), 0.0);
    assert_eq!(p.value("joker_api_subscriptions"), 0.0);
    assert_eq!(p.value("joker_api_dropped_total"), 0.0);
    assert_eq!(p.types["joker_api_dropped_total"], "counter");
}
