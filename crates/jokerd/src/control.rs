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
