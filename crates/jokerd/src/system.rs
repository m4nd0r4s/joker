//! What the node reports about itself: temperature, throttling, memory and
//! disk (docs/01 §19). `GET /v1/metrics` and the `joker.system.*` signals
//! read the same sample (ADR 0090). Each read touches a few files; never
//! call it on the tick path.

use std::path::Path;

/// One reading of the node; a field the node cannot read is `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// Thermal zone 0, in °C.
    pub temp_celsius: Option<f64>,
    /// The firmware reports under-voltage, frequency capping or throttling
    /// now.
    pub throttled: Option<bool>,
    /// `MemTotal` less `MemAvailable`, in bytes.
    pub memory_used_bytes: Option<u64>,
    /// The space an unprivileged process can still use on the state
    /// directory's filesystem, in bytes.
    pub disk_free_bytes: Option<u64>,
}

/// Reads the node now, with `state_dir` for the disk.
pub fn sample(state_dir: &Path) -> Sample {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    Sample {
        temp_celsius: read("/sys/class/thermal/thermal_zone0/temp")
            .and_then(|t| t.trim().parse::<i64>().ok())
            .map(|milli| milli as f64 / 1000.0),
        throttled: read("/sys/devices/platform/soc/soc:firmware/get_throttled")
            .and_then(|t| u64::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok())
            .map(|flags| flags & 0xF != 0),
        memory_used_bytes: read("/proc/meminfo")
            .and_then(|t| memory_used_kib(&t))
            .map(|kib| kib.saturating_mul(1024)),
        disk_free_bytes: disk_free_bytes(state_dir),
    }
}

/// `MemTotal` less `MemAvailable` of `/proc/meminfo`, in KiB.
fn memory_used_kib(meminfo: &str) -> Option<u64> {
    let field = |name: &str| {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|rest| {
                rest.trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
    };
    Some(field("MemTotal:")?.saturating_sub(field("MemAvailable:")?))
}

#[cfg(target_os = "linux")]
fn disk_free_bytes(dir: &Path) -> Option<u64> {
    let s = nix::sys::statvfs::statvfs(dir).ok()?;
    Some((s.blocks_available() as u64).saturating_mul(s.fragment_size() as u64))
}

#[cfg(not(target_os = "linux"))]
fn disk_free_bytes(_dir: &Path) -> Option<u64> {
    None
}
