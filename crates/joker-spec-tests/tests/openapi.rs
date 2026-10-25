//! `spec/openapi/joker-api.yaml` is a valid OpenAPI 3.1 document, follows
//! the reservation rule of ADR 0035 and agrees with ref/09 (docs/13 §4,
//! task 0a).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken corpus"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

fn to_json(y: &yaml_rust2::Yaml) -> Value {
    use yaml_rust2::Yaml;
    match y {
        Yaml::Real(s) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        Yaml::Integer(i) => Value::from(*i),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(a) => Value::Array(a.iter().map(to_json).collect()),
        Yaml::Hash(h) => Value::Object(
            h.iter()
                .map(|(k, v)| {
                    let key = match k {
                        Yaml::String(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Real(r) => r.clone(),
                        Yaml::Boolean(b) => b.to_string(),
                        _ => String::new(),
                    };
                    (key, to_json(v))
                })
                .collect(),
        ),
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => Value::Null,
    }
}

static DOC: LazyLock<Value> = LazyLock::new(|| {
    let text = common::read("spec/openapi/joker-api.yaml");
    let docs = yaml_rust2::YamlLoader::load_from_str(&text).unwrap();
    assert_eq!(docs.len(), 1, "one YAML document");
    to_json(&docs[0])
});

fn resolve(v: &Value) -> &Value {
    match v.get("$ref").and_then(Value::as_str) {
        Some(r) => DOC
            .pointer(r.trim_start_matches('#'))
            .unwrap_or(&Value::Null),
        None => v,
    }
}

fn operations() -> Vec<(String, String, &'static Value, &'static Value)> {
    let mut out = Vec::new();
    for (path, item) in DOC["paths"].as_object().unwrap() {
        for method in METHODS {
            if let Some(op) = item.get(method) {
                out.push((method.to_owned(), path.clone(), op, item));
            }
        }
    }
    out
}

fn refs(v: &Value, at: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == "$ref" {
                    if let Some(r) = child.as_str() {
                        out.push((at.to_owned(), r.to_owned()));
                    }
                } else {
                    refs(child, &format!("{at}/{k}"), out);
                }
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                refs(child, &format!("{at}/{i}"), out);
            }
        }
        _ => {}
    }
}

fn since(op: &Value) -> Option<(u32, u32)> {
    let s = op.get("x-joker-since")?.as_str()?;
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn schema_with_components(schema: &Value) -> Value {
    let mut root = Map::new();
    root.insert(
        "$schema".to_owned(),
        json!("https://json-schema.org/draft/2020-12/schema"),
    );
    if let Some(m) = schema.as_object() {
        for (k, v) in m {
            root.insert(k.clone(), v.clone());
        }
    } else {
        root.insert("allOf".to_owned(), json!([schema]));
    }
    root.insert("components".to_owned(), DOC["components"].clone());
    Value::Object(root)
}

#[test]
fn the_document_has_the_required_structure() {
    let version = DOC["openapi"].as_str().unwrap_or("");
    assert!(version.starts_with("3.1."), "openapi {version}, not 3.1");
    assert!(DOC["info"]["title"].is_string(), "info.title");
    assert!(DOC["info"]["version"].is_string(), "info.version");
    let mut problems = Vec::new();
    let mut ids = BTreeMap::new();
    let template = Regex::new(r"\{([^}]+)\}").unwrap();
    for (method, path, op, item) in operations() {
        let at = format!("{} {path}", method.to_uppercase());
        if !path.starts_with("/v1/") && path != "/v1" && path != "/metrics" {
            problems.push(format!(
                "{at}: path outside /v1 (ADR 0022; /metrics is ADR 0071)"
            ));
        }
        match op["operationId"].as_str() {
            Some(id) => {
                if let Some(other) = ids.insert(id.to_owned(), at.clone()) {
                    problems.push(format!("{at}: operationId {id} repeats {other}"));
                }
            }
            None => problems.push(format!("{at}: no operationId")),
        }
        let responses = op["responses"].as_object();
        match responses {
            Some(r) if !r.is_empty() => {
                for code in r.keys() {
                    let ok = code == "default"
                        || (code.len() == 3
                            && (code.chars().all(|c| c.is_ascii_digit()) || code.ends_with("XX")));
                    if !ok {
                        problems.push(format!("{at}: response code {code}"));
                    }
                }
            }
            _ => problems.push(format!("{at}: no responses")),
        }
        let params: Vec<&Value> = item["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(op["parameters"].as_array().into_iter().flatten())
            .map(resolve)
            .collect();
        for name in template.captures_iter(&path) {
            let name = &name[1];
            let declared = params.iter().any(|p| {
                p["in"] == "path" && p["name"] == name && p["required"] == Value::Bool(true)
            });
            if !declared {
                problems.push(format!(
                    "{at}: path parameter {name} not declared as required"
                ));
            }
        }
        if since(op).is_none() {
            problems.push(format!("{at}: no x-joker-since"));
        }
    }
    let mut all = Vec::new();
    refs(&DOC, "", &mut all);
    for (at, r) in all {
        let resolved = r.strip_prefix('#').and_then(|p| DOC.pointer(p)).is_some();
        if !resolved {
            problems.push(format!("{at}: $ref {r} does not resolve"));
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn every_schema_compiles_and_every_example_fits_it() {
    let mut problems = Vec::new();
    for (name, schema) in DOC["components"]["schemas"].as_object().unwrap() {
        let root = schema_with_components(schema);
        match jsonschema::validator_for(&root) {
            Ok(validator) => {
                for example in schema
                    .get("examples")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .chain(schema.get("example"))
                {
                    for e in validator.iter_errors(example) {
                        problems.push(format!(
                            "schema {name}: example: {e} at {}",
                            e.instance_path
                        ));
                    }
                }
            }
            Err(e) => problems.push(format!("schema {name}: {e}")),
        }
    }
    for (method, path, op, _) in operations() {
        let at = format!("{} {path}", method.to_uppercase());
        let bodies = op["requestBody"].get("content").into_iter().chain(
            op["responses"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(_, r)| resolve(r).get("content")),
        );
        for content in bodies {
            for (media, m) in content.as_object().into_iter().flatten() {
                let Some(schema) = m.get("schema") else {
                    continue;
                };
                let examples: Vec<&Value> = m
                    .get("examples")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .filter_map(|(_, e)| resolve(e).get("value"))
                    .chain(m.get("example"))
                    .collect();
                if examples.is_empty() {
                    continue;
                }
                match jsonschema::validator_for(&schema_with_components(schema)) {
                    Ok(validator) => {
                        for example in examples {
                            for e in validator.iter_errors(example) {
                                problems.push(format!(
                                    "{at} {media}: example: {e} at {}",
                                    e.instance_path
                                ));
                            }
                        }
                    }
                    Err(e) => problems.push(format!("{at} {media}: {e}")),
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn every_operation_after_1_0_answers_501() {
    let not_implemented = "#/components/responses/NotImplemented";
    let description = DOC
        .pointer("/components/responses/NotImplemented/description")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(
        description.contains("J-E1017"),
        "components/responses/NotImplemented does not name J-E1017"
    );
    let mut problems = Vec::new();
    for (method, path, op, _) in operations() {
        let Some(version) = since(op) else {
            continue;
        };
        if version <= (1, 0) {
            continue;
        }
        let answers = op["responses"]["501"]["$ref"].as_str() == Some(not_implemented);
        if !answers {
            problems.push(format!(
                "{} {path} (x-joker-since {}.{}): no 501 response with {not_implemented} (ADR 0035 §3)",
                method.to_uppercase(),
                version.0,
                version.1
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn ref09_and_the_openapi_list_the_same_operations() {
    let text = common::read("docs/ref/09-http-api.md");
    let row = Regex::new(
        r"(?m)^\| ((?:GET|POST|PUT|PATCH|DELETE)(?:, (?:GET|POST|PUT|PATCH|DELETE))*) \| (.+?) \|",
    )
    .unwrap();
    let code = Regex::new(r"`(/[^`]+)`").unwrap();
    let mut documented = BTreeSet::new();
    for m in row.captures_iter(&text) {
        for path in code.captures_iter(&m[2]) {
            let path = path[1].split('?').next().unwrap_or("").to_owned();
            for method in m[1].split(", ") {
                documented.insert((method.to_owned(), path.clone()));
            }
        }
    }
    let contract: BTreeSet<(String, String)> = operations()
        .into_iter()
        .map(|(m, p, _, _)| (m.to_uppercase(), p))
        .collect();
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (method, path) in documented.difference(&contract) {
        found
            .entry("ref/09".to_owned())
            .or_default()
            .insert(format!(
                "{method} {path}: in a ref/09 table, not in the OpenAPI"
            ));
    }
    for (method, path) in contract.difference(&documented) {
        if !text.contains(&format!("`{path}`")) && !text.contains(&format!("{method} {path}")) {
            found
                .entry("ref/09".to_owned())
                .or_default()
                .insert(format!(
                    "{method} {path}: in the OpenAPI, never named in ref/09"
                ));
        }
    }
    assert!(documented.len() > 100, "ref/09 tables not found");
    common::ratchet("openapi", &found);
}
