//! The generation controller: prepares, activates, confirms and rolls back
//! generations, one transition at a time (docs/01 §15, ADR 0015, ref/09
//! §5.13).
//!
//! A switch stops the running generation after its current tick, takes the
//! last applied value of every output, and starts the new one from those
//! values (docs/16 A8). With a confirmation window, the previous
//! generation is reactivated if `confirm` does not arrive in time.

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use joker_manifest::MemoryFileSystem;
use serde_json::{Map, Value, json};

use crate::decisions::{SharedLog, lock};
use crate::devices::{DeviceSet, Devices};
use crate::generations::{Activation, GenState, Generation, Origin, Store, descriptor_hash};
use crate::load::{LoadError, Loaded, instantiate, load};
use crate::pacer::Pacer;
use crate::plan::Plan;
use crate::runner::{Running, Start};
use crate::task::TaskProvider;
use crate::trace_ring::digest_bytes;

/// The name of the effective manifest inside a generation.
pub const EFFECTIVE: &str = "joker.toml";

/// A refused request, with its code (ref/09 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlError {
    /// Registry code.
    pub code: &'static str,
    /// HTTP status of the API.
    pub status: u16,
    /// The concrete case.
    pub detail: String,
    /// Diagnostics of a failed validation: (code, rendered).
    pub errors: Vec<(String, String)>,
}

impl ControlError {
    fn new(code: &'static str, status: u16, detail: impl Into<String>) -> Self {
        Self {
            code,
            status,
            detail: detail.into(),
            errors: Vec::new(),
        }
    }

    fn io(e: std::io::Error) -> Self {
        Self::new("J-E1000", 500, format!("state store: {e}"))
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code, self.detail)
    }
}

impl std::error::Error for ControlError {}

/// Limits of the controller.
#[derive(Debug, Clone)]
pub struct Config {
    /// Shortest confirmation window (30 s, `J-E1623`).
    pub min_window: Duration,
    /// Longest confirmation window (24 h, `J-E1623`).
    pub max_window: Duration,
    /// Retention of the continuous trace.
    pub retention: Duration,
    /// UTC time at the daemon's monotonic zero, to stamp log entries.
    pub utc_origin_ns: i64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_window: Duration::from_secs(30),
            max_window: Duration::from_secs(24 * 3600),
            retention: crate::trace_ring::DEFAULT_RETENTION,
            utc_origin_ns: 0,
        }
    }
}

/// Makes the pacer of each activation.
pub type PacerFactory = Box<dyn Fn() -> Box<dyn Pacer> + Send + Sync>;

struct Active {
    number: u64,
    hash: String,
    running: Running,
    devices: Box<dyn DeviceSet>,
}

struct Confirming {
    number: u64,
    previous: Option<u64>,
    deadline: StdInstant,
}

/// The generation controller of a node.
pub struct Controller {
    store: Store,
    log: SharedLog,
    tasks: Arc<dyn TaskProvider>,
    devices: Arc<dyn Devices>,
    pacer: PacerFactory,
    config: Config,
    active: Option<Active>,
    confirming: Option<Confirming>,
    actions: crate::actions::Dispatcher<ActionResult>,
}

/// What invoking an action produced (ref/09 §5.10, ADR 0058). Mirrors
/// [`crate::drivers::ActionOutcome`] (same distinction, same reason: a
/// timeout and an equipment failure each have their own documented API
/// code, ref/04 §7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionResult {
    /// Every step completed.
    Succeeded,
    /// An `until` step never saw its target before its own `timeout`.
    TimedOut,
    /// The driver reported a communication failure; the message is its
    /// `Debug` rendering.
    Device(String),
    /// Any other interpreter failure; the message is its `Debug`
    /// rendering.
    Failed(String),
}

/// What [`Controller::invoke_action`] decided, and what it ran if
/// admitted (docs/02 §8, ADR 0058).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invoked {
    /// Ran now.
    Ran(ActionResult),
    /// Served from the dispatcher's dedup cache, within the action's own
    /// `dedup_window`; nothing ran again.
    Duplicate(ActionResult),
    /// Not run, and why.
    Rejected(crate::actions::Rejected),
}

fn outcome_tag(result: &ActionResult) -> (&'static str, Option<&str>) {
    match result {
        ActionResult::Succeeded => ("ok", None),
        ActionResult::TimedOut => ("failed", None),
        ActionResult::Device(m) | ActionResult::Failed(m) => ("failed", Some(m.as_str())),
    }
}

impl fmt::Debug for Controller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Controller")
            .field("active", &self.active.as_ref().map(|a| a.number))
            .field("confirming", &self.confirming.as_ref().map(|c| c.number))
            .finish()
    }
}

/// What `activate` and `rollback` report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switched {
    /// The generation now running.
    pub generation: Generation,
    /// The one left, if any.
    pub from: Option<u64>,
}

fn loaded_of(manifest: &str, tasks: &dyn TaskProvider) -> Result<Loaded, ControlError> {
    let mut fs = MemoryFileSystem::default();
    fs.file(EFFECTIVE, manifest);
    for path in app_paths(manifest) {
        fs.dir(path);
    }
    load(&fs, Path::new(EFFECTIVE), tasks).map_err(|e| validation(&e))
}

/// The `path` of every app: on the node, an app is an artifact of the
/// generation and not a project folder, so its folder is taken as present.
fn app_paths(manifest: &str) -> Vec<String> {
    let Ok(table) = manifest.parse::<toml::Table>() else {
        return Vec::new();
    };
    table
        .get("apps")
        .and_then(toml::Value::as_table)
        .map(|apps| {
            apps.values()
                .filter_map(|a| {
                    a.get("path")
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn validation(e: &LoadError) -> ControlError {
    let mut err = ControlError::new("J-E1601", 422, "the generation did not pass validation");
    match e {
        LoadError::Check { rendered, codes } => {
            err.errors = codes.iter().map(|c| (c.clone(), String::new())).collect();
            if let Some(first) = err.errors.first_mut() {
                first.1 = rendered.clone();
            }
        }
        LoadError::Plan(p) => err
            .errors
            .push((p.code.unwrap_or("J-E1601").to_owned(), p.to_string())),
        LoadError::Task(t) => err.errors.push(("J-E1601".to_owned(), t.clone())),
    }
    err
}

fn lock_digest(lock: &str) -> [u8; 32] {
    let report = joker_manifest::lock::read(Path::new("joker.lock"), lock);
    let digest = report.lock.unwrap_or_default().digest();
    digest_bytes(&digest)
}

/// The descriptor hash of an effective manifest and its lock.
pub fn generation_hash(loaded: &Loaded, lock: &str) -> String {
    let canon = loaded
        .report
        .effective
        .as_ref()
        .map(joker_manifest::effective::cbor);
    descriptor_hash(
        canon.as_deref().unwrap_or(&[0xA0]),
        lock_digest(lock),
        &[],
        loaded.plan.dev,
    )
}

impl Controller {
    /// A controller over the store in `state_dir/generations`.
    pub fn new(
        state_dir: &Path,
        log: SharedLog,
        tasks: Arc<dyn TaskProvider>,
        devices: Arc<dyn Devices>,
        pacer: PacerFactory,
        config: Config,
    ) -> Result<Self, ControlError> {
        Ok(Self {
            store: Store::open(&state_dir.join("generations")).map_err(ControlError::io)?,
            log,
            tasks,
            devices,
            pacer,
            config,
            active: None,
            confirming: None,
            actions: crate::actions::Dispatcher::new(),
        })
    }

    /// The store.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The running generation: number, hash and its runner.
    pub fn current(&self) -> Option<(u64, &str, &Running)> {
        self.active
            .as_ref()
            .map(|a| (a.number, a.hash.as_str(), &a.running))
    }

    /// The generation waiting for `confirm`, with the time left.
    pub fn confirming(&self) -> Option<(u64, Duration)> {
        self.confirming.as_ref().map(|c| {
            (
                c.number,
                c.deadline.saturating_duration_since(StdInstant::now()),
            )
        })
    }

    /// Validates a proposal; with `dry_run` stores nothing, otherwise stores
    /// it as `pending`, replacing an older pending one.
    pub fn propose(
        &mut self,
        manifest: &str,
        lock: &str,
        message: Option<String>,
        dry_run: bool,
    ) -> Result<Generation, ControlError> {
        self.propose_by(manifest, lock, message, dry_run, None)
    }

    /// [`Controller::propose`] on behalf of a principal.
    pub fn propose_by(
        &mut self,
        manifest: &str,
        lock: &str,
        message: Option<String>,
        dry_run: bool,
        created_by: Option<String>,
    ) -> Result<Generation, ControlError> {
        if self.confirming.is_some() {
            return Err(ControlError::new(
                "J-E1608",
                409,
                "a generation is waiting for confirmation",
            ));
        }
        let loaded = loaded_of(manifest, self.tasks.as_ref())?;
        let hash = generation_hash(&loaded, lock);
        if dry_run {
            return Ok(Generation {
                number: 0,
                hash,
                state: GenState::Pending,
                created_utc_ns: 0,
                message,
                created_by,
                failure: None,
                confirm_window_ns: None,
            });
        }
        for g in self.store.list().map_err(ControlError::io)? {
            if g.state == GenState::Pending {
                self.store
                    .set_state(g.number, GenState::Discarded, Some("superseded".into()))
                    .map_err(ControlError::io)?;
            }
        }
        self.store
            .create(&hash, manifest, lock, message, created_by)
            .map_err(ControlError::io)
    }

    /// Activates a pending generation. With a window it waits for `confirm`
    /// and rolls back when the window ends.
    pub fn activate(
        &mut self,
        number: u64,
        window: Option<Duration>,
        principal: Option<String>,
    ) -> Result<Switched, ControlError> {
        if self.confirming.is_some() {
            return Err(ControlError::new(
                "J-E1608",
                409,
                "a generation is waiting for confirmation",
            ));
        }
        if let Some(w) = window
            && (w < self.config.min_window || w > self.config.max_window)
        {
            return Err(ControlError::new(
                "J-E1623",
                400,
                "confirmation window outside 30s–24h",
            ));
        }
        let g = self.get(number)?;
        if g.state != GenState::Pending {
            return Err(ControlError::new(
                "J-E1612",
                409,
                format!("generation {number} is {}, not pending", g.state),
            ));
        }
        let from = self.active.as_ref().map(|a| a.number);
        match self.switch(number, Origin::Local, principal, None) {
            Ok(()) => {}
            Err(e) => {
                self.store
                    .set_state(number, GenState::Failed, Some(e.detail.clone()))
                    .map_err(ControlError::io)?;
                self.log_generation(number, "failed", from, Some(&e.detail));
                return Err(e);
            }
        }
        let mut g = self.get(number)?;
        g.confirm_window_ns = window.map(|w| u64::try_from(w.as_nanos()).unwrap_or(u64::MAX));
        g.state = if window.is_some() {
            GenState::Confirming
        } else {
            GenState::Confirmed
        };
        self.store.save(&g).map_err(ControlError::io)?;
        if let Some(w) = window {
            self.confirming = Some(Confirming {
                number,
                previous: from,
                deadline: StdInstant::now() + w,
            });
        }
        self.log_generation(number, g.state.name(), from, None);
        Ok(Switched {
            generation: g,
            from,
        })
    }

    /// Confirms the generation in `confirming`.
    pub fn confirm(&mut self, number: Option<u64>) -> Result<Generation, ControlError> {
        let Some(c) = &self.confirming else {
            return Err(ControlError::new("J-E1611", 409, "nothing to confirm"));
        };
        if number.is_some_and(|g| g != c.number) {
            let state = self
                .get(number.unwrap_or(0))
                .map(|g| g.state.name())
                .unwrap_or("unknown");
            return Err(ControlError::new(
                "J-E1611",
                409,
                format!("generation is {state}"),
            ));
        }
        let number = c.number;
        self.confirming = None;
        let g = self
            .store
            .set_state(number, GenState::Confirmed, None)
            .map_err(ControlError::io)?;
        self.log_generation(number, "confirmed", None, None);
        Ok(g)
    }

    /// Reactivates a stored generation (`previous` by default): no new
    /// generation is created (ref/09 §5.13).
    pub fn rollback(
        &mut self,
        to: Option<u64>,
        principal: Option<String>,
        reason: Option<String>,
    ) -> Result<Switched, ControlError> {
        let current = self.active.as_ref().map(|a| a.number);
        let target = match to {
            Some(t) => t,
            None => self
                .confirming
                .as_ref()
                .and_then(|c| c.previous)
                .or_else(|| self.previous_confirmed(current))
                .ok_or_else(|| ControlError::new("J-E1613", 404, "no previous generation"))?,
        };
        self.get(target)?;
        self.switch(target, Origin::Rollback, principal, reason.clone())?;
        if let Some(cur) = current {
            self.store
                .set_state(cur, GenState::RolledBack, reason.clone())
                .map_err(ControlError::io)?;
            self.log_generation(cur, "rolled_back", Some(cur), reason.as_deref());
        }
        self.confirming = None;
        let g = self
            .store
            .set_state(target, GenState::Confirmed, None)
            .map_err(ControlError::io)?;
        self.log_generation(target, "confirmed", current, Some("rollback"));
        Ok(Switched {
            generation: g,
            from: current,
        })
    }

    /// Rolls back when the confirmation window has ended; returns the
    /// rollback when one happened.
    pub fn poll(&mut self) -> Result<Option<Switched>, ControlError> {
        let expired = self
            .confirming
            .as_ref()
            .is_some_and(|c| StdInstant::now() >= c.deadline);
        if expired {
            return self
                .rollback(None, None, Some("confirmation window expired".into()))
                .map(Some);
        }
        if let Some(a) = &self.active
            && !a.running.is_running()
            && self.confirming.is_some()
        {
            return self
                .rollback(None, None, Some("the cycle thread stopped".into()))
                .map(Some);
        }
        Ok(None)
    }

    /// Starts the last confirmed generation after a restart (`origin =
    /// boot`). A generation left in `confirming` by a crash is rolled back.
    pub fn boot(&mut self) -> Result<Option<Generation>, ControlError> {
        for g in self.store.list().map_err(ControlError::io)? {
            if g.state == GenState::Confirming {
                self.store
                    .set_state(
                        g.number,
                        GenState::RolledBack,
                        Some("not confirmed before a restart".into()),
                    )
                    .map_err(ControlError::io)?;
            }
        }
        let Some(g) = self
            .store
            .latest(GenState::Confirmed)
            .map_err(ControlError::io)?
        else {
            return Ok(None);
        };
        self.switch(g.number, Origin::Boot, None, None)?;
        Ok(Some(g))
    }

    /// Stops the running generation.
    pub fn shutdown(&mut self) {
        if let Some(a) = self.active.take() {
            a.devices.stop();
            let _ = a.running.stop();
        }
    }

    fn get(&self, number: u64) -> Result<Generation, ControlError> {
        self.store
            .get(number)
            .map_err(|_| ControlError::new("J-E1600", 404, format!("unknown generation {number}")))
    }

    fn previous_confirmed(&self, current: Option<u64>) -> Option<u64> {
        let activations = self.store.activations().ok()?;
        activations
            .iter()
            .rev()
            .filter(|a| Some(a.number) != current)
            .find(|a| {
                self.store
                    .get(a.number)
                    .is_ok_and(|g| g.state == GenState::Confirmed)
            })
            .map(|a| a.number)
    }

    fn switch(
        &mut self,
        number: u64,
        origin: Origin,
        principal: Option<String>,
        reason: Option<String>,
    ) -> Result<(), ControlError> {
        let g = self.get(number)?;
        let (manifest, _lock) = self.store.content(number).map_err(|_| {
            ControlError::new(
                "J-E1613",
                404,
                format!("generation {number} is no longer stored"),
            )
        })?;
        let loaded = loaded_of(&manifest, self.tasks.as_ref())?;
        let tasks = instantiate(&loaded.plan, &loaded.manifest, self.tasks.as_ref())
            .map_err(|e| validation(&e))?;
        let from = self.active.as_ref().map(|a| a.number);
        let inherit = match self.active.take() {
            Some(a) => {
                a.devices.stop();
                a.running
                    .stop()
                    .map_err(|e| ControlError::new("J-E1000", 500, e.0))?
                    .applied
            }
            None => Default::default(),
        };
        let plan = Arc::new(loaded.plan);
        let running = Running::start(
            Arc::clone(&plan),
            tasks,
            (self.pacer)(),
            Arc::clone(&self.log),
            Start {
                generation: number,
                inherit,
                retention: self.config.retention,
                max_ticks: None,
                utc_origin_ns: self.config.utc_origin_ns,
                cpus: crate::resources::rt_cpus(&loaded.manifest),
            },
        )
        .map_err(|e| ControlError::new("J-E1000", 500, e.0))?;
        let devices = self.devices.start(&plan, &running.shared);
        self.active = Some(Active {
            number,
            hash: g.hash.clone(),
            running,
            devices,
        });
        self.store
            .record_activation(&Activation {
                at_utc_ns: utc_now_ns(),
                number,
                from,
                origin,
                principal,
                reason,
            })
            .map_err(ControlError::io)?;
        Ok(())
    }

    fn log_generation(&self, number: u64, event: &str, from: Option<u64>, reason: Option<&str>) {
        let mut m = Map::new();
        m.insert("gen".into(), json!(number));
        m.insert("record".into(), json!("generation"));
        m.insert("event".into(), json!(event));
        m.insert("from".into(), from.map_or(Value::Null, |f| json!(f)));
        m.insert("reason".into(), reason.map_or(Value::Null, |r| json!(r)));
        m.insert("durability".into(), json!("immediate"));
        m.insert("ts".into(), json!(crate::wire::timestamp(utc_now_ns())));
        let _ = lock(&self.log).append(m, true);
    }

    /// Invokes `device`'s `action` (ref/09 §5.10, ADR 0058): serializes
    /// per device and deduplicates by `key` through [`Dispatcher`]
    /// (`crate::actions`), then — on `Admit` — runs the action's compiled
    /// steps against the device's real driver instance over its own
    /// action channel (`DeviceSet::action_sender`, AUR-405) and logs the
    /// outcome either way (invariant 8). The manifest's own
    /// `dedup_window` override (`DeviceActionPlan::dedup_window_ns`) is
    /// used when declared, ref/09 §5.10's 60s default otherwise. The
    /// action's own declared `permit` (`Plan::permits`, AUR-416/
    /// AUR-417) is evaluated for real against live `Shared.slots`
    /// values; an action with none declared is always permitted, and
    /// an evaluation failure fails safe (not permitted), never silently
    /// `true`. `$arg` substitution into a step's value is still a
    /// flagged, separate gap (AUR-405).
    ///
    /// [`Dispatcher`]: crate::actions::Dispatcher
    pub fn invoke_action(
        &mut self,
        device: &str,
        action: &str,
        key: &str,
        now_mono_ns: u64,
        writer: &str,
    ) -> Result<Invoked, ControlError> {
        // ref/09 §5.10's own documented default when the manifest
        // declares no `dedup_window` override.
        const DEFAULT_DEDUP_WINDOW_NS: u64 = 60_000_000_000;
        let (generation, steps, dedup_window_ns, permitted, sender) = {
            let active = self
                .active
                .as_ref()
                .ok_or_else(|| ControlError::new("J-E1015", 503, "no generation is running"))?;
            let Some(decl) = active
                .running
                .plan
                .devices
                .iter()
                .find(|d| d.name == device)
                .and_then(|d| d.actions.iter().find(|a| a.name == action))
            else {
                return Err(ControlError::new(
                    "J-E1400",
                    404,
                    format!("unknown action `{device}.{action}`"),
                ));
            };
            // No declared `permit` means always permitted (ref/01
            // §5.8.2: `permit` is optional). Actions have no
            // `permit_uncertain` override key (unlike an output's own
            // `interlock`, docs/02 §4), so `Uncertain` quality always
            // fails the permit, with no way to relax that.
            let permitted = match active
                .running
                .plan
                .permits
                .get(&(device.to_owned(), action.to_owned()))
            {
                Some(program) => {
                    match crate::expr_plan::eval_bool(program, &active.running.shared, now_mono_ns)
                    {
                        Ok(output) => output.permits(false),
                        Err(_) => false,
                    }
                }
                None => true,
            };
            (
                active.running.generation,
                decl.steps.clone(),
                decl.dedup_window_ns.unwrap_or(DEFAULT_DEDUP_WINDOW_NS),
                permitted,
                active.devices.action_sender(device),
            )
        };
        let decision =
            self.actions
                .begin(device, action, key, now_mono_ns, dedup_window_ns, permitted);
        match decision {
            crate::actions::Decision::Rejected(r) => {
                let tag = match r {
                    crate::actions::Rejected::Busy => "rejected_busy",
                    crate::actions::Rejected::NotPermitted => "rejected_not_permitted",
                };
                self.log_action(
                    generation,
                    device,
                    action,
                    key,
                    now_mono_ns,
                    writer,
                    tag,
                    None,
                );
                Ok(Invoked::Rejected(r))
            }
            crate::actions::Decision::Duplicate(result) => {
                let (tag, failure) = outcome_tag(&result);
                self.log_action(
                    generation,
                    device,
                    action,
                    key,
                    now_mono_ns,
                    writer,
                    tag,
                    failure,
                );
                Ok(Invoked::Duplicate(result))
            }
            crate::actions::Decision::Admit => {
                let Some(sender) = sender else {
                    let result =
                        ActionResult::Failed(format!("device `{device}` is not connected"));
                    self.actions
                        .finish(device, action, key, now_mono_ns, result.clone());
                    let (tag, failure) = outcome_tag(&result);
                    self.log_action(
                        generation,
                        device,
                        action,
                        key,
                        now_mono_ns,
                        writer,
                        tag,
                        failure,
                    );
                    return Err(ControlError::new(
                        "J-E1415",
                        503,
                        format!("device `{device}` is not connected"),
                    ));
                };
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let result = if sender
                    .blocking_send(crate::drivers::ActionCall {
                        steps,
                        reply: reply_tx,
                    })
                    .is_err()
                {
                    ActionResult::Failed(format!("device `{device}`'s action queue is full"))
                } else {
                    match reply_rx.blocking_recv() {
                        Ok(crate::drivers::ActionOutcome::Ok) => ActionResult::Succeeded,
                        Ok(crate::drivers::ActionOutcome::TimedOut) => ActionResult::TimedOut,
                        Ok(crate::drivers::ActionOutcome::Device(msg)) => ActionResult::Device(msg),
                        Ok(crate::drivers::ActionOutcome::Failed(msg)) => ActionResult::Failed(msg),
                        Err(_) => ActionResult::Failed(
                            "the driver task ended before answering".to_owned(),
                        ),
                    }
                };
                self.actions
                    .finish(device, action, key, now_mono_ns, result.clone());
                let (tag, failure) = outcome_tag(&result);
                self.log_action(
                    generation,
                    device,
                    action,
                    key,
                    now_mono_ns,
                    writer,
                    tag,
                    failure,
                );
                Ok(Invoked::Ran(result))
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "each parameter is a distinct field of ADR 0058's `action` decision-log record; a wrapper struct would only rename this list"
    )]
    fn log_action(
        &self,
        generation: u64,
        device: &str,
        action: &str,
        key: &str,
        at_mono_ns: u64,
        writer: &str,
        outcome: &str,
        failure: Option<&str>,
    ) {
        let mut m = Map::new();
        m.insert("gen".into(), json!(generation));
        m.insert("record".into(), json!("action"));
        m.insert("device".into(), json!(device));
        m.insert("action".into(), json!(action));
        m.insert("key".into(), json!(key));
        m.insert("at_mono_ns".into(), json!(at_mono_ns));
        m.insert("writer".into(), json!(writer));
        m.insert("outcome".into(), json!(outcome));
        if let Some(f) = failure {
            m.insert("failure".into(), json!(f));
        }
        m.insert("durability".into(), json!("immediate"));
        m.insert("ts".into(), json!(crate::wire::timestamp(utc_now_ns())));
        let _ = lock(&self.log).append(m, true);
    }
}

fn utc_now_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The plan of the running generation, for readers.
pub fn plan_of(c: &Controller) -> Option<Arc<Plan>> {
    c.current().map(|(_, _, r)| Arc::clone(&r.plan))
}

#[cfg(test)]
mod action_tests {
    use super::*;
    use crate::bridge::DeviceActionPlan;
    use crate::decisions::DecisionLog;
    use crate::devices::NoDevices;
    use crate::native::NativeTasks;
    use crate::pacer::{Monotonic, SystemPacer};
    use std::sync::Mutex;

    /// A device plan with one `reset_fault` action (no profile file
    /// needed: `Controller::switch`'s own catalog lookup is CWD-relative
    /// and cannot see a test's tempdir, so the device-with-action plan is
    /// built directly here, the same shape `bridge::device_plan` already
    /// produces from a real profile — see `tests/bridge.rs`).
    fn plan_with_one_action() -> Plan {
        let mut fs = MemoryFileSystem::default();
        fs.file(
            "joker.toml",
            "[project]\nname = \"t\"\njoker = \"1.0\"\nmode = \"dev\"\n",
        );
        let loaded =
            load(&fs, Path::new("joker.toml"), &NativeTasks).unwrap_or_else(|e| panic!("{e}"));
        let mut plan = loaded.plan;
        plan.devices.push(crate::bridge::DevicePlan {
            name: "pcs".into(),
            driver: "modbus-rtu".into(),
            bus: None,
            bus_kind: None,
            bus_params: Vec::new(),
            config: Vec::new(),
            quirks: Vec::new(),
            timeout_ns: 1_000_000_000,
            fallback_keepalive_ns: None,
            points: Vec::new(),
            actions: vec![DeviceActionPlan {
                name: "reset_fault".into(),
                timeout_ns: 1_000_000_000,
                dedup_window_ns: None,
                role: "operator".into(),
                steps: Vec::new(),
            }],
        });
        plan
    }

    struct FakeDeviceSet {
        sender: tokio::sync::mpsc::Sender<crate::drivers::ActionCall>,
    }

    impl DeviceSet for FakeDeviceSet {
        fn stop(self: Box<Self>) {}

        fn action_sender(
            &self,
            device: &str,
        ) -> Option<tokio::sync::mpsc::Sender<crate::drivers::ActionCall>> {
            (device == "pcs").then(|| self.sender.clone())
        }
    }

    /// A controller with an `Active` generation whose device has a
    /// declared action and a real (thread-backed) action channel.
    fn controller_with_action(dir: &std::path::Path) -> Controller {
        let log = Arc::new(Mutex::new(
            DecisionLog::open(&dir.join("decisions.jsonl")).unwrap(),
        ));
        let clock = Monotonic::start();
        let mut c = Controller::new(
            dir,
            Arc::clone(&log),
            Arc::new(NativeTasks),
            Arc::new(NoDevices),
            Box::new(move || Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced))),
            Config::default(),
        )
        .unwrap();

        let plan = Arc::new(plan_with_one_action());
        let running = Running::start(
            Arc::clone(&plan),
            Vec::new(),
            Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced)),
            Arc::clone(&log),
            Start {
                generation: 1,
                inherit: Default::default(),
                retention: Duration::from_secs(60),
                max_ticks: None,
                utc_origin_ns: 0,
                cpus: Vec::new(),
            },
        )
        .unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::drivers::ActionCall>(4);
        std::thread::spawn(move || {
            while let Some(call) = rx.blocking_recv() {
                let _ = call.reply.send(crate::drivers::ActionOutcome::Ok);
            }
        });
        c.active = Some(Active {
            number: 1,
            hash: "sha256:test".into(),
            running,
            devices: Box::new(FakeDeviceSet { sender: tx }),
        });
        c
    }

    /// Like [`plan_with_one_action`], but the manifest also declares a
    /// real `pi.act_led` bool signal, and `reset_fault`'s `permit` is
    /// wired to reference it directly (`Plan::permits`) — for proving
    /// `invoke_action` evaluates a real `permit` rather than defaulting
    /// to always-permitted.
    fn plan_with_permit_on_act_led() -> Plan {
        let mut fs = MemoryFileSystem::default();
        fs.file(
            "joker.toml",
            "[project]\nname = \"t\"\njoker = \"1.0\"\nmode = \"dev\"\n\n\
             [outputs.\"pi.act_led\"]\nsafe = false\n\n\
             [devices.pi]\ndriver = \"sim\"\n\n\
             [devices.pi.signals]\nact_led = { type = \"bool\", direction = \"output\" }\n",
        );
        let loaded =
            load(&fs, Path::new("joker.toml"), &NativeTasks).unwrap_or_else(|e| panic!("{e}"));
        let mut plan = loaded.plan;
        plan.devices.push(crate::bridge::DevicePlan {
            name: "pcs".into(),
            driver: "modbus-rtu".into(),
            bus: None,
            bus_kind: None,
            bus_params: Vec::new(),
            config: Vec::new(),
            quirks: Vec::new(),
            timeout_ns: 1_000_000_000,
            fallback_keepalive_ns: None,
            points: Vec::new(),
            actions: vec![DeviceActionPlan {
                name: "reset_fault".into(),
                timeout_ns: 1_000_000_000,
                dedup_window_ns: None,
                role: "operator".into(),
                steps: Vec::new(),
            }],
        });
        let program = crate::expr_plan::compile_bool(
            "pi.act_led",
            &plan,
            joker_units::Duration::from_millis(20),
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        plan.permits
            .insert(("pcs".to_owned(), "reset_fault".to_owned()), program);
        plan
    }

    /// A controller with an `Active` generation whose `pcs.reset_fault`
    /// has a real, compiled `permit` referencing `pi.act_led`, already
    /// set to `write_led`.
    fn controller_with_permit(dir: &std::path::Path, write_led: bool) -> Controller {
        let log = Arc::new(Mutex::new(
            DecisionLog::open(&dir.join("decisions.jsonl")).unwrap(),
        ));
        let clock = Monotonic::start();
        let mut c = Controller::new(
            dir,
            Arc::clone(&log),
            Arc::new(NativeTasks),
            Arc::new(NoDevices),
            Box::new(move || Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced))),
            Config::default(),
        )
        .unwrap();

        let plan = Arc::new(plan_with_permit_on_act_led());
        let running = Running::start(
            Arc::clone(&plan),
            Vec::new(),
            Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced)),
            Arc::clone(&log),
            Start {
                generation: 1,
                inherit: Default::default(),
                retention: Duration::from_secs(60),
                max_ticks: None,
                utc_origin_ns: 0,
                cpus: Vec::new(),
            },
        )
        .unwrap();
        let id = plan.id("pi.act_led").unwrap();
        running.shared.slots[id.index()].write(
            u64::from(write_led),
            joker_core::Quality::GOOD,
            joker_core::value::NO_TS_SRC,
            0,
        );

        let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::drivers::ActionCall>(4);
        std::thread::spawn(move || {
            while let Some(call) = rx.blocking_recv() {
                let _ = call.reply.send(crate::drivers::ActionOutcome::Ok);
            }
        });
        c.active = Some(Active {
            number: 1,
            hash: "sha256:test".into(),
            running,
            devices: Box::new(FakeDeviceSet { sender: tx }),
        });
        c
    }

    #[test]
    fn a_true_permit_admits_the_invocation() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = controller_with_permit(dir.path(), true);
        let result = c
            .invoke_action("pcs", "reset_fault", "k1", 0, "user:test")
            .unwrap();
        assert_eq!(result, Invoked::Ran(ActionResult::Succeeded));
    }

    #[test]
    fn a_false_permit_rejects_the_invocation_without_running_anything() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = controller_with_permit(dir.path(), false);
        let result = c
            .invoke_action("pcs", "reset_fault", "k1", 0, "user:test")
            .unwrap();
        assert_eq!(
            result,
            Invoked::Rejected(crate::actions::Rejected::NotPermitted)
        );
    }

    #[test]
    fn invoking_an_action_with_no_generation_running_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Mutex::new(
            DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
        ));
        let clock = Monotonic::start();
        let mut c = Controller::new(
            dir.path(),
            log,
            Arc::new(NativeTasks),
            Arc::new(NoDevices),
            Box::new(move || Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced))),
            Config::default(),
        )
        .unwrap();
        let err = c
            .invoke_action("pcs", "reset_fault", "k1", 0, "user:test")
            .unwrap_err();
        assert_eq!(err.code, "J-E1015");
    }

    #[test]
    fn invoking_an_unknown_device_or_action_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = controller_with_action(dir.path());
        let err = c
            .invoke_action("nope", "reset_fault", "k1", 0, "user:test")
            .unwrap_err();
        assert_eq!(err.code, "J-E1400");
        let err = c
            .invoke_action("pcs", "nope", "k1", 0, "user:test")
            .unwrap_err();
        assert_eq!(err.code, "J-E1400");
    }

    #[test]
    fn an_admitted_action_runs_against_the_registered_driver_and_is_logged() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = controller_with_action(dir.path());

        let result = c
            .invoke_action("pcs", "reset_fault", "k1", 0, "user:test")
            .unwrap();
        assert_eq!(result, Invoked::Ran(ActionResult::Succeeded));

        // A second, differently-keyed call admits and runs again.
        let result = c
            .invoke_action("pcs", "reset_fault", "k2", 1, "user:test")
            .unwrap();
        assert_eq!(result, Invoked::Ran(ActionResult::Succeeded));

        // The same key, still inside the dedup window, is served from
        // cache and does not run again.
        let result = c
            .invoke_action("pcs", "reset_fault", "k1", 2, "user:test")
            .unwrap();
        assert_eq!(result, Invoked::Duplicate(ActionResult::Succeeded));

        let entries =
            DecisionLog::read_all(&dir.path().join("decisions.jsonl")).unwrap_or_default();
        let action_entries: Vec<_> = entries.iter().filter(|e| e["record"] == "action").collect();
        assert_eq!(action_entries.len(), 3);
        assert_eq!(action_entries[0]["outcome"], "ok");
        assert_eq!(action_entries[0]["device"], "pcs");
        assert_eq!(action_entries[0]["key"], "k1");
    }

    #[test]
    fn a_disconnected_device_fails_the_action_and_logs_it() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Mutex::new(
            DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
        ));
        let clock = Monotonic::start();
        let mut c = Controller::new(
            dir.path(),
            Arc::clone(&log),
            Arc::new(NativeTasks),
            Arc::new(NoDevices),
            Box::new(move || Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced))),
            Config::default(),
        )
        .unwrap();
        let plan = Arc::new(plan_with_one_action());
        let running = Running::start(
            Arc::clone(&plan),
            Vec::new(),
            Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced)),
            Arc::clone(&log),
            Start {
                generation: 1,
                inherit: Default::default(),
                retention: Duration::from_secs(60),
                max_ticks: None,
                utc_origin_ns: 0,
                cpus: Vec::new(),
            },
        )
        .unwrap();
        c.active = Some(Active {
            number: 1,
            hash: "sha256:test".into(),
            running,
            devices: Box::new(NoDevices),
        });

        let err = c
            .invoke_action("pcs", "reset_fault", "k1", 0, "user:test")
            .unwrap_err();
        assert_eq!(err.code, "J-E1415");

        let entries =
            DecisionLog::read_all(&dir.path().join("decisions.jsonl")).unwrap_or_default();
        let action_entries: Vec<_> = entries.iter().filter(|e| e["record"] == "action").collect();
        assert_eq!(action_entries.len(), 1);
        assert_eq!(action_entries[0]["outcome"], "failed");
    }
}

/// Runs `conformance/actions/*.action.toml` (AUR-418): M19 action cases
/// against a real `Controller::invoke_action`, loaded through a real
/// manifest and profile file, with a double only at the driver
/// boundary (ref/06 §2.10 delegates the wire protocol to the driver;
/// this suite is about the dispatch/decision-log mechanics above it).
/// Lives here, not as a `tests/*.rs` integration test, because it
/// needs `Active`/`Controller::active`, both private — see
/// `conformance/README.md`'s "Device actions" section for the file
/// format and why `cargo xtask conformance`'s core-only engine does
/// not run these (same reasoning as `link/`'s and `abi/`'s own
/// special-cased formats).
#[cfg(test)]
mod conformance_actions {
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use joker_manifest::OsFileSystem;
    use toml::{Table, Value as Toml};

    use super::*;
    use crate::decisions::DecisionLog;
    use crate::devices::{DeviceSet, NoDevices};
    use crate::drivers::{ActionCall, ActionOutcome};
    use crate::native::NativeTasks;
    use crate::pacer::{Monotonic, SystemPacer};
    use crate::runner::{Running, Start};

    struct FakeDeviceSet {
        sender: tokio::sync::mpsc::Sender<ActionCall>,
    }

    impl DeviceSet for FakeDeviceSet {
        fn stop(self: Box<Self>) {}

        fn action_sender(&self, _device: &str) -> Option<tokio::sync::mpsc::Sender<ActionCall>> {
            Some(self.sender.clone())
        }
    }

    /// A background "driver": answers every `ActionCall` with `Ok`,
    /// unless `should_fail` is set, in which case it answers `Failed`.
    fn spawn_driver() -> (tokio::sync::mpsc::Sender<ActionCall>, Arc<AtomicBool>) {
        let should_fail = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&should_fail);
        let (tx, mut rx) = tokio::sync::mpsc::channel::<ActionCall>(4);
        std::thread::spawn(move || {
            while let Some(call) = rx.blocking_recv() {
                let outcome = if flag.load(Ordering::SeqCst) {
                    ActionOutcome::Failed("the double refused the write".to_owned())
                } else {
                    ActionOutcome::Ok
                };
                let _ = call.reply.send(outcome);
            }
        });
        (tx, should_fail)
    }

    fn toml_str<'a>(t: &'a Toml, key: &str) -> &'a str {
        t.get(key)
            .and_then(Toml::as_str)
            .unwrap_or_else(|| panic!("missing or non-string `{key}`"))
    }

    /// Runs every `[[case]]` of one suite file in order, against one
    /// `Controller` (so `after` — a later case replaying an earlier
    /// one's key — shares the same dispatcher and decision log).
    fn run_suite(path: &Path) {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let doc: Table = text
            .parse()
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let fixture = doc.get("fixture").expect("suite has a [fixture]");
        let joker_toml = toml_str(fixture, "joker_toml");
        let profile_toml = toml_str(fixture, "profile_toml");

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("joker.toml"), joker_toml).unwrap();
        std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
        std::fs::write(
            dir.path().join("profiles/acme/pcs@1.profile.toml"),
            profile_toml,
        )
        .unwrap();

        let log = Arc::new(Mutex::new(
            DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
        ));
        let clock = Monotonic::start();
        let mut c = Controller::new(
            dir.path(),
            Arc::clone(&log),
            Arc::new(NativeTasks),
            Arc::new(NoDevices),
            Box::new(move || Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced))),
            Config::default(),
        )
        .unwrap();

        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            !loaded.report.has_errors(),
            "{}: {}",
            path.display(),
            loaded.report.render()
        );
        let plan = Arc::new(loaded.plan);
        let running = Running::start(
            Arc::clone(&plan),
            Vec::new(),
            Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced)),
            Arc::clone(&log),
            Start {
                generation: 1,
                inherit: Default::default(),
                retention: Duration::from_secs(60),
                max_ticks: None,
                utc_origin_ns: 0,
                cpus: Vec::new(),
            },
        )
        .unwrap();

        let (tx, should_fail) = spawn_driver();
        c.active = Some(Active {
            number: 1,
            hash: "sha256:conformance".into(),
            running,
            devices: Box::new(FakeDeviceSet { sender: tx }),
        });

        let cases = doc
            .get("case")
            .and_then(Toml::as_array)
            .unwrap_or_else(|| panic!("{}: no [[case]] entries", path.display()));
        for (i, case) in cases.iter().enumerate() {
            let name = toml_str(case, "name");
            let device = toml_str(case, "device");
            let action = toml_str(case, "action");
            let key = toml_str(case, "key");
            let driver = toml_str(case, "driver");
            should_fail.store(driver == "fails", Ordering::SeqCst);

            let result = c
                .invoke_action(device, action, key, i as u64, "user:conformance")
                .unwrap_or_else(|e| panic!("{}/{name}: {e}", path.display()));

            let expect = case.get("expect").expect("case has [case.expect]");
            let expected_state = toml_str(expect, "state");
            let actual_state = match &result {
                Invoked::Ran(ActionResult::Succeeded)
                | Invoked::Duplicate(ActionResult::Succeeded) => "succeeded",
                Invoked::Ran(
                    ActionResult::TimedOut | ActionResult::Device(_) | ActionResult::Failed(_),
                )
                | Invoked::Duplicate(
                    ActionResult::TimedOut | ActionResult::Device(_) | ActionResult::Failed(_),
                ) => "failed",
                Invoked::Rejected(_) => "rejected",
            };
            assert_eq!(
                actual_state,
                expected_state,
                "{}/{name}: expected state `{expected_state}`, got `{actual_state}`",
                path.display()
            );
            if expect.get("replay").and_then(Toml::as_bool) == Some(true) {
                assert!(
                    matches!(result, Invoked::Duplicate(_)),
                    "{}/{name}: expected a dedup replay, got {result:?}",
                    path.display()
                );
            }
        }

        let entries =
            DecisionLog::read_all(&dir.path().join("decisions.jsonl")).unwrap_or_default();
        let action_entries: Vec<_> = entries.iter().filter(|e| e["record"] == "action").collect();
        assert_eq!(
            action_entries.len(),
            cases.len(),
            "{}: every invocation must enter the decision log exactly once (invariant 8)",
            path.display()
        );
        for (case, entry) in cases.iter().zip(&action_entries) {
            let expect = case.get("expect").unwrap();
            assert_eq!(entry["record"], toml_str(expect, "decision_kind"));
            assert_eq!(entry["outcome"], toml_str(expect, "outcome"));
        }
    }

    #[test]
    fn conformance_action_cases_pass() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/actions");
        let mut ran = 0;
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                run_suite(&path);
                ran += 1;
            }
        }
        assert!(ran > 0, "no conformance/actions/*.toml suite found");
    }
}
