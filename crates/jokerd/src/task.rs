//! Cycle tasks: the code a group runs in step 4 of each tick (docs/01 §4).
//!
//! The Wasm host implements [`TaskProvider`] for components; the daemon
//! also accepts native tasks (`kind = "native"`, docs/01 §4) and test tasks
//! through the same trait.

use std::collections::BTreeMap;
use std::fmt;

use joker_core::image::{ImageError, Intent, NO_TASK, ProcessImage, Proposal, RELINQUISH, Source};
use joker_core::{Sample, SignalId, TickInfo, WriterId};
use joker_manifest::context::AppInterface;
use joker_manifest::model::App;

/// A task failed in this tick. The group keeps running; the error is
/// counted and the task's proposals of this tick stay staged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskError(pub String);

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TaskError {}

/// What a task sees during its call: the latched snapshot and the staging
/// buffer of its group, under its own writer.
#[derive(Debug)]
pub struct TaskContext<'a, 'img> {
    image: &'a mut ProcessImage<'img>,
    writer: WriterId,
    dropped: u64,
}

impl<'a, 'img> TaskContext<'a, 'img> {
    /// A context for `writer` on a latched image.
    pub fn new(image: &'a mut ProcessImage<'img>, writer: WriterId) -> Self {
        Self {
            image,
            writer,
            dropped: 0,
        }
    }

    /// The tick: index, latch instant (`now()`) and `dt()`.
    pub fn tick(&self) -> Option<TickInfo> {
        self.image.tick()
    }

    /// The latched sample of a signal the group reads.
    pub fn read(&self, signal: SignalId) -> Option<Sample> {
        self.image.sample(signal).copied()
    }

    /// Proposes value bits for an output (ref/00 §4.1).
    pub fn propose(&mut self, signal: SignalId, bits: u64) -> Result<(), ImageError> {
        self.stage(signal, bits, 0)
    }

    /// Releases the writer's level on an output.
    pub fn relinquish(&mut self, signal: SignalId) -> Result<(), ImageError> {
        self.stage(signal, 0, RELINQUISH)
    }

    /// Proposals dropped because the staging buffer was full.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    fn stage(&mut self, signal: SignalId, value: u64, flags: u16) -> Result<(), ImageError> {
        let result = self.image.stage_cycle(Proposal {
            writer: self.writer,
            signal,
            value,
            flags,
            source: Source::Cycle,
            ticket: 0,
            ttl_ns: 0,
            intent: Intent::Write,
            task: NO_TASK,
        });
        if result.is_err() {
            self.dropped += 1;
        }
        result
    }
}

/// One cycle task instance.
pub trait CycleTask: Send {
    /// Runs the task for one tick. Must not allocate, block or wait.
    fn run(&mut self, cx: &mut TaskContext<'_, '_>) -> Result<(), TaskError>;

    /// The task's state for a recording or a hot swap (`export-state`,
    /// ref/04 §4.12), taken between two ticks; `None` without `persist`.
    fn export_state(&mut self) -> Result<Option<Vec<u8>>, TaskError> {
        Ok(None)
    }

    /// Restores a state exported by [`CycleTask::export_state`].
    fn import_state(&mut self, _blob: &[u8]) -> Result<(), TaskError> {
        Ok(())
    }
}

/// Everything a provider needs to start one task.
#[derive(Debug, Clone, Copy)]
pub struct TaskSpec<'a> {
    /// The app name.
    pub app: &'a str,
    /// The task name.
    pub task: &'a str,
    /// The app's manifest entry (params, source, digest).
    pub manifest: &'a App,
    /// Port name to `SignalId`, for the bound ports.
    pub bindings: &'a BTreeMap<String, SignalId>,
}

/// Where cycle tasks come from: the Wasm host, native tasks, or tests.
pub trait TaskProvider: Send + Sync {
    /// The interfaces of the apps this provider can run, by app name
    /// (`joker:meta`, ref/04), for the manifest checks.
    fn interfaces(&self) -> BTreeMap<String, AppInterface>;

    /// Instantiates one cycle task.
    fn instantiate(&self, spec: TaskSpec<'_>) -> Result<Box<dyn CycleTask>, TaskError>;
}

/// A provider that combines several, asking each in turn.
#[derive(Default)]
pub struct Providers {
    providers: Vec<Box<dyn TaskProvider>>,
}

impl fmt::Debug for Providers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Providers({})", self.providers.len())
    }
}

impl Providers {
    /// Adds a provider; earlier ones win.
    pub fn push(&mut self, provider: Box<dyn TaskProvider>) {
        self.providers.push(provider);
    }
}

impl TaskProvider for Providers {
    fn interfaces(&self) -> BTreeMap<String, AppInterface> {
        let mut all = BTreeMap::new();
        for p in self.providers.iter().rev() {
            all.extend(p.interfaces());
        }
        all
    }

    fn instantiate(&self, spec: TaskSpec<'_>) -> Result<Box<dyn CycleTask>, TaskError> {
        for p in &self.providers {
            if p.interfaces().contains_key(spec.app) {
                return p.instantiate(spec);
            }
        }
        Err(TaskError(format!("no provider for app `{}`", spec.app)))
    }
}
