//! `jk board probe` and `jk board check --node` (ref/03 §2.3.7): a board
//! profile draft written from the node's hardware inventory (`NodeHardware`,
//! ref/09 §5.2), and the confirmation of a board file against it.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde_json::Value;

/// A draft written by `jk board probe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The board name.
    pub name: String,
    /// The text of `boards/<name>.board.toml`.
    pub text: String,
    /// How many lines carry a `# REVIEW` marker.
    pub reviews: usize,
}

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A name in lowercase with hyphens (`Raspberry Pi Zero W Rev 1.1` →
/// `raspberry-pi-zero-w-rev-1-1`).
pub fn slug(text: &str, sep: char) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with(sep) {
            out.push(sep);
        }
    }
    while out.ends_with(sep) {
        out.pop();
    }
    out
}

/// Bytes of an IEC size literal (`"427 MiB"`).
pub fn iec_bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let (number, unit) = text.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let factor: u64 = match unit.trim() {
        "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        _ => return None,
    };
    Some((number * factor as f64).round() as u64)
}

/// The power of two in MiB at or above `bytes`, as an IEC size (427 MiB →
/// `"512 MiB"`, 3.7 GiB → `"4 GiB"`).
pub fn memory_size(bytes: u64) -> String {
    let mib = bytes.div_ceil(1 << 20).max(1);
    let size = mib.next_power_of_two();
    if size >= 1024 {
        format!("{} GiB", size / 1024)
    } else {
        format!("{size} MiB")
    }
}

/// Seconds of a duration literal (`"15s"`, `"1 min"`), as far as the
/// watchdog timeouts go.
pub fn seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let (number, unit) = text.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let factor = match unit.trim() {
        "ms" => 0.001,
        "s" => 1.0,
        "m" | "min" => 60.0,
        "h" => 3600.0,
        _ => return None,
    };
    Some(number * factor)
}

fn cpu_set(cpus: &[u32]) -> String {
    let items: Vec<String> = cpus.iter().map(u32::to_string).collect();
    format!("[{}]", items.join(", "))
}

/// The suggested `recommended` and the `rt` CPUs of a board with `count`
/// CPUs (ref/03 §2.3.7).
fn recommended(count: u32) -> (String, Vec<u32>) {
    match count {
        0 | 1 => ("{ rt = [0], io = [0], bg = [0] }".to_owned(), Vec::new()),
        2 => ("{ rt = [1], io = [0], bg = [0] }".to_owned(), vec![1]),
        n => {
            let bg: Vec<u32> = (0..n - 2).collect();
            (
                format!(
                    "{{ rt = [{}], io = [{}], bg = {} }}",
                    n - 1,
                    n - 2,
                    cpu_set(&bg)
                ),
                vec![n - 1],
            )
        }
    }
}

struct Writer {
    out: String,
    reviews: usize,
}

impl Writer {
    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
        if text.contains("# REVIEW:") {
            self.reviews += 1;
        }
    }

    fn key(&mut self, key: &str, value: &str, note: Option<&str>) {
        let pair = format!("{key:<7} = {value}");
        match note {
            Some(n) => self.line(&format!("{pair:<40} # {n}")),
            None => self.line(&pair),
        }
    }
}

fn soc_chip(chips: &[Value]) -> Option<&Value> {
    chips
        .iter()
        .find(|c| s(c, "label").is_some_and(|l| l.starts_with("pinctrl-")))
        .or_else(|| {
            chips
                .iter()
                .max_by_key(|c| c.get("lines").and_then(Value::as_u64).unwrap_or(0))
        })
}

fn used_lines(chip: &Value) -> String {
    let used: Vec<String> = list(chip, "used")
        .iter()
        .map(|u| {
            let offset = u.get("offset").and_then(Value::as_u64).unwrap_or(0);
            match s(u, "consumer").or_else(|| s(u, "name")) {
                Some(who) => format!("{offset} ({who})"),
                None => offset.to_string(),
            }
        })
        .collect();
    if used.is_empty() {
        "none".to_owned()
    } else {
        used.join(", ")
    }
}

fn unique(name: String, taken: &mut BTreeSet<String>) -> String {
    let mut candidate = name.clone();
    let mut n = 1;
    while !taken.insert(candidate.clone()) {
        candidate = format!("{name}{n}");
        n += 1;
    }
    candidate
}

fn thermal_name(kind: &str) -> String {
    let base = kind
        .trim_end_matches("-thermal")
        .trim_end_matches("_thermal")
        .trim_end_matches("_temp");
    let base = slug(base, '_');
    if base.is_empty() {
        "zone_temp".to_owned()
    } else {
        format!("{base}_temp")
    }
}

/// The draft of a board profile from the inventory `hw` of node `node`
/// (ref/03 §2.3.7). `name` is `--name`; without it, the model in lowercase
/// with hyphens, marked for review.
pub fn draft(hw: &Value, node: &str, name: Option<&str>) -> Result<Draft, String> {
    let model = s(hw, "model");
    let (name, derived) = match (name, model) {
        (Some(n), _) => (n.to_owned(), false),
        (None, Some(m)) => (slug(m, '-'), true),
        (None, None) => {
            return Err("the node reports no model: give the board a name with --name".to_owned());
        }
    };
    let arch = s(hw, "arch").unwrap_or("other");
    if arch == "other" {
        return Err(format!(
            "the node's architecture `{}` is not one of a Linux board (ref/03 §2.3.1)",
            s(hw, "machine").unwrap_or("?")
        ));
    }
    let mut w = Writer {
        out: String::new(),
        reviews: 0,
    };
    w.line(&format!(
        "# Generated by jk board probe --node {node}. Resolve the \"# REVIEW\" lines and run jk board check --node {node}."
    ));
    w.line("schema = 1");
    w.line("");
    w.line("[board]");
    w.key(
        "name",
        &format!("\"{name}\""),
        derived.then_some("REVIEW: derived from the model"),
    );
    w.key("version", "\"1.0.0\"", None);
    w.key("arch", &format!("\"{arch}\""), None);
    if let Some(m) = model {
        w.key("model", &format!("\"{}\"", m.replace('"', "\\\"")), None);
    }
    w.line("# REVIEW: vendor, docs, license");
    w.key(
        "tier",
        "\"dev\"",
        Some("REVIEW: support level (01 §22); \"1\" or \"2\" requires CI with this board"),
    );
    let cpus = &hw["cpus"];
    let count = cpus
        .get("possible")
        .or_else(|| cpus.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let count = u32::try_from(count).unwrap_or(1);
    w.key("cpus", &count.to_string(), None);
    let total = s(&hw["memory"], "total").unwrap_or("0 B");
    let bytes = iec_bytes(total).unwrap_or(0);
    w.key(
        "memory",
        &format!("\"{}\"", memory_size(bytes)),
        Some(&format!("MemTotal {total}")),
    );
    if let Some(f) = s(cpus, "max_freq") {
        w.key("clock", &format!("\"{f}\""), None);
    }
    if let Some(storage) = hw.get("storage").filter(|v| v.is_object())
        && let Some(medium) = s(storage, "medium")
    {
        let device = s(storage, "device").unwrap_or("?");
        w.key(
            "storage",
            &format!("\"{medium}\""),
            Some(device.trim_start_matches("/dev/")),
        );
    }
    let kernel = &hw["kernel"];
    let rt = kernel.get("preempt_rt").and_then(Value::as_bool) == Some(true);
    let release = s(kernel, "release").unwrap_or("?");
    w.key(
        "kernel",
        if rt { "\"preempt_rt\"" } else { "\"standard\"" },
        Some(&format!(
            "REVIEW: target kernel; the current one is {release}, {}",
            if rt {
                "with PREEMPT_RT"
            } else {
                "without PREEMPT_RT"
            }
        )),
    );
    let (rec, rt_cpus) = recommended(count);
    w.line(&format!(
        "{:<40} # REVIEW: {}",
        format!("recommended = {rec}"),
        if count <= 1 {
            "with 1 CPU there is no isolation"
        } else {
            "it depends on what runs on the board besides Joker"
        }
    ));
    if !rt_cpus.is_empty() {
        let set = rt_cpus
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        w.line(&format!(
            "{:<40} # REVIEW: isolation of the rt CPUs",
            format!(
                "boot_cmdline = [\"isolcpus={set}\", \"nohz_full={set}\", \"rcu_nocbs={set}\"]"
            )
        ));
    }
    let cmdline: Vec<&str> = list(hw, "cmdline")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    w.line(&format!("# current boot lines: {}", cmdline.join(" ")));

    if let Some(wd) = list(hw, "watchdogs").first() {
        w.line("");
        w.line("[watchdog]");
        let device = s(wd, "device").unwrap_or("/dev/watchdog0");
        match s(wd, "identity") {
            Some(id) => w.key("device", &format!("\"{device}\""), Some(id)),
            None => w.key("device", &format!("\"{device}\""), None),
        }
        let max = s(wd, "max_timeout");
        let timeout = match max.and_then(seconds) {
            Some(m) if m < 15.0 => max.unwrap_or("15s").to_owned(),
            _ => "15s".to_owned(),
        };
        w.key("timeout", &format!("\"{timeout}\""), None);
        if let Some(v) = s(wd, "min_timeout") {
            w.line(&format!("min_timeout = \"{v}\""));
        }
        if let Some(v) = max {
            w.line(&format!("max_timeout = \"{v}\""));
        }
    }

    let chips = list(hw, "gpio_chips");
    if let Some(chip) = soc_chip(chips) {
        w.line("");
        w.line("[gpio]");
        let label = s(chip, "label").unwrap_or("auto");
        let lines = chip.get("lines").and_then(Value::as_u64).unwrap_or(0);
        w.key(
            "chip",
            &format!("\"{label}\""),
            Some(&format!(
                "{lines} lines; in use by the kernel: {}",
                used_lines(chip)
            )),
        );
        for other in chips.iter().filter(|c| !std::ptr::eq(*c, chip)) {
            w.line(&format!(
                "# other chip: \"{}\" ({} lines; in use: {})",
                s(other, "label").unwrap_or("?"),
                other.get("lines").and_then(Value::as_u64).unwrap_or(0),
                used_lines(other)
            ));
        }
    }

    let mut io_names = BTreeSet::new();
    for led in list(hw, "leds") {
        let Some(led_name) = s(led, "name") else {
            continue;
        };
        let key = unique(format!("{}_led", slug(led_name, '_')), &mut io_names);
        w.line("");
        w.line(&format!("[io.{key}]"));
        w.line("driver    = \"sysfs\"");
        w.line(&format!("led       = \"{led_name}\""));
        w.line("direction = \"output\"");
        w.line(&format!(
            "{:<40} # REVIEW: what the LED does when jokerd stops (current trigger: {})",
            "# fallback = false",
            s(led, "trigger").unwrap_or("none")
        ));
    }
    for zone in list(hw, "thermal_zones") {
        let Some(zone_name) = s(zone, "name") else {
            continue;
        };
        let kind = s(zone, "type").unwrap_or("");
        let key = unique(thermal_name(kind), &mut io_names);
        w.line("");
        w.line(&format!("[io.{key}]"));
        w.line("driver  = \"sysfs\"");
        let note = match s(zone, "temp") {
            Some(t) => format!("{kind}, {t} now"),
            None => kind.to_owned(),
        };
        w.key("thermal", &format!("\"{zone_name}\""), Some(&note));
    }

    let serial = list(hw, "serial");
    let soc: Vec<&Value> = serial
        .iter()
        .filter(|p| p.get("soc").and_then(Value::as_bool) == Some(true))
        .collect();
    let i2c = list(hw, "i2c");
    let spi = list(hw, "spi");
    if soc.is_empty() && i2c.is_empty() && spi.is_empty() {
        w.line("");
        w.line(
            "# [serial], [i2c], [spi]: none on the node (they may be disabled in the boot configuration)",
        );
    }
    if !soc.is_empty() {
        w.line("");
        w.line("[serial]");
        let mut taken = BTreeSet::new();
        for port in &soc {
            let path = s(port, "path").unwrap_or("?");
            let alias = list(port, "aliases")
                .iter()
                .filter_map(Value::as_str)
                .find(|a| a.starts_with("/dev/serial") && !a.contains("/by-"));
            let (key, target) = match alias {
                Some(a) => (
                    format!("uart{}", a.trim_start_matches("/dev/serial")),
                    a.to_owned(),
                ),
                None => (slug(path.trim_start_matches("/dev/"), '_'), path.to_owned()),
            };
            let key = unique(key, &mut taken);
            w.key(
                &format!("\"{key}\""),
                &format!("\"{target}\""),
                Some(path.trim_start_matches("/dev/")),
            );
        }
    }
    for port in serial
        .iter()
        .filter(|p| p.get("soc").and_then(Value::as_bool) != Some(true))
    {
        let stable = list(port, "aliases")
            .iter()
            .filter_map(Value::as_str)
            .find(|a| a.contains("/by-id/"))
            .unwrap_or(s(port, "path").unwrap_or("?"));
        w.line(&format!(
            "# adapter, not the board's (copy it into a [buses] of the manifest): {stable}"
        ));
    }
    if !i2c.is_empty() {
        w.line("");
        w.line("[i2c]");
        for bus in i2c {
            let path = s(bus, "path").unwrap_or("?");
            let n = path.trim_start_matches("/dev/i2c-");
            w.line(&format!("\"i2c{n}\" = \"{path}\""));
        }
    }
    if !spi.is_empty() {
        w.line("");
        w.line("[spi]");
        for dev in spi {
            let path = s(dev, "path").unwrap_or("?");
            let bus = dev.get("bus").and_then(Value::as_u64).unwrap_or(0);
            let cs = dev.get("cs").and_then(Value::as_u64).unwrap_or(0);
            let key = if cs == 0 {
                format!("spi{bus}")
            } else {
                format!("spi{bus}_{cs}")
            };
            w.line(&format!("\"{key}\" = \"{path}\""));
        }
    }
    let mut net_lines = Vec::new();
    let (mut eth, mut wifi) = (0, 0);
    for iface in list(hw, "net") {
        let Some(iname) = s(iface, "name") else {
            continue;
        };
        let key = match s(iface, "kind") {
            Some("ethernet") => {
                eth += 1;
                if eth == 1 {
                    "eth".to_owned()
                } else {
                    format!("eth{}", eth - 1)
                }
            }
            Some("wifi") => {
                wifi += 1;
                if wifi == 1 {
                    "wifi".to_owned()
                } else {
                    format!("wifi{}", wifi - 1)
                }
            }
            _ => continue,
        };
        net_lines.push(format!("\"{key}\" = \"{iname}\""));
    }
    if !net_lines.is_empty() {
        w.line("");
        w.line("[net]");
        for l in net_lines {
            w.line(&l);
        }
    }
    w.line("");
    let tpm = if hw["tpm"].get("present").and_then(Value::as_bool) == Some(true) {
        format!(
            "TPM {} at {}",
            s(&hw["tpm"], "version").unwrap_or("?"),
            s(&hw["tpm"], "device").unwrap_or("?")
        )
    } else {
        "TPM: none".to_owned()
    };
    let usb: Vec<String> = list(hw, "usb")
        .iter()
        .map(|d| {
            format!(
                "{}:{} {}",
                s(d, "vendor_id").unwrap_or("?"),
                s(d, "product_id").unwrap_or("?"),
                s(d, "product").unwrap_or("")
            )
            .trim_end()
            .to_owned()
        })
        .collect();
    let usb = if usb.is_empty() {
        "nothing connected".to_owned()
    } else {
        usb.join("; ")
    };
    w.line(&format!("# {tpm}. USB: {usb}."));
    Ok(Draft {
        name,
        text: w.out,
        reviews: w.reviews,
    })
}

/// The lines `jk board probe` prints about the inventory before writing.
pub fn summary(hw: &Value) -> String {
    let mut out = String::new();
    let cpus = &hw["cpus"];
    let _ = writeln!(
        out,
        "cpu     {} × {} ({}){}",
        cpus.get("count").and_then(Value::as_u64).unwrap_or(0),
        s(cpus, "model").unwrap_or("?"),
        s(hw, "machine").unwrap_or("?"),
        s(cpus, "max_freq")
            .map(|f| format!(" · cpufreq max {f}"))
            .unwrap_or_default()
    );
    let _ = writeln!(
        out,
        "memory  the kernel sees {}",
        s(&hw["memory"], "total").unwrap_or("?")
    );
    let kernel = &hw["kernel"];
    let _ = writeln!(
        out,
        "kernel  {} · {}",
        s(kernel, "release").unwrap_or("?"),
        if kernel.get("preempt_rt").and_then(Value::as_bool) == Some(true) {
            "PREEMPT_RT"
        } else {
            "no PREEMPT_RT"
        }
    );
    let leds: Vec<&str> = list(hw, "leds")
        .iter()
        .filter_map(|l| s(l, "name"))
        .collect();
    let zones: Vec<String> = list(hw, "thermal_zones")
        .iter()
        .map(|z| {
            format!(
                "{} ({})",
                s(z, "name").unwrap_or("?"),
                s(z, "type").unwrap_or("?")
            )
        })
        .collect();
    let dogs: Vec<String> = list(hw, "watchdogs")
        .iter()
        .map(|d| {
            format!(
                "{} ({})",
                s(d, "device").unwrap_or("?").trim_start_matches("/dev/"),
                s(d, "identity").unwrap_or("?")
            )
        })
        .collect();
    let _ = writeln!(
        out,
        "sysfs   leds: {} · thermal: {} · {}",
        if leds.is_empty() {
            "none".to_owned()
        } else {
            leds.join(", ")
        },
        if zones.is_empty() {
            "none".to_owned()
        } else {
            zones.join(", ")
        },
        if dogs.is_empty() {
            "no watchdog".to_owned()
        } else {
            dogs.join(", ")
        },
    );
    for chip in list(hw, "gpio_chips") {
        let _ = writeln!(
            out,
            "gpio    {} \"{}\" ({} lines)",
            s(chip, "name").unwrap_or("?"),
            s(chip, "label").unwrap_or("?"),
            chip.get("lines").and_then(Value::as_u64).unwrap_or(0)
        );
    }
    for port in list(hw, "serial") {
        let _ = writeln!(
            out,
            "tty     {}{}",
            s(port, "path").unwrap_or("?"),
            if port.get("soc").and_then(Value::as_bool) == Some(true) {
                ""
            } else {
                " (adapter)"
            }
        );
    }
    let ifaces: Vec<&str> = list(hw, "net")
        .iter()
        .filter(|n| matches!(s(n, "kind"), Some("ethernet" | "wifi")))
        .filter_map(|n| s(n, "name"))
        .collect();
    if !ifaces.is_empty() {
        let _ = writeln!(out, "net     {}", ifaces.join(", "));
    }
    out
}

/// One finding of `jk board check --node`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// `J0828` or `J0829`; `None` for a note.
    pub code: Option<&'static str>,
    /// What is wrong.
    pub message: String,
    /// The 1-based line of the board file it is about, when known.
    pub line: Option<usize>,
}

impl Finding {
    fn error(code: &'static str, message: String, line: Option<usize>) -> Self {
        Self {
            code: Some(code),
            message,
            line,
        }
    }

    fn note(message: String, line: Option<usize>) -> Self {
        Self {
            code: None,
            message,
            line,
        }
    }
}

/// The 1-based line of the first line of `text` that holds the quoted
/// `value` (or, unquoted, a `key =` pair).
fn line_of(text: &str, needle: &str) -> Option<usize> {
    text.lines()
        .position(|l| {
            let code = l.split(" #").next().unwrap_or(l);
            code.contains(needle)
        })
        .map(|i| i + 1)
}

fn quoted_line(text: &str, value: &str) -> Option<usize> {
    line_of(text, &format!("\"{value}\""))
}

fn key_line(text: &str, key: &str) -> Option<usize> {
    text.lines()
        .position(|l| {
            let t = l.trim_start();
            t.strip_prefix(key)
                .is_some_and(|r| r.trim_start().starts_with('='))
        })
        .map(|i| i + 1)
}

fn names<'a>(hw: &'a Value, list_key: &str, key: &str) -> Vec<&'a str> {
    list(hw, list_key)
        .iter()
        .filter_map(|v| s(v, key))
        .collect()
}

fn check_paths(
    text: &str,
    table: Option<&toml::Value>,
    known: &[&str],
    what: &str,
    node: &str,
    out: &mut Vec<Finding>,
) {
    let Some(t) = table.and_then(toml::Value::as_table) else {
        return;
    };
    for value in t.values() {
        let path = match value {
            toml::Value::String(p) => p.as_str(),
            toml::Value::Table(sub) => match sub.get("path").and_then(toml::Value::as_str) {
                Some(p) => p,
                None => continue,
            },
            _ => continue,
        };
        if !known.contains(&path) {
            out.push(Finding::error(
                "J0828",
                format!("`{path}`: node {node} does not have this {what}"),
                quoted_line(text, path),
            ));
        }
    }
}

/// Confirms the board file `text` against the inventory `hw` of `node`
/// (ref/03 §2.3.7, `jk board check --node`): errors for what the node
/// contradicts, notes for the kernel and for the `# REVIEW` lines left.
pub fn confirm(text: &str, hw: &Value, node: &str) -> Result<Vec<Finding>, String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("{e}"))?;
    let mut out = Vec::new();
    let empty = toml::Table::new();
    let board = doc
        .get("board")
        .and_then(toml::Value::as_table)
        .unwrap_or(&empty);
    let node_arch = s(hw, "arch").unwrap_or("?");
    if let Some(arch) = board.get("arch").and_then(toml::Value::as_str)
        && arch != node_arch
    {
        out.push(Finding::error(
            "J0829",
            format!("`arch = \"{arch}\"`, but node {node} is {node_arch}"),
            key_line(text, "arch"),
        ));
    }
    let possible = hw["cpus"]
        .get("possible")
        .or_else(|| hw["cpus"].get("count"))
        .and_then(Value::as_i64);
    if let (Some(cpus), Some(p)) = (
        board.get("cpus").and_then(toml::Value::as_integer),
        possible,
    ) && cpus != p
    {
        out.push(Finding::error(
            "J0829",
            format!("`cpus = {cpus}`, but node {node} has {p}"),
            key_line(text, "cpus"),
        ));
    }
    let total = s(&hw["memory"], "total").and_then(iec_bytes);
    if let (Some(memory), Some(total)) = (
        board
            .get("memory")
            .and_then(toml::Value::as_str)
            .and_then(iec_bytes),
        total,
    ) && total > 0
        && (memory < total || memory >= 2 * total)
    {
        out.push(Finding::error(
            "J0829",
            format!(
                "`memory` must be at least the node's MemTotal ({}) and less than twice it",
                s(&hw["memory"], "total").unwrap_or("?")
            ),
            key_line(text, "memory"),
        ));
    }

    if let Some(wd) = doc.get("watchdog").and_then(toml::Value::as_table)
        && let Some(device) = wd.get("device").and_then(toml::Value::as_str)
    {
        match list(hw, "watchdogs")
            .iter()
            .find(|d| s(d, "device") == Some(device))
        {
            None => out.push(Finding::error(
                "J0828",
                format!("`{device}`: node {node} does not have this watchdog"),
                quoted_line(text, device),
            )),
            Some(found) => {
                if let Some(t) = wd
                    .get("timeout")
                    .and_then(toml::Value::as_str)
                    .and_then(seconds)
                {
                    let min = s(found, "min_timeout").and_then(seconds);
                    let max = s(found, "max_timeout").and_then(seconds);
                    if min.is_some_and(|m| t < m) || max.is_some_and(|m| t > m) {
                        out.push(Finding::error(
                            "J0829",
                            format!(
                                "the watchdog timeout is outside what node {node}'s {device} accepts ({} to {})",
                                s(found, "min_timeout").unwrap_or("?"),
                                s(found, "max_timeout").unwrap_or("?")
                            ),
                            key_line(text, "timeout"),
                        ));
                    }
                }
            }
        }
    }

    let chips = list(hw, "gpio_chips");
    let gpio = doc.get("gpio").and_then(toml::Value::as_table);
    let chip = gpio
        .and_then(|g| g.get("chip"))
        .and_then(toml::Value::as_str)
        .filter(|c| *c != "auto");
    let node_chip = chip.and_then(|c| {
        chips
            .iter()
            .find(|k| [s(k, "label"), s(k, "path"), s(k, "name")].contains(&Some(c)))
    });
    if let Some(c) = chip
        && node_chip.is_none()
    {
        out.push(Finding::error(
            "J0828",
            format!("`{c}`: node {node} has no GPIO chip with this label or path"),
            quoted_line(text, c),
        ));
    }
    if let (Some(k), Some(lines)) = (
        node_chip,
        gpio.and_then(|g| g.get("lines"))
            .and_then(toml::Value::as_table),
    ) {
        let count = k.get("lines").and_then(Value::as_i64).unwrap_or(0);
        for (name, offset) in lines {
            if offset.as_integer().is_none_or(|o| o < 0 || o >= count) {
                out.push(Finding::error(
                    "J0828",
                    format!("line `{name}`: the chip of node {node} has {count} lines"),
                    key_line(text, name).or_else(|| quoted_line(text, name)),
                ));
            }
        }
    }

    let leds = names(hw, "leds", "name");
    let zones = names(hw, "thermal_zones", "name");
    if let Some(io) = doc.get("io").and_then(toml::Value::as_table) {
        for (name, point) in io {
            let Some(point) = point.as_table() else {
                continue;
            };
            let driver = point.get("driver").and_then(toml::Value::as_str);
            match driver {
                Some("sysfs") => {
                    if let Some(led) = point.get("led").and_then(toml::Value::as_str)
                        && !leds.contains(&led)
                    {
                        out.push(Finding::error(
                            "J0828",
                            format!("`io.{name}`: node {node} has no LED `{led}`"),
                            quoted_line(text, led),
                        ));
                    }
                    if let Some(zone) = point.get("thermal").and_then(toml::Value::as_str)
                        && !zones.contains(&zone)
                    {
                        out.push(Finding::error(
                            "J0828",
                            format!("`io.{name}`: node {node} has no thermal zone `{zone}`"),
                            quoted_line(text, zone),
                        ));
                    }
                }
                Some("gpio") => {
                    let Some(k) = node_chip else {
                        continue;
                    };
                    let line = point.get("line");
                    let named = gpio
                        .and_then(|g| g.get("lines"))
                        .and_then(toml::Value::as_table);
                    let offset = match line {
                        Some(toml::Value::Integer(o)) => Some(*o),
                        Some(toml::Value::String(n)) => named
                            .and_then(|l| l.get(n.as_str()))
                            .and_then(toml::Value::as_integer),
                        _ => None,
                    };
                    let line_name = line.and_then(toml::Value::as_str);
                    let busy = list(k, "used").iter().find(|u| {
                        offset.is_some_and(|o| u.get("offset").and_then(Value::as_i64) == Some(o))
                            || line_name.is_some_and(|n| s(u, "name") == Some(n))
                    });
                    if let Some(u) = busy {
                        out.push(Finding::error(
                            "J0828",
                            format!(
                                "`io.{name}`: the line is in use on node {node} by `{}`",
                                s(u, "consumer").unwrap_or("?")
                            ),
                            line_of(text, &format!("[io.{name}]")),
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    let mut serial: Vec<&str> = Vec::new();
    for port in list(hw, "serial") {
        serial.extend(s(port, "path"));
        serial.extend(list(port, "aliases").iter().filter_map(Value::as_str));
    }
    check_paths(
        text,
        doc.get("serial"),
        &serial,
        "serial port",
        node,
        &mut out,
    );
    let i2c = names(hw, "i2c", "path");
    check_paths(text, doc.get("i2c"), &i2c, "I²C bus", node, &mut out);
    let spi = names(hw, "spi", "path");
    check_paths(text, doc.get("spi"), &spi, "SPI device", node, &mut out);
    let net = names(hw, "net", "name");
    check_paths(
        text,
        doc.get("net"),
        &net,
        "network interface",
        node,
        &mut out,
    );

    let kernel = &hw["kernel"];
    let node_rt = kernel.get("preempt_rt").and_then(Value::as_bool) == Some(true);
    if board.get("kernel").and_then(toml::Value::as_str) == Some("preempt_rt") && !node_rt {
        out.push(Finding::note(
            format!(
                "the board targets a PREEMPT_RT kernel; node {node} runs {} without it (jk doctor compares)",
                s(kernel, "release").unwrap_or("?")
            ),
            key_line(text, "kernel"),
        ));
    }
    let cmdline: Vec<&str> = list(hw, "cmdline")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if let Some(boot) = board.get("boot_cmdline").and_then(toml::Value::as_array) {
        let missing: Vec<&str> = boot
            .iter()
            .filter_map(toml::Value::as_str)
            .filter(|b| !cmdline.contains(b))
            .collect();
        if !missing.is_empty() {
            out.push(Finding::note(
                format!(
                    "node {node} boots without {} (jk doctor --rt compares)",
                    missing.join(" ")
                ),
                key_line(text, "boot_cmdline"),
            ));
        }
    }
    let reviews: Vec<String> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("# REVIEW:"))
        .map(|(i, _)| (i + 1).to_string())
        .collect();
    if !reviews.is_empty() {
        out.push(Finding::note(
            format!(
                "{} \"# REVIEW\" lines unresolved ({})",
                reviews.len(),
                reviews.join(", ")
            ),
            None,
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn zero() -> Value {
        json!({
            "collected_at": "2026-10-30T10:00:00Z",
            "arch": "armv6",
            "machine": "armv6l",
            "model": "Raspberry Pi Zero W Rev 1.1",
            "compatible": ["raspberrypi,model-zero-w", "brcm,bcm2835"],
            "cpus": {"count": 1, "possible": 1, "model": "ARMv6-compatible processor rev 7 (v6l)", "max_freq": "1 GHz", "isolated": [], "nohz_full": []},
            "memory": {"total": "427 MiB"},
            "kernel": {"release": "6.12.25+rpt-rpi-v6", "version": "#1", "preempt_rt": false, "preempt": null},
            "cmdline": ["console=serial0,115200", "console=tty1", "rootwait"],
            "gpio_chips": [{"path": "/dev/gpiochip0", "name": "gpiochip0", "label": "pinctrl-bcm2835", "lines": 54, "named": 54,
                "used": [{"offset": 47, "name": "ACT", "consumer": "ACT"}]}],
            "leds": [{"name": "ACT", "max_brightness": 1, "trigger": "actpwr", "gpio": {"chip": "pinctrl-bcm2835", "offset": 47}}],
            "thermal_zones": [{"name": "thermal_zone0", "type": "cpu-thermal", "temp": "41.2 °C"}],
            "watchdogs": [{"device": "/dev/watchdog0", "identity": "bcm2835-wdt", "timeout": "15s", "min_timeout": "1s", "max_timeout": "15s", "nowayout": false, "active": false}],
            "serial": [{"path": "/dev/ttyUSB0", "aliases": ["/dev/serial/by-id/usb-Silicon_Labs_CP2102_0001-if00-port0"], "driver": "cp210x", "soc": false}],
            "i2c": [],
            "spi": [],
            "net": [{"name": "lo", "kind": "loopback"}, {"name": "wlan0", "kind": "wifi"}],
            "usb": [],
            "tpm": {"present": false},
            "storage": {"device": "/dev/mmcblk0", "medium": "sd"},
            "unreadable": [],
        })
    }

    #[test]
    fn the_draft_of_a_pi_zero_w_follows_ref_03() {
        let d = draft(&zero(), "zero", Some("rpi-zero-w")).unwrap();
        assert_eq!(d.name, "rpi-zero-w");
        let doc: toml::Table = d.text.parse().unwrap();
        let board = doc["board"].as_table().unwrap();
        assert_eq!(board["name"].as_str(), Some("rpi-zero-w"));
        assert_eq!(board["arch"].as_str(), Some("armv6"));
        assert_eq!(board["tier"].as_str(), Some("dev"));
        assert_eq!(board["cpus"].as_integer(), Some(1));
        assert_eq!(board["memory"].as_str(), Some("512 MiB"));
        assert_eq!(board["clock"].as_str(), Some("1 GHz"));
        assert_eq!(board["storage"].as_str(), Some("sd"));
        assert_eq!(board["kernel"].as_str(), Some("standard"));
        assert!(board.get("boot_cmdline").is_none());
        assert_eq!(doc["watchdog"]["timeout"].as_str(), Some("15s"));
        assert_eq!(doc["gpio"]["chip"].as_str(), Some("pinctrl-bcm2835"));
        assert_eq!(doc["io"]["act_led"]["led"].as_str(), Some("ACT"));
        assert!(doc["io"]["act_led"].get("fallback").is_none());
        assert_eq!(
            doc["io"]["cpu_temp"]["thermal"].as_str(),
            Some("thermal_zone0")
        );
        assert!(doc.get("serial").is_none());
        assert!(d.text.contains("usb-Silicon_Labs_CP2102_0001-if00-port0"));
        assert_eq!(doc["net"]["wifi"].as_str(), Some("wlan0"));
        assert_eq!(d.reviews, 5, "{}", d.text);
        assert!(
            confirm(&d.text, &zero(), "zero")
                .unwrap()
                .iter()
                .all(|f| f.code.is_none())
        );
    }

    #[test]
    fn without_a_name_the_model_names_the_board_for_review() {
        let d = draft(&zero(), "zero", None).unwrap();
        assert_eq!(d.name, "raspberry-pi-zero-w-rev-1-1");
        assert_eq!(d.reviews, 6);
        let mut hw = zero();
        hw["model"] = Value::Null;
        assert!(draft(&hw, "zero", None).is_err());
    }

    #[test]
    fn four_cpus_isolate_the_last_one() {
        let mut hw = zero();
        hw["cpus"]["possible"] = json!(4);
        hw["memory"]["total"] = json!("7.8 GiB");
        let d = draft(&hw, "pi", Some("x")).unwrap();
        let doc: toml::Table = d.text.parse().unwrap();
        assert_eq!(doc["board"]["memory"].as_str(), Some("8 GiB"));
        let rec = doc["board"]["recommended"].as_table().unwrap();
        assert_eq!(rec["rt"].as_array().unwrap()[0].as_integer(), Some(3));
        assert_eq!(rec["io"].as_array().unwrap()[0].as_integer(), Some(2));
        assert_eq!(rec["bg"].as_array().unwrap().len(), 2);
        assert_eq!(
            doc["board"]["boot_cmdline"].as_array().unwrap()[0].as_str(),
            Some("isolcpus=3")
        );
    }

    #[test]
    fn the_node_contradicts_a_board_it_does_not_match() {
        let text = concat!(
            "schema = 1\n",
            "[board]\n",
            "name = \"x\"\n",
            "version = \"1.0.0\"\n",
            "arch = \"aarch64\"\n",
            "cpus = 4\n",
            "memory = \"256 MiB\"\n",
            "kernel = \"preempt_rt\"\n",
            "[watchdog]\n",
            "device = \"/dev/watchdog1\"\n",
            "[gpio]\n",
            "chip = \"pinctrl-rp1\"\n",
            "[io.act_led]\n",
            "driver = \"sysfs\"\n",
            "led = \"PWR\"\n",
            "[io.board_temp]\n",
            "driver = \"sysfs\"\n",
            "thermal = \"thermal_zone3\"\n",
            "[i2c]\n",
            "\"i2c1\" = \"/dev/i2c-1\"\n",
            "[net]\n",
            "\"eth\" = \"eth0\"\n",
        );
        let found = confirm(text, &zero(), "zero").unwrap();
        let errors: Vec<(&str, Option<usize>)> = found
            .iter()
            .filter_map(|f| f.code.map(|c| (c, f.line)))
            .collect();
        assert_eq!(
            errors,
            vec![
                ("J0829", Some(5)),
                ("J0829", Some(6)),
                ("J0829", Some(7)),
                ("J0828", Some(10)),
                ("J0828", Some(12)),
                ("J0828", Some(15)),
                ("J0828", Some(18)),
                ("J0828", Some(20)),
                ("J0828", Some(22)),
            ],
            "{found:#?}"
        );
        assert!(found.iter().any(|f| f.code.is_none() && f.line == Some(8)));
    }

    #[test]
    fn a_gpio_io_line_held_by_the_kernel_is_refused() {
        let text = "schema = 1\n[board]\narch = \"armv6\"\n[gpio]\nchip = \"pinctrl-bcm2835\"\n[io.led]\ndriver = \"gpio\"\nline = 47\ndirection = \"output\"\n";
        let found = confirm(text, &zero(), "zero").unwrap();
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].code, Some("J0828"));
        assert!(found[0].message.contains("`ACT`"));
        assert_eq!(found[0].line, Some(6));
    }

    #[test]
    fn a_too_long_watchdog_timeout_is_inconsistent() {
        let text = "schema = 1\n[board]\narch = \"armv6\"\n[watchdog]\ndevice = \"/dev/watchdog0\"\ntimeout = \"60s\"\n";
        let found = confirm(text, &zero(), "zero").unwrap();
        assert_eq!(found[0].code, Some("J0829"));
        assert_eq!(found[0].line, Some(6));
    }

    #[test]
    fn sizes() {
        assert_eq!(memory_size(427 << 20), "512 MiB");
        assert_eq!(memory_size(1 << 30), "1 GiB");
        assert_eq!(memory_size((3 << 30) + (700 << 20)), "4 GiB");
        assert_eq!(seconds("15s"), Some(15.0));
        assert_eq!(seconds("1 min"), Some(60.0));
        assert_eq!(slug("ACT", '_'), "act");
        assert_eq!(thermal_name("cpu-thermal"), "cpu_temp");
        assert_eq!(thermal_name("x86_pkg_temp"), "x86_pkg_temp");
    }
}
