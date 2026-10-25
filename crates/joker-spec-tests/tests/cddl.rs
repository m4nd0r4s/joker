//! The CDDL files of `spec/cddl/` are complete and the examples of the
//! references fit them (docs/13 §4, task 0a), checked with the reference
//! CDDL tool of RFC 8610 (`cddl`) and `diag2cbor.rb` of `cbor-diag` (ADR
//! 0080).
//!
//! Every file must generate instances that validate back against it, which
//! fails on an undefined, duplicated or unsatisfiable rule. Every example
//! block of the references (`cbor-diag` everywhere, `json` where the
//! reference defines a CDDL encoding) is mapped in `known.toml` to its
//! file and rule, or listed as an excerpt with the reason.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken corpus"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const FILES: [&str; 6] = [
    "agent.cddl",
    "jdp.cddl",
    "joker-link.cddl",
    "joker-meta.cddl",
    "joker-micro.cddl",
    "secrets.cddl",
];

const EXAMPLE_DOCS: [&str; 4] = [
    "docs/ref/07-joker-link.md",
    "docs/ref/08-joker-micro-abi.md",
    "docs/ref/10-agent-protocol.md",
    "docs/ref/04-wit-and-rust-sdk.md",
];

struct Tools {
    cddl: PathBuf,
    diag2cbor: PathBuf,
}

fn tools() -> Option<Tools> {
    let cddl = common::which("cddl");
    let diag2cbor = common::which("diag2cbor.rb");
    if !common::tool("cddl (gem install cddl)", cddl.is_some())
        || !common::tool("diag2cbor.rb (gem install cbor-diag)", diag2cbor.is_some())
    {
        return None;
    }
    Some(Tools {
        cddl: cddl.unwrap(),
        diag2cbor: diag2cbor.unwrap(),
    })
}

fn errors_of(out: &str) -> String {
    out.lines()
        .filter(|l| !l.starts_with("*** Unused rule") && !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate(tools: &Tools, cddl: &Path, instance: &Path) -> Result<(), String> {
    let (ok, out) = common::run(
        Command::new(&tools.cddl)
            .arg(cddl)
            .arg("validate")
            .arg(instance),
    );
    let rest = errors_of(&out);
    if ok && !rest.contains("CDDL validation failure") {
        Ok(())
    } else {
        Err(rest)
    }
}

fn to_cbor(tools: &Tools, dir: &Path, name: &str, diag: &str) -> Result<PathBuf, String> {
    let source = dir.join(format!("{name}.diag"));
    std::fs::write(&source, diag).unwrap();
    let out = Command::new(&tools.diag2cbor)
        .arg(&source)
        .output()
        .unwrap();
    if !out.status.success() || out.stdout.is_empty() {
        return Err(format!(
            "not valid CBOR diagnostic notation: {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
        ));
    }
    let path = dir.join(format!("{name}.cbor"));
    std::fs::write(&path, out.stdout).unwrap();
    Ok(path)
}

#[test]
fn every_cddl_file_generates_instances_that_fit_it() {
    let Some(tools) = tools() else {
        return;
    };
    let dir = common::scratch("cddl-generate");
    let mut problems = Vec::new();
    for file in FILES {
        let path = common::root().join("spec/cddl").join(file);
        let (ok, out) = common::run(
            Command::new(&tools.cddl)
                .arg(&path)
                .arg("generate")
                .arg("5"),
        );
        let instances: Vec<&str> = out
            .lines()
            .filter(|l| !l.starts_with("***") && !l.trim().is_empty())
            .collect();
        if !ok || instances.is_empty() || instances.iter().any(|l| l.contains(".rb:")) {
            problems.push(format!(
                "{file}: generation failed: {}",
                errors_of(&out).lines().next().unwrap_or("")
            ));
            continue;
        }
        for (i, instance) in instances.iter().enumerate() {
            let checked = to_cbor(&tools, &dir, &format!("{file}.{i}"), instance)
                .and_then(|cbor| validate(&tools, &path, &cbor));
            if let Err(e) = checked {
                problems.push(format!(
                    "{file}: generated instance {i} does not validate: {}\n{instance}",
                    e.lines().next().unwrap_or("")
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

#[test]
fn every_example_of_the_references_fits_its_rule() {
    let known = common::Known::load();
    let examples = known.examples();
    let excerpts = known.excerpts("cddl");
    let blocks: Vec<common::Block> = EXAMPLE_DOCS
        .iter()
        .flat_map(|d| common::blocks(d))
        .filter(|b| b.lang == "cbor-diag" || (b.lang == "json" && b.doc.contains("/10-")))
        .collect();
    let mut unmapped = Vec::new();
    let mut seen = BTreeSet::new();
    for block in &blocks {
        let fp = block.fingerprint();
        let key = (fp.clone(), block.section.clone());
        seen.insert(key.clone());
        seen.insert((fp.clone(), String::new()));
        if !examples.contains_key(&key) && !excerpts.contains_key(&fp) {
            unmapped.push(format!(
                "{} §{}: {}",
                block.at(),
                block.section,
                block.text.lines().next().unwrap_or("")
            ));
        }
    }
    let stale: Vec<String> = examples
        .keys()
        .cloned()
        .chain(excerpts.keys().map(|fp| (fp.clone(), String::new())))
        .filter(|key| !seen.contains(key))
        .map(|(fp, section)| format!("{fp} §{section}"))
        .collect();
    assert!(
        unmapped.is_empty() && stale.is_empty(),
        "example blocks without a rule in known.toml:\n{}\nstale entries in known.toml:\n{:?}",
        unmapped.join("\n"),
        stale
    );
    let Some(tools) = tools() else {
        return;
    };
    let dir = common::scratch("cddl-examples");
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for block in &blocks {
        let fp = block.fingerprint();
        let Some((file, rule)) = examples.get(&(fp.clone(), block.section.clone())) else {
            continue;
        };
        let source = common::read(&format!("spec/cddl/{file}"));
        let cddl = dir.join(format!("{fp}-{}.cddl", block.line));
        std::fs::write(&cddl, format!("start = {rule}\n{source}")).unwrap();
        let name = format!("{fp}-{}", block.line);
        let instance = if block.lang == "cbor-diag" {
            match to_cbor(&tools, &dir, &name, &block.text) {
                Ok(path) => path,
                Err(e) => {
                    found
                        .entry(block.short.clone())
                        .or_default()
                        .insert(format!("{fp} §{} ({rule}): {e}", block.section));
                    continue;
                }
            }
        } else {
            let path = dir.join(format!("{name}.json"));
            std::fs::write(&path, &block.text).unwrap();
            path
        };
        if let Err(e) = validate(&tools, &cddl, &instance) {
            let first: String = e
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
                .chars()
                .take(60)
                .collect();
            found
                .entry(block.short.clone())
                .or_default()
                .insert(format!("{fp} §{} ({file} {rule}): {first}", block.section));
        }
    }
    common::ratchet("cddl", &found);
}
