//! The counts that the prose still repeats agree with their sources
//! (docs/13 §5.1, docs/26 §4 E7), and the code registry follows the
//! reservation rule of ADR 0035 §4.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken corpus"
)]

mod common;

use regex::Regex;

fn openapi_operations() -> usize {
    let text = common::read("spec/openapi/joker-api.yaml");
    let method = Regex::new(r"(?m)^    (get|put|post|delete|options|head|patch|trace):").unwrap();
    method.find_iter(&text).count()
}

fn link_bodies() -> usize {
    let text = common::read("spec/cddl/joker-link.cddl");
    let start = text.find("link-body =").unwrap();
    let rest = &text[start + "link-body =".len()..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    rest[..end]
        .split('/')
        .filter(|a| !a.trim().is_empty())
        .count()
}

fn link_table_messages() -> usize {
    let text = common::read("docs/ref/07-joker-link.md");
    let start = text.find("### 6.1 General table").unwrap();
    let end = start + text[start..].find("\n\n- ").unwrap();
    let row = Regex::new(r"(?m)^\| `0x[0-9A-F]{2}` \| `[A-Z_]+` \|.*\| 1\.0 \|$").unwrap();
    row.find_iter(&text[start..end]).count()
}

fn number(doc: &str, pattern: &str) -> usize {
    let text = common::read(doc);
    let re = Regex::new(pattern).unwrap();
    let caps = re
        .captures(&text)
        .unwrap_or_else(|| panic!("{doc}: the count `{pattern}` is gone; update this test"));
    caps[1].parse().unwrap()
}

#[test]
fn the_joker_link_message_count_matches_the_cddl_and_the_table() {
    let cddl = link_bodies();
    let table = link_table_messages();
    assert_eq!(
        cddl, table,
        "spec/cddl/joker-link.cddl link-body has {cddl} bodies, ref/07 §6.1 has {table} 1.0 messages"
    );
    for (doc, pattern) in [
        (
            "docs/ref/07-joker-link.md",
            r"the (\d+) messages, with CDDL",
        ),
        ("docs/26-final-review.md", r"(\d+) Joker Link messages"),
    ] {
        let prose = number(doc, pattern);
        assert_eq!(
            prose, cddl,
            "{doc} says {prose} messages, the CDDL has {cddl}"
        );
    }
}

#[test]
fn the_api_operation_count_is_close_to_the_openapi() {
    let ops = openapi_operations();
    let prose = number("docs/26-final-review.md", r"~(\d+) operations in the API");
    let low = ops * 85 / 100;
    let high = ops * 115 / 100;
    assert!(
        (low..=high).contains(&prose),
        "docs/26 says ~{prose} API operations, the OpenAPI has {ops}"
    );
}

#[test]
fn the_reference_count_matches_docs_ref() {
    let files = common::markdown("docs/ref").len();
    let prose = number("docs/26-final-review.md", r"counted: (\d+) references");
    assert_eq!(
        prose, files,
        "docs/26 says {prose} references, docs/ref has {files}"
    );
}

#[test]
fn every_code_after_1_0_is_reserved() {
    let text = common::read("crates/joker-manifest/codes.toml");
    let registry: toml::Table = text.parse().unwrap();
    let mut problems = Vec::new();
    for code in registry["code"].as_array().unwrap() {
        let id = code["id"].as_str().unwrap();
        let status = code
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("active");
        let since = code.get("since").and_then(|s| s.as_str());
        let later = since.is_some_and(|s| {
            let mut parts = s.split('.');
            let major: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            let minor = parts.next().unwrap_or("0");
            major > 1
                || (major == 1 && (minor == "x" || minor.parse::<u32>().unwrap_or(0) > 0))
                || parts.next().is_some()
        });
        if later && status != "reserved" {
            problems.push(format!(
                "{id}: since {} but status {status} (ADR 0035 §4)",
                since.unwrap_or("")
            ));
        }
        if status == "reserved" && since.is_none() {
            problems.push(format!("{id}: reserved without since (ADR 0035 §4)"));
        }
        if !matches!(status, "active" | "reserved") {
            problems.push(format!("{id}: unknown status {status}"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
