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

use crate::apps::{APP_CWASM, NodeApps, is_prod};
use crate::blobs::{Blobs, digest_of, parse_digest};
use crate::catalog::{self, CatalogFiles};
use crate::decisions::{SharedLog, lock};
use crate::devices::{DeviceSet, Devices};
use crate::generations::{
    Activation, DescriptorArtifact, GenArtifact, GenState, Generation, Origin, Store,
    descriptor_hash,
};
use crate::load::{LoadError, Loaded, instantiate, load_with};
use crate::pacer::Pacer;
use crate::plan::Plan;
use crate::runner::{Running, Start, StartContext};
use crate::task::{Providers, TaskProvider};
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
    /// The node's clock quality, which [`Controller::refresh_clock`]
    /// re-evaluates under the running generation's `[clock]` (ADR 0069).
    pub clock: Arc<crate::clock::ClockMonitor>,
    /// The project a proposal that carries no profile or board artifact
    /// takes them from (`profiles/`, `boards/` under it, `jk dev` and the
    /// conformance runners): they become artifacts of the generation, so
    /// every activation runs with the files it was proposed with. Empty is
    /// no project: a generation has only the profiles and boards it
    /// carries, never those of the daemon's working directory.
    pub project_root: std::path::PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_window: Duration::from_secs(30),
            max_window: Duration::from_secs(24 * 3600),
            retention: crate::trace_ring::DEFAULT_RETENTION,
            utc_origin_ns: 0,
            clock: Arc::default(),
            project_root: std::path::PathBuf::new(),
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
    blobs: Blobs,
    log: SharedLog,
    tasks: Arc<dyn TaskProvider>,
    apps: Option<Arc<NodeApps>>,
    devices: Arc<dyn Devices>,
    pacer: PacerFactory,
    config: Config,
    active: Option<Active>,
    confirming: Option<Confirming>,
    hub: Arc<crate::actions::Hub>,
    permit_state: std::collections::BTreeMap<(String, String), Vec<u64>>,
    heartbeat: Arc<crate::supervisor::Heartbeat>,
    persistent: crate::persistent::Persistent,
    clock_policy: crate::clock::ClockPolicy,
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

/// Checks and plans a generation's manifest with exactly the profiles and
/// boards it carries (`files`), never with files found on the disk
/// around the daemon.
fn loaded_of(
    manifest: &str,
    tasks: &dyn TaskProvider,
    files: &CatalogFiles,
) -> Result<Loaded, ControlError> {
    let mut fs = MemoryFileSystem::default();
    fs.file(EFFECTIVE, manifest);
    for path in app_paths(manifest) {
        fs.dir(path);
    }
    load_with(&fs, Path::new(EFFECTIVE), tasks, files).map_err(|e| validation(&e))
}

/// The profile and board files among a generation's `artifacts` (kinds
/// `joker.profile.v1+toml` and `joker.board.v1+toml`), read from the blob
/// store by digest. A name that is not a profile or board path is
/// `J-E1012`; a blob that is missing, is not UTF-8 or no longer has its
/// digest is `J-E1602`: a generation never runs with files other than its
/// own.
fn catalog_of(blobs: &Blobs, artifacts: &[GenArtifact]) -> Result<CatalogFiles, ControlError> {
    let mut files = CatalogFiles::new();
    for a in artifacts
        .iter()
        .filter(|a| a.kind == catalog::PROFILE_KIND || a.kind == catalog::BOARD_KIND)
    {
        if !catalog::is_catalog_path(&a.name) || catalog::kind_of(&a.name) != a.kind {
            return Err(ControlError::new(
                "J-E1012",
                400,
                format!("`{}` is not a {} path", a.name, a.kind),
            ));
        }
        let digest = parse_digest(&a.digest).ok_or_else(|| {
            ControlError::new("J-E1012", 400, format!("bad digest `{}`", a.digest))
        })?;
        let missing = |detail: String| ControlError::new("J-E1602", 422, detail);
        let bytes = blobs
            .get(&digest)
            .map_err(|e| missing(format!("`{}` ({}): {e}", a.name, a.digest)))?;
        if digest_of(&bytes) != a.digest {
            return Err(missing(format!(
                "`{}` no longer matches its digest {}",
                a.name, a.digest
            )));
        }
        let text =
            String::from_utf8(bytes).map_err(|_| missing(format!("`{}` is not UTF-8", a.name)))?;
        files.insert(a.name.clone(), text);
    }
    Ok(files)
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
    generation_hash_with(loaded, lock, &[])
}

/// The descriptor hash of an effective manifest, its lock and the
/// artifacts it refers to. A `.cwasm` is derived from its `.wasm` and stays
/// out of the descriptor, as it stays out of the lock (ADR 0067).
pub fn generation_hash_with(loaded: &Loaded, lock: &str, artifacts: &[GenArtifact]) -> String {
    let artifacts: Vec<DescriptorArtifact> = artifacts
        .iter()
        .filter(|a| a.kind != APP_CWASM)
        .filter_map(|a| {
            parse_digest(&a.digest).map(|digest| DescriptorArtifact {
                name: a.name.clone(),
                media_type: a.kind.clone(),
                digest,
            })
        })
        .collect();
    let canon = loaded
        .report
        .effective
        .as_ref()
        .map(joker_manifest::effective::cbor);
    descriptor_hash(
        canon.as_deref().unwrap_or(&[0xA0]),
        lock_digest(lock),
        &artifacts,
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
        let first = pacer();
        let persistent = crate::persistent::Persistent::load(state_dir, first.now())
            .map_err(ControlError::io)?;
        let clock = first
            .monotonic()
            .unwrap_or_else(crate::pacer::Monotonic::start);
        let hub = Arc::new(crate::actions::Hub::new(Arc::clone(&log), clock));
        Ok(Self {
            store: Store::open(&state_dir.join("generations")).map_err(ControlError::io)?,
            blobs: Blobs::new(state_dir),
            log,
            tasks,
            apps: None,
            devices,
            pacer,
            config,
            active: None,
            confirming: None,
            hub,
            permit_state: std::collections::BTreeMap::new(),
            heartbeat: Arc::default(),
            persistent,
            clock_policy: crate::clock::ClockPolicy::default(),
        })
    }

    /// Runs the Wasm apps of each generation from its artifacts (ADR 0067);
    /// without it, the artifacts are stored and the apps come from the
    /// fixed task provider alone (`jk dev`, tests).
    pub fn with_apps(mut self, apps: Arc<NodeApps>) -> Self {
        self.apps = Some(apps);
        self
    }

    fn provider(
        &self,
        manifest: &str,
        artifacts: &[GenArtifact],
    ) -> Result<Arc<dyn TaskProvider>, ControlError> {
        let Some(apps) = &self.apps else {
            return Ok(Arc::clone(&self.tasks));
        };
        let wasm = apps.provider(artifacts, is_prod(manifest))?;
        let mut providers = Providers::default();
        providers.push(Box::new(wasm));
        providers.push(Box::new(Arc::clone(&self.tasks)));
        Ok(Arc::new(providers))
    }

    /// The persisted trips (docs/01 §7).
    pub fn trips(&self) -> Arc<crate::trips::TripTable> {
        self.persistent.trips()
    }

    /// The manual levels held (docs/16 A13, A14).
    pub fn manual(&self) -> Arc<crate::manual::ManualTable> {
        self.persistent.manual()
    }

    /// The decision log.
    pub fn log(&self) -> SharedLog {
        Arc::clone(&self.log)
    }

    /// The heartbeat the supervisor of every generation beats, for the
    /// watchdog feed to read without this controller's lock (ADR 0061).
    pub fn heartbeat(&self) -> Arc<crate::supervisor::Heartbeat> {
        Arc::clone(&self.heartbeat)
    }

    /// The node's clock quality.
    pub fn clock(&self) -> Arc<crate::clock::ClockMonitor> {
        Arc::clone(&self.config.clock)
    }

    /// Re-evaluates the clock quality under the running generation's
    /// `[clock]` and, when it changed, records the change in the decision
    /// log (`clock`, ADR 0069). Reads the time daemon's state: call it
    /// from the service loop, never from the tick path.
    pub fn refresh_clock(&self) {
        self.refresh_clock_for(self.active.as_ref().map(|a| a.number));
    }

    fn refresh_clock_for(&self, generation: Option<u64>) {
        let now = utc_now_ns();
        let Some(t) = self.config.clock.refresh(self.clock_policy, now) else {
            return;
        };
        let state = self.config.clock.state();
        let mut m = Map::new();
        m.insert("gen".into(), generation.map_or(Value::Null, |g| json!(g)));
        m.insert("record".into(), json!("clock"));
        m.insert("quality".into(), json!(t.to.name()));
        m.insert("from".into(), json!(t.from.name()));
        m.insert(
            "last_sync".into(),
            state
                .last_sync_utc_ns
                .map_or(Value::Null, |ns| json!(crate::wire::timestamp(ns))),
        );
        m.insert("ts".into(), json!(crate::wire::timestamp(now)));
        if let Err(e) = crate::decisions::lock(&self.log).append(m, true) {
            eprintln!("jokerd: decision log: {e}");
        }
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
        self.propose_with(manifest, lock, &[], message, dry_run, created_by)
    }

    /// Puts profile and board files into the blob store and returns them as
    /// artifacts (named by their path, with the media type of their kind
    /// and the digest of their bytes). A name that is not a profile or
    /// board path is `J-E1601`.
    fn store_catalog(&self, files: &CatalogFiles) -> Result<Vec<GenArtifact>, ControlError> {
        let mut out = Vec::with_capacity(files.len());
        for (name, text) in files {
            if !catalog::is_catalog_path(name) {
                return Err(ControlError::new(
                    "J-E1601",
                    422,
                    format!("`{name}` is not a profile or board path"),
                ));
            }
            let digest = digest_of(text.as_bytes());
            let bytes =
                parse_digest(&digest).ok_or_else(|| ControlError::new("J-E1000", 500, "digest"))?;
            self.blobs.put(&bytes, text.as_bytes()).map_err(|e| {
                ControlError::new("J-E1000", 500, format!("blob of `{name}`: {e:?}"))
            })?;
            out.push(GenArtifact {
                name: name.clone(),
                kind: catalog::kind_of(name).to_owned(),
                digest,
            });
        }
        Ok(out)
    }

    /// [`Controller::propose_by`] with the profile and board files the
    /// generation carries, by their path relative to a project root
    /// (`profiles/acme/pcs@1.profile.toml`): they are stored as artifacts
    /// of the generation ([`Controller::propose_with`]).
    pub fn propose_files(
        &mut self,
        manifest: &str,
        lock: &str,
        files: &CatalogFiles,
        message: Option<String>,
        dry_run: bool,
        created_by: Option<String>,
    ) -> Result<Generation, ControlError> {
        let artifacts = self.store_catalog(files)?;
        self.propose_with(manifest, lock, &artifacts, message, dry_run, created_by)
    }

    /// [`Controller::propose_by`] with the artifacts the generation refers
    /// to (ref/09 §5.13): its Wasm apps are loaded and checked now, and the
    /// artifacts are stored with it.
    pub fn propose_with(
        &mut self,
        manifest: &str,
        lock: &str,
        artifacts: &[GenArtifact],
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
        let carries_catalog = artifacts
            .iter()
            .any(|a| a.kind == catalog::PROFILE_KIND || a.kind == catalog::BOARD_KIND);
        let mut artifacts = artifacts.to_vec();
        if !carries_catalog && !self.config.project_root.as_os_str().is_empty() {
            let (files, errors) = catalog::catalog_files(&self.config.project_root);
            if let Some(e) = errors.first() {
                return Err(ControlError::new("J-E1601", 422, e.clone()));
            }
            artifacts.extend(self.store_catalog(&files)?);
        }
        let artifacts = artifacts.as_slice();
        let files = catalog_of(&self.blobs, artifacts)?;
        let tasks = self.provider(manifest, artifacts)?;
        let loaded = loaded_of(manifest, tasks.as_ref(), &files)?;
        let hash = generation_hash_with(&loaded, lock, artifacts);
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
        let g = self
            .store
            .create(&hash, manifest, lock, message, created_by)
            .map_err(ControlError::io)?;
        if !artifacts.is_empty() {
            self.store
                .save_artifacts(g.number, artifacts)
                .map_err(ControlError::io)?;
        }
        Ok(g)
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
        self.log_traps();
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
        let artifacts = self.store.artifacts(number).map_err(ControlError::io)?;
        let provider = self.provider(&manifest, &artifacts)?;
        let files = catalog_of(&self.blobs, &artifacts)?;
        let loaded = loaded_of(&manifest, provider.as_ref(), &files)?;
        let tasks = instantiate(&loaded.plan, &loaded.manifest, provider.as_ref())
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
        self.clock_policy = crate::clock::ClockPolicy::of(&loaded.manifest);
        self.refresh_clock_for(Some(number));
        let plan = Arc::new(loaded.plan);
        let pacer = (self.pacer)();
        let restore = self
            .persistent
            .restore_for(&plan, pacer.now().mono_ns, &self.log, number);
        let running = Running::start_with(
            Arc::clone(&plan),
            tasks,
            pacer,
            Arc::clone(&self.log),
            Start {
                generation: number,
                inherit,
                retention: self.config.retention,
                max_ticks: None,
                utc_origin_ns: self.config.utc_origin_ns,
                cpus: crate::resources::rt_cpus(&loaded.manifest),
            },
            StartContext {
                heartbeat: Some(Arc::clone(&self.heartbeat)),
                trips: Some(self.persistent.trips()),
                restore,
            },
        )
        .map_err(|e| ControlError::new("J-E1000", 500, e.0))?;
        self.persistent.started(&self.log, number, utc_now_ns());
        let devices = self.devices.start(&plan, &running.shared);
        self.permit_state.clear();
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

    /// Writes the traps the cycle thread reported since the last poll to
    /// the decision log (`app_trap`, ADR 0067). The outputs of a trapped
    /// app are not touched here: its writer stops proposing, and they go to
    /// `safe` when its lease ends (docs/01 §9).
    pub fn log_traps(&self) {
        let Some(apps) = &self.apps else {
            return;
        };
        let traps = apps.traps();
        let generation = self.active.as_ref().map(|a| a.number);
        for t in traps.drain() {
            eprintln!(
                "jokerd: app `{}` trapped in task `{}` at tick {} ({})",
                t.app, t.task, t.tick, t.code
            );
            let mut m = Map::new();
            m.insert("gen".into(), generation.map_or(Value::Null, |g| json!(g)));
            m.insert("record".into(), json!("app_trap"));
            m.insert("app".into(), json!(&*t.app));
            m.insert("task".into(), json!(&*t.task));
            m.insert("code".into(), json!(t.code));
            m.insert("tick".into(), json!(t.tick));
            m.insert("at_mono_ns".into(), json!(t.at_mono_ns));
            m.insert(
                "ts".into(),
                json!(crate::wire::timestamp(
                    self.config
                        .utc_origin_ns
                        .saturating_add(i64::try_from(t.at_mono_ns).unwrap_or(i64::MAX))
                )),
            );
            if let Err(e) = lock(&self.log).append(m, true) {
                eprintln!("jokerd: decision log: {e}");
            }
        }
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

    /// The invocations of this node (ADR 0066).
    pub fn actions(&self) -> Arc<crate::actions::Hub> {
        Arc::clone(&self.hub)
    }

    /// Resolves `target`'s `action` in the running generation and
    /// evaluates its permissive at `now_mono_ns` (ref/09 §5.10, ADR 0060,
    /// ADR 0066): a device action, the action of a device's implicit
    /// instance (`[implements]`), or the action of a composite instance.
    /// An action without a declared `permit` is permitted; a declared one
    /// whose program is missing or fails to evaluate denies, naming why.
    pub fn action_target(
        &mut self,
        target: &str,
        action: &str,
        now_mono_ns: u64,
    ) -> Result<crate::actions::Target, ControlError> {
        self.resolve_action(target, action, now_mono_ns, true)
    }

    /// The value of `target`'s `action` permissive at `now_mono_ns`, for
    /// the catalog (`permit_now`): evaluated on a copy of its window
    /// state, which stays as the last invocation left it.
    pub fn permit_now(&mut self, target: &str, action: &str, now_mono_ns: u64) -> Option<bool> {
        self.resolve_action(target, action, now_mono_ns, false)
            .ok()
            .map(|t| t.permit.is_ok())
    }

    fn resolve_action(
        &mut self,
        target: &str,
        action: &str,
        now_mono_ns: u64,
        keep_state: bool,
    ) -> Result<crate::actions::Target, ControlError> {
        let active = self
            .active
            .as_ref()
            .ok_or_else(|| ControlError::new("J-E1015", 503, "no generation is running"))?;
        let plan = &active.running.plan;
        let unknown = || {
            ControlError::new(
                "J-E1400",
                404,
                format!("unknown action `{target}.{action}`"),
            )
        };
        let device_action = |device: &str, name: &str| {
            plan.devices
                .iter()
                .find(|d| d.name == device)
                .and_then(|d| d.actions.iter().find(|a| a.name == name))
        };
        let (device, instance, decl, permit_key) = if let Some(decl) = device_action(target, action)
        {
            (
                target.to_owned(),
                None,
                decl.clone(),
                (target, decl.name.as_str()),
            )
        } else if let Some(decl) = plan
            .devices
            .iter()
            .find(|d| d.name == target)
            .and_then(|d| {
                d.actions
                    .iter()
                    .find(|a| a.implements.iter().any(|m| m == action))
            })
        {
            (
                target.to_owned(),
                Some(target.to_owned()),
                decl.clone(),
                (target, decl.name.as_str()),
            )
        } else if let Some(i) = plan
            .instance_actions
            .iter()
            .find(|i| i.instance == target && i.action == action)
        {
            let mut decl = device_action(&i.device, &i.device_action)
                .cloned()
                .ok_or_else(unknown)?;
            if let Some(t) = i.timeout_ns {
                decl.timeout_ns = t;
            }
            if i.dedup_window_ns.is_some() {
                decl.dedup_window_ns = i.dedup_window_ns;
            }
            if let Some(r) = &i.role {
                decl.role.clone_from(r);
            }
            (
                i.device.clone(),
                Some(target.to_owned()),
                decl,
                (target, action),
            )
        } else {
            return Err(unknown());
        };
        let name = (permit_key.0.to_owned(), permit_key.1.to_owned());
        let permit = match (plan.permit_sources.get(&name), plan.permits.get(&name)) {
            (_, Some(program)) => {
                let kept = self.permit_state.entry(name.clone()).or_default();
                let mut scratch = kept.clone();
                let state = if keep_state { kept } else { &mut scratch };
                match crate::expr_plan::eval_bool(
                    program,
                    &active.running.shared,
                    now_mono_ns,
                    state,
                ) {
                    Ok(output) if output.permits(false) => Ok(()),
                    Ok(_) => Err(None),
                    Err(e) => Err(Some(format!("the permit could not be evaluated: {e:?}"))),
                }
            }
            (Some(_), None) => Err(Some(
                "the declared permit has no compiled program".to_owned(),
            )),
            (None, None) => Ok(()),
        };
        let tick = active
            .running
            .shared
            .groups
            .first()
            .map_or(0, |g| g.last_tick.get());
        Ok(crate::actions::Target {
            generation: active.running.generation,
            tick,
            target: target.to_owned(),
            action: action.to_owned(),
            instance,
            sender: active.devices.action_sender(&device),
            device,
            plan: decl,
            permit,
        })
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
    #![allow(clippy::unwrap_used, reason = "a broken fixture fails the test")]

    use super::*;
    use crate::actions::{InstanceAction, Request, Role, State};
    use crate::bridge::DeviceActionPlan;
    use crate::decisions::DecisionLog;
    use crate::devices::{DeviceSet, NoDevices};
    use crate::load::load;
    use crate::native::NativeTasks;
    use crate::pacer::{Monotonic, SystemPacer};
    use crate::runner::{Running, Start};
    use std::sync::Mutex;

    fn plan_with(permit: Option<&str>) -> Plan {
        let mut fs = MemoryFileSystem::default();
        fs.file(
            "joker.toml",
            "[project]\nname = \"t\"\njoker = \"1.0\"\nmode = \"dev\"\n\n\
             [outputs.\"pi.act_led\"]\nsafe = false\n\n\
             [devices.pi]\ndriver = \"sim\"\n\n\
             [devices.pi.signals]\nact_led = { type = \"bool\", direction = \"output\" }\n",
        );
        let mut plan = load(&fs, Path::new("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"))
            .plan;
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
            fallback_outputs: None,
            points: Vec::new(),
            actions: vec![DeviceActionPlan {
                name: "reset_fault".into(),
                timeout_ns: 1_000_000_000,
                role: "operator".into(),
                implements: vec!["reset".into()],
                ..DeviceActionPlan::default()
            }],
        });
        plan.instance_actions.push(InstanceAction {
            instance: "p1".into(),
            action: "clear".into(),
            device: "pcs".into(),
            device_action: "reset_fault".into(),
            role: Some("admin".into()),
            timeout_ns: Some(500_000_000),
            ..InstanceAction::default()
        });
        if let Some(permit) = permit {
            let program = crate::expr_plan::compile_bool(
                permit,
                &plan,
                joker_units::Duration::from_millis(20),
            )
            .unwrap_or_else(|e| panic!("{e:?}"));
            plan.permit_sources.insert(
                ("pcs".to_owned(), "reset_fault".to_owned()),
                permit.to_owned(),
            );
            plan.permits
                .insert(("pcs".to_owned(), "reset_fault".to_owned()), program);
        }
        plan
    }

    struct Answering(tokio::sync::mpsc::Sender<crate::drivers::ActionCall>);

    impl DeviceSet for Answering {
        fn stop(self: Box<Self>) {}

        fn action_sender(
            &self,
            device: &str,
        ) -> Option<tokio::sync::mpsc::Sender<crate::drivers::ActionCall>> {
            (device == "pcs").then(|| self.0.clone())
        }
    }

    fn controller(dir: &Path, plan: Plan, led: bool) -> Controller {
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
        let plan = Arc::new(plan);
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
            u64::from(led),
            joker_core::Quality::GOOD,
            joker_core::value::NO_TS_SRC,
            0,
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::drivers::ActionCall>(8);
        std::thread::spawn(move || {
            while let Some(call) = rx.blocking_recv() {
                let _ = call
                    .reply
                    .send(crate::drivers::ActionOutcome::Ok(serde_json::json!({})));
            }
        });
        c.active = Some(Active {
            number: 1,
            hash: "sha256:test".into(),
            running,
            devices: Box::new(Answering(tx)),
        });
        c
    }

    #[test]
    fn a_permit_is_evaluated_against_live_values() {
        let dir = tempfile::tempdir().unwrap();
        let mut on = controller(dir.path(), plan_with(Some("pi.act_led")), true);
        assert_eq!(
            on.action_target("pcs", "reset_fault", 0).unwrap().permit,
            Ok(())
        );
        let dir = tempfile::tempdir().unwrap();
        let mut off = controller(dir.path(), plan_with(Some("pi.act_led")), false);
        assert_eq!(
            off.action_target("pcs", "reset_fault", 0).unwrap().permit,
            Err(None)
        );
    }

    #[test]
    fn a_windowed_permit_keeps_its_state_and_the_catalog_does_not_advance_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = controller(dir.path(), plan_with(Some("held(pi.act_led, 1s)")), true);
        assert!(
            c.action_target("pcs", "reset_fault", 0)
                .unwrap()
                .permit
                .is_err()
        );
        assert_eq!(
            c.permit_now("pcs", "reset_fault", 2_000_000_000),
            Some(true)
        );
        assert!(
            c.action_target("pcs", "reset_fault", 500_000_000)
                .unwrap()
                .permit
                .is_err()
        );
        assert!(
            c.action_target("pcs", "reset_fault", 1_500_000_000)
                .unwrap()
                .permit
                .is_ok()
        );
    }

    #[test]
    fn a_declared_permit_without_a_program_denies_naming_why() {
        let dir = tempfile::tempdir().unwrap();
        let mut plan = plan_with(None);
        plan.permit_sources.insert(
            ("pcs".to_owned(), "reset_fault".to_owned()),
            "pi.act_led".to_owned(),
        );
        let mut c = controller(dir.path(), plan, true);
        let permit = c.action_target("pcs", "reset_fault", 0).unwrap().permit;
        assert!(
            permit
                .as_ref()
                .unwrap_err()
                .as_deref()
                .is_some_and(|m| m.contains("no compiled program")),
            "{permit:?}"
        );
    }

    #[test]
    fn unknown_targets_and_a_stopped_node_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = controller(dir.path(), plan_with(None), true);
        assert_eq!(
            c.action_target("nope", "reset_fault", 0).unwrap_err().code,
            "J-E1400"
        );
        assert_eq!(
            c.action_target("pcs", "nope", 0).unwrap_err().code,
            "J-E1400"
        );
        c.shutdown();
        assert_eq!(
            c.action_target("pcs", "reset_fault", 0).unwrap_err().code,
            "J-E1015"
        );
    }

    #[test]
    fn an_implicit_instance_action_runs_the_device_action_it_maps() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = controller(dir.path(), plan_with(None), true);
        let t = c.action_target("pcs", "reset", 0).unwrap();
        assert_eq!(t.instance.as_deref(), Some("pcs"));
        assert_eq!(t.device, "pcs");
        assert_eq!(t.plan.name, "reset_fault");
    }

    #[test]
    fn a_composite_instance_action_takes_its_own_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = controller(dir.path(), plan_with(None), true);
        let t = c.action_target("p1", "clear", 0).unwrap();
        assert_eq!(t.instance.as_deref(), Some("p1"));
        assert_eq!(t.device, "pcs");
        assert_eq!(t.plan.name, "reset_fault");
        assert_eq!(t.plan.role, "admin");
        assert_eq!(t.plan.timeout_ns, 500_000_000);
        assert!(t.sender.is_some());
    }

    #[test]
    fn an_invocation_runs_without_the_controller_lock_and_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let c = controller(dir.path(), plan_with(None), true);
        let hub = c.actions();
        let shared: crate::daemon::SharedController = Arc::new(Mutex::new(c));
        let request = Request {
            key: "k1".into(),
            wait: true,
            principal: "user:test".into(),
            role: Some(Role::Operator),
            ..Request::default()
        };
        let answer = crate::actions::invoke(&shared, &hub, "p1", "clear", request.clone());
        assert_eq!(answer.unwrap_err().code, "J-E1002");
        let answer = crate::actions::invoke(&shared, &hub, "pcs", "reset_fault", request).unwrap();
        let inv = hub.wait(&answer.id, crate::actions::MAX_WAIT).unwrap();
        assert_eq!(inv.state, State::Succeeded);
        let records: Vec<_> = DecisionLog::read_all(&dir.path().join("decisions.jsonl"))
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e["record"] == "action")
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["outcome"], "rejected_forbidden");
        assert_eq!(records[0]["instance"], "p1");
        assert_eq!(records[1]["outcome"], "ok");
        assert_eq!(records[1]["gen"], 1);
    }
}
