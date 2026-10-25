//! From an entry manifest to a runnable plan: check, plan and instantiate
//! the cycle tasks (ref/01 §8.1, docs/01 §15).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use joker_manifest::{FileSystem, Manifest, Report};

use crate::catalog;
use crate::cycle::GroupTasks;
use crate::plan::{Plan, PlanError};
use crate::task::{TaskProvider, TaskSpec};

/// Why a manifest could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The checks found errors; the rendered diagnostics, as `jk check`
    /// prints them, and their codes.
    Check {
        /// Rendered diagnostics.
        rendered: String,
        /// Their codes, in order.
        codes: Vec<String>,
    },
    /// The checked manifest uses something the M0 runtime cannot plan.
    Plan(PlanError),
    /// A cycle task could not be instantiated.
    Task(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Check { rendered, .. } => f.write_str(rendered),
            Self::Plan(e) => write!(f, "{e}"),
            Self::Task(e) => write!(f, "error: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// A checked and planned manifest.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The check report (sources, diagnostics, effective manifest, hash).
    pub report: Report,
    /// The decoded effective manifest.
    pub manifest: Manifest,
    /// The plan.
    pub plan: Plan,
}

/// Checks and plans the manifest whose entry layer is `entry`, with the
/// profiles and boards of the project around it (`profiles/`, `boards/`
/// next to `entry`, read from disk): the project side (`jk check`,
/// `jk replay`, tests).
pub fn load(
    fs: &dyn FileSystem,
    entry: &Path,
    tasks: &dyn TaskProvider,
) -> Result<Loaded, LoadError> {
    let root = entry.parent().unwrap_or(Path::new("."));
    let (files, read_errors) = catalog::catalog_files(root);
    for e in &read_errors {
        eprintln!("warning: {e}");
    }
    load_with(fs, entry, tasks, &files)
}

/// Checks and plans the manifest whose entry layer is `entry`, with
/// exactly the profiles and boards of `files` and nothing from the disk
/// around it: a generation on a node, whose files are its own.
pub fn load_with(
    fs: &dyn FileSystem,
    entry: &Path,
    tasks: &dyn TaskProvider,
    files: &catalog::CatalogFiles,
) -> Result<Loaded, LoadError> {
    let (context, catalog_errors) = catalog::context_for_files(tasks, files);
    for e in &catalog_errors {
        eprintln!("warning: {e}");
    }
    let report = joker_manifest::check(fs, entry, &context);
    let (Some(manifest), Some(hash), false) = (
        report.manifest.clone(),
        report.hash.clone(),
        report.has_errors(),
    ) else {
        return Err(LoadError::Check {
            rendered: report.render(),
            codes: report.codes().iter().map(|c| (*c).to_owned()).collect(),
        });
    };
    let contracts = catalog::contracts_for_files(files);
    let plan = Plan::new(&manifest, &report.paths, &context, &contracts, &hash)
        .map_err(LoadError::Plan)?;
    Ok(Loaded {
        report,
        manifest,
        plan,
    })
}

/// Instantiates the cycle tasks of every group, in plan order.
pub fn instantiate(
    plan: &Plan,
    manifest: &Manifest,
    tasks: &dyn TaskProvider,
) -> Result<Vec<GroupTasks>, LoadError> {
    let mut out = Vec::with_capacity(plan.groups.len());
    for group in &plan.groups {
        let mut list: GroupTasks = Vec::with_capacity(group.tasks.len());
        for t in &group.tasks {
            let app = manifest
                .apps
                .get(&t.app)
                .ok_or_else(|| LoadError::Task(format!("unknown app `{}`", t.app)))?;
            let bindings: BTreeMap<String, joker_core::SignalId> = t
                .bindings
                .iter()
                .filter_map(|(port, path)| plan.id(path).map(|id| (port.clone(), id)))
                .collect();
            let task = tasks
                .instantiate(TaskSpec {
                    app: &t.app,
                    task: &t.task,
                    manifest: app,
                    bindings: &bindings,
                })
                .map_err(|e| LoadError::Task(e.0))?;
            list.push((t.writer, task));
        }
        out.push(list);
    }
    Ok(out)
}
