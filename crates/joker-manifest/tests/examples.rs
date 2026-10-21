//! Every manifest in the documentation checks (AUR-226): the complete
//! manifests outside the references are checked in open-world mode, since
//! their profiles, classes and apps are not at hand, and every manifest
//! fragment is validated against the schema.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken corpus"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use joker_manifest::diag::Diagnostics;
use joker_manifest::layers::{self, MemoryFileSystem};
use joker_manifest::schema::{self, Options};
use joker_manifest::source::Sources;
use joker_manifest::{Context, check};
use regex::Regex;

fn docs() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut stack = vec![root.clone()];
    let mut out = Vec::new();
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "ref" || n == "propostas" || n == "proposals")
            {
                continue;
            }
            for entry in std::fs::read_dir(&path).unwrap() {
                stack.push(entry.unwrap().path());
            }
        } else if path.extension().is_some_and(|e| e == "md") {
            let text = std::fs::read_to_string(&path).unwrap();
            out.push((path.strip_prefix(&root).unwrap().to_path_buf(), text));
        }
    }
    out.sort();
    out
}

struct Block {
    doc: PathBuf,
    line: usize,
    name: Option<String>,
    text: String,
}

fn manifest_blocks() -> Vec<Block> {
    let fence = Regex::new(r"(?s)```toml\n(.*?)```").unwrap();
    let named = Regex::new(r"^#\s*(?:[\w./-]+/)?(joker(?:\.[a-z0-9-]+)?\.toml)\b").unwrap();
    let mut blocks = Vec::new();
    for (doc, text) in docs() {
        for m in fence.captures_iter(&text) {
            let body = m.get(1).unwrap();
            let Ok(table) = body.as_str().parse::<toml::Table>() else {
                continue;
            };
            let manifest = !table.is_empty()
                && table
                    .keys()
                    .all(|k| k.starts_with("x_") || schema::root_key(k).is_some());
            if !manifest {
                continue;
            }
            let headers = body
                .as_str()
                .lines()
                .filter(|l| l.starts_with("# ") && l.contains(".toml"))
                .count();
            if headers > 1
                || body.as_str().contains("# proposta")
                || body.as_str().contains("# proposal")
            {
                continue;
            }
            let first = body.as_str().lines().next().unwrap_or("");
            blocks.push(Block {
                line: text[..body.start()].lines().count() + 1,
                name: named.captures(first).map(|c| c[1].to_owned()),
                doc: doc.clone(),
                text: body.as_str().to_owned(),
            });
        }
    }
    blocks
}

/// Places the layers a block extends next to it, by their file name, and
/// whether every one was found in the same document.
fn place(
    fs: &mut MemoryFileSystem,
    dir: &Path,
    text: &str,
    named: &BTreeMap<&str, &Block>,
    depth: usize,
) -> bool {
    let Ok(table) = text.parse::<toml::Table>() else {
        return false;
    };
    let targets: Vec<String> = table
        .get("extends")
        .and_then(|e| e.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    targets.iter().all(|target| {
        let path = joker_manifest::source::normalize(&dir.join(target));
        let name = target.rsplit('/').next().unwrap_or(target);
        match named.get(name) {
            Some(block) if depth < 8 => {
                fs.file(path.clone(), block.text.as_str());
                place(
                    fs,
                    path.parent().unwrap_or(Path::new("")),
                    &block.text,
                    named,
                    depth + 1,
                )
            }
            _ => false,
        }
    })
}

fn extends_of(text: &str) -> Vec<String> {
    text.parse::<toml::Table>()
        .ok()
        .and_then(|t| t.get("extends").and_then(|e| e.as_array()).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            v.as_str()
                .map(|s| s.rsplit('/').next().unwrap_or(s).to_owned())
        })
        .collect()
}

fn errors(report: &joker_manifest::Report) -> String {
    let mode = report.mode();
    report
        .diagnostics
        .sorted(&report.sources)
        .into_iter()
        .filter(|d| d.level(mode) == joker_manifest::Level::Error)
        .map(|d| d.render(&report.sources, mode))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_complete_manifest_in_the_docs_checks() {
    let blocks = manifest_blocks();
    let base = Context {
        open_world: true,
        ..Context::default()
    };
    let mut by_doc: BTreeMap<PathBuf, Vec<&Block>> = BTreeMap::new();
    for block in &blocks {
        by_doc.entry(block.doc.clone()).or_default().push(block);
    }
    let mut failures = Vec::new();
    let mut checked = 0;
    for (doc, blocks) in &by_doc {
        let extended: Vec<String> = blocks.iter().flat_map(|b| extends_of(&b.text)).collect();
        let named: BTreeMap<&str, &Block> = blocks
            .iter()
            .filter_map(|b| b.name.as_deref().map(|n| (n, *b)))
            .collect();
        for block in blocks.iter() {
            let Ok(table) = block.text.parse::<toml::Table>() else {
                continue;
            };
            let has_name = table.get("project").and_then(|p| p.get("name")).is_some();
            let only_project = table.keys().all(|k| k == "project");
            let leaf = block.name.as_ref().is_none_or(|n| !extended.contains(n));
            let extends = extends_of(&block.text);
            if !leaf || only_project || (extends.is_empty() && !has_name) {
                continue;
            }
            let mut fs = MemoryFileSystem::default();
            fs.permissive();
            let root = Path::new("w/x/y");
            let entry = root.join(
                block
                    .name
                    .clone()
                    .unwrap_or_else(|| "joker.toml".to_owned()),
            );
            fs.file(entry.clone(), block.text.as_str());
            if !place(&mut fs, root, &block.text, &named, 0) {
                continue;
            }
            let entry = entry.display().to_string();
            let mut context = base.clone();
            if blocks.iter().any(|b| b.text.contains("joker = \"1.1\"")) {
                context.runtime = joker_manifest::model::SchemaVersion { major: 1, minor: 1 };
            }
            let report = check(&fs, Path::new(&entry), &context);
            let found = errors(&report);
            checked += 1;
            if !found.is_empty() {
                failures.push(format!(
                    "{}:{} ({entry})\n{found}",
                    doc.display(),
                    block.line
                ));
            }
        }
    }
    assert!(checked >= 10, "only {checked} complete manifests found");
    assert!(
        failures.is_empty(),
        "{} manifests fail:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn every_manifest_fragment_in_the_docs_fits_the_schema() {
    let mut failures = Vec::new();
    for block in manifest_blocks() {
        let fs = {
            let mut fs = MemoryFileSystem::default();
            fs.file("joker.fragment.toml", block.text.as_str());
            fs
        };
        let mut sources = Sources::default();
        let mut diags = Diagnostics::default();
        let layers = layers::load(
            &fs,
            Path::new("joker.fragment.toml"),
            &mut sources,
            &mut diags,
        );
        let micro = [
            "[link]",
            "[net.wifi]",
            "[target.micro]",
            "board = \"esp32",
            "board = \"rp2350",
            "kind = \"extern\"",
            "driver = \"extern\"",
            "accept_unsafe_reset",
        ]
        .iter()
        .any(|m| block.text.contains(m));
        for layer in &layers {
            for (key, entry) in &layer.table {
                if let Some(spec) = schema::root_key(key) {
                    schema::validate_entry(
                        spec,
                        entry,
                        key,
                        Options {
                            micro,
                            required: false,
                            v1_1: block.text.contains("joker = \"1.1\""),
                            declared_v1_1: block.text.contains("joker = \"1.1\""),
                        },
                        &mut diags,
                    );
                }
            }
        }
        let found: Vec<String> = diags
            .sorted(&sources)
            .into_iter()
            .filter(|d| !matches!(d.code, "J0290" | "J0160" | "J0161" | "J0162" | "J0169"))
            .map(|d| d.render(&sources, joker_manifest::Mode::Dev))
            .collect();
        if !found.is_empty() {
            failures.push(format!(
                "{}:{}\n{}",
                block.doc.display(),
                block.line,
                found.join("\n")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} fragments fail:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
