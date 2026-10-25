//! `cargo xtask rt-bench`: the first numbers of the tick path (docs/13 §4
//! task 20, docs/01 §21).
//!
//! Two measurements on one fixed workload, `N` outputs with range and rate
//! and two writers that propose on every output in every tick:
//!
//! - **cost**: ticks back to back; the time of the commit (arbiter,
//!   envelope and commit, steps 5–7) divided by `N` is the cost per output,
//!   the figure of "envelope + arbiter per output ≤ 300 ns";
//! - **jitter**: ticks paced by the host clock at `period`; the jitter is
//!   how late each tick starts against its deadline.
//!
//! The workload is deterministic: the same options give the same sequence
//! of proposals and results on every host. The scheduling is that of the
//! launching shell: on a PREEMPT_RT node, run it under `chrt -f 80 taskset
//! -c <cpu>` on an isolated CPU (docs/01 §21). Every tick runs inside the
//! no-alloc guard, and an allocation fails the run.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant as HostInstant};

use joker_core::commit::OutputState;
use joker_core::image::ImageStorage;
use joker_core::output::{ClockRequirement, Level, OutputConfig, WriterKind};
use joker_core::registry::{Alias, GroupMeta, OwnedRegistry, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::{ClockQuality, Instant};
use joker_core::{
    OutputsStorage, ProcessImage, Proposal, SafeValue, Sample, SignalId, Slot, ValueType,
    WriterConfig, WriterId,
};
use serde_json::{Value, json};

use crate::conformance::exec::TickGuard;
use crate::conformance::literal;
use crate::guard::NoAlloc;

/// Options of a benchmark run.
#[derive(Clone, Debug)]
pub struct Options {
    /// Outputs in the group.
    pub outputs: usize,
    /// Period of the paced ticks, in nanoseconds.
    pub period_ns: u64,
    /// Paced ticks for the jitter measurement.
    pub ticks: u64,
    /// Back-to-back ticks for the cost measurement.
    pub iterations: u64,
    /// Where to write the JSON result.
    pub json: Option<PathBuf>,
    /// A previous JSON result to compare with.
    pub compare: Option<PathBuf>,
    /// Allowed regression of p99.9 against the baseline, in percent.
    pub threshold_pct: f64,
    /// The largest median cost per output, in ns: the gate of docs/11
    /// Phase 0 gate 4 (docs/01 §21: envelope + arbiter ≤ 300 ns).
    pub max_output_ns: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            outputs: 64,
            period_ns: 1_000_000,
            ticks: 10_000,
            iterations: 20_000,
            json: None,
            compare: None,
            threshold_pct: 5.0,
            max_output_ns: None,
        }
    }
}

const WRITERS: usize = 2;

/// The fixed workload: a registry, its slots and the configuration of the
/// outputs and writers.
#[derive(Debug)]
pub struct Workload {
    paths: Vec<String>,
    period_ns: u64,
}

impl Workload {
    /// A workload of `outputs` outputs in a group of period `period_ns`.
    pub fn new(outputs: usize, period_ns: u64) -> Self {
        Self {
            paths: (0..outputs).map(|i| format!("bench.p{i:04}")).collect(),
            period_ns,
        }
    }

    /// Runs `ticks` ticks through `driver`, which receives each tick's
    /// closure and must call it once.
    pub fn run(
        &self,
        ticks: u64,
        guard: &impl TickGuard,
        mut driver: impl FnMut(u64, &mut dyn FnMut() -> Result<u64, String>),
    ) -> Result<(), String> {
        let signals: Vec<SignalMeta<'_>> = self
            .paths
            .iter()
            .map(|p| SignalMeta::output(p, ValueType::F64, UnitRef("kW"), "bench"))
            .collect();
        let groups = vec![GroupMeta {
            name: "bench",
            period_ns: self.period_ns,
        }];
        let registry = OwnedRegistry::build(signals, Vec::<Alias<'_>>::new(), groups)
            .map_err(|e| format!("registry: {e:?}"))?;
        let registry = registry.as_registry();
        let group = registry
            .resolve_group("bench")
            .ok_or("the bench group is missing")?;
        let mut ids: Vec<SignalId> = self
            .paths
            .iter()
            .filter_map(|p| registry.resolve(p))
            .collect();
        ids.sort_unstable();
        let configs: Vec<OutputConfig<'static>> = ids
            .iter()
            .map(|&id| {
                let mut c =
                    OutputConfig::new(id, ValueType::F64, group, SafeValue::Value(0f64.to_bits()));
                c.range = Some((-100.0, 100.0));
                c.rate = Some(1_000.0);
                c
            })
            .collect();
        let level = |text: &str| Level::parse(text).ok_or("invalid level");
        let writers = [
            WriterConfig {
                kind: WriterKind::Cycle,
                level: level("control:10")?,
                lease_ns: None,
                requires_clock: ClockRequirement::Any,
            },
            WriterConfig {
                kind: WriterKind::Cycle,
                level: level("optimizer:13")?,
                lease_ns: None,
                requires_clock: ClockRequirement::Any,
            },
        ];
        let staging = ids.len() * WRITERS;
        let mut states = vec![OutputState::EMPTY; configs.len()];
        let mut intakes = vec![None; staging];
        let mut outputs = joker_core::Outputs::new(
            &registry,
            &writers,
            OutputsStorage {
                configs: &configs,
                states: &mut states,
                intakes: &mut intakes,
            },
        )
        .map_err(|e| format!("outputs: {e:?}"))?;
        let slots: Vec<Slot> = (0..registry.len()).map(|_| Slot::new()).collect();
        let mut proposals = vec![Proposal::EMPTY; staging];
        let mut snapshot: Vec<Sample> = Vec::new();
        let mut extenders: Vec<SeqExtender> = Vec::new();
        let mut image = ProcessImage::new(
            group,
            self.period_ns,
            ImageStorage {
                ports: &[],
                snapshot: &mut snapshot,
                extenders: &mut extenders,
                proposals: &mut proposals,
            },
        )
        .map_err(|e| format!("image: {e:?}"))?;
        for k in 0..ticks {
            let mut tick = || -> Result<u64, String> {
                let mut commit_ns = 0;
                let mut failed = None;
                guard.run(|| {
                    let at = Instant {
                        mono_ns: k.saturating_mul(self.period_ns),
                        utc_ns: 0,
                        clock: ClockQuality::Synced,
                    };
                    if image.latch(k, at, &slots, &registry).is_err() {
                        failed = Some("latch");
                        return;
                    }
                    for (w, writer) in [WriterId(0), WriterId(1)].into_iter().enumerate() {
                        for (i, &signal) in ids.iter().enumerate() {
                            let value = proposal_value(k, i, w);
                            let p = Proposal {
                                writer,
                                signal,
                                value: value.to_bits(),
                                ..Proposal::EMPTY
                            };
                            if image.stage_cycle(p).is_err() {
                                failed = Some("staging");
                                return;
                            }
                        }
                    }
                    let start = HostInstant::now();
                    if outputs.commit(&image, &slots, &mut ()).is_err() {
                        failed = Some("commit");
                        return;
                    }
                    commit_ns = elapsed_ns(start);
                    if image.end_tick().is_err() {
                        failed = Some("end of tick");
                    }
                });
                match failed {
                    Some(step) => Err(format!("tick {k}: {step} failed")),
                    None => Ok(commit_ns),
                }
            };
            driver(k, &mut tick);
        }
        Ok(())
    }
}

/// A deterministic proposal: a square wave of ±80 kW whose phase depends on
/// the output, so that the rate limit, the range clamp and the arbiter all
/// work on every tick.
fn proposal_value(tick: u64, output: usize, writer: usize) -> f64 {
    let phase = (tick / 50 + output as u64) % 2;
    let base = if phase == 0 { 80.0 } else { -80.0 };
    if writer == 0 { base } else { base * 1.5 }
}

fn elapsed_ns(start: HostInstant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// The value at quantile `q` of sorted samples (nearest rank).
pub fn quantile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Buckets of a log2 histogram in microseconds: `[0,1)`, `[1,2)`, `[2,4)`, …
pub fn histogram(samples_ns: &[u64]) -> Vec<(u64, u64)> {
    let mut buckets: Vec<(u64, u64)> = Vec::new();
    for &ns in samples_ns {
        let us = ns / 1_000;
        let low = if us == 0 { 0 } else { 1 << us.ilog2() };
        match buckets.iter_mut().find(|(l, _)| *l == low) {
            Some((_, n)) => *n += 1,
            None => buckets.push((low, 1)),
        }
    }
    buckets.sort_unstable();
    buckets
}

fn stats(mut samples: Vec<u64>, scale: f64) -> Value {
    samples.sort_unstable();
    let n = samples.len().max(1) as f64;
    let mean = samples.iter().map(|&x| x as f64).sum::<f64>() / n;
    let q = |p| quantile(&samples, p) as f64 / scale;
    json!({
        "samples": samples.len(),
        "mean": mean / scale,
        "p50": q(0.5),
        "p99": q(0.99),
        "p99_9": q(0.999),
        "max": samples.last().copied().unwrap_or(0) as f64 / scale,
    })
}

/// Runs the benchmark and returns the JSON result.
pub fn bench(options: &Options) -> Result<Value, String> {
    if options.outputs == 0 || options.period_ns == 0 {
        return Err("--outputs and --period must be positive".to_string());
    }
    let workload = Workload::new(options.outputs, options.period_ns);
    let guard = NoAlloc;
    let before = guard.violations();
    let mut cost = Vec::with_capacity(usize::try_from(options.iterations).unwrap_or(0));
    let mut error = None;
    workload.run(options.iterations, &guard, |_, tick| match tick() {
        Ok(ns) => cost.push(ns),
        Err(e) => error = Some(e),
    })?;
    let mut lateness = Vec::with_capacity(usize::try_from(options.ticks).unwrap_or(0));
    let mut overruns = 0u64;
    let period = Duration::from_nanos(options.period_ns);
    let start = HostInstant::now() + period;
    workload.run(options.ticks, &guard, |k, tick| {
        let deadline = start + period * u32::try_from(k).unwrap_or(u32::MAX);
        let now = HostInstant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }
        let late = HostInstant::now().saturating_duration_since(deadline);
        lateness.push(u64::try_from(late.as_nanos()).unwrap_or(u64::MAX));
        if let Err(e) = tick() {
            error = Some(e);
        }
        if HostInstant::now() > deadline + period {
            overruns += 1;
        }
    })?;
    if let Some(e) = error {
        return Err(e);
    }
    let allocations = guard.violations().saturating_sub(before);
    if allocations > 0 {
        return Err(format!(
            "the tick path made {allocations} allocator calls (docs/10 R7)"
        ));
    }
    let per_output_ps: Vec<u64> = cost
        .iter()
        .map(|&ns| ns.saturating_mul(1_000) / options.outputs as u64)
        .collect();
    let histogram: Vec<Value> = histogram(&lateness)
        .into_iter()
        .map(|(low, n)| json!({"from_us": low, "count": n}))
        .collect();
    let commit = stats(cost, 1.0);
    let ns_per_output = stats(per_output_ps, 1_000.0);
    Ok(json!({
        "schema": 1,
        "tool": "cargo xtask rt-bench",
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "host": {
            "arch": std::env::consts::ARCH,
            "os": std::env::consts::OS,
            "cpus": std::thread::available_parallelism().map_or(0, usize::from),
        },
        "workload": {
            "outputs": options.outputs,
            "writers": WRITERS,
            "period": literal::format_ns(options.period_ns),
            "paced_ticks": options.ticks,
            "cost_ticks": options.iterations,
            "envelope": "range, rate; no atomic groups",
        },
        "cost": {
            "commit_ns": commit,
            "ns_per_output": ns_per_output,
            "target_ns_per_output": 300,
        },
        "jitter": {
            "lateness_us": stats(lateness, 1_000.0),
            "overruns": overruns,
            "histogram": histogram,
            "targets_us": {"rt_1ms_group": 100, "non_rt_100ms_group": 2_000},
        },
        "allocations": allocations,
    }))
}

/// Compares a result with a baseline: a p99.9 more than `threshold_pct`
/// above the baseline is a regression (AGENTS.md §5).
pub fn compare(
    current: &Value,
    baseline: &Value,
    threshold_pct: f64,
) -> Result<Vec<String>, String> {
    let mut regressions = Vec::new();
    if current["profile"] != baseline["profile"] || current["workload"] != baseline["workload"] {
        return Err(
            "the baseline has another profile or workload; it does not compare".to_string(),
        );
    }
    for (name, pointer) in [
        ("jitter p99.9 (us)", "/jitter/lateness_us/p99_9"),
        ("ns/output p99.9", "/cost/ns_per_output/p99_9"),
    ] {
        let (Some(now), Some(base)) = (
            current.pointer(pointer).and_then(Value::as_f64),
            baseline.pointer(pointer).and_then(Value::as_f64),
        ) else {
            return Err(format!("{name}: missing in the result or the baseline"));
        };
        if now > base * (1.0 + threshold_pct / 100.0) {
            regressions.push(format!(
                "{name}: {now:.3} against {base:.3} (+{:.1}%, allowed {threshold_pct}%)",
                (now / base - 1.0) * 100.0
            ));
        }
    }
    Ok(regressions)
}

/// Checks the median cost per output against `max_ns`: `Err` with the
/// figures when it is above (docs/11 Phase 0 gate 4).
pub fn check_output_cost(result: &Value, max_ns: f64) -> Result<(), String> {
    let p50 = result
        .pointer("/cost/ns_per_output/p50")
        .and_then(Value::as_f64)
        .ok_or("the result has no cost per output")?;
    if p50 <= max_ns {
        Ok(())
    } else {
        Err(format!(
            "envelope + arbiter per output: p50 {p50:.1} ns, above {max_ns} ns"
        ))
    }
}

/// A one-screen summary of a result.
pub fn summary(result: &Value) -> String {
    let f = |p: &str| {
        result
            .pointer(p)
            .and_then(Value::as_f64)
            .unwrap_or(f64::NAN)
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "rt-bench ({} build, {} {}, {} outputs, period {})",
        result["profile"].as_str().unwrap_or("?"),
        result["host"]["arch"].as_str().unwrap_or("?"),
        result["host"]["os"].as_str().unwrap_or("?"),
        result["workload"]["outputs"],
        result["workload"]["period"].as_str().unwrap_or("?"),
    );
    let _ = writeln!(
        out,
        "  envelope + arbiter per output: p50 {:.1} ns, p99 {:.1} ns, p99.9 {:.1} ns (target ≤ 300 ns)",
        f("/cost/ns_per_output/p50"),
        f("/cost/ns_per_output/p99"),
        f("/cost/ns_per_output/p99_9"),
    );
    let _ = writeln!(
        out,
        "  tick jitter: p50 {:.1} us, p99 {:.1} us, p99.9 {:.1} us, max {:.1} us, overruns {}",
        f("/jitter/lateness_us/p50"),
        f("/jitter/lateness_us/p99"),
        f("/jitter/lateness_us/p99_9"),
        f("/jitter/lateness_us/max"),
        result["jitter"]["overruns"],
    );
    if result["profile"] == "debug" {
        let _ = writeln!(
            out,
            "  warning: debug build; use `cargo xtask rt-bench` (release) for numbers"
        );
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn quantiles_use_the_nearest_rank() {
        let v: Vec<u64> = (1..=1000).collect();
        assert_eq!(quantile(&v, 0.5), 500);
        assert_eq!(quantile(&v, 0.999), 999);
        assert_eq!(quantile(&v, 1.0), 1000);
        assert_eq!(quantile(&[], 0.5), 0);
    }

    #[test]
    fn the_histogram_has_log2_buckets() {
        let h = histogram(&[0, 999, 1_000, 1_999, 2_000, 5_000, 70_000]);
        assert_eq!(h, [(0, 2), (1, 2), (2, 1), (4, 1), (64, 1)]);
    }

    #[test]
    fn the_workload_runs_without_allocating() {
        let guard = NoAlloc;
        let before = guard.violations();
        let mut ticks = 0;
        Workload::new(16, 1_000_000)
            .run(500, &guard, |_, tick| {
                tick().unwrap();
                ticks += 1;
            })
            .unwrap();
        assert_eq!(ticks, 500);
        assert_eq!(guard.violations(), before);
    }

    #[test]
    fn a_short_run_reports_both_measurements() {
        let options = Options {
            outputs: 8,
            period_ns: 200_000,
            ticks: 50,
            iterations: 200,
            ..Options::default()
        };
        let r = bench(&options).unwrap();
        assert_eq!(r["allocations"], 0);
        assert_eq!(r["cost"]["commit_ns"]["samples"], 200);
        assert_eq!(r["jitter"]["lateness_us"]["samples"], 50);
        assert!(
            r.pointer("/cost/ns_per_output/p99_9")
                .unwrap()
                .as_f64()
                .unwrap()
                > 0.0
        );
        assert!(compare(&r, &r, 5.0).unwrap().is_empty());
        let mut worse = r.clone();
        worse["cost"]["ns_per_output"]["p99_9"] = json!(1e9);
        assert_eq!(compare(&worse, &r, 5.0).unwrap().len(), 1);
    }

    #[test]
    fn the_output_cost_gate_reads_the_median() {
        let r = json!({"cost": {"ns_per_output": {"p50": 120.0, "p99_9": 900.0}}});
        assert!(check_output_cost(&r, 300.0).is_ok());
        assert!(check_output_cost(&r, 100.0).is_err());
        assert!(check_output_cost(&json!({}), 300.0).is_err());
    }
}
