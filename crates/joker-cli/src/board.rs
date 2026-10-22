//! `jk board check`/`jk board new`/`jk board show` (docs/07, ref/03 §2.3).

use std::path::{Path, PathBuf};

use joker_manifest::board::{
    BoardPeripherals, Gpio, IoPoint, SerialPort, Watchdog, new_from, parse,
};

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

/// `jk board show <file>` (AUR-279): a local, no-node command that
/// renders a board file's own declared IO/ports/watchdog/storage, same
/// spirit as `jk board check` but printing instead of just validating.
pub fn show(file: &Path) -> Result<u8, Failure> {
    let text = std::fs::read_to_string(file).map_err(|e| Failure::usage(format!("{e}")))?;
    let report = parse(file, &text);
    let rendered = report.render();
    if !rendered.is_empty() {
        eprintln!("{rendered}");
    }
    if report.board.is_none() {
        return Ok(2);
    }
    print!("{}", render_peripherals(&report.peripherals));
    Ok(0)
}

fn render_peripherals(p: &BoardPeripherals) -> String {
    let mut out = String::new();
    if let Some(storage) = &p.storage {
        out.push_str(&format!("storage   {storage}\n"));
    }
    if let Some(w) = &p.watchdog {
        out.push_str(&format!("watchdog  {}\n", render_watchdog(w)));
    }
    if let Some(g) = &p.gpio {
        out.push_str(&format!("gpio      {}\n", render_gpio(g)));
    }
    for (name, io) in &p.io {
        out.push_str(&format!("io.{name:<6} {}\n", render_io(io)));
    }
    for (name, s) in &p.serial {
        out.push_str(&format!("serial.{name:<4} {}\n", render_serial(s)));
    }
    for (name, path) in &p.i2c {
        out.push_str(&format!("i2c.{name:<6} {path}\n"));
    }
    for (name, path) in &p.spi {
        out.push_str(&format!("spi.{name:<6} {path}\n"));
    }
    for (name, path) in &p.can {
        out.push_str(&format!("can.{name:<6} {path}\n"));
    }
    out
}

fn render_watchdog(w: &Watchdog) -> String {
    let mut parts = Vec::new();
    if let Some(v) = &w.device {
        parts.push(format!("device={v}"));
    }
    if let Some(v) = &w.timeout {
        parts.push(format!("timeout={v}"));
    }
    if let Some(v) = &w.min_timeout {
        parts.push(format!("min_timeout={v}"));
    }
    if let Some(v) = &w.max_timeout {
        parts.push(format!("max_timeout={v}"));
    }
    parts.join(" ")
}

fn render_gpio(g: &Gpio) -> String {
    let mut parts = Vec::new();
    if let Some(v) = &g.chip {
        parts.push(format!("chip={v}"));
    }
    for (name, offset) in &g.lines {
        parts.push(format!("{name}={offset}"));
    }
    parts.join(" ")
}

fn render_io(io: &IoPoint) -> String {
    let mut parts = Vec::new();
    if let Some(v) = &io.driver {
        parts.push(format!("driver={v}"));
    }
    if let Some(v) = &io.direction {
        parts.push(format!("direction={v}"));
    }
    if let Some(v) = &io.fallback {
        parts.push(format!("fallback={v}"));
    }
    for (k, v) in &io.extra {
        parts.push(format!("{k}={v}"));
    }
    parts.join(" ")
}

fn render_serial(s: &SerialPort) -> String {
    let mut parts = vec![s.path.clone()];
    if s.rs485 {
        parts.push("rs485=true".to_owned());
    }
    if let Some(b) = s.max_baud {
        parts.push(format!("max_baud={b}"));
    }
    parts.join(" ")
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
