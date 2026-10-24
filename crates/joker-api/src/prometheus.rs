//! The Prometheus text exposition of `Metrics` (ref/09 §5.2, ADR 0071):
//! `GET /metrics` renders the snapshot of `GET /v1/metrics`, with the `api`
//! object of the subscription engine, in the text format 0.0.4.
//!
//! The rendering writes every family straight into one buffer; a family
//! without samples is rolled back, header included, so absent fields never
//! show up as zeros. Nothing here reaches the cycle domain.

use std::fmt::Write as _;

use serde_json::Value;

/// The `Content-Type` of the exposition.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// The values of `LinkState`, in the order of the state set (ADR 0071).
const LINK_STATES: [&str; 6] = ["connecting", "up", "degraded", "down", "drift", "stopped"];

/// The results of the envelope, as `Metrics.envelope` names them.
const ENVELOPE_RESULTS: [&str; 5] = ["accepted", "clamped", "rejected", "suppressed", "tripped"];

/// The quantiles of a `Percentiles` object and the fields that carry them.
const QUANTILES: [(&str, &str); 3] = [("0.5", "p50"), ("0.99", "p99"), ("0.999", "p999")];

#[derive(Clone, Copy)]
enum Kind {
    Counter,
    Gauge,
    Summary,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Gauge => "gauge",
            Self::Summary => "summary",
        }
    }
}

/// How a field of `Metrics` becomes a number.
#[derive(Clone, Copy)]
enum Read {
    Number,
    Duration,
    Size,
    Celsius,
    Bool,
}

impl Read {
    fn value(self, v: &Value) -> Option<Number> {
        match self {
            Self::Number => v
                .as_u64()
                .map(Number::Int)
                .or_else(|| v.as_f64().map(Number::Float)),
            Self::Duration => v
                .as_str()
                .and_then(|t| joker_units::Duration::parse(t).ok())
                .map(|d| Number::Float(d.as_secs_f64())),
            Self::Size => v.as_str().and_then(size_bytes).map(Number::Int),
            Self::Celsius => v.as_str().and_then(celsius).map(Number::Float),
            Self::Bool => v.as_bool().map(|b| Number::Int(u64::from(b))),
        }
    }
}

#[derive(Clone, Copy)]
enum Number {
    Int(u64),
    Float(f64),
}

/// One label of a sample.
type Label<'a> = (&'static str, &'a str);

/// The exposition being written.
struct Exposition {
    out: String,
    family_start: usize,
    samples: usize,
}

impl Exposition {
    fn begin(&mut self, name: &str, kind: Kind, help: &str) {
        self.family_start = self.out.len();
        self.samples = 0;
        let _ = writeln!(self.out, "# HELP {name} {help}");
        let _ = writeln!(self.out, "# TYPE {name} {}", kind.name());
    }

    fn end(&mut self) {
        if self.samples == 0 {
            self.out.truncate(self.family_start);
        }
    }

    fn sample(&mut self, name: &str, labels: &[Label<'_>], value: Number) {
        self.out.push_str(name);
        if !labels.is_empty() {
            self.out.push('{');
            for (i, (k, v)) in labels.iter().enumerate() {
                if i > 0 {
                    self.out.push(',');
                }
                self.out.push_str(k);
                self.out.push_str("=\"");
                escape_into(&mut self.out, v);
                self.out.push('"');
            }
            self.out.push('}');
        }
        self.out.push(' ');
        match value {
            Number::Int(n) => {
                let _ = write!(self.out, "{n}");
            }
            Number::Float(f) if f.is_nan() => self.out.push_str("NaN"),
            Number::Float(f) if f.is_infinite() => {
                self.out.push_str(if f > 0.0 { "+Inf" } else { "-Inf" });
            }
            Number::Float(f) => {
                let _ = write!(self.out, "{f}");
            }
        }
        self.out.push('\n');
        self.samples += 1;
    }

    /// A family of plain samples (ADR 0071 point 7).
    fn plain(&mut self, metrics: &Value, f: &Family) {
        self.begin(f.name, f.kind, f.help);
        match f.from {
            From::Each {
                section,
                key,
                label,
            } => {
                for entity in entities(metrics, section) {
                    let Some(id) = entity.get(key).and_then(Value::as_str) else {
                        continue;
                    };
                    if let Some(n) = entity.get(f.field).and_then(|v| f.read.value(v)) {
                        self.sample(f.name, &[(label, id)], n);
                    }
                }
            }
            From::One(section) => {
                if let Some(n) = metrics
                    .get(section)
                    .and_then(|s| s.get(f.field))
                    .and_then(|v| f.read.value(v))
                {
                    self.sample(f.name, &[], n);
                }
            }
        }
        self.end();
    }

    /// The summary and the max gauge of a `Percentiles` field of each
    /// element of `section` (ADR 0071 point 8).
    fn percentiles(
        &mut self,
        metrics: &Value,
        section: &str,
        (key, label): (&str, &'static str),
        field: &str,
        (name, max_name): (&str, &str),
        help: &str,
    ) {
        self.begin(name, Kind::Summary, help);
        for entity in entities(metrics, section) {
            let (Some(id), Some(p)) = (entity.get(key).and_then(Value::as_str), entity.get(field))
            else {
                continue;
            };
            for (quantile, f) in QUANTILES {
                if let Some(n) = p.get(f).and_then(|v| Read::Duration.value(v)) {
                    self.sample(name, &[(label, id), ("quantile", quantile)], n);
                }
            }
        }
        self.end();
        self.begin(max_name, Kind::Gauge, help);
        for entity in entities(metrics, section) {
            let (Some(id), Some(p)) = (entity.get(key).and_then(Value::as_str), entity.get(field))
            else {
                continue;
            };
            if let Some(n) = p.get("max").and_then(|v| Read::Duration.value(v)) {
                self.sample(max_name, &[(label, id)], n);
            }
        }
        self.end();
    }
}

fn entities<'a>(metrics: &'a Value, section: &str) -> impl Iterator<Item = &'a Value> {
    metrics
        .get(section)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn escape_into(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
}

/// Bytes of an IEC or SI size (`"12 MiB"`, `"512 kB"`, `"1024"`).
fn size_bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let scale: f64 = match unit.trim() {
        "" | "B" => 1.0,
        "kB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1_048_576.0,
        "GiB" => 1_073_741_824.0,
        "TiB" => 1_099_511_627_776.0,
        _ => return None,
    };
    let bytes = (number * scale).round();
    (bytes.is_finite() && bytes >= 0.0 && bytes < u64::MAX as f64).then_some(bytes as u64)
}

/// Degrees of a temperature in Celsius (`"61 °C"`).
fn celsius(text: &str) -> Option<f64> {
    let number = text.trim().strip_suffix("°C")?.trim_end();
    number.parse().ok()
}

/// Where a family reads its samples.
#[derive(Clone, Copy)]
enum From {
    /// Each element of an array section, labelled with its `key`.
    Each {
        section: &'static str,
        key: &'static str,
        label: &'static str,
    },
    /// An object section.
    One(&'static str),
}

/// A family of plain samples: one field of a section of `Metrics`.
struct Family {
    from: From,
    field: &'static str,
    read: Read,
    name: &'static str,
    kind: Kind,
    help: &'static str,
}

const GROUP: From = From::Each {
    section: "groups",
    key: "group",
    label: "group",
};

const DRIVER: From = From::Each {
    section: "drivers",
    key: "device",
    label: "device",
};

const APP: From = From::Each {
    section: "apps",
    key: "app",
    label: "app",
};

const fn family(
    from: From,
    field: &'static str,
    read: Read,
    name: &'static str,
    kind: Kind,
    help: &'static str,
) -> Family {
    Family {
        from,
        field,
        read,
        name,
        kind,
        help,
    }
}

const CYCLE: [Family; 3] = [
    family(
        GROUP,
        "ticks",
        Read::Number,
        "joker_cycle_ticks_total",
        Kind::Counter,
        "Ticks of the cycle group.",
    ),
    family(
        GROUP,
        "overruns",
        Read::Number,
        "joker_cycle_overruns_total",
        Kind::Counter,
        "Overruns of the cycle group.",
    ),
    family(
        GROUP,
        "consecutive_overruns",
        Read::Number,
        "joker_cycle_consecutive_overruns",
        Kind::Gauge,
        "Consecutive overruns of the cycle group.",
    ),
];

const DRIVERS: [Family; 7] = [
    family(
        DRIVER,
        "timeouts",
        Read::Number,
        "joker_driver_timeouts_total",
        Kind::Counter,
        "Request timeouts of the device.",
    ),
    family(
        DRIVER,
        "reconnects",
        Read::Number,
        "joker_driver_reconnects_total",
        Kind::Counter,
        "Reconnections of the device.",
    ),
    family(
        DRIVER,
        "dropped_samples",
        Read::Number,
        "joker_driver_dropped_samples_total",
        Kind::Counter,
        "Samples the sample ring of the device refused.",
    ),
    family(
        DRIVER,
        "invalid_samples",
        Read::Number,
        "joker_driver_invalid_samples_total",
        Kind::Counter,
        "Samples of the device that broke ref/06 2.8.",
    ),
    family(
        DRIVER,
        "bytes_rx",
        Read::Number,
        "joker_driver_received_bytes_total",
        Kind::Counter,
        "Bytes received from the device.",
    ),
    family(
        DRIVER,
        "bytes_tx",
        Read::Number,
        "joker_driver_sent_bytes_total",
        Kind::Counter,
        "Bytes sent to the device.",
    ),
    family(
        DRIVER,
        "coalescing",
        Read::Number,
        "joker_driver_coalescing_ratio",
        Kind::Gauge,
        "Useful over read registers of the device.",
    ),
];

const REST: [Family; 18] = [
    family(
        From::One("arbiter"),
        "manual_active",
        Read::Number,
        "joker_arbiter_manual_active",
        Kind::Gauge,
        "Active manual overrides.",
    ),
    family(
        From::One("arbiter"),
        "leases_expiring",
        Read::Number,
        "joker_arbiter_leases_expiring",
        Kind::Gauge,
        "Leases about to expire.",
    ),
    family(
        APP,
        "traps",
        Read::Number,
        "joker_app_traps_total",
        Kind::Counter,
        "Traps of the app.",
    ),
    family(
        APP,
        "restarts",
        Read::Number,
        "joker_app_restarts_total",
        Kind::Counter,
        "Restarts of the app.",
    ),
    family(
        APP,
        "memory",
        Read::Size,
        "joker_app_memory_bytes",
        Kind::Gauge,
        "Memory of the app.",
    ),
    family(
        APP,
        "cpu_async",
        Read::Duration,
        "joker_app_cpu_async_seconds_total",
        Kind::Counter,
        "Async CPU time of the app.",
    ),
    family(
        From::One("store"),
        "queue",
        Read::Number,
        "joker_store_queue",
        Kind::Gauge,
        "Samples queued for the store.",
    ),
    family(
        From::One("store"),
        "dropped",
        Read::Number,
        "joker_store_dropped_total",
        Kind::Counter,
        "Samples the store dropped by policy.",
    ),
    family(
        From::One("store"),
        "disk_used",
        Read::Size,
        "joker_store_disk_used_bytes",
        Kind::Gauge,
        "Disk used by the store.",
    ),
    family(
        From::One("api"),
        "connections",
        Read::Number,
        "joker_api_connections",
        Kind::Gauge,
        "Open WebSocket and SSE connections.",
    ),
    family(
        From::One("api"),
        "subscriptions",
        Read::Number,
        "joker_api_subscriptions",
        Kind::Gauge,
        "Live subscriptions.",
    ),
    family(
        From::One("api"),
        "sampled_per_s",
        Read::Number,
        "joker_api_sampled_per_second",
        Kind::Gauge,
        "Samples per second reserved by the subscriptions.",
    ),
    family(
        From::One("api"),
        "dropped",
        Read::Number,
        "joker_api_dropped_total",
        Kind::Counter,
        "Items dropped from subscription rings.",
    ),
    family(
        From::One("api"),
        "rate_limited",
        Read::Number,
        "joker_api_rate_limited_total",
        Kind::Counter,
        "Requests refused by the rate limit.",
    ),
    family(
        From::One("system"),
        "temp",
        Read::Celsius,
        "joker_system_temperature_celsius",
        Kind::Gauge,
        "Temperature of the node.",
    ),
    family(
        From::One("system"),
        "throttled",
        Read::Bool,
        "joker_system_throttled",
        Kind::Gauge,
        "1 while the CPU is throttled.",
    ),
    family(
        From::One("system"),
        "memory_used",
        Read::Size,
        "joker_system_memory_used_bytes",
        Kind::Gauge,
        "Memory used on the node.",
    ),
    family(
        From::One("system"),
        "disk_free",
        Read::Size,
        "joker_system_disk_free_bytes",
        Kind::Gauge,
        "Free disk on the node.",
    ),
];

/// Renders `metrics` (a `Metrics` object) in the Prometheus text format
/// 0.0.4, with the families, names and labels of ADR 0071.
pub fn render(metrics: &Value) -> String {
    let groups = entities(metrics, "groups").count();
    let drivers = entities(metrics, "drivers").count();
    let outputs = entities(metrics, "envelope").count();
    let apps = entities(metrics, "apps").count();
    let estimate = 2048 + 640 * groups + 1280 * drivers + 512 * outputs + 384 * apps;
    let mut x = Exposition {
        out: String::with_capacity(estimate),
        family_start: 0,
        samples: 0,
    };
    for f in &CYCLE {
        x.plain(metrics, f);
    }
    x.percentiles(
        metrics,
        "groups",
        ("group", "group"),
        "jitter",
        (
            "joker_cycle_jitter_seconds",
            "joker_cycle_jitter_max_seconds",
        ),
        "Start jitter of the cycle group.",
    );
    x.percentiles(
        metrics,
        "groups",
        ("group", "group"),
        "duration",
        (
            "joker_cycle_duration_seconds",
            "joker_cycle_duration_max_seconds",
        ),
        "Tick duration of the cycle group.",
    );
    x.begin(
        "joker_driver_link_state",
        Kind::Gauge,
        "Link state of the device, 1 for the current state.",
    );
    for d in entities(metrics, "drivers") {
        let (Some(id), Some(link)) = (
            d.get("device").and_then(Value::as_str),
            d.get("link").and_then(Value::as_str),
        ) else {
            continue;
        };
        for state in LINK_STATES {
            x.sample(
                "joker_driver_link_state",
                &[("device", id), ("state", state)],
                Number::Int(u64::from(state == link)),
            );
        }
    }
    x.end();
    x.percentiles(
        metrics,
        "drivers",
        ("device", "device"),
        "latency",
        (
            "joker_driver_latency_seconds",
            "joker_driver_latency_max_seconds",
        ),
        "Request latency of the device.",
    );
    for f in &DRIVERS {
        x.plain(metrics, f);
    }
    x.begin(
        "joker_envelope_results_total",
        Kind::Counter,
        "Results of the envelope per output.",
    );
    for e in entities(metrics, "envelope") {
        let Some(output) = e.get("output").and_then(Value::as_str) else {
            continue;
        };
        for result in ENVELOPE_RESULTS {
            if let Some(n) = e.get(result).and_then(|v| Read::Number.value(v)) {
                x.sample(
                    "joker_envelope_results_total",
                    &[("output", output), ("result", result)],
                    n,
                );
            }
        }
    }
    x.end();
    for f in &REST {
        x.plain(metrics, f);
    }
    x.out
}

#[cfg(test)]
mod tests {
    use super::{celsius, size_bytes};

    #[test]
    fn sizes_and_temperatures_parse_in_base_units() {
        assert_eq!(size_bytes("12 MiB"), Some(12 * 1_048_576));
        assert_eq!(size_bytes("512kB"), Some(512_000));
        assert_eq!(size_bytes("1024"), Some(1024));
        assert_eq!(size_bytes("12 parsecs"), None);
        assert_eq!(celsius("61 °C"), Some(61.0));
        assert_eq!(celsius("61.5°C"), Some(61.5));
        assert_eq!(celsius("61 K"), None);
    }
}
