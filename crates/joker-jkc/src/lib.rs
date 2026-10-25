//! `jkc`, the class compiler (docs/17, ADR 0018, ADR 0059).
//!
//! [`compile`] reads a set of `.jkc` files (the project's `classes/` and the
//! packages they use), checks them with the rules of docs/17 §7 and
//! returns, for every class without errors, its canonical contract
//! ([`contract::ContractFile`]): the bytes of `<name>@<version>.contract.json`,
//! the contract hash the lock pins and the documentation hash.
//! [`compat::check`] compares a class with its previous version (`JC0601`).
//!
//! Diagnostics carry the codes of `codes.toml` and render as `jk check`
//! renders the manifest's (ref/01 §11.4).

#![allow(clippy::result_large_err)]

pub mod ast;
pub mod check;
pub mod compat;
pub mod contract;
pub mod expr;
pub mod info;
pub mod parser;
pub mod show;
pub mod stdlib;
#[cfg(feature = "verify")]
pub mod verify;

use std::collections::BTreeMap;
use std::path::PathBuf;

use joker_manifest::{Diagnostic, Diagnostics, Sources};

use crate::check::{Parsed, Universe};
use crate::contract::ContractFile;

/// The result of one compilation.
#[derive(Debug, Default)]
pub struct Compilation {
    /// The files read, for rendering diagnostics.
    pub sources: Sources,
    /// Every finding, errors and warnings.
    pub diagnostics: Diagnostics,
    /// The classes without errors, by reference (`std/pump@1`).
    pub classes: BTreeMap<String, ContractFile>,
}

impl Compilation {
    /// Whether any finding is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| !d.code.starts_with("JW"))
    }

    /// The findings rendered as `jk check` prints them, sorted by file and
    /// position.
    pub fn render(&self) -> String {
        self.diagnostics
            .sorted(&self.sources)
            .iter()
            .map(|d| d.render(&self.sources, joker_manifest::Mode::Prod))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Compiles `.jkc` files given by path (relative to the project root) and
/// text. Every class of every file is checked; the classes of all files
/// resolve against each other.
pub fn compile(files: impl IntoIterator<Item = (PathBuf, String)>) -> Compilation {
    let mut out = Compilation::default();
    let mut parsed = Vec::new();
    for (path, text) in files {
        let id = out.sources.add(path.clone(), text.clone());
        match parser::parse(id, &text) {
            Ok(ast) => parsed.push(Parsed { path, ast }),
            Err(d) => out.diagnostics.push(d),
        }
    }
    let universe = Universe::new(&parsed, &mut out.diagnostics);
    for p in &parsed {
        check::check_file(&universe, p, &mut out.diagnostics);
    }
    for p in &parsed {
        for class in &p.ast.classes {
            if let Some(file) = check::check_class(&universe, p, class, &mut out.diagnostics) {
                out.classes.insert(file.contract.reference(), file);
            }
        }
    }
    out
}

/// `JC0601` for every class of `next` whose version does not follow from
/// its previous contract in `previous` (same reference).
pub fn compatibility(
    previous: &BTreeMap<String, ContractFile>,
    next: &BTreeMap<String, ContractFile>,
) -> Vec<Diagnostic> {
    next.iter()
        .filter_map(|(reference, new)| {
            let old = previous.get(reference)?;
            compat::check(old, new).err().map(|why| {
                Diagnostic::new(
                    "JC0601",
                    format!("`{reference}`: version incompatible with the previous one: {why}"),
                )
                .help("bump the version as docs/17 §9 requires")
            })
        })
        .collect()
}
