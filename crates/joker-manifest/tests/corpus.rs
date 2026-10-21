//! Checks the code registry against the corpus (docs/16 §9, ref/00 §8).
//!
//! Every code named in the documentation, the machine-readable files and the
//! code is registered; ranges (`A`–`B`) and lists of free numbers do not count.
//! Every `doc` and `also` remission of the registry points at a section that
//! names the code, every code lies in the range of the reference that defines
//! it, and no number a reference lists as free is registered.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken corpus"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use joker_manifest::codes::{CodeId, Document, Family, Registry, Remission, Severity};
use regex::Regex;

const CODE: &str = r"(?:J-E\d{4}|JC\d{4}|JW\d{4}|J\d{4}|W\d{4})";

static MENTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?:^|[^\w-])({CODE})\b")).unwrap());

static RANGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"`?({CODE})`?\s*(?:–|—|-|…|\.\.)\s*`?({CODE})`?|`?({CODE})`?\s+(?:em diante|onwards?|and (?:up|above))"
    ))
    .unwrap()
});

static FREE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"ficam livres|fica livre|(?:stays?|remains?|are|is|left) free").unwrap()
});

static HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#+)\s+(?:§\s*)?(\d+(?:\.\d+)*)\.?\s").unwrap());

const SCANNED_DIRS: [&str; 7] = [
    "docs",
    "spec",
    "wit",
    "crates",
    "conformance",
    "classes",
    "xtask",
];

const SCANNED_EXTENSIONS: [&str; 11] = [
    "md", "toml", "yaml", "yml", "json", "rs", "wit", "cddl", "h", "c", "ts",
];

/// Codes defined by one reference inside the range of another, where the range
/// owner records the assignment. ref/10 §2.20 states that `J0987` was assigned
/// by ref/02 §2.6, against the "no overlaps" rule of docs/16 §9.
const CEDED: [(&str, Document); 1] = [("J0987", Document::Reference(2))];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registry() -> &'static Registry {
    Registry::builtin().unwrap()
}

fn corpus() -> Vec<(PathBuf, String)> {
    let root = root();
    let registry_file = root.join("crates/joker-manifest/codes.toml");
    let mut stack: Vec<PathBuf> = SCANNED_DIRS.iter().map(|dir| root.join(dir)).collect();
    let mut files = Vec::new();
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name.starts_with('.') {
                continue;
            }
            for entry in fs::read_dir(&path).unwrap() {
                stack.push(entry.unwrap().path());
            }
        } else if path.is_file()
            && path != registry_file
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| SCANNED_EXTENSIONS.contains(&e))
        {
            let text = fs::read_to_string(&path).unwrap();
            files.push((path.strip_prefix(&root).unwrap().to_path_buf(), text));
        }
    }
    files.sort();
    files
}

fn sentences(line: &str) -> impl Iterator<Item = &str> {
    line.split_inclusive(". ")
}

/// The codes a line names as codes, without ranges and free lists.
fn mentions(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for sentence in sentences(line) {
        let sentence = RANGE.replace_all(sentence, " ");
        let named = match FREE.find(&sentence) {
            Some(marker) => &sentence[marker.end()..],
            None => &sentence[..],
        };
        found.extend(MENTION.captures_iter(named).map(|c| c[1].to_owned()));
    }
    found
}

/// The numbers a line lists as free, ranges expanded.
fn free_codes(line: &str) -> Vec<CodeId> {
    let mut free = Vec::new();
    for sentence in sentences(line) {
        let Some(marker) = FREE.find(sentence) else {
            continue;
        };
        let listed = &sentence[..marker.start()];
        for range in RANGE.captures_iter(listed) {
            if let (Some(lo), Some(hi)) = (range.get(1), range.get(2)) {
                let (Ok(lo), Ok(hi)) =
                    (lo.as_str().parse::<CodeId>(), hi.as_str().parse::<CodeId>())
                else {
                    continue;
                };
                free.extend(
                    (lo.number()..=hi.number()).filter_map(|n| CodeId::new(lo.family(), n).ok()),
                );
            }
        }
        let singles = RANGE.replace_all(listed, " ");
        free.extend(
            MENTION
                .captures_iter(&singles)
                .filter_map(|c| c[1].parse::<CodeId>().ok()),
        );
    }
    free
}

/// Why a mention is not a registered code, if it is not.
fn unregistered(registry: &Registry, text: &str) -> Option<String> {
    if let Some(digits) = text.strip_prefix('W') {
        let error = format!("J{digits}");
        return match registry.lookup(&error) {
            Some(code) if code.severity() == Some(Severity::ErrorInProd) => None,
            Some(_) => Some(format!("{text} names {error}, which is not error-in-prod")),
            None => Some(format!("{text} names {error}, which is not registered")),
        };
    }
    match registry.lookup(text) {
        Some(_) => None,
        None => Some(format!("{text} is not registered")),
    }
}

fn violations(registry: &Registry, files: &[(PathBuf, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in files {
        for (index, line) in text.lines().enumerate() {
            for mention in mentions(line) {
                if let Some(why) = unregistered(registry, &mention) {
                    found.push(format!("{}:{}: {why}", path.display(), index + 1));
                }
            }
        }
    }
    found
}

fn document_path(document: Document) -> Option<PathBuf> {
    let (dir, prefix) = match document {
        Document::Doc(n) => ("docs", format!("{n:02}-")),
        Document::Reference(n) => ("docs/ref", format!("{n:02}-")),
        Document::Adr(n) => ("docs/adr", format!("{n:04}-")),
    };
    let mut matches: Vec<PathBuf> = fs::read_dir(root().join(dir))
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension().is_some_and(|e| e == "md")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn section<'t>(text: &'t str, number: &str) -> Option<Vec<&'t str>> {
    let mut body: Option<(usize, Vec<&str>)> = None;
    for line in text.lines() {
        if let Some(heading) = HEADING.captures(line) {
            let level = heading[1].len();
            match &body {
                Some((open, _)) if level <= *open => break,
                None if &heading[2] == number => {
                    body = Some((level, Vec::new()));
                    continue;
                }
                _ => {}
            }
        }
        if let Some((_, lines)) = &mut body {
            lines.push(line);
        }
    }
    body.map(|(_, lines)| lines)
}

fn remission_names(remission: &Remission, id: &str) -> Result<(), String> {
    let path = document_path(remission.document())
        .ok_or_else(|| format!("{id}: `{remission}` names no single document"))?;
    let text = fs::read_to_string(&path).map_err(|e| format!("{id}: {e}"))?;
    let lines = match remission.section() {
        Some(number) => section(&text, number)
            .ok_or_else(|| format!("{id}: `{remission}` names a missing section"))?,
        None => text.lines().collect(),
    };
    if lines
        .iter()
        .any(|line| mentions(line).iter().any(|m| m == id))
    {
        Ok(())
    } else {
        Err(format!("{id}: `{remission}` does not cite the code"))
    }
}

/// The subranges of ref/00 §8, read from its table: each row lists ranges in
/// the first cell and names one `ref/NN` owner in the second.
fn ownership() -> Vec<(CodeId, CodeId, Document)> {
    let text = fs::read_to_string(document_path(Document::Reference(0)).unwrap()).unwrap();
    let span = Regex::new(r"`((?:J-E|J)\d{3,4}?)(x?)`\s*–\s*`((?:J-E|J)\d{3,4}?)(x?)`").unwrap();
    let owner = Regex::new(r"\bref/(\d{2})\b").unwrap();
    let mut ranges = Vec::new();
    for line in section(&text, "8").unwrap() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let [_, first, second, ..] = cells[..] else {
            continue;
        };
        let owners: Vec<u8> = owner
            .captures_iter(second)
            .map(|c| c[1].parse().unwrap())
            .collect();
        let [owner] = owners[..] else {
            continue;
        };
        for range in span.captures_iter(first) {
            let lo: CodeId = format!(
                "{}{}",
                &range[1],
                if range[2].is_empty() { "" } else { "0" }
            )
            .parse()
            .unwrap();
            let hi: CodeId = format!(
                "{}{}",
                &range[3],
                if range[4].is_empty() { "" } else { "9" }
            )
            .parse()
            .unwrap();
            ranges.push((lo, hi, Document::Reference(owner)));
        }
    }
    ranges
}

#[test]
fn every_code_in_the_corpus_is_registered() {
    let found = violations(registry(), &corpus());
    assert!(
        found.is_empty(),
        "unregistered codes:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_remission_of_the_registry_cites_its_code() {
    let mut found = Vec::new();
    for code in registry().iter() {
        let id = code.id().to_string();
        for remission in std::iter::once(code.doc()).chain(code.also()) {
            if let Err(why) = remission_names(remission, &id) {
                found.push(why);
            }
        }
    }
    assert!(found.is_empty(), "broken remissions:\n{}", found.join("\n"));
}

#[test]
fn every_code_lies_in_the_range_of_its_reference() {
    let ranges = ownership();
    assert!(ranges.len() >= 14, "ref/00 §8 table not found: {ranges:?}");
    let mut found = Vec::new();
    for code in registry().iter() {
        let id = code.id();
        let defined_by = code.doc().document();
        let Some((_, _, owner)) = ranges.iter().find(|(lo, hi, _)| {
            lo.family() == id.family() && (lo.number()..=hi.number()).contains(&id.number())
        }) else {
            continue;
        };
        let ceded = CEDED
            .iter()
            .any(|(ceded, by)| *ceded == id.to_string() && *by == defined_by);
        if *owner != defined_by && !ceded {
            found.push(format!(
                "{id} is in the range of {owner:?} but defined in {}",
                code.doc()
            ));
        }
    }
    assert!(
        found.is_empty(),
        "codes outside their range:\n{}",
        found.join("\n")
    );
}

#[test]
fn no_free_number_is_registered() {
    let registry = registry();
    let mut found = Vec::new();
    for (path, text) in corpus() {
        for (index, line) in text.lines().enumerate() {
            for id in free_codes(line) {
                if registry.get(id).is_some() {
                    found.push(format!(
                        "{}:{}: {id} is listed as free",
                        path.display(),
                        index + 1
                    ));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "registered codes listed as free:\n{}",
        found.join("\n")
    );
}

#[test]
fn an_injected_unknown_code_is_reported() {
    let unknown = ["J", "0998"].concat();
    let dev_of_error = ["W", "0107"].concat();
    let text = format!("see `{unknown}` and `{dev_of_error}`, but not `J0140`–`J0149`\n");
    let found = violations(registry(), &[(PathBuf::from("injected.md"), text)]);
    assert_eq!(
        found,
        [
            format!("injected.md:1: {unknown} is not registered"),
            format!("injected.md:1: {dev_of_error} names J0107, which is not error-in-prod"),
        ]
    );
}

#[test]
fn ranges_and_free_lists_are_not_mentions() {
    let line = "`J0244`–`J0249` and `J0278` ficam livres (o `J0987` foi atribuído). Use `J0107`.";
    assert_eq!(mentions(line), ["J0987", "J0107"]);
    let free: Vec<String> = free_codes(line).iter().map(ToString::to_string).collect();
    let expected: Vec<String> = (244..=249)
        .chain([278])
        .map(|n| format!("J{n:04}"))
        .collect();
    assert_eq!(free, expected);
    assert!(mentions("códigos `J-E2500` em diante").is_empty());
    assert!(mentions("codes `J-E2500` onwards").is_empty());
    let english = "`J0244`–`J0249` and `J0278` stay free (`J0987` was assigned by ref/02).";
    assert_eq!(mentions(english), ["J0987"]);
    assert_eq!(free_codes(english).len(), 7);
}

#[test]
fn class_codes_are_separate_families() {
    let ids: BTreeSet<Family> = registry().iter().map(|c| c.id().family()).collect();
    assert_eq!(ids.len(), 4);
}
