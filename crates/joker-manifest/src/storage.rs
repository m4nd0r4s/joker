//! The storage write-load projection of `jk check` (`J0381`, ADR 0105):
//! bytes per day and fsyncs per hour by source, against what the board's
//! `storage` class withstands.

use std::collections::BTreeMap;

use joker_units::Duration;

use crate::check::Checker;
use crate::context::SignalType;
use crate::diag::Diagnostic;
use crate::grammar;
use crate::value::Node;

const DAY_S: f64 = 86_400.0;
const HOUR_S: f64 = 3_600.0;
const GIB: f64 = 1_073_741_824.0;
const MIB: f64 = 1_048_576.0;

/// The bytes a recorded sample costs: its WAL record and its row.
pub const SAMPLE_BYTES: f64 = 64.0;
/// The snapshot of an open block at a checkpoint: half a block.
pub const SNAPSHOT_BYTES: f64 = 2_048.0;
/// Checkpoints per day, one every 5 min (ADR 0081, rule 9).
pub const CHECKPOINTS_PER_DAY: f64 = 288.0;
/// The size of a decision-log entry.
pub const DECISION_BYTES: f64 = 512.0;
/// The assumed rate of decision-log entries of one output, per second.
pub const DECISIONS_PER_S: f64 = 0.1;
/// The fastest rate of a recorded signal, per second (`on_change` reads
/// every 10 ms).
pub const MAX_SAMPLE_RATE: f64 = 100.0;

/// What a storage class withstands (ADR 0105, point 4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Bytes per day.
    pub bytes_per_day: f64,
    /// Fsyncs per hour.
    pub fsyncs_per_hour: f64,
}

/// The limits of a `storage` class; `None` for an unknown class.
pub fn limits(storage: &str) -> Option<Limits> {
    let (gib, per_s) = match storage {
        "sd" => (2.0, 3.0),
        "emmc" => (20.0, 10.0),
        "ssd" => (200.0, 100.0),
        _ => return None,
    };
    Some(Limits {
        bytes_per_day: gib * GIB,
        fsyncs_per_hour: per_s * HOUR_S,
    })
}

/// The projected write load, by source, in a fixed source order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Projection {
    /// Bytes per day of each source.
    pub bytes_per_day: Vec<(&'static str, f64)>,
    /// Fsyncs per hour of each source.
    pub fsyncs_per_hour: Vec<(&'static str, f64)>,
}

impl Projection {
    /// The total bytes per day.
    pub fn bytes(&self) -> f64 {
        self.bytes_per_day.iter().map(|(_, b)| b).sum()
    }

    /// The total fsyncs per hour.
    pub fn fsyncs(&self) -> f64 {
        self.fsyncs_per_hour.iter().map(|(_, f)| f).sum()
    }
}

fn secs(d: Duration) -> f64 {
    d.as_nanos() as f64 / 1e9
}

fn duration(node: &Node) -> Option<Duration> {
    node.as_str().and_then(|s| Duration::parse(s).ok())
}

fn bytes_text(b: f64) -> String {
    if b >= GIB {
        format!("{:.1} GiB", b / GIB)
    } else {
        format!("{:.1} MiB", b / MIB)
    }
}

impl Checker<'_> {
    /// The recorded scalar signals with the rule that applies to each: an
    /// exact path beats a selector, then more literal segments, then the
    /// longer literal prefix (ref/01 §5.17.1).
    fn recorded(&self) -> BTreeMap<String, &crate::value::Table> {
        let mut chosen: BTreeMap<String, ((u32, usize), &crate::value::Table)> = BTreeMap::new();
        let Some(record) = self
            .manifest
            .pending
            .get("telemetry")
            .and_then(Node::as_table)
            .and_then(|t| t.get("record"))
            .and_then(|e| e.node.as_table())
        else {
            return BTreeMap::new();
        };
        for (selector, entry) in record {
            let Some(rule) = entry.node.as_table() else {
                continue;
            };
            let wild = grammar::is_wildcard(selector);
            let rank = if wild {
                (
                    selector.split('.').filter(|s| !s.contains('*')).count() as u32,
                    selector.find('*').unwrap_or(selector.len()),
                )
            } else {
                (u32::MAX, usize::MAX)
            };
            for (path, r) in &self.paths {
                let recordable = !matches!(r.info.ty, SignalType::Text | SignalType::Bytes);
                let hit = if wild {
                    grammar::matches(selector, path)
                } else {
                    path == selector
                };
                if *path != r.canonical || !recordable || !hit {
                    continue;
                }
                if chosen.get(path).is_none_or(|(have, _)| *have < rank) {
                    chosen.insert(path.clone(), (rank, rule));
                }
            }
        }
        chosen.into_iter().map(|(p, (_, r))| (p, r)).collect()
    }

    /// The projected write load of the manifest (ADR 0105, points 2 and 3).
    pub(crate) fn storage_projection(&self) -> Projection {
        let manifest = self.manifest;
        let recorded = self.recorded();
        let mut rate = 0.0;
        let mut tiers = 0.0;
        for (path, rule) in &recorded {
            let r = if let Some(every) = rule.get("every").and_then(|e| duration(&e.node)) {
                1.0 / secs(every).max(1e-9)
            } else {
                let poll = self
                    .paths
                    .get(path)
                    .and_then(|r| r.device.as_ref())
                    .and_then(|d| manifest.devices.get(d))
                    .map(|d| d.poll.as_ref().map_or(1.0, |p| secs(p.value)));
                poll.map_or(1.0, |p| 1.0 / p.max(1e-9))
            };
            rate += r.min(MAX_SAMPLE_RATE);
            tiers += rule
                .get("keep")
                .and_then(|e| e.node.as_table())
                .map_or(1, |k| k.len().max(1)) as f64;
        }
        let outputs = manifest.outputs.len() as f64;
        let signals = self
            .paths
            .iter()
            .filter(|(p, r)| **p == r.canonical)
            .count() as f64;
        let trace: f64 = manifest
            .cycles
            .values()
            .filter(|c| c.trace_continuous.is_some())
            .filter_map(|c| c.period.as_ref())
            .map(|p| (64.0 + 16.0 * signals) / secs(p.value).max(1e-9))
            .sum();
        let store = manifest
            .pending
            .get("telemetry")
            .and_then(Node::as_table)
            .and_then(|t| t.get("store"))
            .and_then(|e| e.node.as_table());
        let fsync = store
            .and_then(|s| s.get("fsync"))
            .and_then(|e| e.node.as_str())
            .unwrap_or("batch");
        let interval = store
            .and_then(|s| s.get("fsync_interval"))
            .and_then(|e| duration(&e.node))
            .map_or(1.0, secs);
        let alarms = manifest
            .pending
            .get("alarms")
            .and_then(Node::as_table)
            .is_some_and(|a| !a.is_empty());
        let wal = if recorded.is_empty() && !alarms {
            0.0
        } else {
            match fsync {
                "always" => rate.min(MAX_SAMPLE_RATE) * HOUR_S,
                "never" => 0.0,
                _ => HOUR_S / interval.max(1e-3),
            }
        };
        let any = !recorded.is_empty();
        Projection {
            bytes_per_day: vec![
                ("historian samples", rate * SAMPLE_BYTES * DAY_S),
                (
                    "historian checkpoints",
                    tiers * SNAPSHOT_BYTES * CHECKPOINTS_PER_DAY,
                ),
                (
                    "decision log",
                    outputs * DECISIONS_PER_S * DECISION_BYTES * DAY_S,
                ),
                ("continuous trace", trace * DAY_S),
            ],
            fsyncs_per_hour: vec![
                ("historian WAL", wal),
                ("historian checkpoints", if any { 24.0 } else { 0.0 }),
                ("decision log", if outputs > 0.0 { HOUR_S } else { 0.0 }),
            ],
        }
    }

    /// `J0381`: the projection above what the board's `storage` withstands.
    pub(crate) fn storage_load(&mut self) {
        let Some(board) = self.manifest.target.as_ref().and_then(|t| t.board.as_ref()) else {
            return;
        };
        let Some(info) = self.context.boards.get(&board.value.name) else {
            return;
        };
        if info.micro {
            return;
        }
        let class = info.storage.as_deref().unwrap_or("sd");
        let Some(limit) = limits(class) else {
            return;
        };
        let p = self.storage_projection();
        let (bytes, fsyncs) = (p.bytes(), p.fsyncs());
        if bytes <= limit.bytes_per_day && fsyncs <= limit.fsyncs_per_hour {
            return;
        }
        let mut sources: Vec<String> = Vec::new();
        let mut by_bytes = p.bytes_per_day.clone();
        by_bytes.sort_by(|a, b| b.1.total_cmp(&a.1));
        sources.extend(
            by_bytes
                .iter()
                .filter(|(_, b)| *b > 0.0)
                .map(|(s, b)| format!("{s} {}/day", bytes_text(*b))),
        );
        let mut by_sync = p.fsyncs_per_hour.clone();
        by_sync.sort_by(|a, b| b.1.total_cmp(&a.1));
        sources.extend(
            by_sync
                .iter()
                .filter(|(_, f)| *f > 0.0)
                .map(|(s, f)| format!("{s} {f:.0} fsync/h")),
        );
        let span = board.span;
        self.push(
            Diagnostic::new(
                "J0381",
                format!("projected write load above what `storage = \"{class}\"` withstands"),
            )
            .at(span)
            .label(format!(
                "{}/day (limit {}), {fsyncs:.0} fsync/h (limit {:.0})",
                bytes_text(bytes),
                bytes_text(limit.bytes_per_day),
                limit.fsyncs_per_hour
            ))
            .help(format!(
                "from {}; lower `every`, `on_change` or `keep` in `[telemetry.record]`, `fsync` or `fsync_interval` in `[telemetry.store]`, or `trace.continuous` (ADR 0105)",
                sources.join(", ")
            )),
        );
    }
}
