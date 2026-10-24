//! Replay of a `.jktrace` (docs/16 §7, docs/04 §7): the apps run again on
//! the recorded inputs and async proposals, from the recorded state, and
//! each tick's results are compared byte for byte with the recording.

use std::collections::BTreeMap;

use joker_core::alarm::{AlarmRuntime, Alarms};
use joker_core::class::Classes;
use joker_core::commit::{CommitSink, OutputResult, OutputState};
use joker_core::image::{ImageStorage, ProcessImage, Proposal, Source};
use joker_core::slot::SeqExtender;
use joker_core::trace::{
    Header, Item, Items, PROPOSAL_FLAG_ASYNC, RESULT_LEN, States, Ticks, TraceResult,
};

use crate::compress::decompress;
use joker_core::{
    ClockQuality, GroupId, Instant, Outputs, OutputsStorage, Quality, Sample, SignalId, Slot,
};
use joker_manifest::Manifest;

use crate::load::instantiate;
use crate::plan::Plan;
use crate::shared::STAGING_CAPACITY;
use crate::task::{Prepare, TaskContext, TaskProvider};

/// One tick whose results differ from the recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// Group name.
    pub group: String,
    /// Tick index.
    pub tick: u64,
    /// Recorded results.
    pub recorded: Vec<TraceResult>,
    /// Replayed results.
    pub replayed: Vec<TraceResult>,
}

/// The outcome of a replay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Ticks replayed.
    pub ticks: u64,
    /// Groups in the file.
    pub groups: usize,
    /// Apps restored from the state segment.
    pub restored: Vec<String>,
    /// Apps that start from `init` (no state, or no `persist`).
    pub from_init: Vec<String>,
    /// Ticks that differ before every app's state converged: not counted
    /// (docs/16 §7).
    pub before_state: u64,
    /// Ticks that differ.
    pub divergences: Vec<Divergence>,
}

impl Report {
    /// True when every counted tick matches the recording.
    pub fn identical(&self) -> bool {
        self.divergences.is_empty()
    }
}

struct Tick {
    group: usize,
    index: u64,
    at: Instant,
    inputs: Vec<(u32, Quality, u64)>,
    proposals: Vec<Proposal>,
    results: Vec<u8>,
}

struct Collect<'a> {
    results: &'a mut Vec<TraceResult>,
}

impl CommitSink for Collect<'_> {
    fn output(&mut self, result: &OutputResult) {
        self.results.push(TraceResult::from(result));
    }
}

fn results_of(bytes: &[u8]) -> Vec<TraceResult> {
    bytes
        .as_chunks::<RESULT_LEN>()
        .0
        .iter()
        .map(|c| TraceResult {
            signal: SignalId(u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
            outcome: c[4],
            applied: u64::from_le_bytes([c[5], c[6], c[7], c[8], c[9], c[10], c[11], c[12]]),
        })
        .collect()
}

fn encode(results: &[TraceResult]) -> Vec<u8> {
    let mut out = Vec::with_capacity(results.len() * RESULT_LEN);
    for r in results {
        out.extend_from_slice(&r.signal.0.to_le_bytes());
        out.push(r.outcome);
        out.extend_from_slice(&r.applied.to_le_bytes());
    }
    out
}

/// Replays `file` with the plan and apps of the project it was recorded
/// from. The registry of the plan must be the recording's.
pub fn replay(
    plan: &Plan,
    manifest: &Manifest,
    tasks: &dyn TaskProvider,
    file: &[u8],
) -> Result<Report, String> {
    let (header, rest) = Header::decode(file).map_err(|e| format!("not a trace: {e:?}"))?;
    if header.registry_hash != plan.registry_hash {
        return Err("the trace was recorded with another registry: replay it from the generation that recorded it".into());
    }
    let mut groups = instantiate(plan, manifest, tasks).map_err(|e| e.to_string())?;
    let mut report = Report {
        groups: plan.groups.len(),
        ..Report::default()
    };
    let mut ticks: Vec<Tick> = Vec::new();
    let mut restored: BTreeMap<(u32, String), Vec<u8>> = BTreeMap::new();
    for item in Items::new(rest) {
        match item.map_err(|e| format!("malformed trace: {e:?}"))? {
            Item::State { n_apps, payload } => {
                let raw = if header.uncompressed() {
                    payload.to_vec()
                } else {
                    decompress(payload).map_err(|e| format!("{e:?}"))?
                };
                for app in States::new(&raw, n_apps) {
                    let app = app.map_err(|e| format!("malformed state: {e:?}"))?;
                    if app.state == 0 {
                        restored.insert((app.group, app.name.to_owned()), app.blob.to_vec());
                    }
                }
            }
            Item::Ticks {
                group,
                n_ticks,
                payload,
            } => {
                let g = group as usize;
                let ports = plan.groups.get(g).map_or(0, |p| p.ports.len());
                let raw = if header.uncompressed() {
                    payload.to_vec()
                } else {
                    decompress(payload).map_err(|e| format!("{e:?}"))?
                };
                for t in Ticks::new(&raw, n_ticks, ports) {
                    let t = t.map_err(|e| format!("malformed tick: {e:?}"))?;
                    ticks.push(Tick {
                        group: g,
                        index: t.index,
                        at: Instant {
                            mono_ns: t.t_mono_ns,
                            utc_ns: t.t_utc_ns,
                            clock: ClockQuality::from_u8(t.clock).unwrap_or(ClockQuality::Unsynced),
                        },
                        inputs: t.inputs().map(|i| (i.seq, i.quality, i.value)).collect(),
                        proposals: t
                            .proposals()
                            .filter(|p| p.flags & PROPOSAL_FLAG_ASYNC != 0)
                            .map(|p| Proposal {
                                source: Source::Async,
                                ..p.to_proposal()
                            })
                            .collect(),
                        results: t.result_bytes().to_vec(),
                    });
                }
            }
        }
    }
    ticks.sort_by_key(|t| (t.at.mono_ns, t.group));

    for (g, group) in plan.groups.iter().enumerate() {
        for (i, (_, task)) in groups[g].iter_mut().enumerate() {
            let Some(app) = group.tasks.get(i).map(|t| t.app.clone()) else {
                continue;
            };
            match restored.get(&(g as u32, app.clone())) {
                Some(blob) => {
                    task.import_state(blob).map_err(|e| format!("{app}: {e}"))?;
                    if !report.restored.contains(&app) {
                        report.restored.push(app);
                    }
                }
                None => {
                    if !report.from_init.contains(&app) {
                        report.from_init.push(app);
                    }
                }
            }
        }
    }

    let tables = plan.tables().map_err(|e| e.to_string())?;
    let registry = tables.registry.as_registry();
    let slots: Vec<Slot> = (0..registry.len()).map(|_| Slot::new()).collect();
    let mut states = vec![OutputState::EMPTY; tables.outputs.len()];
    let mut intakes = vec![None; STAGING_CAPACITY];
    let mut output_arena = vec![0u64; tables.outputs.iter().map(|c| c.state_words()).sum()];
    let derived = crate::expr_plan::derived_defs(plan, &registry)?;
    let mut derived_arena = vec![0u64; derived.iter().map(|d| d.expr.state_words()).sum()];
    let mut method_scratch = Vec::new();
    let mut classes = Classes::new(&[], &derived, &mut method_scratch);
    classes
        .set_state_arena(&mut derived_arena)
        .map_err(|e| format!("derived signals: {e:?}"))?;
    let alarm_configs = crate::expr_plan::alarm_configs(plan, &registry)?;
    let mut alarm_states = vec![AlarmRuntime::EMPTY; alarm_configs.len()];
    let mut alarms =
        Alarms::new(&alarm_configs, &mut alarm_states).map_err(|e| format!("alarms: {e:?}"))?;
    let mut alarm_arena = vec![0u64; alarms.state_words()];
    alarms
        .set_state_arena(&mut alarm_arena)
        .map_err(|e| format!("alarms: {e:?}"))?;
    let mut outputs = Outputs::new(
        &registry,
        &tables.writers,
        OutputsStorage {
            configs: &tables.outputs,
            states: &mut states,
            intakes: &mut intakes,
        },
    )
    .map_err(|e| format!("outputs: {e:?}"))?;
    outputs
        .set_state_arena(&mut output_arena)
        .map_err(|e| format!("outputs: {e:?}"))?;
    let ports: Vec<Vec<SignalId>> = plan
        .groups
        .iter()
        .map(|g| g.ports.iter().filter_map(|p| plan.id(p)).collect())
        .collect();
    let mut snapshots: Vec<Vec<Sample>> = ports
        .iter()
        .map(|p| vec![Sample::INITIAL; p.len()])
        .collect();
    let mut extenders: Vec<Vec<SeqExtender>> = ports
        .iter()
        .map(|p| vec![SeqExtender::for_slot(); p.len()])
        .collect();
    let mut staging: Vec<Vec<Proposal>> = plan
        .groups
        .iter()
        .map(|_| vec![Proposal::EMPTY; STAGING_CAPACITY])
        .collect();
    let mut images = Vec::new();
    for (i, (((p, s), e), q)) in ports
        .iter()
        .zip(snapshots.iter_mut())
        .zip(extenders.iter_mut())
        .zip(staging.iter_mut())
        .enumerate()
    {
        let period = plan.groups[i].period_ns;
        images.push(
            ProcessImage::new(
                GroupId(i as u32),
                period,
                ImageStorage {
                    ports: p,
                    snapshot: s,
                    extenders: e,
                    proposals: q,
                },
            )
            .map_err(|e| format!("{e:?}"))?,
        );
    }

    let generation_hash = crate::trace_ring::digest_bytes(&plan.manifest_hash);
    for ((group, image), tasks) in plan.groups.iter().zip(&images).zip(groups.iter_mut()) {
        for (writer, task) in tasks.iter_mut() {
            task.prepare(&Prepare {
                registry: &registry,
                image,
                group: &group.name,
                writer: *writer,
                generation_hash,
            })
            .map_err(|e| format!("group `{}`: {e}", group.name))?;
        }
    }

    let mut last_input: Vec<Vec<Option<(u32, Quality, u64)>>> =
        ports.iter().map(|p| vec![None; p.len()]).collect();
    let mut converged = report.from_init.is_empty();
    let mut results = Vec::with_capacity(plan.outputs.len());
    for t in &ticks {
        let g = t.group;
        for (k, input) in t.inputs.iter().enumerate() {
            if last_input[g][k] != Some(*input)
                && let Some(slot) = ports[g].get(k).and_then(|id| slots.get(id.index()))
            {
                slot.write(input.2, input.1, joker_core::value::NO_TS_SRC, t.at.mono_ns);
                last_input[g][k] = Some(*input);
            }
        }
        let image = &mut images[g];
        image
            .latch(t.index, t.at, &slots, &registry)
            .map_err(|e| format!("tick {}: {e:?}", t.index))?;
        for p in &t.proposals {
            let _ = image.stage_async(*p);
        }
        classes
            .evaluate_derived(image, &slots)
            .map_err(|e| format!("tick {}: {e:?}", t.index))?;
        results.clear();
        alarms
            .evaluate(
                image,
                &slots,
                &mut outputs,
                &mut Collect {
                    results: &mut results,
                },
            )
            .map_err(|e| format!("tick {}: {e:?}", t.index))?;
        for (writer, task) in &mut groups[g] {
            let mut cx = TaskContext::new(image, &outputs, *writer);
            let _ = task.run(&mut cx);
        }
        results.clear();
        outputs
            .commit(
                image,
                &slots,
                &mut Collect {
                    results: &mut results,
                },
            )
            .map_err(|e| format!("tick {}: {e:?}", t.index))?;
        let _ = image.end_tick();
        report.ticks += 1;
        if encode(&results) == t.results {
            converged = true;
            continue;
        }
        if !converged {
            report.before_state += 1;
            continue;
        }
        report.divergences.push(Divergence {
            group: plan.groups[g].name.clone(),
            tick: t.index,
            recorded: results_of(&t.results),
            replayed: results.clone(),
        });
    }
    Ok(report)
}
