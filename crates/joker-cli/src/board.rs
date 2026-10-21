//! `jk board check`/`jk board new` (docs/07, ref/03 §2.3).

use std::path::{Path, PathBuf};

use joker_manifest::board::{new_from, parse};

use crate::Failure;

/// `jk board check <file>`. Local checks only (`J0800`–`J0849`): the
/// `--node` confirmation of docs/07 (architecture, CPUs, peripherals and
/// paths actually present) needs a hardware-inventory endpoint that does
/// not exist yet.
pub fn check(file: &Path) -> Result<u8, Failure> {
    let text = std::fs::read_to_string(file).map_err(|e| Failure::usage(format!("{e}")))?;
    let report = parse(file, &text);
    let rendered = report.render();
    if !rendered.is_empty() {
        eprintln!("{rendered}");
    }
    if report.board.is_none() {
        return Ok(2);
    }
    println!("✓ board    {}", file.display());
    Ok(0)
}

fn resolve_from(from: &str) -> PathBuf {
    if from.contains('/') || from.ends_with(".toml") {
        PathBuf::from(from)
    } else {
        PathBuf::from("boards").join(format!("{from}.board.toml"))
    }
}

/// `jk board new <name> --from <board>`. The registry half of `--from`
/// (docs/07: "from the project or from the registry") is not implemented:
/// only a project file or a `boards/<name>.board.toml` name.
pub fn new(name: &str, from: &str) -> Result<u8, Failure> {
    let source_path = resolve_from(from);
    let source = std::fs::read_to_string(&source_path)
        .map_err(|e| Failure::usage(format!("{}: {e}", source_path.display())))?;
    let out = PathBuf::from("boards").join(format!("{name}.board.toml"));
    if out.exists() {
        return Err(Failure::usage(format!("{} already exists", out.display())));
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Failure::usage(format!("{e}")))?;
    }
    std::fs::write(&out, new_from(&source, name))
        .map_err(|e| Failure::usage(format!("writing {}: {e}", out.display())))?;
    println!("wrote {}", out.display());
    Ok(0)
}
