//! Cycle tasks of Wasm components (`kind = "wasm"`, the default; docs/01
//! §4, ref/04): a [`TaskProvider`] over `joker-host`.
//!
//! The provider reads the interface of each component from its `joker:meta`
//! section, for the manifest checks. A task gets its cycle instance in
//! [`CycleTask::prepare`], before the first tick of its group, and runs
//! `on-cycle` in step 4 of every tick.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;

use joker_core::ValueType;
use joker_host::meta::{MetaPort, PortKind as MetaKind, TaskKind};
use joker_host::{
    AppComponent, AppMeta, CycleInstance, CycleSetup, CycleStatus, HostEngine, InstanceLimits,
    PortBinding, PortSource, RunMode, RuntimeMode,
};
use joker_manifest::context::{AppInterface, Param, Port, PortKind, SignalType};
use joker_manifest::model::App;
use joker_manifest::value::Value;
use joker_units::{Duration, Literal, Unit};

use crate::task::{CycleTask, Prepare, TaskContext, TaskError, TaskProvider, TaskSpec};

/// A trap of a Wasm cycle task, as the cycle thread saw it (docs/01 §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trap {
    /// The app.
    pub app: Arc<str>,
    /// The cycle task.
    pub task: Arc<str>,
    /// The trap code (`J-E0905`).
    pub code: &'static str,
    /// The tick of its group.
    pub tick: u64,
    /// The latch instant of that tick.
    pub at_mono_ns: u64,
}

/// Where the cycle thread reports traps without allocating or blocking: a
/// bounded queue that the daemon drains into the decision log (ADR 0067).
/// A full queue drops the trap and counts it.
#[derive(Debug)]
pub struct Traps {
    queue: ArrayQueue<Trap>,
    dropped: AtomicU64,
}

impl Default for Traps {
    fn default() -> Self {
        Self::new(64)
    }
}

impl Traps {
    /// A queue of `capacity` traps.
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: ArrayQueue::new(capacity.max(1)),
            dropped: AtomicU64::new(0),
        }
    }

    /// Reports a trap.
    pub fn report(&self, trap: Trap) {
        if self.queue.push(trap).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Takes the traps reported so far, oldest first.
    pub fn drain(&self) -> Vec<Trap> {
        let mut out = Vec::with_capacity(self.queue.len());
        while let Some(t) = self.queue.pop() {
            out.push(t);
        }
        out
    }

    /// Traps dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Bytes of an [`AppLine`]'s text: its message, then the key and the
/// value of each field.
const APP_LINE_BYTES: usize = 512;

/// Fields an [`AppLine`] keeps.
const APP_LINE_FIELDS: usize = 8;

/// A log line an app wrote during a tick, copied into a fixed buffer so
/// the cycle thread hands it off without allocating. A message or a field
/// past the buffer is cut at a character boundary.
#[derive(Debug, Clone)]
pub struct AppLine {
    /// The app.
    pub app: Arc<str>,
    /// The level.
    pub level: joker_host::LogLevel,
    /// The latch instant of the tick that wrote it.
    pub at_mono_ns: u64,
    message: u16,
    fields: [(u16, u16); APP_LINE_FIELDS],
    n_fields: u8,
    text: [u8; APP_LINE_BYTES],
}

impl AppLine {
    fn new(app: &Arc<str>, at_mono_ns: u64, record: &joker_host::LogRecord<'_>) -> Self {
        let mut line = Self {
            app: Arc::clone(app),
            level: record.level,
            at_mono_ns,
            message: 0,
            fields: [(0, 0); APP_LINE_FIELDS],
            n_fields: 0,
            text: [0; APP_LINE_BYTES],
        };
        let mut at = 0usize;
        line.message = line.put(&mut at, record.message);
        for (key, value) in record.fields().take(APP_LINE_FIELDS) {
            let k = line.put(&mut at, key);
            let v = line.put(&mut at, value);
            line.fields[usize::from(line.n_fields)] = (k, v);
            line.n_fields += 1;
        }
        line
    }

    fn put(&mut self, at: &mut usize, s: &str) -> u16 {
        let mut n = s.len().min(APP_LINE_BYTES - *at);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.text[*at..*at + n].copy_from_slice(&s.as_bytes()[..n]);
        *at += n;
        n as u16
    }

    fn slice(&self, from: usize, len: u16) -> &str {
        self.text
            .get(from..from + usize::from(len))
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    /// The message.
    pub fn message(&self) -> &str {
        self.slice(0, self.message)
    }

    /// The fields, in the order written.
    pub fn fields(&self) -> impl Iterator<Item = (&str, &str)> {
        let mut at = usize::from(self.message);
        self.fields[..usize::from(self.n_fields)]
            .iter()
            .map(move |&(k, v)| {
                let key = self.slice(at, k);
                let value = self.slice(at + usize::from(k), v);
                at += usize::from(k) + usize::from(v);
                (key, value)
            })
    }
}

/// Where the cycle thread hands off the log lines of the apps without
/// allocating or blocking: a bounded queue the daemon drains into the
/// node's log ring (`GET /v1/logs`). A full queue drops the line and
/// counts it (AGENTS.md invariant 7).
#[derive(Debug)]
pub struct AppLogs {
    queue: ArrayQueue<AppLine>,
    dropped: AtomicU64,
}

impl Default for AppLogs {
    fn default() -> Self {
        Self::new(256)
    }
}

impl AppLogs {
    /// A queue of `capacity` lines.
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: ArrayQueue::new(capacity.max(1)),
            dropped: AtomicU64::new(0),
        }
    }

    fn report(&self, line: AppLine) {
        if self.queue.push(line).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Takes the lines handed off so far, oldest first.
    pub fn drain(&self) -> Vec<AppLine> {
        let mut out = Vec::with_capacity(self.queue.len());
        while let Some(l) = self.queue.pop() {
            out.push(l);
        }
        out
    }

    /// Lines dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The Wasm apps a node can run, by app name.
#[derive(Clone)]
pub struct WasmTasks {
    engine: Arc<HostEngine>,
    apps: BTreeMap<String, Arc<AppComponent>>,
    mode: RunMode,
    traps: Arc<Traps>,
    logs: Arc<AppLogs>,
}

impl fmt::Debug for WasmTasks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WasmTasks")
            .field("apps", &self.apps.keys().collect::<Vec<_>>())
            .field("mode", &self.mode)
            .finish()
    }
}

fn host(e: joker_host::HostError) -> TaskError {
    TaskError(e.to_string())
}

impl WasmTasks {
    /// No apps yet, on an engine for this machine.
    pub fn new(mode: RunMode) -> Result<Self, TaskError> {
        Ok(Self::with_engine(
            Arc::new(HostEngine::native().map_err(host)?),
            mode,
        ))
    }

    /// No apps yet, on `engine`, which must be this machine's.
    pub fn with_engine(engine: Arc<HostEngine>, mode: RunMode) -> Self {
        Self {
            engine,
            apps: BTreeMap::new(),
            mode,
            traps: Arc::default(),
            logs: Arc::default(),
        }
    }

    /// Reports the traps of its tasks to `traps`.
    pub fn with_traps(mut self, traps: Arc<Traps>) -> Self {
        self.traps = traps;
        self
    }

    /// The traps of its tasks.
    pub fn traps(&self) -> Arc<Traps> {
        Arc::clone(&self.traps)
    }

    /// Hands the log lines of its tasks to `logs`.
    #[must_use]
    pub fn with_app_logs(mut self, logs: Arc<AppLogs>) -> Self {
        self.logs = logs;
        self
    }

    /// The log lines of its tasks.
    pub fn app_logs(&self) -> Arc<AppLogs> {
        Arc::clone(&self.logs)
    }

    /// Adds a component loaded elsewhere (from the AOT cache of a node)
    /// under the name of its `joker:meta`, which is returned.
    pub fn insert(&mut self, component: Arc<AppComponent>) -> String {
        let name = component.meta().name.clone();
        self.apps.insert(name.clone(), component);
        name
    }

    /// Compiles a component and adds it under the name of its `joker:meta`,
    /// which is returned. `dev` accepts a component without an AOT
    /// artifact (ref/04 §7, `J-E0010` in `prod`).
    pub fn add(&mut self, wasm: &[u8], dev: bool) -> Result<String, TaskError> {
        let mode = if dev {
            RuntimeMode::Dev
        } else {
            RuntimeMode::Prod
        };
        let component = AppComponent::compile(&self.engine, wasm, mode).map_err(host)?;
        let name = component.meta().name.clone();
        self.apps.insert(name.clone(), Arc::new(component));
        Ok(name)
    }

    /// The names of the apps.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.apps.keys().map(String::as_str)
    }
}

fn signal_type(ty: Option<ValueType>) -> SignalType {
    match ty {
        Some(ValueType::Bool) => SignalType::Bool,
        Some(ValueType::I64 | ValueType::U64 | ValueType::Bits | ValueType::Enum) => {
            SignalType::Integer
        }
        Some(ValueType::F64 | ValueType::F32) => SignalType::Float,
        Some(ValueType::String) => SignalType::Text,
        Some(ValueType::Bytes) => SignalType::Bytes,
        _ => SignalType::Unknown,
    }
}

fn unit(port: &MetaPort) -> Option<Unit> {
    port.unit.as_deref().and_then(|u| Unit::parse(u).ok())
}

/// The interface the manifest checks see, from the component's metadata.
pub fn interface(meta: &AppMeta) -> AppInterface {
    let mut i = AppInterface::default();
    for p in &meta.ports {
        let kind = match (p.kind, p.many) {
            (MetaKind::Input, false) => PortKind::Input,
            (MetaKind::Input, true) => PortKind::InputMany,
            (MetaKind::Output, false) => PortKind::Output,
            (MetaKind::Output, true) => PortKind::OutputMany,
            (MetaKind::Action, _) => PortKind::Action,
            (MetaKind::Param, _) => {
                i.params.insert(
                    p.name.clone(),
                    Param {
                        ty: signal_type(p.ty),
                        unit: unit(p),
                        has_default: p.default.is_some(),
                        range: None,
                    },
                );
                continue;
            }
            _ => continue,
        };
        i.ports.insert(
            p.name.clone(),
            Port {
                kind,
                ty: signal_type(p.ty),
                unit: unit(p),
                optional: p.optional,
            },
        );
    }
    for t in meta.tasks.iter().filter(|t| t.kind == TaskKind::Cycle) {
        i.tasks
            .insert(t.name.clone(), t.period_ns.map(Duration::from_nanos));
        if let Some(w) = t.wcet_ns {
            i.wcet.insert(t.name.clone(), Duration::from_nanos(w));
        }
        if t.optional {
            i.optional_tasks.insert(t.name.clone());
        }
    }
    i.memory = meta.cycle_memory_max.map(|c| (c, 0));
    i
}

/// The bits of a parameter in the port's type and unit (ref/00 §4.1), from
/// the manifest value or the app's default.
fn param_bits(port: &MetaPort, manifest: &App) -> Result<u64, TaskError> {
    let given = manifest.params.get(&port.name).map(|p| &p.value.value);
    let missing = || TaskError(format!("parameter `{}` has no value", port.name));
    match port.ty {
        Some(ValueType::Bool) => match given {
            Some(Value::Bool(b)) => Ok(u64::from(*b)),
            None => port
                .default
                .map(|d| u64::from(d != 0.0))
                .ok_or_else(missing),
            Some(_) => Err(TaskError(format!(
                "parameter `{}` is not a bool",
                port.name
            ))),
        },
        Some(ValueType::I64 | ValueType::U64 | ValueType::Enum | ValueType::Bits) => match given {
            Some(Value::Integer(n)) => Ok(*n as u64),
            None => port.default.map(|d| d as i64 as u64).ok_or_else(missing),
            Some(_) => Err(TaskError(format!(
                "parameter `{}` is not an integer",
                port.name
            ))),
        },
        _ => {
            let value = match given {
                Some(Value::Float(f)) => *f,
                Some(Value::Integer(n)) => *n as f64,
                Some(Value::String(s)) => {
                    match Literal::parse(s)
                        .map_err(|e| TaskError(format!("parameter `{}`: {e:?}", port.name)))?
                    {
                        Literal::Number(n) => n,
                        Literal::Quantity(q) => {
                            let target = unit(port).ok_or_else(|| {
                                TaskError(format!("parameter `{}` has no unit", port.name))
                            })?;
                            q.value_in(&target).map_err(|e| {
                                TaskError(format!("parameter `{}`: {e:?}", port.name))
                            })?
                        }
                    }
                }
                None => port.default.ok_or_else(missing)?,
                Some(_) => {
                    return Err(TaskError(format!(
                        "parameter `{}` is not a number",
                        port.name
                    )));
                }
            };
            Ok(value.to_bits())
        }
    }
}

impl TaskProvider for WasmTasks {
    fn interfaces(&self) -> BTreeMap<String, AppInterface> {
        self.apps
            .iter()
            .map(|(name, c)| (name.clone(), interface(c.meta())))
            .collect()
    }

    fn instantiate(&self, spec: TaskSpec<'_>) -> Result<Box<dyn CycleTask>, TaskError> {
        let component = self
            .apps
            .get(spec.app)
            .ok_or_else(|| TaskError(format!("no Wasm app `{}`", spec.app)))?;
        let meta = component.meta();
        let task = meta
            .tasks
            .iter()
            .position(|t| t.kind == TaskKind::Cycle && t.name == spec.task)
            .ok_or_else(|| {
                TaskError(format!(
                    "app `{}` has no cycle task `{}`",
                    spec.app, spec.task
                ))
            })?;
        let mut ports = Vec::with_capacity(meta.ports.len());
        for p in &meta.ports {
            let binding = match p.kind {
                MetaKind::Param => Binding::Param(param_bits(p, spec.manifest)?),
                _ => match spec.bindings.get(&p.name) {
                    Some(id) => Binding::Signal(*id),
                    None => Binding::Unbound,
                },
            };
            ports.push((binding, format!("app.{}.{}", spec.app, p.name)));
        }
        Ok(Box::new(WasmTask {
            engine: Arc::clone(&self.engine),
            component: Arc::clone(component),
            mode: self.mode,
            task: task as u32,
            ports,
            instance: None,
            app: Arc::from(spec.app),
            task_name: Arc::from(spec.task),
            traps: Arc::clone(&self.traps),
            logs: Arc::clone(&self.logs),
        }))
    }
}

#[derive(Debug, Clone, Copy)]
enum Binding {
    Signal(joker_core::SignalId),
    Param(u64),
    Unbound,
}

/// One cycle task of a Wasm app.
struct WasmTask {
    engine: Arc<HostEngine>,
    component: Arc<AppComponent>,
    mode: RunMode,
    task: u32,
    ports: Vec<(Binding, String)>,
    instance: Option<CycleInstance>,
    app: Arc<str>,
    task_name: Arc<str>,
    traps: Arc<Traps>,
    logs: Arc<AppLogs>,
}

impl fmt::Debug for WasmTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WasmTask")
            .field("app", &self.component.meta().name)
            .field("task", &self.task)
            .finish()
    }
}

impl CycleTask for WasmTask {
    fn prepare(&mut self, cx: &Prepare<'_, '_, '_>) -> Result<(), TaskError> {
        let names: Vec<String> = self
            .ports
            .iter()
            .map(|(binding, param)| match binding {
                Binding::Signal(id) => cx
                    .registry
                    .signal(*id)
                    .map_or_else(|| param.clone(), |m| m.path.to_owned()),
                _ => param.clone(),
            })
            .collect();
        let ports: Vec<PortBinding<'_>> = self
            .ports
            .iter()
            .zip(&names)
            .map(|((binding, _), name)| PortBinding {
                source: match binding {
                    Binding::Signal(id) => PortSource::Signal(*id),
                    Binding::Param(bits) => PortSource::Param(*bits),
                    Binding::Unbound => PortSource::Unbound,
                },
                name,
            })
            .collect();
        let tasks = [self.task];
        let instance = CycleInstance::new(
            &self.engine,
            &self.component,
            cx.registry,
            cx.image,
            &CycleSetup {
                node: "local",
                mode: self.mode,
                generation: 0,
                generation_hash: cx.generation_hash,
                writer: cx.writer,
                group: cx.group,
                ports: &ports,
                tasks: &tasks,
                limits: InstanceLimits::default(),
            },
        )
        .map_err(host)?;
        self.instance = Some(instance);
        Ok(())
    }

    fn run(&mut self, cx: &mut TaskContext<'_, '_, '_>) -> Result<(), TaskError> {
        let instance = self
            .instance
            .as_mut()
            .ok_or_else(|| TaskError("the Wasm task was not prepared".into()))?;
        if instance.is_poisoned() {
            return Err(TaskError(String::new()));
        }
        let tick = cx.tick();
        instance.set_causal_task(cx.task());
        let (image, outputs) = cx.parts();
        let report = instance.run(self.task, image, outputs).map_err(host)?;
        let at_mono_ns = tick.map_or(0, |t| t.at.mono_ns);
        instance.drain_log(|record| {
            self.logs
                .report(AppLine::new(&self.app, at_mono_ns, &record));
        });
        match report.status {
            CycleStatus::Ok => Ok(()),
            CycleStatus::AppError => Err(TaskError(String::new())),
            CycleStatus::Trap(code) => {
                self.traps.report(Trap {
                    app: Arc::clone(&self.app),
                    task: Arc::clone(&self.task_name),
                    code,
                    tick: tick.map_or(0, |t| t.index),
                    at_mono_ns: tick.map_or(0, |t| t.at.mono_ns),
                });
                Err(TaskError(String::new()))
            }
        }
    }
}
