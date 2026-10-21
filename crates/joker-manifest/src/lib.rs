//! The Joker manifest front: the `joker.toml` model, its layers and checks,
//! the `joker.lock` and the code registry shared by every surface (docs/16 §9,
//! ref/00 §8, ref/01).
//!
//! [`check`] runs the steps of ref/01 §8.1 that the M0 subset covers: read
//! and merge the layers, decode the effective manifest, check it against a
//! [`Context`], and hash it.

pub mod admission;
pub mod check;
pub mod codes;
pub mod context;
pub mod decode;
pub mod diag;
pub mod effective;
pub mod expr;
pub mod grammar;
pub mod layers;
pub mod lock;
pub mod model;
pub mod schema;
mod sections;
pub mod source;
pub mod tighten;
pub mod value;

use std::path::Path;

pub use context::Context;
pub use diag::{Diagnostic, Diagnostics, Level};
pub use layers::{FileSystem, MemoryFileSystem, OsFileSystem};
pub use model::{Manifest, Mode};
pub use source::Sources;

/// The result of checking one entry manifest.
#[derive(Debug, Clone)]
pub struct Report {
    /// Every file read.
    pub sources: Sources,
    /// The findings.
    pub diagnostics: Diagnostics,
    /// The decoded effective manifest, when the layers merged.
    pub manifest: Option<Manifest>,
    /// Every path of the site, when the checks ran.
    pub paths: check::PathRegistry,
    /// The canonical effective manifest, when there are no errors.
    pub effective: Option<effective::Canon>,
    /// Its hash (ref/01 §8.3), when there are no errors.
    pub hash: Option<String>,
    /// The widenings that `widen` covers, for `[accepted]` in the lock.
    pub widenings: Vec<tighten::Widening>,
    /// The `accept_missing_fallback` keys, for `[accepted]` in the lock.
    pub acceptances: Vec<tighten::Acceptance>,
}

impl Report {
    /// The mode the diagnostics are judged in.
    pub fn mode(&self) -> Mode {
        self.manifest
            .as_ref()
            .map_or(Mode::Prod, |m| m.project.mode)
    }

    /// Whether any finding is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors(self.mode())
    }

    /// The findings, rendered as `jk check` prints them, in order.
    pub fn render(&self) -> String {
        let mode = self.mode();
        self.diagnostics
            .sorted(&self.sources)
            .iter()
            .map(|d| d.render(&self.sources, mode))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The codes of the findings, in order.
    pub fn codes(&self) -> Vec<&'static str> {
        self.diagnostics
            .sorted(&self.sources)
            .iter()
            .map(|d| d.code)
            .collect()
    }
}

/// Whether a manifest whose board is unknown reads as a Joker Micro one:
/// it has `[link]`, `[net]` or a key of `[target]` that only an MCU has.
fn looks_micro(merged: &value::Table) -> bool {
    let target = merged.get("target").and_then(|e| e.node.as_table());
    let board = target
        .and_then(|t| t.get("board"))
        .and_then(|e| e.node.as_str())
        .unwrap_or("");
    merged.contains_key("link")
        || merged.contains_key("net")
        || ["esp32", "rp2040", "rp2350", "stm32", "nrf52"]
            .iter()
            .any(|chip| board.contains(chip))
        || target.is_some_and(|t| {
            ["capacity", "extern", "micro", "accept_unsafe_reset"]
                .iter()
                .any(|k| t.contains_key(*k))
        })
}

/// Checks the manifest whose entry layer is `entry`.
pub fn check(fs: &dyn FileSystem, entry: &Path, context: &Context) -> Report {
    let mut sources = Sources::default();
    let mut diagnostics = Diagnostics::default();
    let layers = layers::load(fs, entry, &mut sources, &mut diagnostics);
    let mut report = Report {
        sources,
        diagnostics,
        manifest: None,
        paths: check::PathRegistry::new(),
        effective: None,
        hash: None,
        widenings: Vec::new(),
        acceptances: Vec::new(),
    };
    let Some(top) = layers.last() else {
        return report;
    };
    let top_span = source::Span::new(top.file, 0, 0);
    let merged = layers::merge(&layers, &mut report.diagnostics);
    report.widenings = tighten::check(&layers, &merged, &mut report.diagnostics);
    report.acceptances = tighten::acceptances(&merged);
    if report.diagnostics.has_errors(Mode::Dev) {
        return report;
    }
    let micro = merged
        .get("target")
        .and_then(|e| e.node.as_table())
        .and_then(|t| t.get("board"))
        .and_then(|e| e.node.as_str())
        .map(|board| board.split('@').next().unwrap_or(board))
        .and_then(|name| context.boards.get(name))
        .is_some_and(|b| b.micro)
        || (context.open_world && looks_micro(&merged));
    let v1_1 = context.runtime >= model::SchemaVersion { major: 1, minor: 1 };
    let declared_v1_1 = merged
        .get("project")
        .and_then(|e| e.node.as_table())
        .and_then(|p| p.get("joker"))
        .and_then(|e| e.node.as_str())
        .and_then(grammar::schema_version)
        .is_some_and(|v| v >= model::SchemaVersion { major: 1, minor: 1 });
    let manifest = decode::Decoder::new(&mut report.diagnostics, micro)
        .with_v1_1(v1_1, declared_v1_1)
        .manifest(&merged, top_span);
    let mode = manifest.project.mode;
    if report.diagnostics.has_errors(mode) {
        report.manifest = Some(manifest);
        return report;
    }
    let root = entry.parent().unwrap_or(Path::new(""));
    report.paths = check::check(
        &manifest,
        context,
        fs,
        &report.sources,
        root,
        micro,
        &mut report.diagnostics,
    );
    if !report.diagnostics.has_errors(mode) {
        let canonical = effective::canonical(&manifest, context, &report.sources);
        report.hash = Some(effective::hash(&canonical));
        report.effective = Some(canonical);
    }
    report.manifest = Some(manifest);
    report
}
