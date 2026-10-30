//! The hardware inventory of the node (`GET /v1/node/hardware`, ref/09
//! §5.2): what exists now on this machine, read from `/proc`, `/sys` and
//! `/dev` only. Nothing is opened for writing; the GPIO lines are read
//! through the line-information ioctl, which does not request them.
//!
//! Every path is read below a root (`/` on a node), so the inventory of a
//! board can be reproduced from a copy of its files.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::wire::{duration, timestamp};

/// One line of a GPIO chip, as the line-information ioctl reports it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    /// The kernel's name of the line; empty when none.
    pub name: String,
    /// Who holds the line; empty when nobody does.
    pub consumer: String,
    /// Whether the kernel or another process has requested it.
    pub used: bool,
}

/// One GPIO chip and its lines.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Chip {
    /// The kernel's name (`gpiochip0`).
    pub name: String,
    /// The label (`pinctrl-bcm2835`).
    pub label: String,
    /// The lines, by offset.
    pub lines: Vec<Line>,
}

/// What the inventory asks of the kernel beyond reading files.
pub trait Kernel {
    /// The GPIO chip at `path` (`/dev/gpiochip0`, already below the root),
    /// read without requesting any of its lines.
    fn chip(&self, path: &Path) -> std::io::Result<Chip>;
}

/// The running kernel: GPIO through `joker-gpio-sys`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveKernel;

impl Kernel for LiveKernel {
    fn chip(&self, path: &Path) -> std::io::Result<Chip> {
        use std::os::fd::AsFd;
        let file = std::fs::File::open(path)?;
        let info = joker_gpio_sys::chip_info(file.as_fd())?;
        let mut lines = Vec::new();
        for offset in 0..info.lines {
            let l = joker_gpio_sys::line_info(file.as_fd(), offset)?;
            lines.push(Line {
                name: l.name,
                consumer: l.consumer,
                used: l.used,
            });
        }
        Ok(Chip {
            name: info.name,
            label: info.label,
            lines,
        })
    }
}

/// What the inventory reads besides the files below the root.
#[derive(Debug, Clone)]
pub struct Probe<'a> {
    /// The root the paths are read below (`/` on a node).
    pub root: &'a Path,
    /// When the inventory is read, in UTC ns.
    pub now_utc_ns: i64,
    /// The CPUs available to this process, used when `/sys` has no CPU
    /// list.
    pub fallback_cpus: usize,
}

struct Reader<'a> {
    root: &'a Path,
    unreadable: Vec<Value>,
}

fn reason(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::PermissionDenied => "permission",
        ErrorKind::NotFound => "absent",
        _ => "error",
    }
}

impl Reader<'_> {
    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel.trim_start_matches('/'))
    }

    fn text(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.path(rel))
            .ok()
            .map(|s| s.trim_end_matches(['\n', '\0']).to_owned())
    }

    fn text_at(path: &Path) -> Option<String> {
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_owned())
    }

    fn noted(&mut self, field: &str, rel: &str) -> Option<String> {
        match std::fs::read_to_string(self.path(rel)) {
            Ok(s) => Some(s.trim_end_matches(['\n', '\0']).to_owned()),
            Err(e) => {
                self.note(field, rel, e.kind());
                None
            }
        }
    }

    fn note(&mut self, field: &str, rel: &str, kind: ErrorKind) {
        self.unreadable
            .push(json!({"field": field, "path": rel, "reason": reason(kind)}));
    }

    fn entries(&mut self, field: &str, rel: &str, required: bool) -> Vec<(String, PathBuf)> {
        match std::fs::read_dir(self.path(rel)) {
            Ok(dir) => {
                let mut out: Vec<(String, PathBuf)> = dir
                    .filter_map(Result::ok)
                    .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                    .collect();
                out.sort_by_key(|a| natural(&a.0));
                out
            }
            Err(e) => {
                if required || e.kind() != ErrorKind::NotFound {
                    self.note(field, rel, e.kind());
                }
                Vec::new()
            }
        }
    }
}

/// A sort key that orders `ttyS10` after `ttyS2`.
fn natural(name: &str) -> (String, u64) {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (stem, number) = name.split_at(name.len() - digits);
    (stem.to_owned(), number.parse().unwrap_or(0))
}

/// The last component of the target of the link at `path`.
fn link_name(path: &Path) -> Option<String> {
    std::fs::read_link(path)
        .ok()
        .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// The CPUs of a kernel CPU list (`0-3,5`).
pub fn cpu_list(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in text.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                    out.extend(a..=b);
                }
            }
            None => {
                if let Ok(c) = part.trim().parse() {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// `[board] arch` of a raw `uname -m` (ref/03 §2.3.1), `other` when none
/// matches.
pub fn normalize_arch(machine: &str) -> &'static str {
    match machine {
        "aarch64" | "arm64" | "armv8l" => "aarch64",
        "x86_64" | "amd64" => "x86_64",
        "riscv64" => "riscv64",
        m if m.starts_with("armv7") => "armv7",
        m if m.starts_with("armv6") => "armv6",
        _ => "other",
    }
}

/// An IEC size of `bytes`, in the largest unit that keeps it whole
/// (`"427 MiB"`), or a whole number of KiB.
pub fn iec(bytes: u64) -> String {
    const UNITS: [(&str, u64); 4] = [
        ("TiB", 1 << 40),
        ("GiB", 1 << 30),
        ("MiB", 1 << 20),
        ("KiB", 1 << 10),
    ];
    for (unit, size) in UNITS {
        if bytes >= size && bytes.is_multiple_of(size) {
            return format!("{} {unit}", bytes / size);
        }
    }
    if bytes >= 1 << 20 {
        return format!("{} MiB", bytes >> 20);
    }
    if bytes >= 1 << 10 {
        return format!("{} KiB", bytes >> 10);
    }
    format!("{bytes} B")
}

/// A frequency of `khz` as a quantity (`"1.5 GHz"`, `"600 MHz"`).
pub fn frequency(khz: u64) -> String {
    let mhz = khz as f64 / 1000.0;
    if mhz >= 1000.0 {
        format!("{} GHz", trim_float(mhz / 1000.0))
    } else {
        format!("{} MHz", trim_float(mhz))
    }
}

fn trim_float(v: f64) -> String {
    let text = format!("{v:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn meminfo_kib(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.strip_prefix(':')?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

fn machine(r: &Reader<'_>) -> String {
    if let Some(m) = r.text("proc/sys/kernel/arch").filter(|m| !m.is_empty()) {
        return m;
    }
    match std::env::consts::ARCH {
        "arm" => {
            let cpuinfo = r.text("proc/cpuinfo").unwrap_or_default();
            if cpuinfo.contains("ARMv6") || cpuinfo.contains("v6l") {
                "armv6l".to_owned()
            } else {
                "armv7l".to_owned()
            }
        }
        other => other.to_owned(),
    }
}

fn cpus(r: &mut Reader<'_>, fallback: usize) -> Value {
    let count = r
        .text("sys/devices/system/cpu/online")
        .map(|t| cpu_list(&t).len())
        .filter(|n| *n > 0)
        .unwrap_or(fallback.max(1));
    let possible = r
        .text("sys/devices/system/cpu/possible")
        .map(|t| cpu_list(&t).len())
        .filter(|n| *n > 0)
        .unwrap_or(count);
    let cpuinfo = r.text("proc/cpuinfo").unwrap_or_default();
    let model = ["model name", "Processor", "cpu model", "uarch"]
        .iter()
        .find_map(|key| {
            cpuinfo.lines().find_map(|line| {
                let (k, v) = line.split_once(':')?;
                (k.trim() == *key).then(|| v.trim().to_owned())
            })
        })
        .filter(|m| !m.is_empty());
    let max_freq = r
        .text("sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .and_then(|t| t.trim().parse::<u64>().ok())
        .map(frequency);
    let isolated = r
        .text("sys/devices/system/cpu/isolated")
        .map(|t| cpu_list(&t))
        .unwrap_or_default();
    let nohz_full = r
        .text("sys/devices/system/cpu/nohz_full")
        .map(|t| cpu_list(&t))
        .unwrap_or_default();
    json!({
        "count": count,
        "possible": possible,
        "model": model,
        "max_freq": max_freq,
        "isolated": isolated,
        "nohz_full": nohz_full,
    })
}

fn memory(r: &mut Reader<'_>) -> Value {
    let text = r.noted("memory", "proc/meminfo").unwrap_or_default();
    let total = meminfo_kib(&text, "MemTotal").unwrap_or(0) * 1024;
    let mut m = Map::new();
    m.insert("total".into(), json!(iec(total)));
    if let Some(a) = meminfo_kib(&text, "MemAvailable") {
        m.insert("available".into(), json!(iec(a * 1024)));
    }
    Value::Object(m)
}

/// The preemption model of a kernel: the selected entry of
/// `sched/preempt` (`none voluntary (full)`), else from `uname -v`.
fn preempt(debug: Option<&str>, version: &str, rt: bool) -> Value {
    if let Some(text) = debug {
        let selected = text
            .split_whitespace()
            .find_map(|w| w.strip_prefix('(').and_then(|w| w.strip_suffix(')')));
        if let Some(s) = selected.filter(|s| matches!(*s, "none" | "voluntary" | "full" | "rt")) {
            return json!(s);
        }
    }
    if rt {
        json!("rt")
    } else if version.contains("PREEMPT_DYNAMIC") {
        Value::Null
    } else if version.contains("PREEMPT") {
        json!("full")
    } else {
        Value::Null
    }
}

fn kernel_info(r: &mut Reader<'_>) -> Value {
    let release = r
        .noted("kernel", "proc/sys/kernel/osrelease")
        .unwrap_or_default();
    let version = r.text("proc/sys/kernel/version").unwrap_or_default();
    let realtime = r
        .text("sys/kernel/realtime")
        .is_some_and(|t| t.trim() == "1");
    let preempt_rt = realtime || version.contains("PREEMPT_RT");
    let debug = r.text("sys/kernel/debug/sched/preempt");
    json!({
        "release": release,
        "version": version,
        "preempt_rt": preempt_rt,
        "preempt": preempt(debug.as_deref(), &version, preempt_rt),
    })
}

const BOOT_PARAMS: [&str; 4] = ["isolcpus", "nohz_full", "rcu_nocbs", "irqaffinity"];

fn cmdline(r: &mut Reader<'_>) -> (Vec<String>, Value) {
    let text = r.noted("cmdline", "proc/cmdline").unwrap_or_default();
    let lines: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
    let mut params = Map::new();
    for name in BOOT_PARAMS {
        let value = lines
            .iter()
            .find_map(|l| l.strip_prefix(name).and_then(|v| v.strip_prefix('=')));
        params.insert(name.into(), json!(value));
    }
    (lines, Value::Object(params))
}

fn gpio_chips(r: &mut Reader<'_>, gpio: &dyn Kernel) -> (Vec<Value>, BTreeMap<String, Value>) {
    let mut chips = Vec::new();
    let mut by_consumer = BTreeMap::new();
    for (name, path) in r.entries("gpio_chips", "dev", true) {
        if !name.starts_with("gpiochip") {
            continue;
        }
        let rel = format!("/dev/{name}");
        let chip = match gpio.chip(&path) {
            Ok(c) => c,
            Err(e) => {
                r.note("gpio_chips", &rel, e.kind());
                continue;
            }
        };
        let mut used = Vec::new();
        let mut named = 0;
        for (offset, line) in chip.lines.iter().enumerate() {
            if !line.name.is_empty() {
                named += 1;
            }
            if line.used {
                let consumer = (!line.consumer.is_empty()).then(|| line.consumer.clone());
                let line_name = (!line.name.is_empty()).then(|| line.name.clone());
                if let Some(c) = &consumer {
                    by_consumer
                        .entry(c.clone())
                        .or_insert_with(|| json!({"chip": chip.label, "offset": offset}));
                }
                used.push(json!({"offset": offset, "name": line_name, "consumer": consumer}));
            }
        }
        chips.push(json!({
            "path": rel,
            "name": if chip.name.is_empty() { name.clone() } else { chip.name.clone() },
            "label": chip.label,
            "lines": chip.lines.len(),
            "named": named,
            "used": used,
        }));
    }
    (chips, by_consumer)
}

fn leds(r: &mut Reader<'_>, by_consumer: &BTreeMap<String, Value>) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, path) in r.entries("leds", "sys/class/leds", false) {
        let max_brightness = Reader::text_at(&path.join("max_brightness"))
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or(0);
        let trigger = Reader::text_at(&path.join("trigger")).and_then(|t| {
            t.split_whitespace()
                .find_map(|w| w.strip_prefix('[').and_then(|w| w.strip_suffix(']')))
                .map(str::to_owned)
        });
        let gpio = by_consumer.get(&name).cloned().unwrap_or(Value::Null);
        out.push(json!({
            "name": name,
            "max_brightness": max_brightness,
            "trigger": trigger,
            "gpio": gpio,
        }));
    }
    out
}

fn thermal_zones(r: &mut Reader<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, path) in r.entries("thermal_zones", "sys/class/thermal", false) {
        if !name.starts_with("thermal_zone") {
            continue;
        }
        let kind = Reader::text_at(&path.join("type")).unwrap_or_default();
        let temp = Reader::text_at(&path.join("temp"))
            .and_then(|t| t.parse::<i64>().ok())
            .map(|m| format!("{} °C", trim_float(m as f64 / 1000.0)));
        out.push(json!({"name": name, "type": kind, "temp": temp}));
    }
    out
}

fn seconds(path: &Path) -> Value {
    Reader::text_at(path)
        .and_then(|t| t.parse::<u64>().ok())
        .map_or(Value::Null, |s| json!(duration(s * 1_000_000_000)))
}

fn watchdogs(r: &mut Reader<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, path) in r.entries("watchdogs", "sys/class/watchdog", false) {
        if !name.starts_with("watchdog") {
            continue;
        }
        out.push(json!({
            "device": format!("/dev/{name}"),
            "identity": Reader::text_at(&path.join("identity")),
            "timeout": seconds(&path.join("timeout")),
            "min_timeout": seconds(&path.join("min_timeout")),
            "max_timeout": seconds(&path.join("max_timeout")),
            "nowayout": Reader::text_at(&path.join("nowayout")).is_some_and(|t| t == "1"),
            "active": Reader::text_at(&path.join("state")).is_some_and(|t| t == "active"),
        }));
    }
    out
}

/// The nearest directory at or above `start` that has an `idVendor`: the
/// USB device a tty or an interface hangs from.
fn usb_device(start: &Path) -> Option<PathBuf> {
    let mut dir = std::fs::canonicalize(start).ok()?;
    loop {
        if dir.join("idVendor").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn hex4(path: &Path) -> Option<String> {
    Reader::text_at(path).map(|t| t.to_lowercase())
}

/// The links of `dir` (below the root), by the name of the device they
/// point to: `ttyAMA0 → [/dev/serial0]`.
fn aliases(r: &Reader<'_>, rel: &str, into: &mut BTreeMap<String, Vec<String>>) {
    let Ok(dir) = std::fs::read_dir(r.path(rel)) else {
        return;
    };
    let mut entries: Vec<_> = dir.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        if let Some(target) = link_name(&entry.path()) {
            into.entry(target).or_default().push(format!(
                "/{}/{}",
                rel.trim_matches('/'),
                entry.file_name().to_string_lossy()
            ));
        }
    }
}

fn serial(r: &mut Reader<'_>) -> Vec<Value> {
    let mut links: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in ["serial0", "serial1"] {
        if let Some(target) = link_name(&r.path(&format!("dev/{name}"))) {
            links
                .entry(target)
                .or_default()
                .push(format!("/dev/{name}"));
        }
    }
    aliases(r, "dev/serial/by-id", &mut links);
    aliases(r, "dev/serial/by-path", &mut links);
    let mut out = Vec::new();
    for (name, path) in r.entries("serial", "sys/class/tty", false) {
        let device = path.join("device");
        if std::fs::symlink_metadata(&device).is_err() {
            continue;
        }
        if Reader::text_at(&path.join("type")).is_some_and(|t| t == "0") {
            continue;
        }
        let driver = link_name(&device.join("driver"));
        let usb = usb_device(&device).map(|dev| {
            json!({
                "vendor_id": hex4(&dev.join("idVendor")),
                "product_id": hex4(&dev.join("idProduct")),
                "serial": Reader::text_at(&dev.join("serial")),
            })
        });
        out.push(json!({
            "path": format!("/dev/{name}"),
            "aliases": links.remove(&name).unwrap_or_default(),
            "driver": driver,
            "soc": usb.is_none(),
            "usb": usb,
        }));
    }
    out
}

fn i2c(r: &mut Reader<'_>, dev: &[(String, PathBuf)]) -> Vec<Value> {
    dev.iter()
        .filter(|(name, _)| name.starts_with("i2c-"))
        .map(|(name, _)| {
            let adapter = r.text(&format!("sys/class/i2c-adapter/{name}/name"));
            json!({"path": format!("/dev/{name}"), "name": adapter})
        })
        .collect()
}

fn spi(dev: &[(String, PathBuf)]) -> Vec<Value> {
    dev.iter()
        .filter_map(|(name, _)| {
            let (bus, cs) = name.strip_prefix("spidev")?.split_once('.')?;
            Some(json!({
                "path": format!("/dev/{name}"),
                "bus": bus.parse::<u32>().ok()?,
                "cs": cs.parse::<u32>().ok()?,
            }))
        })
        .collect()
}

fn net(r: &mut Reader<'_>, addresses: &BTreeMap<String, Vec<String>>) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, path) in r.entries("net", "sys/class/net", false) {
        let arp = Reader::text_at(&path.join("type")).and_then(|t| t.parse::<u32>().ok());
        let has_device = std::fs::symlink_metadata(path.join("device")).is_ok();
        let kind = if name == "lo" || arp == Some(772) {
            "loopback"
        } else if path.join("wireless").exists() || path.join("phy80211").exists() {
            "wifi"
        } else if !has_device {
            "virtual"
        } else if arp == Some(1) {
            "ethernet"
        } else {
            "other"
        };
        let usb = has_device
            && std::fs::canonicalize(path.join("device"))
                .ok()
                .is_some_and(|p| {
                    p.components()
                        .any(|c| c.as_os_str().to_string_lossy().starts_with("usb"))
                });
        out.push(json!({
            "name": name,
            "kind": kind,
            "mac": Reader::text_at(&path.join("address")).filter(|m| !m.is_empty()),
            "up": Reader::text_at(&path.join("operstate")).is_some_and(|s| s == "up"),
            "usb": usb,
            "addresses": addresses.get(&name).cloned().unwrap_or_default(),
        }));
    }
    out
}

fn usb(r: &mut Reader<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, path) in r.entries("usb", "sys/bus/usb/devices", false) {
        let Some(vendor) = hex4(&path.join("idVendor")) else {
            continue;
        };
        let mut m = Map::new();
        if let Some(bus) = Reader::text_at(&path.join("busnum")).and_then(|t| t.parse::<u32>().ok())
        {
            m.insert("bus".into(), json!(bus));
        }
        m.insert("port".into(), json!(name));
        m.insert("vendor_id".into(), json!(vendor));
        m.insert(
            "product_id".into(),
            json!(hex4(&path.join("idProduct")).unwrap_or_default()),
        );
        m.insert(
            "manufacturer".into(),
            json!(Reader::text_at(&path.join("manufacturer"))),
        );
        m.insert(
            "product".into(),
            json!(Reader::text_at(&path.join("product"))),
        );
        m.insert(
            "class".into(),
            json!(Reader::text_at(&path.join("bDeviceClass"))),
        );
        out.push(Value::Object(m));
    }
    out
}

fn tpm(r: &Reader<'_>) -> Value {
    let base = r.path("sys/class/tpm/tpm0");
    if !base.exists() {
        return json!({"present": false, "version": null, "device": null});
    }
    let version = Reader::text_at(&base.join("tpm_version_major")).map(|m| format!("{m}.0"));
    let device = ["dev/tpmrm0", "dev/tpm0"]
        .iter()
        .find(|d| r.path(d).exists())
        .map(|d| format!("/{d}"));
    json!({"present": true, "version": version, "device": device})
}

/// The IPv4 addresses of the host, by interface: the `host LOCAL` entries
/// of `/proc/net/fib_trie`, each given to the interface of the longest
/// route of `/proc/net/route` that holds it (`127.*` is `lo`). The route
/// table prints each address as the kernel's integer of its network-order
/// bytes.
fn ipv4(r: &Reader<'_>) -> Vec<(String, String)> {
    let Some(trie) = r.text("proc/net/fib_trie") else {
        return Vec::new();
    };
    let mut local = Vec::new();
    let mut last: Option<std::net::Ipv4Addr> = None;
    for line in trie.lines() {
        let t = line.trim_start();
        if let Some(ip) = t.strip_prefix("|-- ") {
            last = ip.trim().parse().ok();
        } else if t.starts_with("/32 host LOCAL")
            && let Some(ip) = last
            && !local.contains(&ip)
        {
            local.push(ip);
        }
    }
    let routes: Vec<(String, u32, u32)> = r
        .text("proc/net/route")
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let hex = |t: &str| {
                u32::from_str_radix(t, 16)
                    .ok()
                    .map(|raw| u32::from_be_bytes(raw.to_ne_bytes()))
            };
            Some((f.first()?.to_string(), hex(f.get(1)?)?, hex(f.get(7)?)?))
        })
        .collect();
    local
        .into_iter()
        .filter_map(|ip| {
            let value = u32::from(ip);
            let iface = if ip.is_loopback() {
                "lo".to_owned()
            } else {
                routes
                    .iter()
                    .filter(|(_, dest, mask)| value & mask == *dest)
                    .max_by_key(|(_, _, mask)| mask.count_ones())?
                    .0
                    .clone()
            };
            Some((iface, ip.to_string()))
        })
        .collect()
}

/// The addresses of each interface (`net[].addresses`): IPv4 from
/// [`ipv4`], IPv6 from `/proc/net/if_inet6`.
fn addresses(r: &Reader<'_>) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (iface, ip) in ipv4(r) {
        out.entry(iface).or_default().push(ip);
    }
    for line in r.text("proc/net/if_inet6").unwrap_or_default().lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let (Some(hex), Some(iface)) = (f.first(), f.get(5))
            && let Ok(bits) = u128::from_str_radix(hex, 16)
        {
            out.entry((*iface).to_owned())
                .or_default()
                .push(std::net::Ipv6Addr::from(bits).to_string());
        }
    }
    out
}

/// The block device of a partition (`mmcblk0p2` → `mmcblk0`): the parent
/// of its `/sys/class/block` entry when it has a `partition` file.
fn whole_disk(r: &Reader<'_>, name: &str) -> String {
    let entry = r.path(&format!("sys/class/block/{name}"));
    if entry.join("partition").is_file()
        && let Ok(real) = std::fs::canonicalize(&entry)
        && let Some(parent) = real.parent().and_then(Path::file_name)
    {
        return parent.to_string_lossy().into_owned();
    }
    name.to_owned()
}

/// The device mounted at `/`: from `/proc/self/mounts`, through the
/// `/dev/disk/by-*` link of `root=` when the mount names `/dev/root`.
fn root_device(r: &Reader<'_>) -> Option<String> {
    let mounts = r.text("proc/self/mounts")?;
    let source = mounts.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let source = fields.next()?;
        (fields.next()? == "/").then(|| source.to_owned())
    })?;
    let source = if source == "/dev/root" {
        let cmdline = r.text("proc/cmdline").unwrap_or_default();
        let root = cmdline
            .split_whitespace()
            .find_map(|w| w.strip_prefix("root="))?
            .to_owned();
        match root.split_once('=') {
            Some((kind, id)) => {
                let link = format!("dev/disk/by-{}/{id}", kind.to_lowercase());
                format!("/dev/{}", link_name(&r.path(&link))?)
            }
            None => root,
        }
    } else {
        source
    };
    let name = source.strip_prefix("/dev/")?;
    Some(whole_disk(r, name))
}

/// The medium the root file system lives on (ADR 0092): `sd` or `emmc`
/// from the MMC card type, `ssd` for NVMe and SATA disks.
fn storage(r: &Reader<'_>) -> Value {
    let Some(disk) = root_device(r) else {
        return Value::Null;
    };
    let medium = if disk.starts_with("nvme") || disk.starts_with("sd") {
        Some("ssd")
    } else if disk.starts_with("mmcblk") {
        match r.text(&format!("sys/block/{disk}/device/type")).as_deref() {
            Some("SD") => Some("sd"),
            Some("MMC") => Some("emmc"),
            _ => None,
        }
    } else {
        None
    };
    json!({"device": format!("/dev/{disk}"), "medium": medium})
}

fn model(r: &mut Reader<'_>) -> Value {
    if let Some(m) = r.text("proc/device-tree/model").filter(|m| !m.is_empty()) {
        return json!(m);
    }
    if let Some(m) = r
        .text("sys/class/dmi/id/product_name")
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
    {
        return json!(m);
    }
    r.note("model", "/proc/device-tree/model", ErrorKind::NotFound);
    Value::Null
}

/// The inventory of the machine below `probe.root` (`NodeHardware`,
/// ref/09 §5.2).
pub fn inventory(probe: &Probe<'_>, kernel: &dyn Kernel) -> Value {
    let mut r = Reader {
        root: probe.root,
        unreadable: Vec::new(),
    };
    let machine = machine(&r);
    let compatible: Vec<String> = std::fs::read(r.path("proc/device-tree/compatible"))
        .map(|b| {
            b.split(|&c| c == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect()
        })
        .unwrap_or_default();
    let model = model(&mut r);
    let cpus = cpus(&mut r, probe.fallback_cpus);
    let memory = memory(&mut r);
    let kernel_info = kernel_info(&mut r);
    let (cmdline, boot_params) = cmdline(&mut r);
    let (gpio_chips, by_consumer) = gpio_chips(&mut r, kernel);
    let leds = leds(&mut r, &by_consumer);
    let thermal_zones = thermal_zones(&mut r);
    let watchdogs = watchdogs(&mut r);
    let serial = serial(&mut r);
    let dev = r.entries("dev", "dev", false);
    let i2c = i2c(&mut r, &dev);
    let spi = spi(&dev);
    let assigned = addresses(&r);
    let net = net(&mut r, &assigned);
    let storage = storage(&r);
    let usb = usb(&mut r);
    let tpm = tpm(&r);
    json!({
        "collected_at": timestamp(probe.now_utc_ns),
        "arch": normalize_arch(&machine),
        "machine": machine,
        "model": model,
        "compatible": compatible,
        "cpus": cpus,
        "memory": memory,
        "kernel": kernel_info,
        "cmdline": cmdline,
        "boot_params": boot_params,
        "gpio_chips": gpio_chips,
        "leds": leds,
        "thermal_zones": thermal_zones,
        "watchdogs": watchdogs,
        "serial": serial,
        "i2c": i2c,
        "spi": spi,
        "net": net,
        "usb": usb,
        "tpm": tpm,
        "storage": storage,
        "unreadable": r.unreadable,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn sizes_frequencies_and_cpu_lists() {
        assert_eq!(iec(427 << 20), "427 MiB");
        assert_eq!(iec(8 << 30), "8 GiB");
        assert_eq!(iec((427 << 20) + 5), "427 MiB");
        assert_eq!(frequency(1_500_000), "1.5 GHz");
        assert_eq!(frequency(1_000_000), "1 GHz");
        assert_eq!(frequency(600_000), "600 MHz");
        assert_eq!(cpu_list("0-2,5\n"), vec![0, 1, 2, 5]);
        assert_eq!(cpu_list(""), Vec::<u32>::new());
        assert_eq!(normalize_arch("armv6l"), "armv6");
        assert_eq!(normalize_arch("armv7l"), "armv7");
        assert_eq!(normalize_arch("arm64"), "aarch64");
        assert_eq!(normalize_arch("mips"), "other");
    }

    #[test]
    fn the_preemption_model_prefers_the_debug_file() {
        assert_eq!(
            preempt(Some("none voluntary (full)"), "", false),
            json!("full")
        );
        assert_eq!(preempt(None, "#1 SMP PREEMPT_RT", true), json!("rt"));
        assert_eq!(preempt(None, "#1 SMP PREEMPT Debian", false), json!("full"));
        assert_eq!(preempt(None, "#1 SMP", false), Value::Null);
    }

    #[test]
    fn names_sort_by_their_number() {
        let mut v = vec!["ttyS10", "ttyS2", "ttyAMA0"];
        v.sort_by_key(|n| natural(n));
        assert_eq!(v, ["ttyAMA0", "ttyS2", "ttyS10"]);
    }
}
