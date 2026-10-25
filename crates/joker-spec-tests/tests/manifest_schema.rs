//! Every manifest block of the documentation, references included, fits the
//! JSON schema that `joker-manifest` generates from its types (docs/13 §4,
//! task 0a; docs/18 §3).
//!
//! A block is a manifest when every root key is a root key of the schema.
//! Blocks often show a fragment, so the `required` lists are dropped before
//! validating: what is checked is that every key exists where it is
//! written and that every value has the declared type.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken corpus"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

fn without_required(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            map.remove("required");
            for v in map.values_mut() {
                without_required(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(without_required),
        _ => {}
    }
}

fn to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::from(*i),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number)
        }
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.iter().map(to_json).collect()),
        toml::Value::Table(t) => {
            Value::Object(t.iter().map(|(k, v)| (k.clone(), to_json(v))).collect())
        }
    }
}

/// Applies the merge directives of an overlay layer (ref/01 §7.2) the way
/// they reach the effective manifest: `{ unset = true }` removes the key,
/// `{ append = [ … ] }` is the list.
fn resolve_directives(value: &mut toml::Value) {
    if let toml::Value::Table(table) = value {
        table.retain(|_, v| {
            !v.as_table().is_some_and(|t| {
                t.len() == 1 && t.get("unset") == Some(&toml::Value::Boolean(true))
            })
        });
        for (_, v) in table.iter_mut() {
            let append = v
                .as_table()
                .filter(|t| t.len() == 1)
                .and_then(|t| t.get("append"))
                .filter(|a| a.is_array())
                .cloned();
            match append {
                Some(list) => *v = list,
                None => resolve_directives(v),
            }
        }
    }
}

fn is_manifest(table: &toml::Table, block: &common::Block) -> bool {
    !table.is_empty()
        && table
            .keys()
            .all(|k| k.starts_with("x_") || joker_manifest::schema::root_key(k).is_some())
        && !block.text.contains("# proposal")
        && block
            .text
            .lines()
            .filter(|l| l.starts_with("# ") && l.contains(".toml"))
            .count()
            <= 1
}

#[test]
fn the_generated_schema_compiles() {
    let schema: Value = serde_json::from_str(&joker_manifest::schema::json_schema()).unwrap();
    assert!(jsonschema::validator_for(&schema).is_ok());
}

#[test]
fn every_manifest_block_fits_the_generated_schema() {
    let mut schema: Value = serde_json::from_str(&joker_manifest::schema::json_schema()).unwrap();
    without_required(&mut schema);
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut checked = 0;
    let mut references = 0;
    for doc in common::markdown("docs") {
        if doc.contains("/proposals/") || doc.contains("/superpowers/") {
            continue;
        }
        for block in common::blocks(&doc)
            .into_iter()
            .filter(|b| b.lang == "toml")
        {
            let Ok(table) = block.text.parse::<toml::Table>() else {
                continue;
            };
            if !is_manifest(&table, &block) {
                continue;
            }
            checked += 1;
            if doc.starts_with("docs/ref/") {
                references += 1;
            }
            let mut layer = toml::Value::Table(table);
            resolve_directives(&mut layer);
            let instance = to_json(&layer);
            for error in validator.iter_errors(&instance) {
                found
                    .entry(block.short.clone())
                    .or_default()
                    .insert(format!(
                        "{} {}: {}",
                        block.fingerprint(),
                        error.instance_path,
                        error
                    ));
            }
        }
    }
    assert!(checked >= 50, "only {checked} manifest blocks found");
    assert!(
        references >= 20,
        "only {references} manifest blocks found in the references"
    );
    common::ratchet("manifest", &found);
}
