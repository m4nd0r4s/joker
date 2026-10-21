//! Diagnostics in the style of rustc (ref/00 §8, ref/01 §11.4): always a code,
//! the layer and position where the key is written, and help when there is
//! some.

use std::fmt::Write as _;

use crate::codes::{Registry, Severity};
use crate::model::Mode;
use crate::source::{Sources, Span};

/// How a diagnostic is shown once the mode is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Blocks the manifest.
    Error,
    /// Reported, does not block.
    Warning,
}

/// One finding about the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The registry code, such as `J0107`.
    pub code: &'static str,
    /// The headline.
    pub title: String,
    /// Where the finding is, if it has a place in a file.
    pub span: Option<Span>,
    /// The text under the carets.
    pub label: Option<String>,
    /// `help:` lines.
    pub help: Vec<String>,
}

impl Diagnostic {
    /// A diagnostic with a code and a headline.
    pub fn new(code: &'static str, title: impl Into<String>) -> Self {
        Self {
            code,
            title: title.into(),
            span: None,
            label: None,
            help: Vec::new(),
        }
    }

    /// Places the diagnostic at a span.
    #[must_use]
    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// Sets the text under the carets.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Adds a help line.
    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help.push(help.into());
        self
    }

    /// The level in a mode, from the severity of the code in the registry.
    pub fn level(&self, mode: Mode) -> Level {
        let severity = Registry::builtin()
            .ok()
            .and_then(|r| r.lookup(self.code))
            .and_then(|code| code.severity());
        match severity {
            Some(Severity::Warning) => Level::Warning,
            Some(Severity::ErrorInProd) if mode == Mode::Dev => Level::Warning,
            _ => Level::Error,
        }
    }

    /// The code as shown: the `W` form for an `error-in-prod` code in `dev`.
    pub fn shown_code(&self, mode: Mode) -> String {
        match Registry::builtin().ok().and_then(|r| r.lookup(self.code)) {
            Some(code) if mode == Mode::Dev => {
                code.dev_warning().unwrap_or_else(|| self.code.to_owned())
            }
            _ => self.code.to_owned(),
        }
    }

    /// Renders the diagnostic as `jk check` prints it.
    pub fn render(&self, sources: &Sources, mode: Mode) -> String {
        let level = match self.level(mode) {
            Level::Error => "error",
            Level::Warning => "warning",
        };
        let mut out = format!("{level}[{}]: {}\n", self.shown_code(mode), self.title);
        let place = self.span.and_then(|span| {
            let file = sources.get(span.file)?;
            let (line, column) = file.position(span.start);
            Some((span, file, line, column))
        });
        let mut gutter = 1;
        if let Some((span, file, line, column)) = place {
            gutter = line.to_string().len();
            let pad = " ".repeat(gutter);
            let text = file.line(line);
            let covered = sources
                .text(span)
                .split('\n')
                .next()
                .map_or(0, |t| t.chars().count())
                .max(1);
            let _ = writeln!(out, "{pad}--> {}:{line}:{column}", file.path().display());
            let _ = writeln!(out, "{pad} |");
            let _ = writeln!(out, "{line} | {text}");
            let carets = format!("{}{}", " ".repeat(column - 1), "^".repeat(covered));
            match &self.label {
                Some(label) => {
                    let _ = writeln!(out, "{pad} | {carets} {label}");
                }
                None => {
                    let _ = writeln!(out, "{pad} | {carets}");
                }
            }
            if !self.help.is_empty() {
                let _ = writeln!(out, "{pad} |");
            }
        } else if let Some(label) = &self.label {
            let _ = writeln!(out, "{} = note: {label}", " ".repeat(gutter));
        }
        for help in &self.help {
            let _ = writeln!(out, "{} = help: {help}", " ".repeat(gutter));
        }
        out
    }
}

/// The diagnostics of one check, kept in the order of ref/01 §8.1: by file,
/// position and code.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Records a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// The diagnostics, sorted by file, position and code.
    pub fn sorted(&self, sources: &Sources) -> Vec<&Diagnostic> {
        let mut items: Vec<&Diagnostic> = self.items.iter().collect();
        items.sort_by(|a, b| {
            let key = |d: &Diagnostic| {
                (
                    d.span
                        .map(|s| (sources.path(s.file).to_path_buf(), s.start)),
                    d.code,
                    d.title.clone(),
                )
            };
            key(a).cmp(&key(b))
        });
        items.dedup();
        items
    }

    /// Whether any diagnostic is an error in a mode.
    pub fn has_errors(&self, mode: Mode) -> bool {
        self.items.iter().any(|d| d.level(mode) == Level::Error)
    }

    /// The number of diagnostics recorded.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The diagnostics in recording order.
    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }
}
