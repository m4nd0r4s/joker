//! Loads a `.board.toml` file (ref/03 §2.3) into a [`BoardInfo`], the same
//! type `Context` already uses for the manifest checks, and (for `jk
//! board show`) a [`BoardPeripherals`] of whatever `[watchdog]`, `[gpio]`,
//! `[io.*]`, `[serial]`, `[i2c]`, `[spi]` and `[can]` the file declares
//! (ref/03 §2.3.2, Linux boards only — MCU peripherals, §2.3.3, are not
//! modeled here).
//!
//! `BoardInfo` stays scoped to what the manifest checks need: `name`/
//! `version` (just the major), `arch`, `os` (`micro`), `cpus` and `memory`
//! (IEC bytes). `BoardPeripherals` is rendering data for `jk board show`,
//! not validated the way `[board]` is: a malformed entry is just left
//! out, the same way an unreferenced `[io.<name>]` takes no resources
//! until the manifest refers to it (§2.3.2). MCU-only sections
//! (`[capacities]`, `[irq]`, `[areas]`, `[pins.*]`, …) and `recommended`
//! are still not parsed; `jokerd`'s resource-class wiring
//! (`resources::rt_cpus`) reads `[resources]` from the *manifest*, not
//! from the board.

use std::collections::BTreeMap;
use std::path::Path;

use toml::de::DeTable;

use crate::context::BoardInfo;
use crate::diag::{Diagnostic, Diagnostics};
use crate::source::{Sources, Span};
use crate::value::{self, Table, Value};

/// A board's hardware watchdog (`[watchdog]`, ref/03 §2.3.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Watchdog {
    /// The device path.
    pub device: Option<String>,
    /// The timeout the daemon arms it with.
    pub timeout: Option<String>,
    /// The lowest timeout the hardware accepts.
    pub min_timeout: Option<String>,
    /// The highest timeout the hardware accepts.
    pub max_timeout: Option<String>,
}

/// A board's GPIO chip (`[gpio]`, ref/03 §2.3.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Gpio {
    /// The chip label or path (`"pinctrl-rp1"`, or `"auto"`).
    pub chip: Option<String>,
    /// Named lines, by offset; empty when the kernel's own names are used.
    pub lines: BTreeMap<String, i64>,
}

/// One `[io.<name>]`: fixed IO of the board (`board.<name>`, ref/03
/// §2.3.2). `extra` is the family's own address keys (`line`, `var`, …),
/// rendered as given rather than modeled per driver.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IoPoint {
    /// The driver.
    pub driver: Option<String>,
    /// `"input"` or `"output"`.
    pub direction: Option<String>,
    /// What the hardware falls back to when `jokerd` or the board stops.
    pub fallback: Option<String>,
    /// Every other key, rendered.
    pub extra: BTreeMap<String, String>,
}

/// A stable serial port (`[serial]`, ref/03 §2.3.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SerialPort {
    /// The device path.
    pub path: String,
    /// Whether it is wired for RS-485.
    pub rs485: bool,
    /// The highest baud rate the port supports.
    pub max_baud: Option<u32>,
}

/// A board's declared peripherals (ref/03 §2.3.2), for `jk board show`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoardPeripherals {
    /// `storage` of `[board]` (`"sd"`, `"emmc"`, `"ssd"`).
    pub storage: Option<String>,
    /// `[watchdog]`.
    pub watchdog: Option<Watchdog>,
    /// `[gpio]`.
    pub gpio: Option<Gpio>,
    /// `[io.<name>]`, by name.
    pub io: BTreeMap<String, IoPoint>,
    /// `[serial]`, by name.
    pub serial: BTreeMap<String, SerialPort>,
    /// `[i2c]`, by name.
    pub i2c: BTreeMap<String, String>,
    /// `[spi]`, by name.
    pub spi: BTreeMap<String, String>,
    /// `[can]`, by name.
    pub can: BTreeMap<String, String>,
}

/// The result of loading one board file.
#[derive(Debug)]
pub struct BoardReport {
    /// The file, for rendering diagnostics.
    pub sources: Sources,
    /// What went wrong, if anything.
    pub diagnostics: Diagnostics,
    /// The board, when `[board]` parsed.
    pub board: Option<BoardInfo>,
    /// Whatever peripherals the file declares, parsed independently of
    /// whether `[board]` itself parsed.
    pub peripherals: BoardPeripherals,
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
                    peripherals: BoardPeripherals::default(),
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
                peripherals: BoardPeripherals::default(),
            };
        }
    };
    let board = load(&table, &mut diagnostics);
    let peripherals = load_peripherals(&table);
    BoardReport {
        sources,
        diagnostics,
        board,
        peripherals,
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

/// A value, rendered for display (`jk board show`): not a general TOML
/// serializer, just enough for the scalar/array shapes a board's own
/// address keys use.
fn display_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Datetime(d) => d.clone(),
        Value::Array(items) => items
            .iter()
            .map(|n| display_value(&n.value))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Table(_) => "{…}".to_owned(),
    }
}

/// Every `[watchdog]`/`[gpio]`/`[io.*]`/`[serial]`/`[i2c]`/`[spi]`/`[can]`
/// the file declares (ref/03 §2.3.2), independently of whether `[board]`
/// itself parsed. Unlike `load`, nothing here is diagnosed: a malformed
/// entry is just left out, since this is rendering data for `jk board
/// show`, not something `jk board check` validates.
fn load_peripherals(table: &Table) -> BoardPeripherals {
    let storage = table
        .get("board")
        .and_then(|e| e.node.as_table())
        .and_then(|b| b.get("storage"))
        .and_then(|e| e.node.as_str())
        .map(str::to_owned);
    let watchdog = table
        .get("watchdog")
        .and_then(|e| e.node.as_table())
        .map(load_watchdog);
    let gpio = table
        .get("gpio")
        .and_then(|e| e.node.as_table())
        .map(load_gpio);
    let io = table
        .get("io")
        .and_then(|e| e.node.as_table())
        .map(load_io_points)
        .unwrap_or_default();
    let serial = table
        .get("serial")
        .and_then(|e| e.node.as_table())
        .map(load_serial_ports)
        .unwrap_or_default();
    let i2c = table
        .get("i2c")
        .and_then(|e| e.node.as_table())
        .map(load_string_map)
        .unwrap_or_default();
    let spi = table
        .get("spi")
        .and_then(|e| e.node.as_table())
        .map(load_string_map)
        .unwrap_or_default();
    let can = table
        .get("can")
        .and_then(|e| e.node.as_table())
        .map(load_string_map)
        .unwrap_or_default();
    BoardPeripherals {
        storage,
        watchdog,
        gpio,
        io,
        serial,
        i2c,
        spi,
        can,
    }
}

fn load_watchdog(t: &Table) -> Watchdog {
    let text = |key: &str| t.get(key).and_then(|e| e.node.as_str()).map(str::to_owned);
    Watchdog {
        device: text("device"),
        timeout: text("timeout"),
        min_timeout: text("min_timeout"),
        max_timeout: text("max_timeout"),
    }
}

fn load_gpio(t: &Table) -> Gpio {
    let chip = t
        .get("chip")
        .and_then(|e| e.node.as_str())
        .map(str::to_owned);
    let lines = t
        .get("lines")
        .and_then(|e| e.node.as_table())
        .map(|lines| {
            lines
                .iter()
                .filter_map(|(name, e)| match e.node.value {
                    Value::Integer(i) => Some((name.clone(), i)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    Gpio { chip, lines }
}

/// `[io.<name>]` (ref/03 §2.3.2): `driver`/`direction`/`fallback` are
/// pulled out by name; every other key (the driver family's own address,
/// `line`/`var`/…) is kept in `extra`, rendered as given.
fn load_io_points(t: &Table) -> BTreeMap<String, IoPoint> {
    t.iter()
        .filter_map(|(name, e)| {
            let point = e.node.as_table()?;
            let text = |key: &str| {
                point
                    .get(key)
                    .and_then(|e| e.node.as_str())
                    .map(str::to_owned)
            };
            let extra = point
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "driver" | "direction" | "fallback"))
                .map(|(k, e)| (k.clone(), display_value(&e.node.value)))
                .collect();
            Some((
                name.clone(),
                IoPoint {
                    driver: text("driver"),
                    direction: text("direction"),
                    fallback: text("fallback"),
                    extra,
                },
            ))
        })
        .collect()
}

/// `[serial]` (ref/03 §2.3.2): a name maps to either a bare path or a
/// `{ path, rs485, max_baud }` table.
fn load_serial_ports(t: &Table) -> BTreeMap<String, SerialPort> {
    t.iter()
        .filter_map(|(name, e)| {
            let port = match &e.node.value {
                Value::String(path) => SerialPort {
                    path: path.clone(),
                    rs485: false,
                    max_baud: None,
                },
                Value::Table(sub) => SerialPort {
                    path: sub.get("path").and_then(|e| e.node.as_str())?.to_owned(),
                    rs485: sub
                        .get("rs485")
                        .is_some_and(|e| matches!(e.node.value, Value::Bool(true))),
                    max_baud: sub.get("max_baud").and_then(|e| match e.node.value {
                        Value::Integer(i) => u32::try_from(i).ok(),
                        _ => None,
                    }),
                },
                _ => return None,
            };
            Some((name.clone(), port))
        })
        .collect()
}

/// `[i2c]`/`[spi]`/`[can]` (ref/03 §2.3.2): a name maps to a path or
/// interface string.
fn load_string_map(t: &Table) -> BTreeMap<String, String> {
    t.iter()
        .filter_map(|(name, e)| Some((name.clone(), e.node.as_str()?.to_owned())))
        .collect()
}

const TIER_REVIEW: &str =
    "\"dev\"   # REVIEW: support level (01 §22); \"1\" or \"2\" requires CI with this board";

fn rewrite_key(line: &str, key: &str, value: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix(key)?;
    let rest = rest.trim_start();
    rest.starts_with('=').then(|| format!("{key:<7} = {value}"))
}

/// `jk board new <name> --from <board>` (docs/07): copies an existing
/// board profile's text, renaming it to `name`, pinning `version =
/// "1.0.0"`, and marking `tier = "dev"` for review (docs/07, ref/03
/// §2.3.7). Every other line of `[board]`, and every other section, is
/// kept verbatim.
pub fn new_from(source: &str, name: &str) -> String {
    let mut out = String::new();
    let mut in_board = false;
    let mut wrote_tier = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_board && !wrote_tier {
                out.push_str(&format!("tier    = {TIER_REVIEW}\n"));
                wrote_tier = true;
            }
            in_board = trimmed == "[board]";
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_board {
            if let Some(rewritten) = rewrite_key(line, "name", &format!("\"{name}\"")) {
                out.push_str(&rewritten);
                out.push('\n');
                continue;
            }
            if let Some(rewritten) = rewrite_key(line, "version", "\"1.0.0\"") {
                out.push_str(&rewritten);
                out.push('\n');
                continue;
            }
            if rewrite_key(line, "tier", "").is_some() {
                out.push_str(&format!("tier    = {TIER_REVIEW}\n"));
                wrote_tier = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_board && !wrote_tier {
        out.push_str(&format!("tier    = {TIER_REVIEW}\n"));
    }
    out
}
