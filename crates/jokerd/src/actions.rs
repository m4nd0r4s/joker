//! Action invocations (M19, docs/02 §8, ref/09 §5.10, ADR 0058, ADR
//! 0066): who may invoke what, the arguments, the deduplication cache,
//! the device queue, the invocations themselves and their decision-log
//! records.
//!
//! The controller resolves the target and evaluates the permissive under
//! its own lock ([`crate::control::Controller::action_target`]); the
//! [`Hub`] does the rest under its own, and the program runs on the
//! device's io task ([`crate::action_exec`]). No lock is held while a
//! driver runs (ADR 0066 §11).

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant as StdInstant};

use joker_hal::interpreter::Scalar;
use joker_manifest::Manifest;
use joker_manifest::value::Node;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::action_exec::{ActionCall, ActionOutcome, ActionProgram, FailureKind};
use crate::bridge::{DeviceActionPlan, DevicePlan};
use crate::decisions::{SharedLog, lock};
use crate::pacer::Monotonic;
use crate::wire::{timestamp, utc_now_ns};

/// Admitted invocations per device, running and waiting (ADR 0066 §2).
pub const QUEUE_DEPTH: usize = 8;
/// Entries of the deduplication cache (ADR 0066 §3).
pub const DEDUP_CAPACITY: usize = 4096;
/// Invocations kept in memory (ADR 0066 §7).
pub const KEPT_INVOCATIONS: usize = 1024;
/// The longest a waiting caller waits (ADR 0066 §6).
pub const MAX_WAIT: Duration = Duration::from_secs(30);
/// `dedup_window` when the manifest declares none (document 16 E9).
pub const DEFAULT_DEDUP_WINDOW_NS: u64 = 60_000_000_000;

/// A role of the API, ordered (ref/09 §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Reads.
    Viewer,
    /// Operates.
    Operator,
    /// Administers.
    Admin,
}

impl Role {
    /// The role named `name`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "viewer" => Some(Self::Viewer),
            "operator" => Some(Self::Operator),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }
}

/// `[security.roles]` of `manifest`, for the API's single role resolver
/// (`joker_api::server::role_with`, ref/01 §5.20, ADR 0063, ADR 0066).
pub fn socket_roles(manifest: &Manifest) -> joker_api::server::SocketRoles {
    let roles = manifest
        .pending
        .get("security")
        .and_then(Node::as_table)
        .and_then(|t| t.get("roles"))
        .and_then(|e| e.node.as_table());
    let rule = |name: &str| {
        roles
            .and_then(|r| r.get(name))
            .map(|e| strings(Some(&e.node)))
    };
    joker_api::server::SocketRoles {
        dev: manifest.project.mode == joker_manifest::Mode::Dev,
        operator: rule("operator"),
        admin: rule("admin"),
    }
}

fn strings(node: Option<&Node>) -> Vec<String> {
    match node.map(|n| &n.value) {
        Some(joker_manifest::value::Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// One action of a composite instance (`[instances.<i>] map`, ADR 0066
/// §18): the class action `action` runs `device`'s `device_action`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstanceAction {
    /// The instance.
    pub instance: String,
    /// The class action.
    pub action: String,
    /// The device whose action runs.
    pub device: String,
    /// The device's action.
    pub device_action: String,
    /// `[instances.<i>.actions.<a>].timeout`.
    pub timeout_ns: Option<u64>,
    /// `[instances.<i>.actions.<a>].dedup_window`.
    pub dedup_window_ns: Option<u64>,
    /// `[instances.<i>.actions.<a>].role`, floored to `operator`.
    pub role: Option<String>,
    /// `[instances.<i>.actions.<a>].permit`.
    pub permit: Option<String>,
}

fn duration_ns(table: Option<&joker_manifest::value::Table>, key: &str) -> Option<u64> {
    let text = table?.get(key)?.node.as_str()?;
    joker_units::Duration::parse(text)
        .ok()
        .map(joker_units::Duration::as_nanos)
}

/// The actions of the composite instances of `manifest`: every `map`
/// member bound to `"<device>.<action>"` where `device` has that action
/// (ref/01 §5.13). Virtual and simulated instances have none here (ADR
/// 0066 §19).
pub fn instance_actions(manifest: &Manifest, devices: &[DevicePlan]) -> Vec<InstanceAction> {
    let Some(instances) = manifest.pending.get("instances").and_then(Node::as_table) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (instance, entry) in instances {
        let Some(decl) = entry.node.as_table() else {
            continue;
        };
        if decl.contains_key("implemented_by") {
            continue;
        }
        let Some(map) = decl.get("map").and_then(|e| e.node.as_table()) else {
            continue;
        };
        let config = decl.get("actions").and_then(|e| e.node.as_table());
        for (member, target) in map {
            let Some((device, action)) = target.node.as_str().and_then(|t| t.split_once('.'))
            else {
                continue;
            };
            let known = devices
                .iter()
                .any(|d| d.name == device && d.actions.iter().any(|a| a.name == action));
            if !known {
                continue;
            }
            let own = config
                .and_then(|c| c.get(member))
                .and_then(|e| e.node.as_table());
            let text = |key: &str| {
                own.and_then(|t| t.get(key))
                    .and_then(|e| e.node.as_str())
                    .map(str::to_owned)
            };
            out.push(InstanceAction {
                instance: instance.clone(),
                action: member.clone(),
                device: device.to_owned(),
                device_action: action.to_owned(),
                timeout_ns: duration_ns(own, "timeout"),
                dedup_window_ns: duration_ns(own, "dedup_window"),
                role: text("role").map(|r| {
                    if r == "viewer" {
                        "operator".to_owned()
                    } else {
                        r
                    }
                }),
                permit: text("permit"),
            });
        }
    }
    out
}

/// What the controller resolved for one invocation, under its own lock.
#[derive(Debug)]
pub struct Target {
    /// The generation running.
    pub generation: u64,
    /// The last tick of its first cycle group (ADR 0066 §14).
    pub tick: u64,
    /// The name invoked: the device, or the instance.
    pub target: String,
    /// The action invoked.
    pub action: String,
    /// The instance, for an instance action.
    pub instance: Option<String>,
    /// The device whose action runs.
    pub device: String,
    /// The device's action, with the timeout, `dedup_window` and role of
    /// whoever configures this invocation.
    pub plan: DeviceActionPlan,
    /// The permissive: `Ok` permits; `Err` denies, with the cause when
    /// it is not a plain `false` (ADR 0060).
    pub permit: Result<(), Option<String>>,
    /// The device's io task, when it runs.
    pub sender: Option<tokio::sync::mpsc::Sender<ActionCall>>,
}

/// One invocation request (ref/09 §5.10 `InvokeRequest`).
#[derive(Clone, Debug, Default)]
pub struct Request {
    /// The idempotency key.
    pub key: String,
    /// The arguments, by name.
    pub args: Map<String, Value>,
    /// A shorter timeout.
    pub timeout_ns: Option<u64>,
    /// Why.
    pub reason: Option<String>,
    /// Wait for a short action's result.
    pub wait: bool,
    /// Who.
    pub principal: String,
    /// Its role; `None` skips the check (an app's bound `Action` port,
    /// ref/01 §5.8.2).
    pub role: Option<Role>,
}

/// A request refused before anything ran (ref/09 §7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The registry code.
    pub code: &'static str,
    /// The HTTP status.
    pub status: u16,
    /// What happened.
    pub detail: String,
    /// The id its decision record carries, when it has one.
    pub invocation: Option<String>,
}

impl Refusal {
    /// A refusal with `code`, `status` and `detail`.
    pub fn new(code: &'static str, status: u16, detail: impl Into<String>) -> Self {
        Self {
            code,
            status,
            detail: detail.into(),
            invocation: None,
        }
    }
}

/// The shared state between the caller and the device's io task.
#[derive(Debug, Default)]
pub struct Ticket {
    cancel: AtomicBool,
    started: AtomicBool,
    started_utc_ns: AtomicI64,
    progress: Mutex<Option<(u16, String, i64)>>,
}

impl Ticket {
    /// Asks the program to stop.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// Whether it was asked to stop.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// The io task started the program.
    pub fn start(&self, utc_ns: i64) {
        self.started_utc_ns.store(utc_ns, Ordering::Release);
        self.started.store(true, Ordering::Release);
    }

    fn started_at(&self) -> Option<i64> {
        self.started
            .load(Ordering::Acquire)
            .then(|| self.started_utc_ns.load(Ordering::Acquire))
    }

    /// The program's progress, in permille.
    pub fn set_progress(&self, permille: u16, message: &str) {
        let mut p = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        *p = Some((permille.min(1000), message.to_owned(), utc_now_ns()));
    }

    fn progress(&self) -> Option<(u16, String, i64)> {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The state of an invocation (ADR 0066 §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Admitted, waiting in the device's queue.
    Queued,
    /// Running.
    Running,
    /// Completed.
    Succeeded,
    /// Did not complete.
    Failed,
    /// Asked to stop.
    Cancelling,
    /// Stopped.
    Cancelled,
}

impl State {
    /// Its name in the API.
    pub fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether it ended.
    pub fn finished(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// One invocation (ref/09 §5.10 `Invocation`).
#[derive(Clone, Debug)]
pub struct Invocation {
    /// Its id.
    pub id: String,
    /// The name invoked.
    pub target: String,
    /// The action invoked.
    pub action: String,
    /// The instance, for an instance action.
    pub instance: Option<String>,
    /// The device whose action runs.
    pub device: String,
    /// The device's action.
    pub device_action: String,
    /// The idempotency key.
    pub key: String,
    /// A long action.
    pub long: bool,
    /// Who.
    pub principal: String,
    /// Why.
    pub reason: Option<String>,
    /// The arguments, as received.
    pub args: Value,
    /// The generation it ran in.
    pub generation: u64,
    /// The tick of ADR 0066 §14.
    pub tick: u64,
    /// When it was admitted, on the daemon's monotonic clock.
    pub at_mono_ns: u64,
    /// When it was created.
    pub created_utc_ns: i64,
    /// When it started.
    pub started_utc_ns: Option<i64>,
    /// When it ended.
    pub finished_utc_ns: Option<i64>,
    /// Its state.
    pub state: State,
    /// Its progress, in permille, with its message and when.
    pub progress: Option<(u16, String, i64)>,
    /// What it produced.
    pub outcome: Option<ActionOutcome>,
    /// The error of the decision log, when its record was refused.
    pub log_error: Option<String>,
    /// Its record is in the decision log (or was refused).
    pub recorded: bool,
}

impl Invocation {
    /// The `Invocation` of the API; `replay` when it is returned to a
    /// later invocation with the same key.
    pub fn json(&self, replay: bool) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("target".into(), json!(self.target));
        m.insert("action".into(), json!(self.action));
        m.insert("key".into(), json!(self.key));
        m.insert("state".into(), json!(self.state.name()));
        m.insert("long".into(), json!(self.long));
        m.insert("principal".into(), json!(self.principal));
        if let Some(r) = &self.reason {
            m.insert("reason".into(), json!(r));
        }
        m.insert("args".into(), self.args.clone());
        m.insert("created_at".into(), json!(timestamp(self.created_utc_ns)));
        if let Some(t) = self.started_utc_ns {
            m.insert("started_at".into(), json!(timestamp(t)));
        }
        if let Some(t) = self.finished_utc_ns {
            m.insert("finished_at".into(), json!(timestamp(t)));
        }
        m.insert(
            "progress".into(),
            self.progress
                .as_ref()
                .map_or(Value::Null, |(permille, message, at)| {
                    json!({
                        "fraction": f64::from(*permille) / 1000.0,
                        "message": message,
                        "updated_at": timestamp(*at),
                    })
                }),
        );
        let (result, error) = match &self.outcome {
            Some(ActionOutcome::Ok(v)) => (v.clone(), Value::Null),
            Some(ActionOutcome::Failed { kind, detail }) => {
                let (kind, code) = kind.api();
                (
                    Value::Null,
                    json!({"kind": kind, "code": code, "message": detail}),
                )
            }
            None => (Value::Null, Value::Null),
        };
        m.insert("result".into(), result);
        m.insert("error".into(), error);
        m.insert("replay".into(), json!(replay));
        Value::Object(m)
    }
}

/// What [`Hub::invoke`] admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The invocation.
    pub id: String,
    /// A replay of an earlier invocation with the same key.
    pub replay: bool,
    /// The id this request's own decision record carries: `id`, or for a
    /// replay the id of the replay itself.
    pub record: String,
}

struct Entry {
    inv: Invocation,
    ticket: Arc<Ticket>,
}

impl Entry {
    fn view(&self) -> Invocation {
        let mut inv = self.inv.clone();
        if !inv.state.finished() {
            if let Some(at) = self.ticket.started_at() {
                inv.started_utc_ns = Some(at);
                if inv.state == State::Queued {
                    inv.state = State::Running;
                }
            }
            if inv.long {
                inv.progress = self.ticket.progress().or(inv.progress);
            }
        }
        inv
    }
}

struct Dedup {
    id: String,
    body: [u8; 32],
    window_ns: u64,
    ended_at: Option<u64>,
}

#[derive(Default)]
struct HubState {
    invocations: BTreeMap<String, Entry>,
    order: VecDeque<String>,
    dedup: BTreeMap<(String, String, String), Dedup>,
    queued: BTreeMap<String, usize>,
}

impl HubState {
    fn prune(&mut self, now: u64) {
        self.dedup.retain(|_, d| match d.ended_at {
            None => true,
            Some(end) => now.saturating_sub(end) < d.window_ns,
        });
    }

    fn evict(&mut self) {
        while self.order.len() > KEPT_INVOCATIONS {
            let position = self.order.iter().position(|id| {
                self.invocations
                    .get(id)
                    .is_none_or(|e| e.inv.state.finished() && e.inv.recorded)
            });
            let Some(position) = position else {
                return;
            };
            if let Some(id) = self.order.remove(position) {
                self.invocations.remove(&id);
            }
        }
    }
}

/// The invocations of a node: the deduplication cache, the per-device
/// queues and the registry behind `/v1/invocations` (ADR 0066).
pub struct Hub {
    state: Mutex<HubState>,
    changed: Condvar,
    log: SharedLog,
    clock: Monotonic,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub").finish_non_exhaustive()
    }
}

fn body_hash(args: &Map<String, Value>, timeout_ns: Option<u64>) -> [u8; 32] {
    let text =
        serde_json::to_string(&json!({"args": args, "timeout": timeout_ns})).unwrap_or_default();
    Sha256::digest(text.as_bytes()).into()
}

fn number_arg(name: &str, value: &Value, ty: &str) -> Result<Scalar, String> {
    let mut parts = ty.splitn(2, ' ');
    let _base = parts.next();
    let unit = parts.next().map(str::trim).filter(|u| !u.is_empty());
    match value {
        Value::Number(n) => n
            .as_f64()
            .map(Scalar::Float)
            .ok_or_else(|| format!("`{name}` is not a number")),
        Value::String(text) => {
            let unit = unit.ok_or_else(|| format!("`{name}` takes a plain number"))?;
            let declared = joker_units::Unit::parse(unit)
                .map_err(|e| format!("`{name}`: the declared unit `{unit}`: {e:?}"))?;
            let quantity = joker_units::Quantity::parse(text)
                .map_err(|e| format!("`{name}`: `{text}` is not a quantity: {e:?}"))?;
            quantity
                .value_in(&declared)
                .map(Scalar::Float)
                .map_err(|e| format!("`{name}`: `{text}` is not in `{unit}`: {e:?}"))
        }
        _ => Err(format!("`{name}` is not a `{ty}`")),
    }
}

fn enum_arg(
    name: &str,
    value: &Value,
    ty: &str,
    variants: &BTreeMap<String, i64>,
) -> Result<Scalar, String> {
    match value {
        Value::String(variant) => variants
            .get(variant)
            .map(|i| Scalar::Int(*i))
            .ok_or_else(|| format!("`{variant}` is not a variant of `{ty}`")),
        Value::Number(n) => n
            .as_i64()
            .filter(|i| variants.values().any(|v| v == i))
            .map(Scalar::Int)
            .ok_or_else(|| format!("`{name}` is not a value of `{ty}`")),
        _ => Err(format!("`{name}` is not a `{ty}`")),
    }
}

/// Converts `given` against the declared `args` of `plan` (ADR 0066 §5).
pub fn convert_args(
    plan: &DeviceActionPlan,
    given: &Map<String, Value>,
) -> Result<Vec<(String, Scalar)>, String> {
    if let Some(extra) = given.keys().find(|k| !plan.args.contains_key(*k)) {
        return Err(format!("`{extra}` is not an argument of this action"));
    }
    let mut out = Vec::with_capacity(plan.args.len());
    for (name, ty) in &plan.args {
        let value = given
            .get(name)
            .ok_or_else(|| format!("the argument `{name}` is missing"))?;
        let scalar = match (ty.as_str(), value) {
            ("bool", Value::Bool(b)) => Scalar::Bool(*b),
            ("u64" | "u32" | "u16" | "u8", Value::Number(n)) => n
                .as_u64()
                .map(Scalar::Uint)
                .ok_or_else(|| format!("`{name}` is not a non-negative integer"))?,
            ("i64" | "i32" | "i16" | "i8", Value::Number(n)) => n
                .as_i64()
                .map(Scalar::Int)
                .ok_or_else(|| format!("`{name}` is not an integer"))?,
            (t, v) if t.starts_with("f64") || t.starts_with("f32") => number_arg(name, v, t)?,
            (t, v) => match plan.enums.get(t) {
                Some(variants) => enum_arg(name, v, t, variants)?,
                None => return Err(format!("`{name}` is not a `{t}`")),
            },
        };
        out.push((name.clone(), scalar));
    }
    Ok(out)
}

fn native_args(args: &[(String, Scalar)]) -> Vec<u8> {
    use joker_hal::cbor::Value as C;
    C::Map(
        args.iter()
            .map(|(n, v)| {
                (
                    n.clone(),
                    match *v {
                        Scalar::Bool(b) => C::Bool(b),
                        Scalar::Int(i) => C::Int(i),
                        Scalar::Uint(u) => i64::try_from(u).map_or(C::Float(u as f64), C::Int),
                        Scalar::Float(f) => C::Float(f),
                    },
                )
            })
            .collect(),
    )
    .encode()
}

fn ulid() -> String {
    joker_api::server::ulid(u128::try_from(utc_now_ns()).unwrap_or(0))
}

/// Resolves `target`'s `action` under `controller`'s lock, releases it,
/// and admits, replays or refuses `request` through `hub` (ADR 0066 §1,
/// §11).
pub fn invoke(
    controller: &crate::daemon::SharedController,
    hub: &Arc<Hub>,
    target: &str,
    action: &str,
    request: Request,
) -> Result<Answer, Refusal> {
    let now = hub.now();
    let resolved = crate::daemon::lock_controller(controller)
        .action_target(target, action, now)
        .map_err(|e| Refusal::new(e.code, e.status, e.detail))?;
    hub.invoke(resolved, request)
}

impl Hub {
    /// The daemon's monotonic clock, now.
    pub fn now(&self) -> u64 {
        self.clock.now_ns()
    }

    /// A hub that records into `log`, on the daemon's `clock`.
    pub fn new(log: SharedLog, clock: Monotonic) -> Self {
        Self {
            state: Mutex::new(HubState::default()),
            changed: Condvar::new(),
            log,
            clock,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HubState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Admits, replays or refuses `request` for `target` (ADR 0066 §1),
    /// and on admission hands the program to the device's io task.
    pub fn invoke(self: &Arc<Self>, target: Target, request: Request) -> Result<Answer, Refusal> {
        let now = self.clock.now_ns();
        let mut inv = Invocation {
            id: ulid(),
            target: target.target.clone(),
            action: target.action.clone(),
            instance: target.instance.clone(),
            device: target.device.clone(),
            device_action: target.plan.name.clone(),
            key: request.key.clone(),
            long: target.plan.long,
            principal: request.principal.clone(),
            reason: request.reason.clone(),
            args: Value::Object(request.args.clone()),
            generation: target.generation,
            tick: target.tick,
            at_mono_ns: now,
            created_utc_ns: utc_now_ns(),
            started_utc_ns: None,
            finished_utc_ns: None,
            state: State::Queued,
            progress: None,
            outcome: None,
            log_error: None,
            recorded: false,
        };
        let required = Role::parse(&target.plan.role).unwrap_or(Role::Operator);
        if let Some(role) = request.role
            && role < required
        {
            return self.refuse(
                &inv,
                "rejected_forbidden",
                Refusal::new(
                    "J-E1002",
                    403,
                    format!(
                        "`{}.{}` requires the role `{}`; the caller is `{}`",
                        inv.target,
                        inv.action,
                        required.name(),
                        role.name()
                    ),
                ),
            );
        }
        let args = match convert_args(&target.plan, &request.args) {
            Ok(a) => a,
            Err(e) => {
                return self.refuse(&inv, "rejected_invalid", Refusal::new("J-E1401", 400, e));
            }
        };
        let timeout_ns = match request.timeout_ns {
            Some(t) if t > target.plan.timeout_ns => {
                return self.refuse(
                    &inv,
                    "rejected_invalid",
                    Refusal::new(
                        "J-E1401",
                        400,
                        "the request's `timeout` may shorten the configured one, not lengthen it",
                    ),
                );
            }
            Some(t) => t,
            None => target.plan.timeout_ns,
        };
        if let Err(cause) = &target.permit {
            let detail = cause
                .clone()
                .unwrap_or_else(|| "the action's permissive is false".to_owned());
            return self.refuse(
                &inv,
                "rejected_not_permitted",
                Refusal::new("J-E1402", 409, detail),
            );
        }
        let window_ns = target
            .plan
            .dedup_window_ns
            .unwrap_or(DEFAULT_DEDUP_WINDOW_NS);
        let body = body_hash(&request.args, request.timeout_ns);
        let dedup_key = (inv.target.clone(), inv.action.clone(), inv.key.clone());
        let mut state = self.lock();
        state.prune(now);
        if let Some(d) = state.dedup.get(&dedup_key) {
            if d.body != body {
                drop(state);
                return self.refuse(
                    &inv,
                    "rejected_conflict",
                    Refusal::new(
                        "J-E1024",
                        422,
                        "the idempotency key was used with other arguments",
                    ),
                );
            }
            let id = d.id.clone();
            let original = state.invocations.get(&id).map(Entry::view);
            drop(state);
            if let Some(original) = original {
                self.record_replay(&inv, &original)?;
            }
            return Ok(Answer {
                id,
                replay: true,
                record: inv.id,
            });
        }
        if state.dedup.len() >= DEDUP_CAPACITY {
            drop(state);
            return self.refuse(
                &inv,
                "rejected_busy",
                Refusal::new("J-E1403", 503, "the deduplication cache is full"),
            );
        }
        let queued = state.queued.get(&inv.device).copied().unwrap_or(0);
        if queued >= QUEUE_DEPTH {
            drop(state);
            return self.refuse(
                &inv,
                "rejected_busy",
                Refusal::new("J-E1403", 503, "device action queue full"),
            );
        }
        let ticket = Arc::new(Ticket::default());
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        let program = match &target.plan.native {
            Some(_) => ActionProgram::Native {
                id: target.plan.native_id,
                args: native_args(&args),
            },
            None => ActionProgram::Steps(target.plan.steps.clone()),
        };
        let call = ActionCall {
            program,
            args,
            timeout_ns,
            ticket: Arc::clone(&ticket),
            reply,
        };
        let sent = match &target.sender {
            None => Err(false),
            Some(sender) => sender
                .try_send(call)
                .map_err(|e| matches!(e, tokio::sync::mpsc::error::TrySendError::Full(_))),
        };
        if sent == Err(true) {
            drop(state);
            return self.refuse(
                &inv,
                "rejected_busy",
                Refusal::new("J-E1403", 503, "device action queue full"),
            );
        }
        let id = inv.id.clone();
        let not_connected = sent.is_err().then(|| {
            ActionOutcome::failed(
                FailureKind::NotConnected,
                format!("device `{}` is not connected", inv.device),
            )
        });
        state.dedup.insert(
            dedup_key,
            Dedup {
                id: id.clone(),
                body,
                window_ns,
                ended_at: None,
            },
        );
        *state.queued.entry(inv.device.clone()).or_default() += 1;
        inv.outcome = None;
        state.order.push_back(id.clone());
        state.invocations.insert(id.clone(), Entry { inv, ticket });
        drop(state);
        match not_connected {
            Some(outcome) => self.complete(&id, outcome),
            None => {
                let hub = Arc::clone(self);
                let waiter = id.clone();
                let spawned = std::thread::Builder::new()
                    .name("joker-action".into())
                    .spawn(move || {
                        let outcome = reply_rx.blocking_recv().unwrap_or_else(|_| {
                            ActionOutcome::failed(
                                FailureKind::NotConnected,
                                "the device's driver stopped before the action ended",
                            )
                        });
                        hub.complete(&waiter, outcome);
                    });
                if let Err(e) = spawned {
                    self.complete(
                        &id,
                        ActionOutcome::failed(
                            FailureKind::Unsupported,
                            format!("the invocation cannot be followed: {e}"),
                        ),
                    );
                }
            }
        }
        Ok(Answer {
            record: id.clone(),
            id,
            replay: false,
        })
    }

    fn refuse(
        &self,
        inv: &Invocation,
        outcome: &str,
        mut refusal: Refusal,
    ) -> Result<Answer, Refusal> {
        let mut m = self.record_of(inv, outcome);
        m.insert("code".into(), json!(refusal.code));
        m.insert("failure".into(), json!(refusal.detail));
        self.append(m)?;
        refusal.invocation = Some(inv.id.clone());
        Err(refusal)
    }

    fn record_replay(&self, inv: &Invocation, original: &Invocation) -> Result<(), Refusal> {
        let outcome = match &original.outcome {
            Some(ActionOutcome::Ok(_)) => "ok",
            Some(ActionOutcome::Failed { .. }) => "failed",
            None => "in_progress",
        };
        let mut m = self.record_of(inv, outcome);
        m.insert("duplicate_of".into(), json!(original.id));
        m.insert("duplicate_at_mono_ns".into(), json!(original.at_mono_ns));
        Self::outcome_fields(&mut m, original.outcome.as_ref());
        self.append(m)
    }

    fn outcome_fields(m: &mut Map<String, Value>, outcome: Option<&ActionOutcome>) {
        match outcome {
            Some(ActionOutcome::Ok(result)) => {
                m.insert("result".into(), result.clone());
            }
            Some(ActionOutcome::Failed { kind, detail }) => {
                let (k, code) = kind.api();
                m.insert("kind".into(), json!(k));
                m.insert("code".into(), json!(code));
                m.insert("failure".into(), json!(detail));
            }
            None => {}
        }
    }

    fn record_of(&self, inv: &Invocation, outcome: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("gen".into(), json!(inv.generation));
        m.insert("record".into(), json!("action"));
        m.insert("invocation".into(), json!(inv.id));
        m.insert("target".into(), json!(inv.target));
        m.insert("device".into(), json!(inv.device));
        if let Some(i) = &inv.instance {
            m.insert("instance".into(), json!(i));
        }
        m.insert("action".into(), json!(inv.action));
        if inv.device_action != inv.action {
            m.insert("device_action".into(), json!(inv.device_action));
        }
        m.insert("key".into(), json!(inv.key));
        m.insert("tick".into(), json!(inv.tick));
        m.insert("at_mono_ns".into(), json!(inv.at_mono_ns));
        m.insert("writer".into(), json!(inv.principal));
        m.insert("outcome".into(), json!(outcome));
        m.insert("args".into(), inv.args.clone());
        if let Some(r) = &inv.reason {
            m.insert("reason".into(), json!(r));
        }
        m.insert("durability".into(), json!("immediate"));
        m.insert("ts".into(), json!(timestamp(utc_now_ns())));
        m
    }

    fn append(&self, m: Map<String, Value>) -> Result<(), Refusal> {
        lock(&self.log)
            .append(m, true)
            .map(|_| ())
            .map_err(|e| Refusal::new("J-E1000", 500, format!("decision log: {e}")))
    }

    /// Records the outcome of invocation `id`, frees its place in the
    /// queue and starts its deduplication window.
    pub fn complete(&self, id: &str, outcome: ActionOutcome) {
        let now = self.clock.now_ns();
        let snapshot = {
            let mut state = self.lock();
            let Some(entry) = state.invocations.get_mut(id) else {
                return;
            };
            let started = entry.ticket.started_at();
            let progress = entry.ticket.progress();
            let inv = &mut entry.inv;
            inv.started_utc_ns = inv.started_utc_ns.or(started);
            if inv.long {
                inv.progress = progress.or(inv.progress.take());
            }
            inv.finished_utc_ns = Some(utc_now_ns());
            inv.state = match &outcome {
                ActionOutcome::Ok(_) => State::Succeeded,
                ActionOutcome::Failed {
                    kind: FailureKind::Cancelled,
                    ..
                } => State::Cancelled,
                ActionOutcome::Failed { .. } => State::Failed,
            };
            inv.outcome = Some(outcome);
            let inv = inv.clone();
            if let Some(n) = state.queued.get_mut(&inv.device) {
                *n = n.saturating_sub(1);
            }
            if let Some(d) =
                state
                    .dedup
                    .get_mut(&(inv.target.clone(), inv.action.clone(), inv.key.clone()))
                && d.id == inv.id
            {
                d.ended_at = Some(now);
            }
            inv
        };
        let tag = match &snapshot.outcome {
            Some(ActionOutcome::Ok(_)) => "ok",
            _ => "failed",
        };
        let mut m = self.record_of(&snapshot, tag);
        Self::outcome_fields(&mut m, snapshot.outcome.as_ref());
        let logged = self.append(m);
        if let Err(e) = &logged {
            eprintln!(
                "jokerd: invocation {} of `{}.{}`: {}",
                snapshot.id, snapshot.target, snapshot.action, e.detail
            );
        }
        let mut state = self.lock();
        if let Some(entry) = state.invocations.get_mut(id) {
            entry.inv.recorded = true;
            entry.inv.log_error = logged.err().map(|e| e.detail);
        }
        state.evict();
        drop(state);
        self.changed.notify_all();
    }

    /// Invocation `id`, as it stands.
    pub fn get(&self, id: &str) -> Option<Invocation> {
        self.lock().invocations.get(id).map(Entry::view)
    }

    /// Invocation `id` once it ended and its record was written, or as it
    /// stands after `max`.
    pub fn wait(&self, id: &str, max: Duration) -> Option<Invocation> {
        let deadline = StdInstant::now() + max;
        let mut state = self.lock();
        loop {
            let entry = state.invocations.get(id)?;
            if entry.inv.recorded {
                return Some(entry.view());
            }
            let left = deadline.saturating_duration_since(StdInstant::now());
            if left.is_zero() {
                return Some(entry.view());
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Waits until no invocation is queued or running, or `max` passes.
    pub fn settle(&self, max: Duration) -> bool {
        let deadline = StdInstant::now() + max;
        let mut state = self.lock();
        loop {
            if state.invocations.values().all(|e| e.inv.recorded) {
                return true;
            }
            let left = deadline.saturating_duration_since(StdInstant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Every invocation kept, newest first, that `keep` selects.
    pub fn list(&self, keep: impl Fn(&Invocation) -> bool) -> Vec<Invocation> {
        let state = self.lock();
        state
            .order
            .iter()
            .rev()
            .filter_map(|id| state.invocations.get(id).map(Entry::view))
            .filter(|i| keep(i))
            .collect()
    }

    /// Cancels the long invocation `id` (ADR 0066 §9).
    pub fn cancel(&self, id: &str) -> Result<Invocation, Refusal> {
        let mut state = self.lock();
        let entry = state
            .invocations
            .get_mut(id)
            .ok_or_else(|| Refusal::new("J-E1404", 404, format!("unknown invocation `{id}`")))?;
        if !entry.inv.long || entry.inv.state.finished() {
            return Err(Refusal::new(
                "J-E1405",
                409,
                "only a long action that has not ended can be cancelled",
            ));
        }
        entry.ticket.cancel();
        entry.inv.state = State::Cancelling;
        let view = entry.view();
        drop(state);
        self.changed.notify_all();
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a broken fixture fails the test")]

    use super::*;
    use crate::decisions::DecisionLog;

    fn plan(name: &str) -> DeviceActionPlan {
        DeviceActionPlan {
            name: name.into(),
            timeout_ns: 1_000_000_000,
            role: "operator".into(),
            ..DeviceActionPlan::default()
        }
    }

    fn target(
        plan: DeviceActionPlan,
        sender: Option<tokio::sync::mpsc::Sender<ActionCall>>,
    ) -> Target {
        Target {
            generation: 1,
            tick: 0,
            target: "pcs".into(),
            action: plan.name.clone(),
            instance: None,
            device: "pcs".into(),
            plan,
            permit: Ok(()),
            sender,
        }
    }

    fn request(key: &str) -> Request {
        Request {
            key: key.into(),
            wait: true,
            principal: "user:test".into(),
            role: Some(Role::Operator),
            ..Request::default()
        }
    }

    fn hub(dir: &std::path::Path) -> Arc<Hub> {
        let log = Arc::new(Mutex::new(
            DecisionLog::open(&dir.join("decisions.jsonl")).unwrap(),
        ));
        Arc::new(Hub::new(log, Monotonic::start()))
    }

    fn records(dir: &std::path::Path) -> Vec<Value> {
        std::fs::read_to_string(dir.join("decisions.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    fn answering(outcome: ActionOutcome) -> tokio::sync::mpsc::Sender<ActionCall> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<ActionCall>(QUEUE_DEPTH);
        std::thread::spawn(move || {
            while let Some(call) = rx.blocking_recv() {
                call.ticket.start(utc_now_ns());
                let _ = call.reply.send(outcome.clone());
            }
        });
        tx
    }

    #[test]
    fn a_role_below_the_action_role_is_refused_and_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let mut p = plan("reset");
        p.role = "admin".into();
        let refused = hub.invoke(target(p, None), request("k")).unwrap_err();
        assert_eq!(refused.code, "J-E1002");
        let r = records(dir.path());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["outcome"], "rejected_forbidden");
    }

    #[test]
    fn a_false_permissive_is_refused_with_its_cause() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let mut t = target(plan("reset"), None);
        t.permit = Err(Some("pcs.state is not fault".into()));
        let refused = hub.invoke(t, request("k")).unwrap_err();
        assert_eq!(refused.code, "J-E1402");
        assert_eq!(records(dir.path())[0]["failure"], "pcs.state is not fault");
    }

    #[test]
    fn an_admitted_invocation_completes_and_is_recorded_once() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let sender = answering(ActionOutcome::Ok(json!({"n": 1})));
        let answer = hub
            .invoke(target(plan("reset"), Some(sender)), request("k"))
            .unwrap();
        let inv = hub.wait(&answer.id, MAX_WAIT).unwrap();
        assert_eq!(inv.state, State::Succeeded);
        assert_eq!(inv.json(false)["result"]["n"], 1);
        let r = records(dir.path());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["outcome"], "ok");
        assert_eq!(r[0]["invocation"], json!(answer.id));
    }

    #[test]
    fn the_same_key_replays_and_another_body_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let sender = answering(ActionOutcome::Ok(json!({})));
        let first = hub
            .invoke(target(plan("reset"), Some(sender.clone())), request("k"))
            .unwrap();
        hub.wait(&first.id, MAX_WAIT);
        let again = hub
            .invoke(target(plan("reset"), Some(sender.clone())), request("k"))
            .unwrap();
        assert_eq!(again.id, first.id);
        assert!(again.replay);
        assert_ne!(again.record, first.id);
        let mut other = request("k");
        other.timeout_ns = Some(1);
        let refused = hub
            .invoke(target(plan("reset"), Some(sender)), other)
            .unwrap_err();
        assert_eq!(refused.code, "J-E1024");
        let r = records(dir.path());
        assert_eq!(r[1]["duplicate_of"], json!(first.id));
        assert_eq!(r[1]["outcome"], "ok");
        assert_eq!(r[2]["outcome"], "rejected_conflict");
    }

    #[test]
    fn the_same_key_after_the_window_runs_again() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let sender = answering(ActionOutcome::Ok(json!({})));
        let mut p = plan("reset");
        p.dedup_window_ns = Some(1);
        let first = hub
            .invoke(target(p.clone(), Some(sender.clone())), request("k"))
            .unwrap();
        hub.wait(&first.id, MAX_WAIT);
        std::thread::sleep(Duration::from_millis(2));
        let again = hub.invoke(target(p, Some(sender)), request("k")).unwrap();
        assert!(!again.replay);
        assert_ne!(again.id, first.id);
    }

    #[test]
    fn a_full_device_queue_is_busy() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let (tx, _rx) = tokio::sync::mpsc::channel::<ActionCall>(QUEUE_DEPTH + 4);
        for i in 0..QUEUE_DEPTH {
            hub.invoke(
                target(plan("reset"), Some(tx.clone())),
                request(&format!("k{i}")),
            )
            .unwrap();
        }
        let refused = hub
            .invoke(target(plan("reset"), Some(tx)), request("last"))
            .unwrap_err();
        assert_eq!(refused.code, "J-E1403");
    }

    #[test]
    fn a_device_without_an_io_task_fails_not_connected() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let answer = hub
            .invoke(target(plan("reset"), None), request("k"))
            .unwrap();
        let inv = hub.wait(&answer.id, MAX_WAIT).unwrap();
        assert_eq!(inv.state, State::Failed);
        assert_eq!(inv.json(false)["error"]["code"], "J-E1415");
    }

    #[test]
    fn arguments_are_checked_against_their_declared_types() {
        let mut p = plan("set");
        p.args.insert("level".into(), "f64 %".into());
        p.args.insert("on".into(), "bool".into());
        p.args.insert("mode".into(), "Mode".into());
        p.enums.insert(
            "Mode".into(),
            [("off".to_owned(), 0), ("auto".to_owned(), 3)].into(),
        );
        let given = |v: Value| v.as_object().cloned().unwrap_or_default();
        let ok = convert_args(
            &p,
            &given(json!({"level": "50 %", "on": true, "mode": "auto"})),
        )
        .unwrap();
        assert_eq!(
            ok,
            [
                ("level".to_owned(), Scalar::Float(50.0)),
                ("mode".to_owned(), Scalar::Int(3)),
                ("on".to_owned(), Scalar::Bool(true)),
            ]
        );
        assert!(convert_args(&p, &given(json!({"level": 1, "on": true}))).is_err());
        assert!(convert_args(&p, &given(json!({"level": 1, "on": 1, "mode": 0}))).is_err());
        assert!(
            convert_args(
                &p,
                &given(json!({"level": 1, "on": true, "mode": 0, "x": 1}))
            )
            .is_err()
        );
        assert!(convert_args(&p, &given(json!({"level": "5 s", "on": true, "mode": 0}))).is_err());
    }

    #[test]
    fn a_longer_request_timeout_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let mut r = request("k");
        r.timeout_ns = Some(2_000_000_000);
        let refused = hub.invoke(target(plan("reset"), None), r).unwrap_err();
        assert_eq!(refused.code, "J-E1401");
    }

    #[test]
    fn only_a_long_unfinished_invocation_can_be_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let hub = hub(dir.path());
        let (tx, _rx) = tokio::sync::mpsc::channel::<ActionCall>(QUEUE_DEPTH);
        let short = hub
            .invoke(target(plan("reset"), Some(tx.clone())), request("a"))
            .unwrap();
        assert_eq!(hub.cancel(&short.id).unwrap_err().code, "J-E1405");
        let mut p = plan("calibrate");
        p.long = true;
        let long = hub.invoke(target(p, Some(tx)), request("b")).unwrap();
        assert_eq!(hub.cancel(&long.id).map(|i| i.state), Ok(State::Cancelling));
        assert_eq!(hub.cancel("nope").unwrap_err().code, "J-E1404");
    }
}
