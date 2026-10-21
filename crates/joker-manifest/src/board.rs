//! Loads a `.board.toml` file (ref/03 §2.3) into a [`BoardInfo`], the same
//! type `Context` already uses for the manifest checks.
//!
//! Scoped to what `BoardInfo` models: `name`/`version` (just the major),
//! `arch`, `os` (`micro`), `cpus` and `memory` (IEC bytes). Peripherals
//! (`[watchdog]`, `[gpio]`, `[io.*]`, `[serial]`, …), MCU-only sections
//! (`[capacities]`, `[irq]`, `[areas]`, `[pins.*]`, …) and `recommended`
//! are not parsed here (AUR-414); `jokerd`'s resource-class wiring
//! (`resources::rt_cpus`) reads `[resources]` from the *manifest*, not
//! from the board.

use std::path::Path;

use toml::de::DeTable;

use crate::context::BoardInfo;
use crate::diag::{Diagnostic, Diagnostics};
use crate::source::{Sources, Span};
use crate::value::{self, Table};

/// The result of loading one board file.
#[derive(Debug)]
pub struct BoardReport {
    /// The file, for rendering diagnostics.
    pub sources: Sources,
    /// What went wrong, if anything.
    pub diagnostics: Diagnostics,
    /// The board, when `[board]` parsed.
    pub board: Option<BoardInfo>,
}

impl BoardReport {
    /// The findings, rendered as `jk check`/`jk board check` would.
    pub fn render(&self) -> String {
        self.diagnostics
            .sorted(&self.sources)
            .iter()
            .map(|d| d.render(&self.sources, crate::Mode::Prod))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Bytes a size literal with an IEC binary prefix describes (`"512 MiB"`,
/// ref/00 §3.1), or `None` if it is not one.
fn iec_bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let (number, unit) = text.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let unit = unit.trim();
    let factor: u64 = match unit {
        "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        _ => return None,
    };
    Some((number * factor as f64).round() as u64)
}

/// Architectures without a Wasm engine today (docs/01 §22: Pulley needs
/// confirming for `armv6`; everything else listed there has Cranelift or
/// Pulley).
fn no_wasm_arch(arch: &str) -> bool {
    arch == "armv6"
}

/// Parses `text` (the file at `path`, for diagnostics) into a
/// [`BoardInfo`].
pub fn parse(path: &Path, text: &str) -> BoardReport {
    let mut sources = Sources::default();
    let file = sources.add(path.to_path_buf(), text.to_owned());
    let mut diagnostics = Diagnostics::default();
    let table = match DeTable::parse(text) {
        Ok(parsed) => match value::from_toml(file, parsed.get_ref()) {
            Ok(table) => table,
            Err(value::InvalidNumber(span)) => {
                diagnostics.push(Diagnostic::new("J0146", "number out of range").at(span));
                return BoardReport {
                    sources,
                    diagnostics,
                    board: None,
                };
            }
        },
        Err(error) => {
            let range = error.span().unwrap_or(0..0);
            diagnostics.push(
                Diagnostic::new("J0146", "file is not valid TOML")
                    .at(Span::new(file, range.start, range.end))
                    .label(error.message().trim().to_owned()),
            );
            return BoardReport {
                sources,
                diagnostics,
                board: None,
            };
        }
    };
    let board = load(&table, &mut diagnostics);
    BoardReport {
        sources,
        diagnostics,
        board,
    }
}

fn load(table: &Table, diagnostics: &mut Diagnostics) -> Option<BoardInfo> {
    let Some(board) = table.get("board").and_then(|e| e.node.as_table()) else {
        diagnostics.push(Diagnostic::new(
            "J0142",
            "missing required section `[board]`",
        ));
        return None;
    };
    let Some(arch) = board.get("arch").and_then(|e| e.node.as_str()) else {
        diagnostics.push(Diagnostic::new("J0142", "missing required key `arch`"));
        return None;
    };
    let major = board
        .get("version")
        .and_then(|e| e.node.as_str())
        .and_then(|v| v.split('.').next())
        .and_then(|major| major.parse().ok());
    let Some(major) = major else {
        diagnostics.push(Diagnostic::new(
            "J0142",
            "missing or invalid required key `version`",
        ));
        return None;
    };
    let micro = board.get("os").and_then(|e| e.node.as_str()) == Some("none");
    let cpus = board.get("cpus").and_then(|e| match e.node.value {
        value::Value::Integer(i) => u32::try_from(i).ok(),
        _ => None,
    });
    let memory = board
        .get("memory")
        .and_then(|e| e.node.as_str())
        .and_then(iec_bytes);
    Some(BoardInfo {
        majors: vec![major],
        arch: arch.to_owned(),
        micro,
        cpus,
        memory,
        no_wasm: no_wasm_arch(arch),
    })
}
