//! `jk class list`, `jk class show` and `jk class check` (docs/07 §7):
//! the classes of `std/` this runtime ships and those of the project's
//! `classes/` directory, compiled with `jkc` (docs/17).

use std::path::{Path, PathBuf};

use joker_jkc::{Compilation, show, stdlib};

use crate::Failure;

fn project_files(root: &Path) -> Result<Vec<(PathBuf, String)>, Failure> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("classes")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path != root.join("classes/std") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "jkc") {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| Failure::usage(format!("{}: {e}", path.display())))?;
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.push((rel, text));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn compile(root: &Path) -> Result<(Compilation, usize), Failure> {
    let project = project_files(root)?;
    let n = project.len();
    let mut files = stdlib::sources();
    files.extend(project);
    Ok((joker_jkc::compile(files), n))
}

/// `jk class list [--std|--project]`.
pub fn list(root: &Path, only_std: bool, only_project: bool) -> Result<u8, Failure> {
    let (c, _) = compile(root)?;
    for (reference, file) in &c.classes {
        let is_std = reference.starts_with("std/");
        if (only_std && !is_std) || (only_project && is_std) {
            continue;
        }
        println!("{}", show::list_line(file));
    }
    Ok(0)
}

/// `jk class show <class>`; `std/` may be omitted.
pub fn show(root: &Path, class: &str) -> Result<u8, Failure> {
    let reference = if class.contains('/') {
        class.to_owned()
    } else {
        format!("std/{class}")
    };
    let (c, _) = compile(root)?;
    let found = if reference.contains('@') {
        c.classes.get(&reference)
    } else {
        c.classes
            .iter()
            .filter(|(k, _)| k.rsplit_once('@').is_some_and(|(n, _)| n == reference))
            .map(|(_, v)| v)
            .max_by_key(|f| f.contract.version)
    };
    match found {
        Some(file) => {
            print!("{}", show::show(file));
            Ok(0)
        }
        None => Err(Failure::usage(format!(
            "unknown class `{reference}` (see `jk class list`)"
        ))),
    }
}

/// `jk class check`: every class of `std/` and of `classes/`, with the
/// diagnostics of docs/17 §7.
pub fn check(root: &Path) -> Result<u8, Failure> {
    let (c, n) = compile(root)?;
    let rendered = c.render();
    if !rendered.is_empty() {
        eprintln!("{rendered}");
    }
    if c.has_errors() {
        return Ok(2);
    }
    println!(
        "✓ classes  {} class(es) of std/, {n} file(s) in classes/",
        stdlib::CLASSES.len()
    );
    Ok(0)
}
