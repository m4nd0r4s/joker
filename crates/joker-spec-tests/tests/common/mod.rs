//! Shared helpers of the reference checks: the repository paths, the code
//! blocks of the documents, the known divergences of `known.toml` and the
//! external tools.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken corpus"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// The repository root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Reads a file of the repository, by its path from the root.
pub fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The Markdown files under a directory of the repository, sorted, by their
/// path from the root.
pub fn markdown(dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root().join(dir)];
    let base = root();
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path).unwrap() {
                stack.push(entry.unwrap().path());
            }
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = path.strip_prefix(&base).unwrap();
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    out.sort();
    out
}

/// A fenced code block of a document.
#[derive(Debug, Clone)]
pub struct Block {
    /// The document, by its path from the root.
    pub doc: String,
    /// The short name of the document (`ref/04`, `04`).
    pub short: String,
    /// The line of the opening fence.
    pub line: usize,
    /// The info string of the fence.
    pub lang: String,
    /// The number of the closest heading above the block (`6.4.1`), or empty.
    pub section: String,
    /// The text between the fences.
    pub text: String,
}

impl Block {
    /// The first 12 hex digits of the SHA-256 of the text: the identity of
    /// a block in `known.toml`, which survives edits elsewhere in the
    /// document and changes with the block.
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.text)
    }

    /// Where the block is, for messages.
    pub fn at(&self) -> String {
        format!("{}:{} ({})", self.doc, self.line, self.fingerprint())
    }
}

/// The first 12 hex digits of the SHA-256 of a text.
pub fn fingerprint(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// The short name of a document: `docs/ref/04-wit-and-rust-sdk.md` is
/// `ref/04`, `docs/16-core-semantics.md` is `16`.
pub fn short(doc: &str) -> String {
    let rel = doc.trim_start_matches("docs/");
    let (dir, file) = match rel.rsplit_once('/') {
        Some((d, f)) => (Some(d), f),
        None => (None, rel),
    };
    let stem = file
        .split('-')
        .next()
        .unwrap_or(file)
        .trim_end_matches(".md");
    match dir {
        Some(d) => format!("{d}/{stem}"),
        None => stem.to_owned(),
    }
}

/// The fenced code blocks of a document.
pub fn blocks(doc: &str) -> Vec<Block> {
    let text = read(doc);
    let mut out = Vec::new();
    let mut section = String::new();
    let mut open: Option<(usize, String, String)> = None;
    for (i, line) in text.lines().enumerate() {
        match &mut open {
            Some((start, lang, body)) => {
                if line.trim_start().starts_with("```") {
                    out.push(Block {
                        doc: doc.to_owned(),
                        short: short(doc),
                        line: *start,
                        lang: lang.clone(),
                        section: section.clone(),
                        text: std::mem::take(body),
                    });
                    open = None;
                } else {
                    body.push_str(line);
                    body.push('\n');
                }
            }
            None => {
                if let Some(info) = line.strip_prefix("```") {
                    open = Some((i + 1, info.trim().to_owned(), String::new()));
                } else if line.starts_with('#') {
                    let heading = line.trim_start_matches('#').trim();
                    section = heading
                        .split_whitespace()
                        .next()
                        .filter(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
                        .map(|w| w.trim_end_matches('.').to_owned())
                        .unwrap_or_default();
                }
            }
        }
    }
    out
}

/// The blocks of one language in the references (`docs/ref/`).
pub fn reference_blocks(lang: &str) -> Vec<Block> {
    markdown("docs/ref")
        .iter()
        .flat_map(|d| blocks(d))
        .filter(|b| b.lang == lang)
        .collect()
}

/// The known divergences and excerpts (`known.toml`).
#[derive(Debug)]
pub struct Known {
    table: toml::Table,
}

impl Known {
    /// Loads `known.toml`.
    pub fn load() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("known.toml");
        let text = std::fs::read_to_string(path).unwrap();
        Self {
            table: text.parse().unwrap(),
        }
    }

    /// The fingerprints of the blocks listed as excerpts of a check: blocks
    /// that are deliberately partial and are not checked alone.
    pub fn excerpts(&self, check: &str) -> BTreeMap<String, String> {
        self.table
            .get("excerpt")
            .and_then(|e| e.as_array())
            .into_iter()
            .flatten()
            .filter_map(|e| e.as_table())
            .filter(|e| e.get("check").and_then(|c| c.as_str()) == Some(check))
            .map(|e| {
                (
                    e["block"].as_str().unwrap().to_owned(),
                    e["reason"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }

    /// The examples mapped to a CDDL rule: (fingerprint, section) to
    /// (file, rule). The section tells apart identical examples of two
    /// messages.
    pub fn examples(&self) -> BTreeMap<(String, String), (String, String)> {
        self.table
            .get("example")
            .and_then(|e| e.as_array())
            .into_iter()
            .flatten()
            .filter_map(|e| e.as_table())
            .map(|e| {
                (
                    (
                        e["block"].as_str().unwrap().to_owned(),
                        e["section"].as_str().unwrap().to_owned(),
                    ),
                    (
                        e["cddl"].as_str().unwrap().to_owned(),
                        e["rule"].as_str().unwrap().to_owned(),
                    ),
                )
            })
            .collect()
    }

    /// The divergences recorded for a check, by scope (`ref/04`, `openapi`):
    /// each scope names the triage issue that tracks it and the items.
    pub fn divergences(&self, check: &str) -> BTreeMap<String, (String, BTreeSet<String>)> {
        let mut out = BTreeMap::new();
        let Some(scopes) = self
            .table
            .get("divergence")
            .and_then(|d| d.get(check))
            .and_then(|c| c.as_table())
        else {
            return out;
        };
        for (scope, entry) in scopes {
            let issue = entry["issue"].as_str().unwrap().to_owned();
            let items = entry["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_str().unwrap().to_owned())
                .collect();
            out.insert(scope.clone(), (issue, items));
        }
        out
    }
}

/// Compares what a check found with what `known.toml` records for it: a
/// divergence that is not recorded fails (the reference and the code
/// diverged), and so does a recorded one that is gone (the entry is stale
/// and must be removed, so the list only shrinks for a reason).
pub fn ratchet(check: &str, found: &BTreeMap<String, BTreeSet<String>>) {
    let known = Known::load().divergences(check);
    let mut new = Vec::new();
    let mut stale = Vec::new();
    let scopes: BTreeSet<&String> = found.keys().chain(known.keys()).collect();
    for scope in scopes {
        let empty = BTreeSet::new();
        let got = found.get(scope).unwrap_or(&empty);
        let (issue, listed) = known
            .get(scope)
            .map(|(i, l)| (i.as_str(), l))
            .unwrap_or(("", &empty));
        for item in got.difference(listed) {
            new.push(format!("  [{scope}] {item}"));
        }
        for item in listed.difference(got) {
            stale.push(format!("  [{scope}] {item} ({issue})"));
        }
    }
    let mut message = String::new();
    if !new.is_empty() {
        message.push_str(&format!(
            "{} divergences between the references and the sources ({check}); fix the reference or the code, or record them under [divergence.{check}] in crates/joker-spec-tests/known.toml with a triage issue:\n{}\n",
            new.len(),
            new.join("\n")
        ));
    }
    if !stale.is_empty() {
        message.push_str(&format!(
            "{} recorded divergences are gone ({check}); remove them from crates/joker-spec-tests/known.toml:\n{}\n",
            stale.len(),
            stale.join("\n")
        ));
    }
    assert!(message.is_empty(), "{message}");
}

/// Whether the external tools are required: `JOKER_SPEC_TOOLS=required`,
/// which CI sets. Without it, a check whose tool is missing is skipped
/// with a note.
pub fn tools_required() -> bool {
    std::env::var("JOKER_SPEC_TOOLS").is_ok_and(|v| v == "required")
}

/// Finds an executable on `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// Reports a missing tool: a failure when the tools are required, a note
/// otherwise. Returns whether the check can go on.
pub fn tool(name: &str, found: bool) -> bool {
    if found {
        return true;
    }
    assert!(
        !tools_required(),
        "{name} is required (JOKER_SPEC_TOOLS=required) and was not found"
    );
    eprintln!("note: {name} not found; check skipped (set JOKER_SPEC_TOOLS=required to fail)");
    false
}

/// Runs a command and returns its exit status and its standard output and
/// error together.
pub fn run(command: &mut Command) -> (bool, String) {
    let out = command
        .output()
        .unwrap_or_else(|e| panic!("{command:?}: {e}"));
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

/// A scratch directory for one test, under the target directory.
pub fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("spec-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
