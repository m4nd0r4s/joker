//! The daemon behind the local API: the M0 subset of ref/09 §5 over the
//! generation controller and the running generation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use joker_api::model::{
    ActivateRequest, BulkResetRequest, GenerationProposal, InvokeActionRequest, ManualWriteRequest,
    ReasonBody, ReleaseRequest, ResetTripRequest, RollbackRequest, TraceRequest,
};
use joker_api::{Backend, Headers, Principal, Problem, Query, Reply};
use joker_core::image::{Intent, NO_TASK, Proposal, RELINQUISH, Source as ProposalSource};
use joker_core::outcome::Outcome;
use joker_core::output::Band;
use joker_core::{Level, WriterKind};
use serde_json::{Map, Value, json};

use crate::actions::Rejected;
use crate::control::{ActionResult, ControlError, Invoked};
use crate::daemon::{SharedController, lock_controller};
use crate::decisions::{DecisionLog, SharedLog, StateWriteFailure, lock};
use crate::generations::{Activation, GenState, Generation, Origin};
use crate::manual::{ManualTable, Occupant, TicketBook};
use crate::pacer::Monotonic;
use crate::plan::{OutputPlan, Plan};
use crate::runner::Running;
use crate::shared::Shared;
use crate::state_file;
use crate::traces::{Source, TraceEntry, TraceState, Traces, record};
use crate::trips::TripTable;
use crate::wire::{self, duration, input_bits, parse_duration, timestamp, utc_now_ns};

/// The longest a write waits for its tick.
pub const MAX_WAIT: Duration = Duration::from_secs(30);

/// The daemon state the API serves.
pub struct Node {
    controller: SharedController,
    clock: Monotonic,
    utc_origin_ns: i64,
    boot_id: String,
    boot_count: u64,
    started_utc_ns: i64,
    name: String,
    state_dir: PathBuf,
    tickets: TicketBook,
    manual: Arc<ManualTable>,
    trips: Arc<TripTable>,
    traces: Arc<Traces>,
    log: SharedLog,
    quality: Arc<crate::clock::ClockMonitor>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node").field("name", &self.name).finish()
    }
}

fn problem(e: ControlError) -> Problem {
    let mut p = Problem::new(e.code, e.status, e.detail);
    p.errors = e
        .errors
        .into_iter()
        .map(|(code, message)| joker_api::problem::ProblemError { code, message })
        .collect();
    p
}

fn bad(code: &str, detail: impl Into<String>) -> Problem {
    Problem::new(code, 400, detail)
}

fn values_of(query: &Query, key: &str) -> Vec<String> {
    query
        .iter()
        .filter(|(k, _)| k == key)
        .flat_map(|(_, v)| v.split(',').map(str::to_owned).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .collect()
}

fn one(query: &Query, key: &str) -> Option<String> {
    query
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

fn user(who: &Principal) -> String {
    let name = who.id.strip_prefix("unix:").unwrap_or(&who.id);
    format!("user:{name}")
}

fn host_name() -> String {
    let raw = std::fs::read_to_string("/etc/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "node".into());
    let label: String = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(63)
        .collect();
    let label = label.trim_matches('-').to_owned();
    if label.is_empty() {
        "node".into()
    } else {
        label
    }
}

/// What the API reads of the running generation.
struct View {
    number: u64,
    hash: String,
    plan: Arc<Plan>,
    shared: Arc<Shared>,
    ring: crate::trace_ring::SharedRing,
}

impl Node {
    /// The API state of a daemon. The persisted trips and manual levels
    /// are the controller's, already restored into the running generation
    /// before its first tick (ADR 0061).
    pub fn new(
        controller: SharedController,
        clock: Monotonic,
        utc_origin_ns: i64,
        state_dir: &Path,
    ) -> std::io::Result<Self> {
        let (manual, trips, log, quality) = {
            let c = lock_controller(&controller);
            (c.manual(), c.trips(), c.log(), c.clock())
        };
        let boot_count = state_file::bump_counter(state_dir, "boot_count")?;
        Ok(Self {
            controller,
            clock,
            utc_origin_ns,
            boot_id: joker_api::server::ulid(u128::try_from(utc_now_ns()).unwrap_or(0)),
            boot_count,
            started_utc_ns: utc_now_ns(),
            name: host_name(),
            state_dir: state_dir.to_owned(),
            tickets: TicketBook::default(),
            manual,
            trips,
            traces: Arc::new(Traces::open(&state_dir.join("traces"))?),
            log,
            quality,
        })
    }

    /// Records a failure to write a state file in the decision log
    /// (`state_write_failed`, ADR 0070): the request that triggered it
    /// already took effect in the arbiter.
    fn persisted<T>(&self, failure: StateWriteFailure<'_>, r: std::io::Result<T>) -> Option<T> {
        r.map_err(|e| failure.report(&self.log, &e)).ok()
    }

    /// Records a trip reset requested through the API, with who asked and
    /// why, with `fsync` (ref/09 §5.7, ADR 0061), and returns its
    /// `ResetResult`.
    fn record_reset(
        &self,
        v: &View,
        output: &str,
        principal: &str,
        reason: &str,
        result: &Result<&'static str, Problem>,
    ) -> Value {
        let item = reset_item(output, result);
        let mut m = Map::new();
        m.insert("gen".into(), json!(v.number));
        m.insert("record".into(), json!("reset"));
        m.insert("output".into(), json!(output));
        m.insert("principal".into(), json!(principal));
        m.insert("reason".into(), json!(reason));
        m.insert("result".into(), item["result"].clone());
        if let Some(error) = item.get("error") {
            m.insert("error".into(), error.clone());
        }
        m.insert("ts".into(), json!(timestamp(utc_now_ns())));
        if let Err(e) = lock(&self.log).append(m, true) {
            eprintln!("jokerd: decision log: {e}");
        }
        item
    }

    fn view(&self) -> Option<View> {
        let c = lock_controller(&self.controller);
        c.current()
            .map(|(number, hash, r): (u64, &str, &Running)| View {
                number,
                hash: hash.to_owned(),
                plan: Arc::clone(&r.plan),
                shared: Arc::clone(&r.shared),
                ring: Arc::clone(&r.ring),
            })
    }

    fn need_view(&self) -> Result<View, Problem> {
        self.view()
            .ok_or_else(|| Problem::new("J-E1015", 503, "no generation is running"))
    }

    fn now_mono(&self) -> u64 {
        self.clock.now_ns()
    }

    fn clock_json(&self) -> Value {
        let state = self.quality.state();
        let mut clock = json!({
            "quality": state.quality.name(),
            "utc": timestamp(utc_now_ns()),
            "mono_ns": self.now_mono().to_string(),
        });
        if let Some(last) = state.last_sync_utc_ns {
            clock["last_sync"] = json!(timestamp(last));
            if state.quality == joker_core::ClockQuality::Holdover {
                clock["holdover_since"] = json!(timestamp(last));
            }
        }
        clock
    }

    fn generation_json(&self, g: &Generation) -> Value {
        let c = lock_controller(&self.controller);
        let activations: Vec<Activation> = c
            .store()
            .activations()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.number == g.number)
            .collect();
        let mut m = Map::new();
        m.insert("gen".into(), json!(g.number));
        m.insert("hash".into(), json!(g.hash));
        m.insert("state".into(), json!(g.state.name()));
        let origin = activations.last().map_or(Origin::Local, |a| a.origin);
        m.insert(
            "origin".into(),
            serde_json::to_value(origin).unwrap_or(json!("local")),
        );
        m.insert("created_at".into(), json!(timestamp(g.created_utc_ns)));
        m.insert(
            "created_by".into(),
            json!(g.created_by.clone().unwrap_or_else(|| "jokerd".into())),
        );
        if let Some(msg) = &g.message {
            m.insert("message".into(), json!(msg));
        }
        if let Some(a) = activations.last() {
            m.insert("activated_at".into(), json!(timestamp(a.at_utc_ns)));
        }
        if let Some((number, left)) = c.confirming()
            && number == g.number
        {
            let deadline = utc_now_ns().saturating_add(i64::try_from(left.as_nanos()).unwrap_or(0));
            m.insert("confirm_deadline".into(), json!(timestamp(deadline)));
        }
        if let Some(f) = &g.failure {
            let reason = if f.contains("window") {
                "window"
            } else if g.state == GenState::RolledBack {
                "rollback"
            } else {
                "activation"
            };
            m.insert("failure".into(), json!({"reason": reason, "detail": f}));
        }
        if g.state == GenState::Discarded {
            m.insert("discard_reason".into(), json!("superseded"));
        }
        m.insert(
            "activations".into(),
            Value::Array(
                activations
                    .iter()
                    .map(|a| {
                        let mut x = Map::new();
                        x.insert("at".into(), json!(timestamp(a.at_utc_ns)));
                        x.insert(
                            "origin".into(),
                            serde_json::to_value(a.origin).unwrap_or(Value::Null),
                        );
                        if let Some(p) = &a.principal {
                            x.insert("by".into(), json!(p));
                        }
                        if let Some(r) = &a.reason {
                            x.insert("reason".into(), json!(r));
                        }
                        if let Some(from) = a.from.and_then(|f| c.store().get(f).ok()) {
                            x.insert(
                                "from".into(),
                                json!({"gen": from.number, "hash": from.hash}),
                            );
                        }
                        Value::Object(x)
                    })
                    .collect(),
            ),
        );
        Value::Object(m)
    }

    fn resolve_generation(&self, selector: &str) -> Result<Generation, Problem> {
        let c = lock_controller(&self.controller);
        let store = c.store();
        let all = store
            .list()
            .map_err(|e| Problem::new("J-E1000", 500, e.to_string()))?;
        let found = match selector {
            "current" => c
                .current()
                .and_then(|(n, _, _)| all.iter().find(|g| g.number == n)),
            "pending" => all.iter().rev().find(|g| g.state == GenState::Pending),
            "confirming" => all.iter().rev().find(|g| g.state == GenState::Confirming),
            "previous" => {
                let current = c.current().map(|(n, _, _)| n);
                let acts = store.activations().unwrap_or_default();
                acts.iter()
                    .rev()
                    .filter(|a| Some(a.number) != current)
                    .find_map(|a| {
                        all.iter()
                            .find(|g| g.number == a.number && g.state == GenState::Confirmed)
                    })
            }
            s if s.starts_with("sha256:") => all.iter().rev().find(|g| g.hash == s),
            s => s
                .parse::<u64>()
                .ok()
                .and_then(|n| all.iter().find(|g| g.number == n)),
        };
        found
            .cloned()
            .ok_or_else(|| Problem::new("J-E1600", 404, format!("unknown generation `{selector}`")))
    }

    fn check_if_match(&self, if_match: Option<&str>) -> Result<(), Problem> {
        let Some(m) = if_match else {
            return Err(Problem::new("J-E1027", 428, "`If-Match` is required"));
        };
        let current = self.view().map(|v| v.hash);
        let ok = match &current {
            Some(h) => m == h || m == "*",
            None => m == "null" || m == "*" || m.is_empty(),
        };
        if ok {
            Ok(())
        } else {
            Err(
                Problem::new("J-E1026", 412, "the running generation changed")
                    .with("param", json!("If-Match")),
            )
        }
    }

    fn output_state(&self, v: &View, o: &OutputPlan) -> Value {
        let Some(signal) = v.plan.signal(&o.path) else {
            return Value::Null;
        };
        let id = v.plan.id(&o.path);
        let status = id.and_then(|id| v.shared.output(id)).and_then(|s| s.read());
        let safe = match o.safe {
            joker_core::output::SafeValue::Relinquish => json!("relinquish"),
            joker_core::output::SafeValue::Value(bits) => wire::value(signal, bits),
        };
        let (applied, level, outcome, tripped) = match status {
            Some((bits, meta, _)) => {
                let applied = if meta.relinquish {
                    json!("relinquish")
                } else {
                    wire::value(signal, bits)
                };
                (
                    applied,
                    meta.level,
                    Outcome::from_code(meta.outcome, meta.detail),
                    meta.tripped,
                )
            }
            None => (safe.clone(), 0, None, false),
        };
        let active_level = Level::new(level).map_or(json!("none"), |l| json!(l.to_string()));
        let now = self.now_mono();
        let mut levels = Vec::new();
        for (path, lvl, occ) in self.manual.all(now) {
            if path == o.path {
                levels.push(json!({
                    "level": format!("manual:{lvl}"),
                    "writer": occ.principal,
                    "kind": "manual",
                    "value": occ.value,
                    "lease_until": timestamp(occ.until_utc_ns),
                    "remaining": duration(occ.until_mono_ns.saturating_sub(now)),
                    "active": lvl == level,
                    "reason": occ.reason,
                }));
            }
        }
        if let Some(l) = Level::new(level)
            && l.band() != Band::Manual
            && let Some(w) = v.plan.writers.iter().find(|w| w.config.level == l)
        {
            levels.push(json!({
                "level": l.to_string(),
                "writer": w.name,
                "kind": kind_name(w.config.kind),
                "active": true,
            }));
        }
        let writers: Vec<Value> = o
            .writers
            .iter()
            .filter_map(|w| v.plan.writers.get(w.index()))
            .map(|w| {
                json!({
                    "writer": w.name,
                    "level": w.config.level.to_string(),
                    "kind": kind_name(w.config.kind),
                })
            })
            .collect();
        let mut m = Map::new();
        m.insert("path".into(), json!(o.path));
        m.insert("unit".into(), json!(o.unit));
        m.insert("type".into(), json!(wire::type_name(o.ty)));
        m.insert("group".into(), json!(o.group));
        m.insert("safe".into(), safe);
        m.insert("effective".into(), applied.clone());
        m.insert("applied".into(), applied);
        m.insert("active_level".into(), active_level);
        if let Some(out) = outcome {
            m.insert("last_outcome".into(), outcome_json(out, None));
        }
        m.insert("levels".into(), Value::Array(levels));
        m.insert("tripped".into(), json!(tripped));
        m.insert("writers".into(), Value::Array(writers));
        Value::Object(m)
    }

    fn manual_json(&self, v: &View, path: &str, lvl: u8, occ: &Occupant, now: u64) -> Value {
        let active = v
            .plan
            .id(path)
            .and_then(|id| v.shared.output(id))
            .and_then(|s| s.read())
            .is_some_and(|(_, meta, _)| meta.level == lvl);
        json!({
            "output": path,
            "level": format!("manual:{lvl}"),
            "writer": occ.principal,
            "principal": occ.principal,
            "value": occ.value,
            "reason": occ.reason,
            "ttl": duration(occ.ttl_ns),
            "since": timestamp(occ.since_utc_ns),
            "until": timestamp(occ.until_utc_ns),
            "remaining": duration(occ.until_mono_ns.saturating_sub(now)),
            "effective": active,
        })
    }

    fn stage(&self, v: &View, proposals: &[Proposal], group: &str) -> Result<(), Problem> {
        let g = v
            .plan
            .groups
            .iter()
            .position(|g| g.name == group)
            .ok_or_else(|| Problem::new("J-E1000", 500, "owner group missing"))?;
        let inbox = v
            .shared
            .inboxes
            .get(g)
            .ok_or_else(|| Problem::new("J-E1000", 500, "inbox missing"))?;
        for p in proposals {
            inbox
                .push(*p)
                .map_err(|_| Problem::new("J-E1014", 503, "the owner group's inbox is full"))?;
        }
        Ok(())
    }

    /// Queues a reset of `path` (owned by `group`) and, if `wait`,
    /// waits for its resolution (ref/09 §5.7). Shared by the single
    /// (`reset_trip`) and bulk (`reset_trips`) endpoints.
    fn reset_one(
        &self,
        v: &View,
        path: &str,
        group: &str,
        wait: bool,
    ) -> Result<&'static str, Problem> {
        let signal = v
            .plan
            .id(path)
            .ok_or_else(|| Problem::new("J-E1000", 500, "output without signal"))?;
        let group_index = v
            .plan
            .groups
            .iter()
            .position(|g| g.name == group)
            .ok_or_else(|| Problem::new("J-E1000", 500, "output's group not found"))?;
        let resets = v
            .shared
            .resets
            .get(group_index)
            .ok_or_else(|| Problem::new("J-E1000", 500, "output's group has no reset queue"))?;
        let ticket = self.tickets.number();
        resets
            .push(crate::shared::ResetRequest { signal, ticket })
            .map_err(|_| Problem::new("J-E1014", 503, "the reset queue is full"))?;
        if !wait {
            return Ok("queued");
        }
        match self
            .tickets
            .wait_reset(&v.shared, ticket, Duration::from_secs(5))
        {
            Some(outcome) => Ok(match outcome.kind {
                crate::shared::ResetKind::Cleared => {
                    self.persisted(
                        StateWriteFailure {
                            generation: Some(v.number),
                            file: "state/trips",
                            output: Some(path),
                            op: "reset",
                        },
                        self.trips.clear_tripped(path),
                    );
                    "cleared"
                }
                crate::shared::ResetKind::Retripped => "retripped",
                crate::shared::ResetKind::NotTripped => "not_tripped",
            }),
            None => Ok("queued"),
        }
    }

    /// The persisted trips of outputs the running generation does not
    /// declare, matching `sel` (all with an empty `sel`), with when each
    /// tripped (ADR 0070).
    fn kept_trips(&self, v: &View, sel: &[String]) -> Vec<(String, i64)> {
        self.trips
            .paths()
            .into_iter()
            .filter(|p| v.plan.output(p).is_none())
            .filter(|p| sel.is_empty() || sel.iter().any(|s| wire::matches(s, p)))
            .filter_map(|p| self.trips.since(&p).map(|since| (p, since)))
            .collect()
    }

    /// Resets a persisted trip of an output the running generation does
    /// not declare: there is no group to go through, so it only leaves
    /// `state/trips` (ADR 0070).
    fn reset_kept(&self, v: &View, path: &str) -> Result<&'static str, Problem> {
        self.trips.clear_tripped(path).map_err(|e| {
            StateWriteFailure {
                generation: Some(v.number),
                file: "state/trips",
                output: Some(path),
                op: "reset",
            }
            .report(&self.log, &e);
            Problem::new("J-E1000", 500, format!("cannot persist state/trips: {e}"))
        })?;
        Ok("cleared")
    }

    fn manual_level(text: Option<&str>) -> Result<u8, Problem> {
        match text {
            None => Ok(4),
            Some(t) => match Level::parse(t) {
                Some(l) if l.band() == Band::Manual => Ok(l.get()),
                _ => Err(bad("J-E1104", format!("`{t}` is not a manual level"))
                    .with("pointer", json!("/level"))),
            },
        }
    }
}

/// The `ResetResult` of one output (ref/09 §5.7): its result, or
/// `failed` with the problem's code and detail when it could not be reset
/// (ADR 0061).
fn reset_item(output: &str, result: &Result<&'static str, Problem>) -> Value {
    match result {
        Ok(r) => json!({"output": output, "result": r}),
        Err(p) => json!({
            "output": output,
            "result": "failed",
            "error": {"code": p.code, "detail": p.detail.clone().unwrap_or_default()},
        }),
    }
}

fn kind_name(kind: WriterKind) -> &'static str {
    match kind {
        WriterKind::Cycle => "cycle",
        WriterKind::Async => "async",
        WriterKind::Manual => "manual",
        WriterKind::Bridge => "bridge",
        WriterKind::Impl => "impl",
    }
}

/// An `ActionDecl` (openapi `#/components/schemas/ActionDecl`, ref/09
/// §5.10): the catalog entry `GET /v1/actions` lists. `args`/`result`/
/// `permit`/`permit_now` are omitted (optional in the schema): no
/// declared-argument schema is carried through `DeviceActionPlan` yet,
/// and `permit` is neither stored nor evaluated here (see
/// `Controller::invoke_action`'s own doc comment).
fn action_decl_json(target: &str, decl: &crate::bridge::DeviceActionPlan) -> Value {
    let mut m = json!({
        "target": target,
        "target_kind": "device",
        "action": decl.name,
        "role": decl.role,
        "timeout": duration(decl.timeout_ns),
        "long": false,
    });
    if let Some(ns) = decl.dedup_window_ns {
        m["dedup_window"] = json!(duration(ns));
    }
    m
}

/// An action result's `error.kind`/`code`, or `None` on success
/// (ref/04 §7.1's table: `TimedOut` → `timeout`/`J-E1406`, `Device` →
/// `device`/`J-E1407`, anything else the interpreter failed on →
/// `device`/`J-E1408`, the catch-all).
fn action_error(result: &ActionResult) -> Option<(&'static str, &'static str, String)> {
    match result {
        ActionResult::Succeeded => None,
        ActionResult::TimedOut => Some((
            "timeout",
            "J-E1406",
            "the action exceeded its timeout".to_owned(),
        )),
        ActionResult::Device(m) => Some(("device", "J-E1407", m.clone())),
        ActionResult::Failed(m) => Some(("device", "J-E1408", m.clone())),
    }
}

/// An `Invocation` (openapi `#/components/schemas/Invocation`, ref/09
/// §5.10): `error` is `(kind, code, message)` on failure, `None` on
/// success.
#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is a distinct required or near-required field of the openapi Invocation schema; a wrapper struct would only rename this list"
)]
fn invocation_json(
    target: &str,
    action: &str,
    key: &str,
    principal: &str,
    created_at: &str,
    replay: bool,
    state: &str,
    error: Option<(&str, &str, &str)>,
) -> Value {
    let id = joker_api::server::ulid(u128::try_from(utc_now_ns()).unwrap_or(0));
    json!({
        "id": id,
        "target": target,
        "action": action,
        "key": key,
        "state": state,
        "principal": principal,
        "created_at": created_at,
        "replay": replay,
        "result": if error.is_none() { json!({}) } else { Value::Null },
        "error": error.map_or(Value::Null, |(kind, code, message)| {
            json!({"kind": kind, "code": code, "message": message})
        }),
    })
}

fn outcome_json(outcome: Outcome, applied: Option<Value>) -> Value {
    let mut m = Map::new();
    m.insert("outcome".into(), json!(outcome.name()));
    if let Some(a) = applied {
        m.insert("applied".into(), a);
    }
    match outcome {
        Outcome::Rejected(r) => {
            m.insert("reason".into(), json!(r.name()));
        }
        Outcome::Overridden(l) => {
            m.insert("level".into(), json!(l.to_string()));
        }
        _ => {}
    }
    Value::Object(m)
}

impl Backend for Node {
    fn headers(&self) -> Headers {
        Headers {
            generation: self.view().map(|v| format!("{} {}", v.number, v.hash)),
            clock: self.quality.quality().name().to_owned(),
        }
    }

    fn info(&self, who: &Principal) -> Result<Reply, Problem> {
        let v = self.view();
        let mut m = Map::new();
        m.insert("node".into(), json!(self.name));
        m.insert(
            "runtime".into(),
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "boot_id": self.boot_id,
                "boot_count": self.boot_count,
                "started_at": timestamp(self.started_utc_ns),
            }),
        );
        m.insert(
            "api".into(),
            json!({"version": joker_api::server::API_VERSION, "base": "/v1"}),
        );
        m.insert(
            "mode".into(),
            json!(if v.as_ref().is_some_and(|v| v.plan.dev) {
                "dev"
            } else {
                "prod"
            }),
        );
        m.insert("arch".into(), json!(std::env::consts::ARCH));
        if let Some(v) = &v {
            m.insert(
                "generation".into(),
                json!({"gen": v.number, "hash": v.hash}),
            );
        } else {
            m.insert("generation".into(), Value::Null);
        }
        m.insert("clock".into(), self.clock_json());
        m.insert(
            "limits".into(),
            json!({"max_body": "16 MiB", "max_page": 1000}),
        );
        let mut features = Vec::new();
        if v.as_ref().is_some_and(|v| v.plan.dev) {
            features.push("sim");
        }
        m.insert("features".into(), json!(features));
        m.insert(
            "caller".into(),
            json!({
                "principal": {"id": user(who), "transport": who.transport, "uid": who.uid},
                "role": joker_api::server::role_of(who),
                "session": joker_api::server::ulid(u128::try_from(utc_now_ns()).unwrap_or(0)),
            }),
        );
        Ok(Reply::ok(Value::Object(m)))
    }

    fn health(&self, query: &Query) -> Result<Reply, Problem> {
        let probe = one(query, "probe").unwrap_or_else(|| "full".into());
        if !["live", "ready", "full"].contains(&probe.as_str()) {
            return Err(
                bad("J-E1012", format!("unknown probe `{probe}`")).with("param", json!("probe"))
            );
        }
        let v = self.view();
        let ticking = v
            .as_ref()
            .is_some_and(|v| v.shared.groups.iter().all(|g| g.ticks.get() > 0));
        let mut checks = vec![json!({"id": "process", "status": "ok"})];
        if probe != "live" {
            checks.push(json!({
                "id": "cycle",
                "status": if ticking { "ok" } else { "failed" },
                "detail": if ticking { "every group ticked" } else { "no tick yet" },
            }));
        }
        let failed = probe != "live" && !ticking;
        let body = json!({"status": if failed { "failed" } else { "ok" }, "checks": checks});
        Ok(Reply::with_status(if failed { 503 } else { 200 }, body))
    }

    fn status(&self) -> Result<Reply, Problem> {
        let v = self.view();
        let now = self.now_mono();
        let (generation, groups, apps, manual) = match &v {
            None => (Value::Null, Vec::new(), Vec::new(), Vec::new()),
            Some(v) => {
                let mut generation = json!({"gen": v.number, "hash": v.hash});
                if let Some((n, left)) = lock_controller(&self.controller).confirming()
                    && n == v.number
                {
                    let deadline =
                        utc_now_ns().saturating_add(i64::try_from(left.as_nanos()).unwrap_or(0));
                    generation["state"] = json!("confirming");
                    generation["confirm_deadline"] = json!(timestamp(deadline));
                } else {
                    generation["state"] = json!("confirmed");
                }
                let groups = v
                    .plan
                    .groups
                    .iter()
                    .zip(v.shared.groups.iter())
                    .map(|(g, s)| {
                        json!({
                            "name": g.name,
                            "period": duration(g.period_ns),
                            "overrun": "skip",
                            "tasks": g.tasks.iter().map(|t| json!({"app": t.app, "task": t.task})).collect::<Vec<_>>(),
                            "stats": {"ticks": s.ticks.get(), "overruns": s.overruns.get()},
                        })
                    })
                    .collect();
                let mut apps: Vec<String> = v
                    .plan
                    .groups
                    .iter()
                    .flat_map(|g| g.tasks.iter().map(|t| t.app.clone()))
                    .collect();
                apps.sort();
                apps.dedup();
                let apps = apps
                    .into_iter()
                    .map(|a| json!({"name": a, "version": "0.0.0", "state": "running"}))
                    .collect();
                let manual = self
                    .manual
                    .all(now)
                    .iter()
                    .map(|(p, l, o)| self.manual_json(v, p, *l, o, now))
                    .collect();
                (generation, groups, apps, manual)
            }
        };
        Ok(Reply::ok(json!({
            "node": self.name,
            "generation": generation,
            "clock": self.clock_json(),
            "groups": groups,
            "devices": [],
            "apps": apps,
            "alarms": {"active": 0, "unacked": 0, "items": []},
            "trips": [],
            "manual": manual,
        })))
    }

    fn metrics(&self) -> Result<Reply, Problem> {
        let groups: Vec<Value> = match self.view() {
            None => Vec::new(),
            Some(v) => v
                .plan
                .groups
                .iter()
                .zip(v.shared.groups.iter())
                .map(|(g, s)| {
                    json!({
                        "group": g.name,
                        "ticks": s.ticks.get(),
                        "overruns": s.overruns.get(),
                        // `max` is all this build tracks today (a running
                        // worst-case, not a histogram): `p50`/`p99`/`p999`/
                        // `window` are not computed, so they are simply
                        // absent — `Percentiles` has no required fields.
                        "jitter": {"max": duration(s.max_late_ns.get())},
                        "duration": {"max": duration(s.max_duration_ns.get())},
                    })
                })
                .collect(),
        };
        Ok(Reply::ok(json!({
            "at": timestamp(utc_now_ns()),
            "groups": groups,
        })))
    }

    fn signals(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let sel = values_of(query, "sel");
        let want_values = one(query, "values").is_some_and(|x| x == "true");
        let now = self.now_mono();
        let items: Vec<Value> = wire::select(&v.plan, &sel)
            .into_iter()
            .map(|s| self.signal_meta(&v, s, want_values, now))
            .collect();
        Ok(Reply::ok(json!({
            "generation": {"gen": v.number, "hash": v.hash},
            "registry": format!("sha256:{}", crate::generations::hex(&v.plan.registry_hash)),
            "items": items,
            "next_cursor": null,
        })))
    }

    fn signal(&self, path: &str) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let s = v
            .plan
            .signal(path)
            .ok_or_else(|| Problem::new("J0101", 404, format!("unknown signal `{path}`")))?;
        let mut meta = self.signal_meta(&v, s, true, self.now_mono());
        meta["path"] = json!(path);
        Ok(Reply::ok(meta))
    }

    fn values(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let sel = values_of(query, "sel");
        if sel.is_empty() {
            return Err(bad("J-E1013", "`sel` is required").with("param", json!("sel")));
        }
        let now = self.now_mono();
        let items: Vec<Value> = wire::select(&v.plan, &sel)
            .into_iter()
            .filter_map(|s| {
                let id = v.plan.id(&s.path)?;
                let sample = v.shared.slots.get(id.index())?.read(8).ok()?;
                Some(wire::sample(s, &sample, now, self.utc_origin_ns))
            })
            .collect();
        Ok(Reply::ok(json!({
            "generation": {"gen": v.number, "hash": v.hash},
            "read_at": timestamp(utc_now_ns()),
            "items": items,
        })))
    }

    fn outputs(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let sel = values_of(query, "sel");
        let items: Vec<Value> = v
            .plan
            .outputs
            .iter()
            .filter(|o| sel.is_empty() || sel.iter().any(|s| wire::matches(s, &o.path)))
            .map(|o| self.output_state(&v, o))
            .collect();
        Ok(Reply::ok(json!({"items": items, "next_cursor": null})))
    }

    fn output(&self, path: &str) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let o = v
            .plan
            .output(path)
            .ok_or_else(|| Problem::new("J-E1100", 404, format!("`{path}` is not an output")))?;
        let mut r = Reply::ok(self.output_state(&v, o));
        r.etag = Some(v.hash.clone());
        Ok(r)
    }

    fn manual_write(
        &self,
        who: &Principal,
        path: &str,
        req: ManualWriteRequest,
    ) -> Result<Reply, Problem> {
        if req.at.is_some() {
            return Err(Problem::new("J-E1017", 400, "`at` is reserved for 1.1"));
        }
        let v = self.need_view()?;
        let o = v
            .plan
            .output(path)
            .ok_or_else(|| Problem::new("J-E1100", 404, format!("`{path}` is not an output")))?
            .clone();
        let signal = v
            .plan
            .signal(&o.path)
            .ok_or_else(|| Problem::new("J-E1000", 500, "output without signal"))?
            .clone();
        let ttl_text = req.ttl.ok_or_else(|| {
            bad("J-E1101", "a manual write needs a TTL (A10)").with("pointer", json!("/ttl"))
        })?;
        let ttl = parse_duration(&ttl_text)
            .map_err(|e| bad("J-E1012", e).with("pointer", json!("/ttl")))?;
        if ttl < 1_000_000_000 {
            return Err(bad("J-E1103", "the TTL is below 1 s").with("pointer", json!("/ttl")));
        }
        if ttl > o.max_manual_ns {
            return Err(bad(
                "J-E1102",
                format!(
                    "the TTL is above max_manual ({})",
                    duration(o.max_manual_ns)
                ),
            )
            .with("pointer", json!("/ttl")));
        }
        let reason = req.reason.filter(|r| !r.is_empty()).ok_or_else(|| {
            bad("J-E1106", "a manual write needs a reason").with("pointer", json!("/reason"))
        })?;
        let level = Self::manual_level(req.level.as_deref())?;
        let bits = input_bits(&signal, &req.value)
            .map_err(|e| bad(e.code, e.detail).with("pointer", json!("/value")))?;
        let wait_timeout = match &req.wait_timeout {
            Some(t) => Duration::from_nanos(parse_duration(t).map_err(|e| bad("J-E1012", e))?),
            None => Duration::from_secs(5),
        };
        if wait_timeout > MAX_WAIT {
            return Err(bad("J-E1012", "`wait_timeout` is above 30s")
                .with("pointer", json!("/wait_timeout")));
        }
        let principal = user(who);
        let now = self.now_mono();
        let held = self.manual.get(&o.path, level, now);
        let renewed = held.as_ref().is_some_and(|h| h.principal == principal);
        let mut kind = "write";
        if let Some(h) = &held
            && !renewed
        {
            if !req.takeover {
                return Err(Problem::new(
                    "J-E1105",
                    409,
                    format!(
                        "manual:{level} is held by {} until {}: {}",
                        h.principal,
                        timestamp(h.until_utc_ns),
                        h.reason
                    ),
                )
                .with("holder", json!({"principal": h.principal, "until": timestamp(h.until_utc_ns), "reason": h.reason})));
            }
            kind = "takeover";
        }
        let writer = v
            .plan
            .writer(&format!("user:manual:{level}"))
            .ok_or_else(|| Problem::new("J-E1000", 500, "manual writer missing"))?;
        let n = self.tickets.number();
        let mut proposals = Vec::new();
        if kind == "takeover" {
            proposals.push(Proposal {
                writer,
                signal: v.plan.id(&o.path).unwrap_or(joker_core::SignalId(0)),
                value: 0,
                flags: RELINQUISH,
                source: ProposalSource::Async,
                ticket: 0,
                ttl_ns: 0,
                intent: Intent::Write,
                task: NO_TASK,
            });
        }
        proposals.push(Proposal {
            writer,
            signal: v.plan.id(&o.path).unwrap_or(joker_core::SignalId(0)),
            value: bits,
            flags: 0,
            source: ProposalSource::Async,
            ticket: n,
            ttl_ns: ttl,
            intent: Intent::Write,
            task: NO_TASK,
        });
        self.stage(&v, &proposals, &o.group)?;
        let since = utc_now_ns();
        let until = since.saturating_add(i64::try_from(ttl).unwrap_or(i64::MAX));
        let stored = self.manual.set(
            &o.path,
            level,
            Occupant {
                principal: principal.clone(),
                value: wire::value(&signal, bits),
                ttl_ns: ttl,
                since_utc_ns: since,
                until_utc_ns: until,
                until_mono_ns: now.saturating_add(ttl),
                reason: reason.clone(),
            },
            now,
        );
        self.persisted(
            StateWriteFailure {
                generation: Some(v.number),
                file: "state/manual",
                output: Some(&o.path),
                op: "set",
            },
            stored,
        );
        let id = joker_api::server::ulid(u128::try_from(since).unwrap_or(0));
        let mut ticket = json!({
            "id": id,
            "output": o.path,
            "kind": kind,
            "state": "queued",
            "writer": principal,
            "level": format!("manual:{level}"),
            "value": wire::value(&signal, bits),
            "ttl": duration(ttl),
            "lease_until": timestamp(until),
            "renewed": renewed,
            "generation": {"gen": v.number, "hash": v.hash},
        });
        let mut status = 202;
        if req.wait
            && let Some(r) = self.tickets.wait(&v.shared, n, wait_timeout)
        {
            status = 200;
            ticket["state"] = json!("committed");
            let applied = match r.applied {
                joker_core::envelope::Applied::Relinquish => json!("relinquish"),
                joker_core::envelope::Applied::Value(b) => wire::value(&signal, b),
            };
            ticket["outcome"] = outcome_json(r.outcome, Some(applied));
            ticket["tick"] = json!({"group": o.group, "seq": r.tick});
        }
        self.tickets.put(&id, ticket.clone());
        Ok(Reply::with_status(status, ticket))
    }

    fn release(&self, who: &Principal, path: &str, req: ReleaseRequest) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let o = v
            .plan
            .output(path)
            .ok_or_else(|| Problem::new("J-E1100", 404, format!("`{path}` is not an output")))?
            .clone();
        let principal = user(who);
        let now = self.now_mono();
        let level = match req.level.as_deref() {
            Some(l) => Self::manual_level(Some(l))?,
            None => {
                let mine: Vec<u8> = self
                    .manual
                    .all(now)
                    .into_iter()
                    .filter(|(p, _, occ)| *p == o.path && occ.principal == principal)
                    .map(|(_, l, _)| l)
                    .collect();
                match mine.as_slice() {
                    [l] => *l,
                    [] => {
                        return Err(Problem::new(
                            "J-E1107",
                            404,
                            "no manual level held on this output",
                        ));
                    }
                    _ => return Err(bad("J-E1012", "several levels held: give `level`")),
                }
            }
        };
        let held = self
            .manual
            .get(&o.path, level, now)
            .ok_or_else(|| Problem::new("J-E1107", 404, format!("manual:{level} is not held")))?;
        if held.principal != principal && !req.force {
            return Err(Problem::new(
                "J-E1108",
                409,
                format!("manual:{level} is held by {}; use `force`", held.principal),
            ));
        }
        let writer = v
            .plan
            .writer(&format!("user:manual:{level}"))
            .ok_or_else(|| Problem::new("J-E1000", 500, "manual writer missing"))?;
        let n = self.tickets.number();
        self.stage(
            &v,
            &[Proposal {
                writer,
                signal: v.plan.id(&o.path).unwrap_or(joker_core::SignalId(0)),
                value: 0,
                flags: RELINQUISH,
                source: ProposalSource::Async,
                ticket: n,
                ttl_ns: 0,
                intent: Intent::Write,
                task: NO_TASK,
            }],
            &o.group,
        )?;
        self.persisted(
            StateWriteFailure {
                generation: Some(v.number),
                file: "state/manual",
                output: Some(&o.path),
                op: "remove",
            },
            self.manual.remove(&o.path, level, now),
        );
        let id = joker_api::server::ulid(u128::try_from(utc_now_ns()).unwrap_or(0));
        let mut ticket = json!({
            "id": id,
            "output": o.path,
            "kind": "release",
            "state": "queued",
            "writer": principal,
            "level": format!("manual:{level}"),
            "generation": {"gen": v.number, "hash": v.hash},
        });
        if req.wait
            && let Some(r) = self.tickets.wait(&v.shared, n, Duration::from_secs(5))
        {
            ticket["state"] = json!("committed");
            ticket["outcome"] = outcome_json(r.outcome, None);
            ticket["tick"] = json!({"group": o.group, "seq": r.tick});
        }
        self.tickets.put(&id, ticket.clone());
        Ok(Reply::ok(ticket))
    }

    fn reset_trip(
        &self,
        who: &Principal,
        path: &str,
        req: ResetTripRequest,
    ) -> Result<Reply, Problem> {
        if req.reason.is_empty() {
            return Err(bad("J-E1106", "a reset needs a reason").with("pointer", json!("/reason")));
        }
        let v = self.need_view()?;
        let Some(o) = v.plan.output(path).cloned() else {
            if !self.trips.is_tripped(path) {
                return Err(Problem::new(
                    "J-E1100",
                    404,
                    format!("`{path}` is not an output"),
                ));
            }
            let result = self.reset_kept(&v, path);
            let item = self.record_reset(&v, path, &user(who), &req.reason, &result);
            result?;
            return Ok(Reply::ok(item));
        };
        let result = self.reset_one(&v, &o.path, &o.group, req.wait);
        let item = self.record_reset(&v, &o.path, &user(who), &req.reason, &result);
        if result? == "not_tripped" {
            return Err(Problem::new(
                "J-E1120",
                409,
                format!("`{}` is not in trip", o.path),
            ));
        }
        Ok(Reply::ok(item))
    }

    fn trips(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let sel = values_of(query, "sel");
        let items: Vec<Value> = v
            .plan
            .outputs
            .iter()
            .filter(|o| sel.is_empty() || sel.iter().any(|s| wire::matches(s, &o.path)))
            .filter_map(|o| {
                let signal = v.plan.id(&o.path)?;
                let since_mono_ns = v.shared.trip_since(signal)?;
                let since = self
                    .trips
                    .since(&o.path)
                    .unwrap_or_else(|| self.utc_origin_ns.saturating_add(since_mono_ns as i64));
                Some(json!({
                    "output": o.path,
                    "since": timestamp(since),
                    "scope": "output",
                    "cause": {"kind": "range"},
                }))
            })
            .chain(self.kept_trips(&v, &sel).into_iter().map(|(path, since)| {
                json!({
                    "output": path,
                    "since": timestamp(since),
                    "scope": "output",
                    "cause": {"kind": "range"},
                    "declared": false,
                })
            }))
            .collect();
        Ok(Reply::ok(json!({"items": items})))
    }

    fn reset_trips(&self, who: &Principal, req: BulkResetRequest) -> Result<Reply, Problem> {
        if req.reason.is_empty() {
            return Err(bad("J-E1106", "a reset needs a reason").with("pointer", json!("/reason")));
        }
        if req.sel.is_empty() {
            return Err(bad("J-E1013", "`sel` is required").with("pointer", json!("/sel")));
        }
        let v = self.need_view()?;
        let items: Vec<Value> = v
            .plan
            .outputs
            .iter()
            .filter(|o| req.sel.iter().any(|s| wire::matches(s, &o.path)))
            .map(|o| {
                let result = self.reset_one(&v, &o.path, &o.group, true);
                self.record_reset(&v, &o.path, &user(who), &req.reason, &result)
            })
            .chain(self.kept_trips(&v, &req.sel).into_iter().map(|(path, _)| {
                let result = self.reset_kept(&v, &path);
                self.record_reset(&v, &path, &user(who), &req.reason, &result)
            }))
            .collect();
        Ok(Reply::ok(json!({"items": items})))
    }

    fn manual(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let sel = values_of(query, "sel");
        let now = self.now_mono();
        let items: Vec<Value> = self
            .manual
            .all(now)
            .iter()
            .filter(|(p, _, _)| sel.is_empty() || sel.iter().any(|s| wire::matches(s, p)))
            .map(|(p, l, o)| self.manual_json(&v, p, *l, o, now))
            .collect();
        Ok(Reply::ok(json!({"items": items})))
    }

    fn write_ticket(&self, id: &str) -> Result<Reply, Problem> {
        self.tickets
            .get(id)
            .map(|t| Reply::ok(t.body))
            .ok_or_else(|| Problem::new("J-E1110", 404, format!("unknown write `{id}`")))
    }

    fn actions(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let target = one(query, "target");
        let items: Vec<Value> = v
            .plan
            .devices
            .iter()
            .filter(|d| target.as_deref().is_none_or(|t| t == d.name))
            .flat_map(|d| d.actions.iter().map(move |a| action_decl_json(&d.name, a)))
            .collect();
        Ok(Reply::ok(json!({"items": items})))
    }

    fn invoke_action(
        &self,
        who: &Principal,
        target: &str,
        action: &str,
        idempotency_key: Option<&str>,
        _request: InvokeActionRequest,
    ) -> Result<Reply, Problem> {
        let key = idempotency_key
            .ok_or_else(|| Problem::new("J-E1023", 400, "`Idempotency-Key` is required"))?;
        let now = self.now_mono();
        let principal = user(who);
        let outcome =
            lock_controller(&self.controller).invoke_action(target, action, key, now, &principal);
        let created_at = timestamp(utc_now_ns());
        let invocation = |replay: bool, state: &str, error: Option<(&str, &str, &str)>| {
            invocation_json(
                target,
                action,
                key,
                &principal,
                &created_at,
                replay,
                state,
                error,
            )
        };
        match outcome {
            Ok(Invoked::Ran(result)) => Ok(Reply::ok(match action_error(&result) {
                None => invocation(false, "succeeded", None),
                Some((kind, code, msg)) => invocation(false, "failed", Some((kind, code, &msg))),
            })),
            Ok(Invoked::Duplicate(result)) => Ok(Reply::ok(match action_error(&result) {
                None => invocation(true, "succeeded", None),
                Some((kind, code, msg)) => invocation(true, "failed", Some((kind, code, &msg))),
            })),
            Ok(Invoked::Rejected(Rejected::Busy)) => {
                Err(Problem::new("J-E1403", 503, "device action queue full"))
            }
            Ok(Invoked::Rejected(Rejected::NotPermitted)) => {
                Err(Problem::new("J-E1402", 409, "action permissive false"))
            }
            // The device not being connected is one of the action's own
            // result outcomes (ref/09 §5.10's `error.kind = "device"`),
            // not an HTTP-level failure of the request itself.
            Err(e) if e.code == "J-E1415" => Ok(Reply::ok(invocation(
                false,
                "failed",
                Some(("device", "J-E1415", &e.detail)),
            ))),
            Err(e) => Err(problem(e)),
        }
    }

    fn decisions(&self, query: &Query) -> Result<Reply, Problem> {
        let outputs = values_of(query, "output");
        let outcomes = values_of(query, "outcome");
        let writers = values_of(query, "writer");
        let kinds = values_of(query, "kind");
        let limit = one(query, "limit")
            .map(|l| {
                l.parse::<usize>()
                    .map_err(|_| bad("J-E1012", "bad `limit`"))
            })
            .transpose()?
            .unwrap_or(100)
            .clamp(1, 1000);
        let asc = one(query, "order").is_some_and(|o| o == "asc");
        let cursor = one(query, "cursor")
            .map(|c| {
                c.parse::<u64>()
                    .map_err(|_| Problem::new("J-E1032", 400, "bad cursor"))
            })
            .transpose()?;
        let entries =
            DecisionLog::read_all(&self.state_dir.join("decisions.jsonl")).unwrap_or_default();
        let hashes: std::collections::BTreeMap<u64, String> = lock_controller(&self.controller)
            .store()
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|g| (g.number, g.hash))
            .collect();
        let mut items: Vec<(u64, Value)> = entries
            .iter()
            .filter(|e| e["record"] == "output" || e["record"] == "write")
            .filter_map(|e| decision_json(e, &hashes).map(|d| (e["seq"].as_u64().unwrap_or(0), d)))
            .filter(|(_, d)| {
                let out = d["output"].as_str().unwrap_or("");
                (outputs.is_empty() || outputs.iter().any(|s| wire::matches(s, out)))
                    && (outcomes.is_empty()
                        || outcomes
                            .iter()
                            .any(|o| d["outcome"]["outcome"] == *o.as_str()))
                    && (writers.is_empty()
                        || writers.iter().any(|w| d["writer"]["writer"] == *w.as_str()))
                    && (kinds.is_empty() || kinds.iter().any(|k| d["kind"] == *k.as_str()))
            })
            .collect();
        if !asc {
            items.reverse();
        }
        if let Some(c) = cursor {
            items.retain(|(seq, _)| if asc { *seq > c } else { *seq < c });
        }
        let next = (items.len() > limit).then(|| items[limit - 1].0.to_string());
        let page: Vec<Value> = items.into_iter().take(limit).map(|(_, d)| d).collect();
        Ok(Reply::ok(json!({"items": page, "next_cursor": next})))
    }

    fn generations(&self, query: &Query) -> Result<Reply, Problem> {
        let states = values_of(query, "state");
        let all = lock_controller(&self.controller)
            .store()
            .list()
            .map_err(|e| Problem::new("J-E1000", 500, e.to_string()))?;
        let items: Vec<Value> = all
            .iter()
            .filter(|g| states.is_empty() || states.iter().any(|s| s == g.state.name()))
            .map(|g| self.generation_json(g))
            .collect();
        Ok(Reply::ok(json!({"items": items, "next_cursor": null})))
    }

    fn create_generation(
        &self,
        who: &Principal,
        req: GenerationProposal,
        dry_run: bool,
    ) -> Result<Reply, Problem> {
        let manifest = req.manifest.content.ok_or_else(|| {
            Problem::new(
                "J-E1602",
                422,
                "manifest by digest needs the blob store, which M0 does not have",
            )
        })?;
        let lock = req.lock.content.unwrap_or_default();
        let g = {
            let mut c = lock_controller(&self.controller);
            c.propose_by(&manifest, &lock, req.message, dry_run, Some(user(who)))
                .map_err(problem)?
        };
        if dry_run {
            let mut body = json!({
                "gen": 0, "hash": g.hash, "state": "pending", "origin": "local",
                "created_at": timestamp(utc_now_ns()), "created_by": user(who),
            });
            body["mode"] = json!("dev");
            return Ok(Reply::ok(body));
        }
        let mut r = Reply::with_status(201, self.generation_json(&g));
        r.etag = Some(g.hash.clone());
        Ok(r)
    }

    fn generation(&self, selector: &str) -> Result<Reply, Problem> {
        let g = self.resolve_generation(selector)?;
        let mut r = Reply::ok(self.generation_json(&g));
        r.etag = Some(g.hash.clone());
        Ok(r)
    }

    fn activate(
        &self,
        who: &Principal,
        selector: &str,
        if_match: Option<&str>,
        req: ActivateRequest,
    ) -> Result<Reply, Problem> {
        self.check_if_match(if_match)?;
        let g = self.resolve_generation(selector)?;
        let window = req
            .confirm_window
            .as_deref()
            .map(parse_duration)
            .transpose()
            .map_err(|e| bad("J-E1012", e).with("pointer", json!("/confirm_window")))?
            .map(Duration::from_nanos);
        let switched = lock_controller(&self.controller)
            .activate(g.number, window, Some(user(who)))
            .map_err(problem)?;
        Ok(Reply::ok(self.generation_json(&switched.generation)))
    }

    fn confirm(
        &self,
        _who: &Principal,
        selector: &str,
        _req: ReasonBody,
    ) -> Result<Reply, Problem> {
        let g = match self.resolve_generation(selector) {
            Err(_) if selector == "confirming" => {
                return Err(Problem::new("J-E1611", 409, "nothing to confirm"));
            }
            other => other?,
        };
        let confirmed = lock_controller(&self.controller)
            .confirm(Some(g.number))
            .map_err(|e| {
                let mut p = problem(e);
                if g.state == GenState::RolledBack {
                    p.detail = Some("the generation was rolled_back".into());
                }
                p
            })?;
        Ok(Reply::ok(self.generation_json(&confirmed)))
    }

    fn rollback(
        &self,
        who: &Principal,
        if_match: Option<&str>,
        req: RollbackRequest,
    ) -> Result<Reply, Problem> {
        self.check_if_match(if_match)?;
        let to = match req.to.as_deref() {
            None | Some("previous") => None,
            Some(sel) => Some(self.resolve_generation(sel)?.number),
        };
        let switched = lock_controller(&self.controller)
            .rollback(to, Some(user(who)), Some(req.reason))
            .map_err(problem)?;
        Ok(Reply::ok(self.generation_json(&switched.generation)))
    }

    fn traces(&self) -> Result<Reply, Problem> {
        let items: Vec<Value> = self.traces.list().iter().map(trace_json).collect();
        Ok(Reply::ok(json!({"items": items, "next_cursor": null})))
    }

    fn create_trace(&self, who: &Principal, req: TraceRequest) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let id = joker_api::server::ulid(u128::try_from(utc_now_ns()).unwrap_or(0));
        let source = Source {
            plan: Arc::clone(&v.plan),
            shared: Arc::clone(&v.shared),
            ring: Arc::clone(&v.ring),
            generation: (v.number, v.hash.clone()),
            utc_origin_ns: self.utc_origin_ns,
            clock: self.quality.quality() as u8,
        };
        let now = self.now_mono();
        let mut entry = TraceEntry {
            id: id.clone(),
            source: "ring",
            state: TraceState::Recording,
            created_by: user(who),
            created_utc_ns: utc_now_ns(),
            from_utc_ns: None,
            to_utc_ns: None,
            ends_utc_ns: None,
            size: None,
            generation: (v.number, v.hash.clone()),
            registry: format!("sha256:{}", crate::generations::hex(&v.plan.registry_hash)),
            label: req.label.clone(),
            failure: None,
        };
        match req.source.as_str() {
            "ring" => {
                let (first, last) = crate::trace_ring::lock(&v.ring)
                    .bounds()
                    .ok_or_else(|| Problem::new("J-E1702", 422, "the ring is empty"))?;
                let from = match &req.last {
                    Some(l) => {
                        last.saturating_sub(parse_duration(l).map_err(|e| bad("J-E1012", e))?)
                    }
                    None => first,
                };
                let bytes = source
                    .cut(from, last, None)
                    .map_err(|e| Problem::new("J-E1000", 500, e))?;
                entry.from_utc_ns = Some(self.utc_origin_ns.saturating_add(from as i64));
                entry.to_utc_ns = Some(self.utc_origin_ns.saturating_add(last as i64));
                self.traces.put(entry.clone());
                self.traces
                    .finish(&id, &bytes, TraceState::Ready)
                    .map_err(|e| Problem::new("J-E1000", 500, e.to_string()))?;
            }
            "record" => {
                let d = req.duration.as_deref().ok_or_else(|| {
                    bad("J-E1013", "`duration` is required").with("pointer", json!("/duration"))
                })?;
                let d = Duration::from_nanos(parse_duration(d).map_err(|e| bad("J-E1012", e))?);
                entry.source = "record";
                entry.from_utc_ns = Some(self.utc_origin_ns.saturating_add(now as i64));
                entry.ends_utc_ns =
                    Some(utc_now_ns().saturating_add(i64::try_from(d.as_nanos()).unwrap_or(0)));
                self.traces.put(entry.clone());
                let deadline = Duration::from_millis(100 * v.plan.groups.len().max(1) as u64);
                let state = source
                    .state_segment(deadline)
                    .map_err(|e| Problem::new("J-E1000", 500, e))?;
                record(
                    Arc::clone(&self.traces),
                    source,
                    id.clone(),
                    now,
                    d,
                    Some(state),
                );
            }
            other => {
                return Err(Problem::new(
                    "J-E1017",
                    400,
                    format!("`source = \"{other}\"` is not in this build"),
                ));
            }
        }
        let entry = self.traces.get(&id).unwrap_or(entry);
        Ok(Reply::with_status(201, trace_json(&entry)))
    }

    fn trace(&self, id: &str) -> Result<Reply, Problem> {
        self.traces
            .get(id)
            .map(|e| Reply::ok(trace_json(&e)))
            .ok_or_else(|| Problem::new("J-E1700", 404, format!("unknown trace `{id}`")))
    }

    fn trace_file(&self, id: &str) -> Result<Vec<u8>, Problem> {
        let e = self
            .traces
            .get(id)
            .ok_or_else(|| Problem::new("J-E1700", 404, format!("unknown trace `{id}`")))?;
        if !matches!(e.state, TraceState::Ready | TraceState::Stopped) {
            return Err(Problem::new(
                "J-E1706",
                409,
                format!("the trace is {}", e.state.name()),
            ));
        }
        std::fs::read(self.traces.file(id)).map_err(|e| Problem::new("J-E1000", 500, e.to_string()))
    }
}

impl Node {
    fn signal_meta(
        &self,
        v: &View,
        s: &crate::plan::SignalPlan,
        with_sample: bool,
        now: u64,
    ) -> Value {
        let aliases: Vec<&String> = v
            .plan
            .aliases
            .iter()
            .filter(|(_, t)| **t == s.path)
            .map(|(a, _)| a)
            .collect();
        let mut m = Map::new();
        m.insert("path".into(), json!(s.path));
        m.insert("canonical".into(), json!(s.path));
        if !aliases.is_empty() {
            m.insert("aliases".into(), json!(aliases));
        }
        if let Some(id) = v.plan.id(&s.path) {
            m.insert("signal_id".into(), json!(id.0));
        }
        m.insert("type".into(), json!(wire::type_name(s.ty)));
        if !s.variants.is_empty() {
            let e: Map<String, Value> = s
                .variants
                .iter()
                .enumerate()
                .map(|(i, n)| (n.clone(), json!(i)))
                .collect();
            m.insert("enum".into(), Value::Object(e));
        }
        if s.unit != "1" {
            m.insert("unit".into(), json!(s.unit));
        }
        m.insert("shape".into(), json!("scalar"));
        let direction = match s.direction {
            joker_core::registry::Direction::Output => "output",
            joker_core::registry::Direction::Input => "input",
            joker_core::registry::Direction::Derived => "internal",
        };
        m.insert("direction".into(), json!(direction));
        m.insert(
            "origin".into(),
            json!({"kind": "device", "name": s.device.clone().unwrap_or_default()}),
        );
        if let Some(a) = s.max_age_ns {
            m.insert("max_age".into(), json!(duration(a)));
        }
        m.insert("writable".into(), json!(direction == "output"));
        if let Some(g) = &s.group {
            m.insert("group".into(), json!(g));
        }
        if with_sample
            && let Some(sample) = v
                .plan
                .id(&s.path)
                .and_then(|id| v.shared.slots.get(id.index()))
                .and_then(|slot| slot.read(8).ok())
        {
            m.insert(
                "sample".into(),
                wire::sample(s, &sample, now, self.utc_origin_ns),
            );
        }
        Value::Object(m)
    }
}

fn trace_json(e: &TraceEntry) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(e.id));
    m.insert("source".into(), json!(e.source));
    m.insert("state".into(), json!(e.state.name()));
    m.insert("created_at".into(), json!(timestamp(e.created_utc_ns)));
    m.insert("created_by".into(), json!(e.created_by));
    if let Some(f) = e.from_utc_ns {
        m.insert("from".into(), json!(timestamp(f)));
    }
    if let Some(t) = e.to_utc_ns {
        m.insert("to".into(), json!(timestamp(t)));
    }
    if let Some(t) = e.ends_utc_ns {
        m.insert("ends_at".into(), json!(timestamp(t)));
    }
    if let Some(s) = e.size {
        m.insert("size_bytes".into(), json!(s));
    }
    m.insert(
        "generations".into(),
        json!([{"gen": e.generation.0, "hash": e.generation.1}]),
    );
    m.insert("registry".into(), json!(e.registry));
    if let Some(l) = &e.label {
        m.insert("label".into(), json!(l));
    }
    if e.state == TraceState::Ready || e.state == TraceState::Stopped {
        m.insert("file".into(), json!(format!("/v1/traces/{}/file", e.id)));
    }
    Value::Object(m)
}

fn decision_json(e: &Value, hashes: &std::collections::BTreeMap<u64, String>) -> Option<Value> {
    let number = e["gen"].as_u64()?;
    let ts = e["ts"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| timestamp(0));
    let seq = e["seq"].as_u64().unwrap_or(0);
    let ms = humantime::parse_rfc3339(&ts)
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64);
    let id = joker_api::server::ulid_from(ms, u128::from(seq));
    let kind = match e["cause"]["kind"].as_str() {
        Some("expiry") => "expiry",
        Some("reset") => "reset",
        Some("envelope") => "safe",
        _ => "proposal",
    };
    let mut outcome = Map::new();
    outcome.insert("outcome".into(), e["outcome"].clone());
    outcome.insert("applied".into(), e["applied"].clone());
    if let Some(r) = e.get("reason") {
        outcome.insert("reason".into(), r.clone());
    }
    if let Some(b) = e.get("by") {
        outcome.insert("level".into(), b.clone());
    }
    let mut m = Map::new();
    m.insert("id".into(), json!(id));
    m.insert("ts".into(), json!(ts));
    m.insert("tick".into(), json!({"seq": e["tick"]}));
    if let Some(h) = hashes.get(&number) {
        m.insert("generation".into(), json!({"gen": number, "hash": h}));
    }
    m.insert("output".into(), e["output"].clone());
    m.insert("kind".into(), json!(kind));
    if e["record"] == "write"
        && let Some(w) = e["writer"].as_str()
    {
        let level = e["level"].as_str().map(str::to_owned);
        m.insert(
            "writer".into(),
            json!({"writer": w, "level": level.unwrap_or_else(|| "control:12".into()), "kind": if w.starts_with("user:") { "manual" } else { "cycle" }}),
        );
    }
    m.insert("outcome".into(), Value::Object(outcome));
    m.insert("applied".into(), e["applied"].clone());
    m.insert(
        "active_level".into(),
        e["level"].as_str().map_or(json!("none"), |l| json!(l)),
    );
    if let Some(r) = e["previous"]["repeats"].as_u64() {
        m.insert("repeats".into(), json!(r));
    }
    Some(Value::Object(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::DeviceActionPlan;

    #[test]
    fn a_reset_that_could_not_be_queued_is_reported_as_failed_with_its_error() {
        let full = Err(Problem::new("J-E1014", 503, "the reset queue is full"));
        let item = reset_item("pi.p_set", &full);
        assert_eq!(item["result"], "failed");
        assert_eq!(item["error"]["code"], "J-E1014");
        assert_eq!(item["error"]["detail"], "the reset queue is full");
        let cleared = reset_item("pi.p_set", &Ok("cleared"));
        assert_eq!(cleared, json!({"output": "pi.p_set", "result": "cleared"}));
    }

    #[test]
    fn an_action_decl_carries_its_role_and_timeout_but_omits_an_undeclared_dedup_window() {
        let decl = DeviceActionPlan {
            name: "reset_fault".into(),
            timeout_ns: 1_000_000_000,
            dedup_window_ns: None,
            role: "operator".into(),
            steps: Vec::new(),
        };
        let json = action_decl_json("pcs", &decl);
        assert_eq!(json["target"], "pcs");
        assert_eq!(json["target_kind"], "device");
        assert_eq!(json["action"], "reset_fault");
        assert_eq!(json["role"], "operator");
        assert_eq!(json["timeout"], "1s");
        assert_eq!(json["long"], false);
        assert!(json.get("dedup_window").is_none());
    }

    #[test]
    fn a_declared_dedup_window_is_rendered_as_a_duration() {
        let decl = DeviceActionPlan {
            name: "reset_fault".into(),
            timeout_ns: 1_000_000_000,
            dedup_window_ns: Some(30_000_000_000),
            role: "admin".into(),
            steps: Vec::new(),
        };
        let json = action_decl_json("pcs", &decl);
        assert_eq!(json["role"], "admin");
        assert_eq!(json["dedup_window"], "30s");
    }

    /// Each `ActionResult` failure cause gets its own documented code
    /// (ref/04 §7.1): a timeout is `J-E1406`/`kind=timeout`, a driver's
    /// own comm failure is `J-E1407`/`kind=device`, and anything else
    /// the interpreter failed on falls back to `J-E1408`/`kind=device`
    /// — never the same code for all three (AUR-265).
    #[test]
    fn action_error_maps_each_failure_cause_to_its_own_code() {
        assert_eq!(action_error(&ActionResult::Succeeded), None);
        let (kind, code, _) = action_error(&ActionResult::TimedOut).unwrap();
        assert_eq!((kind, code), ("timeout", "J-E1406"));
        let (kind, code, msg) = action_error(&ActionResult::Device("comm".into())).unwrap();
        assert_eq!((kind, code, msg.as_str()), ("device", "J-E1407", "comm"));
        let (kind, code, msg) = action_error(&ActionResult::Failed("trapped".into())).unwrap();
        assert_eq!((kind, code, msg.as_str()), ("device", "J-E1408", "trapped"));
    }
}
