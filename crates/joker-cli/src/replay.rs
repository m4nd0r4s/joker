//! `jk replay`: the project's apps run again on a recorded trace, on this
//! machine, from the recorded state (docs/07 §5, docs/16 §7).

use std::path::Path;

use joker_manifest::OsFileSystem;
use jokerd::native::NativeTasks;
use serde_json::json;

use crate::Failure;
use crate::project::Entry;

/// Replays `file`; exit 6 when ticks diverge (docs/07 §8).
pub fn run(entry: &Entry, file: &Path, diff: bool, as_json: bool) -> Result<u8, Failure> {
    let bytes =
        std::fs::read(file).map_err(|e| Failure::usage(format!("{}: {e}", file.display())))?;
    let loaded =
        jokerd::load::load(&OsFileSystem, &entry.manifest, &NativeTasks).map_err(|e| Failure {
            message: e.to_string(),
            exit: 2,
        })?;
    let report = jokerd::replay::replay(&loaded.plan, &loaded.manifest, &NativeTasks, &bytes)
        .map_err(|e| Failure {
            message: format!("error: {e}"),
            exit: 1,
        })?;
    let path = |id: joker_core::SignalId| loaded.plan.path(id).unwrap_or("?").to_owned();
    if as_json {
        let divergences: Vec<_> = report
            .divergences
            .iter()
            .map(|d| {
                json!({
                    "group": d.group,
                    "tick": d.tick,
                    "recorded": d.recorded.iter().map(|r| json!({"output": path(r.signal), "outcome": r.outcome, "applied": r.applied})).collect::<Vec<_>>(),
                    "replayed": d.replayed.iter().map(|r| json!({"output": path(r.signal), "outcome": r.outcome, "applied": r.applied})).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ticks": report.ticks,
                "restored": report.restored,
                "from_init": report.from_init,
                "before_state": report.before_state,
                "divergences": divergences,
            }))
            .unwrap_or_default()
        );
    } else {
        println!(
            "replayed {} tick(s) of {} group(s) · state of {} · {} divergence(s)",
            report.ticks,
            report.groups,
            if report.restored.is_empty() {
                "no app".to_owned()
            } else {
                report.restored.join(", ")
            },
            report.divergences.len()
        );
        if !report.from_init.is_empty() {
            eprintln!(
                "warning: {} start(s) from init: the first ticks may differ until their state converges",
                report.from_init.join(", ")
            );
        }
        if report.before_state > 0 {
            println!(
                "{} tick(s) before the state converged (not counted)",
                report.before_state
            );
        }
        if diff {
            for d in report.divergences.iter().take(20) {
                println!("tick {} of {}:", d.tick, d.group);
                for (r, p) in d.recorded.iter().zip(d.replayed.iter()) {
                    if r != p {
                        println!(
                            "  {}  recorded outcome {} applied {:#x}  ·  replayed outcome {} applied {:#x}",
                            path(r.signal),
                            r.outcome,
                            r.applied,
                            p.outcome,
                            p.applied
                        );
                    }
                }
            }
        }
    }
    Ok(if report.identical() { 0 } else { 6 })
}
