//! The daemon behind the local API: the M0 subset of ref/09 §5 over the
//! generation controller and the running generation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use joker_api::model::{
    ARTIFACT_KINDS, AckRequest, ActivateRequest, BulkResetRequest, GenerationProposal,
    InvokeActionRequest, ManualWriteRequest, MethodCallRequest, MissingBlobsRequest, ReasonBody,
    ReleaseRequest, ResetTripRequest, RollbackRequest, ShelveRequest, TraceRequest,
};
use joker_api::{Backend, Headers, Principal, Problem, Query, Reply};
use joker_core::image::{
    Intent, MAX_METHOD_ARGS, MethodCall, NO_TASK, Proposal, RELINQUISH, Source as ProposalSource,
};
use joker_core::outcome::Outcome;
use joker_core::output::Band;
use joker_core::{Level, WriterKind};
use serde_json::{Map, Value, json};

use crate::actions::{Hub, Refusal, Role};
use crate::blobs::{Blobs, PutError, parse_digest};
use crate::control::ControlError;
use crate::daemon::{SharedController, lock_controller};
use crate::decisions::{DecisionLog, SharedLog, StateWriteFailure, lock};
use crate::generations::{Activation, GenArtifact, GenState, Generation, Origin};
use crate::manual::{ManualTable, Occupant, TicketBook};
use crate::pacer::Monotonic;
use crate::plan::{OutputPlan, Plan, SignalPlan};
use crate::runner::Running;
use crate::shared::Shared;
use crate::state_file;
use crate::traces::{Source, TraceEntry, TraceState, Traces, record};
use crate::trips::TripTable;
use crate::wire::{self, duration, input_bits, parse_duration, timestamp, utc_now_ns};

/// The longest a write waits for its tick.
pub const MAX_WAIT: Duration = Duration::from_secs(30);

/// How far back `signal_trace` reads the continuous trace ring on each
/// sample of the topic (ADR 0091): the engine keeps only the records it
/// has not sent yet.
const TRACE_WINDOW_NS: u64 = 2_000_000_000;

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
    jobs: Arc<crate::doctor::Jobs>,
    manual: Arc<ManualTable>,
    trips: Arc<TripTable>,
    traces: Arc<Traces>,
    log: SharedLog,
    quality: Arc<crate::clock::ClockMonitor>,
    blobs: Blobs,
    hub: Arc<Hub>,
    alarms: Arc<crate::alarms::AlarmBook>,
    historian: Arc<crate::historian::Historian>,
    logs: Arc<crate::logs::LogRing>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node").field("name", &self.name).finish()
    }
}

fn refused(r: Refusal) -> Problem {
    Problem::new(r.code, r.status, r.detail)
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

fn digest(text: &str, pointer: &str) -> Result<[u8; 32], Problem> {
    parse_digest(text).ok_or_else(|| {
        bad(
            "J-E1012",
            format!("`{text}` is not a `sha256:<hex>` digest"),
        )
        .with("pointer", json!(pointer))
    })
}

fn artifact_kind(kind: &str, pointer: &str) -> Result<(), Problem> {
    if kind == "joker.model.v1+jkm" {
        return Err(bad("J-E1017", "`joker.model.v1+jkm` is reserved for 1.1"));
    }
    if !ARTIFACT_KINDS.contains(&kind) {
        return Err(bad("J-E1012", format!("unknown artifact kind `{kind}`"))
            .with("pointer", json!(pointer)));
    }
    Ok(())
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
        let (manual, trips, log, quality, hub, alarms, historian, logs) = {
            let c = lock_controller(&controller);
            (
                c.manual(),
                c.trips(),
                c.log(),
                c.clock(),
                c.actions(),
                c.alarms(),
                c.historian(),
                c.logs(),
            )
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
            jobs: Arc::default(),
            manual,
            trips,
            traces: Arc::new(Traces::open(&state_dir.join("traces"))?),
            log,
            blobs: Blobs::new(state_dir),
            quality,
            hub,
            alarms,
            historian,
            logs,
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

    /// The role of `who` under the running generation's `[security.roles]`
    /// (ref/09 §2.3, ADR 0063, ADR 0066), through the API's one resolver.
    fn role_of(&self, who: &Principal) -> Role {
        Role::parse(joker_api::server::role_with(who, &self.socket_roles())).unwrap_or(Role::Viewer)
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
        m.insert("force_safe".into(), json!(force_safe(v, o)));
        m.insert("writers".into(), Value::Array(writers));
        m.insert("envelope".into(), envelope_json(v, o));
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

/// The `link_state` of ref/00 §6 for a [`crate::shared::DeviceStats::link`]
/// code.
fn link_name(code: u64) -> &'static str {
    match code {
        0 => "connecting",
        1 => "up",
        2 => "degraded",
        3 => "down",
        4 => "drift",
        _ => "stopped",
    }
}

/// The link state and loss counters of every device with an io task, in
/// `plan.devices` order (ADR 0063, invariant 7).
fn devices_json(v: &View) -> Vec<(String, Value)> {
    v.plan
        .devices
        .iter()
        .zip(v.shared.devices.iter())
        .map(|(d, s)| {
            (
                d.name.clone(),
                json!({
                    "link": link_name(s.link.get()),
                    "reconnects": s.reconnects.get(),
                    "dropped_samples": s.dropped_samples.get(),
                    "invalid_samples": s.invalid_samples.get(),
                }),
            )
        })
        .collect()
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

/// Whether the supervisor holds `o` at its `safe` through the `force_safe`
/// bit of its latest-wins box (`OutputState.force_safe`, ref/09 §3): the
/// driver then sends `safe` whatever the arbiter applied.
fn force_safe(v: &View, o: &OutputPlan) -> bool {
    let Some(id) = v.plan.id(&o.path) else {
        return false;
    };
    v.shared
        .commit_bindings
        .iter()
        .zip(v.shared.commit_boxes.iter())
        .any(|(bindings, boxes)| {
            bindings
                .iter()
                .position(|b| b.signal == id)
                .is_some_and(|i| joker_hal::commit::ForceSafe::new(boxes).is_set(i))
        })
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
/// §5.10) of `target`'s `action`, run by `decl`. `permit` is the
/// declared source and `permit_now` its value now, when one is declared.
fn action_decl_json(
    target: &str,
    kind: &str,
    action: &str,
    decl: &crate::bridge::DeviceActionPlan,
    permit: Option<(&str, Option<bool>)>,
) -> Value {
    let args: Map<String, Value> = decl
        .args
        .iter()
        .map(|(name, ty)| {
            let mut parts = ty.splitn(2, ' ');
            let base = parts.next().unwrap_or("");
            let mut arg = json!({"type": base});
            if let Some(unit) = parts.next().map(str::trim).filter(|u| !u.is_empty()) {
                arg["unit"] = json!(unit);
            }
            (name.clone(), arg)
        })
        .collect();
    let result: Map<String, Value> = decl
        .result
        .iter()
        .map(|(name, ty)| (name.clone(), json!({"type": ty})))
        .collect();
    let mut m = json!({
        "target": target,
        "target_kind": kind,
        "action": action,
        "args": args,
        "result": result,
        "role": decl.role,
        "timeout": duration(decl.timeout_ns),
        "long": decl.long,
        "dedup_window": duration(
            decl.dedup_window_ns
                .unwrap_or(crate::actions::DEFAULT_DEDUP_WINDOW_NS)
        ),
    });
    if let Some((source, now)) = permit {
        m["permit"] = json!(source);
        m["permit_now"] = now.map_or(Value::Null, |b| json!(b));
    }
    m
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

fn alarm_problem(id: &str, e: joker_core::alarm::AlarmError) -> Problem {
    match e {
        joker_core::alarm::AlarmError::BadTtl => {
            Problem::new("J-E1202", 422, "the TTL is above the alarm's max_shelve")
        }
        joker_core::alarm::AlarmError::NotShelved => {
            Problem::new("J-E1203", 409, format!("`{id}` is not shelved"))
        }
        joker_core::alarm::AlarmError::Unknown => {
            Problem::new("J-E1200", 404, format!("unknown alarm `{id}`"))
        }
    }
}

/// What an operator command on an alarm needs checked first.
enum AlarmAction {
    Ack(Option<String>),
    Shelve { ttl_ns: u64, reason: String },
    Unshelve(Option<String>),
}

impl Node {
    /// Where an operator action on a runtime alarm is recorded.
    fn journal(&self, generation: u64) -> crate::alarms::Journal {
        crate::alarms::Journal {
            book: Arc::clone(&self.alarms),
            journal: Some((
                Arc::clone(self.historian.store()),
                self.historian.alarm_series(),
            )),
            log: Arc::clone(&self.log),
            generation,
            utc_origin_ns: self.utc_origin_ns,
        }
    }

    fn alarm_entry(&self, id: &str) -> Result<crate::alarms::Entry, Problem> {
        self.alarms
            .get(id)
            .ok_or_else(|| Problem::new("J-E1200", 404, format!("unknown alarm `{id}`")))
    }

    /// `Status.alarms`: counts, and up to 20 active or unacknowledged
    /// alarms by severity and age (ref/09 §5.2).
    fn alarm_summary(&self) -> Value {
        let view = self.view();
        let live = view.as_ref().map(|v| (v.plan.as_ref(), v.shared.as_ref()));
        let mut open: Vec<crate::alarms::Entry> = self
            .alarms
            .list()
            .into_iter()
            .filter(|e| e.active() || e.unacked())
            .collect();
        let active = open.iter().filter(|e| e.active()).count();
        let unacked = open.iter().filter(|e| e.unacked()).count();
        open.sort_by(|x, y| {
            y.severity
                .cmp(&x.severity)
                .then(x.since.unwrap_or(0).cmp(&y.since.unwrap_or(0)))
        });
        let items: Vec<Value> = open
            .iter()
            .take(20)
            .map(|e| crate::alarms::alarm_json(e, live))
            .collect();
        json!({"active": active, "unacked": unacked, "items": items})
    }

    fn alarm_reply(&self, id: &str) -> Result<Reply, Problem> {
        let e = self.alarm_entry(id)?;
        let view = self.view();
        let live = view.as_ref().map(|v| (v.plan.as_ref(), v.shared.as_ref()));
        Ok(Reply::ok(crate::alarms::alarm_json(&e, live)))
    }

    /// Runs an operator command on an alarm in the cycle thread and waits
    /// until the book has its event (ref/09 §5.8, ADR 0082).
    fn alarm_command(
        &self,
        who: &Principal,
        id: &str,
        action: AlarmAction,
    ) -> Result<Reply, Problem> {
        if self.role_of(who) < Role::Operator {
            return Err(Problem::new(
                "J-E1002",
                403,
                "acting on an alarm requires the role `operator`",
            ));
        }
        let entry = self.alarm_entry(id)?;
        let now = utc_now_ns();
        let (op, annotation) = match action {
            AlarmAction::Ack(reason) => {
                if entry.state == joker_core::alarm::AlarmState::ActiveAcked {
                    return self.alarm_reply(id);
                }
                if !entry.unacked() || entry.state == joker_core::alarm::AlarmState::Unknown {
                    return Err(Problem::new(
                        "J-E1204",
                        409,
                        format!(
                            "`{id}` is {}: there is nothing to acknowledge",
                            crate::alarms::state_name(entry.state)
                        ),
                    ));
                }
                (
                    crate::shared::AlarmOp::Ack,
                    crate::alarms::Annotation {
                        by: user(who),
                        reason,
                        until_utc_ns: None,
                    },
                )
            }
            AlarmAction::Shelve { ttl_ns, reason } => (
                crate::shared::AlarmOp::Shelve(ttl_ns),
                crate::alarms::Annotation {
                    by: user(who),
                    reason: Some(reason),
                    until_utc_ns: Some(
                        now.saturating_add(i64::try_from(ttl_ns).unwrap_or(i64::MAX)),
                    ),
                },
            ),
            AlarmAction::Unshelve(reason) => {
                if entry.shelved.is_none() {
                    return Err(Problem::new(
                        "J-E1203",
                        409,
                        format!("`{id}` is not shelved"),
                    ));
                }
                (
                    crate::shared::AlarmOp::Unshelve,
                    crate::alarms::Annotation {
                        by: user(who),
                        reason,
                        until_utc_ns: None,
                    },
                )
            }
        };
        if entry.source == "runtime" {
            let event = self
                .alarms
                .runtime_command(id, op, annotation, now)
                .map_err(|e| alarm_problem(id, e))?;
            if let Some(event) = event {
                let generation = self.view().map_or(0, |v| v.number);
                self.journal(generation).write(&[(now, event)], &mut false);
            }
            return self.alarm_reply(id);
        }
        let v = self.need_view()?;
        let number = entry
            .number
            .ok_or_else(|| Problem::new("J-E1200", 404, format!("unknown alarm `{id}`")))?;
        let version = self.alarms.version(id);
        self.alarms.annotate(id, annotation);
        let ticket = self.tickets.number();
        let queue = v
            .plan
            .alarms
            .get(number as usize)
            .and_then(|a| v.plan.groups.iter().position(|g| g.name == a.group))
            .and_then(|g| v.shared.alarm_commands.get(g));
        let command = crate::shared::AlarmCommand {
            alarm: number,
            op,
            ticket,
        };
        if queue.is_none_or(|q| q.push(command).is_err()) {
            self.alarms.forget(id);
            return Err(Problem::new(
                "J-E1014",
                503,
                "the alarm command queue is full",
            ));
        }
        let reply = self
            .alarms
            .wait_reply(&v.shared, ticket, Duration::from_secs(5))
            .ok_or_else(|| Problem::new("J-E1014", 503, "no tick ran the alarm command"))?;
        match reply.result {
            Ok(true) => {
                self.alarms.wait_change(id, version, Duration::from_secs(2));
            }
            Ok(false) => self.alarms.forget(id),
            Err(e) => {
                self.alarms.forget(id);
                return Err(alarm_problem(id, e));
            }
        }
        self.alarm_reply(id)
    }
}

impl Backend for Node {
    fn socket_roles(&self) -> joker_api::server::SocketRoles {
        self.view()
            .map(|v| v.plan.roles.clone())
            .unwrap_or_default()
    }

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
                "role": joker_api::server::role_with(who, &self.socket_roles()),
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

    fn node_hardware(&self) -> Result<Reply, Problem> {
        let probe = crate::hardware::Probe {
            root: Path::new("/"),
            now_utc_ns: utc_now_ns(),
            fallback_cpus: std::thread::available_parallelism().map_or(1, usize::from),
        };
        Ok(Reply::ok(crate::hardware::inventory(
            &probe,
            &crate::hardware::LiveKernel,
        )))
    }

    fn status(&self) -> Result<Reply, Problem> {
        let v = self.view();
        let now = self.now_mono();
        let devices: Vec<Value> = v.as_ref().map_or_else(Vec::new, |v| {
            devices_json(v)
                .into_iter()
                .map(|(name, mut d)| {
                    d["name"] = json!(name);
                    d
                })
                .collect()
        });
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
            "devices": devices,
            "apps": apps,
            "alarms": self.alarm_summary(),
            "trips": [],
            "manual": manual,
        })))
    }

    fn lineage(&self, path: &str) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let canonical = v
            .plan
            .aliases
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_owned());
        let s = v
            .plan
            .signal(&canonical)
            .ok_or_else(|| Problem::new("J0101", 404, format!("unknown signal `{path}`")))?;
        let id = v
            .plan
            .id(&canonical)
            .ok_or_else(|| Problem::new("J0101", 404, format!("unknown signal `{path}`")))?;
        let mut m = Map::new();
        m.insert("path".into(), json!(path));
        m.insert("canonical".into(), json!(canonical));
        if let Some(device) = &s.device {
            let mut source = Map::new();
            source.insert("device".into(), json!(device));
            if let Some(d) = v.plan.devices.iter().find(|d| &d.name == device) {
                source.insert("driver".into(), json!(d.driver));
                if let Some(bus) = &d.bus {
                    source.insert("bus".into(), json!(bus));
                }
                if let Some(poll) = d
                    .points
                    .iter()
                    .find(|p| p.path == canonical)
                    .and_then(|p| p.poll_ns)
                {
                    source.insert("poll".into(), json!(duration(poll)));
                }
            }
            m.insert("source".into(), Value::Object(source));
        }
        if let Some(c) = v.plan.calcs.iter().find(|c| c.path == canonical) {
            m.insert("expression".into(), json!(c.expr.source));
        }
        let mut readers = Vec::new();
        for g in &v.plan.groups {
            for t in &g.tasks {
                for (port, bound) in &t.bindings {
                    if *bound == canonical {
                        readers.push(
                            json!({"kind": "app", "name": t.app, "port": port, "group": g.name}),
                        );
                    }
                }
            }
        }
        for c in &v.plan.calcs {
            if c.expr.inputs().any(|i| i == id) {
                readers.push(json!({"kind": "calc", "name": c.path, "group": c.group}));
            }
        }
        for a in &v.plan.alarms {
            if a.when.inputs().any(|i| i == id) {
                readers.push(json!({"kind": "alarm", "name": a.name, "group": a.group}));
            }
        }
        for o in &v.plan.outputs {
            let e = &o.exprs;
            if e.interlock
                .as_ref()
                .is_some_and(|x| x.inputs().any(|i| i == id))
            {
                readers.push(json!({"kind": "interlock", "name": o.path, "group": o.group}));
            }
            if e.requires_good.contains(&id) {
                readers.push(json!({"kind": "requires_good", "name": o.path, "group": o.group}));
            }
            let in_range = e.range.as_ref().is_some_and(|(lo, hi)| {
                [lo, hi].iter().any(|l| match l {
                    crate::expr_plan::LimitPlan::Expr(x) => x.inputs().any(|i| i == id),
                    crate::expr_plan::LimitPlan::Const(_) => false,
                })
            });
            if in_range {
                readers.push(json!({"kind": "range", "name": o.path, "group": o.group}));
            }
        }
        m.insert("readers".into(), Value::Array(readers));
        if let Some(o) = v.plan.outputs.iter().find(|o| o.path == canonical) {
            let mut output = Map::new();
            output.insert("group".into(), json!(o.group));
            if let Some(verify) = &o.verify {
                output.insert("readback".into(), json!(verify.readback));
            }
            m.insert("output".into(), Value::Object(output));
        }
        Ok(Reply::ok(Value::Object(m)))
    }

    fn signal_trace(&self, path: &str) -> Result<Reply, Problem> {
        use joker_core::trace::{SEGMENT_HEADER_LEN, Ticks};
        let v = self.need_view()?;
        let canonical = v
            .plan
            .aliases
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_owned());
        let (Some(s), Some(id)) = (v.plan.signal(&canonical), v.plan.id(&canonical)) else {
            return Err(Problem::new(
                "J0101",
                404,
                format!("unknown signal `{path}`"),
            ));
        };
        let from = self.now_mono().saturating_sub(TRACE_WINDOW_NS);
        let ring = crate::trace_ring::lock(&v.ring);
        let mut items = Vec::new();
        for (g, group) in v.plan.groups.iter().enumerate() {
            let port = group.ports.iter().position(|p| v.plan.id(p) == Some(id));
            let mut last = None;
            for (index, at, segment) in ring.ticks_of(g, from) {
                let Some(payload) = segment.get(SEGMENT_HEADER_LEN..) else {
                    continue;
                };
                let Some(Ok(tick)) = Ticks::new(payload, 1, group.ports.len()).next() else {
                    continue;
                };
                let read = port
                    .and_then(|p| tick.inputs().nth(p))
                    .map(|i| (i.value, i.quality));
                let proposals: Vec<Value> = tick
                    .proposals()
                    .filter(|p| p.signal == id)
                    .map(|p| {
                        let writer = v
                            .plan
                            .writers
                            .get(p.writer.0 as usize)
                            .map_or_else(|| p.writer.0.to_string(), |w| w.name.clone());
                        json!({"writer": writer, "value": wire::value(s, p.value)})
                    })
                    .collect();
                let result = tick
                    .results()
                    .find(|r| r.signal == id)
                    .map(|r| (r.outcome, r.applied));
                let state = (read, result);
                if (read.is_none() && result.is_none() && proposals.is_empty())
                    || (last == Some(state) && proposals.is_empty())
                {
                    continue;
                }
                last = Some(state);
                let mut item = Map::new();
                item.insert("key".into(), json!(format!("{at:020}.{g:04}")));
                item.insert("group".into(), json!(group.name));
                item.insert("tick".into(), json!(index));
                item.insert("mono_ns".into(), json!(at));
                if let Some((value, quality)) = read {
                    item.insert(
                        "read".into(),
                        json!({
                            "value": wire::value(s, value),
                            "quality": wire::quality_name(quality),
                            "reason": wire::reason_name(quality),
                        }),
                    );
                }
                if !proposals.is_empty() {
                    item.insert("proposals".into(), Value::Array(proposals));
                }
                if let Some((outcome, applied)) = result {
                    let outcome = Outcome::from_code(outcome, 0).unwrap_or(Outcome::None);
                    item.insert(
                        "result".into(),
                        outcome_json(outcome, Some(wire::value(s, applied))),
                    );
                }
                items.push(Value::Object(item));
            }
        }
        items.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
        Ok(Reply::ok(json!({ "items": items })))
    }

    fn job(&self, id: &str) -> Result<Reply, Problem> {
        self.jobs
            .get(id)
            .map(Reply::ok)
            .ok_or_else(|| Problem::new("J-E1005", 404, format!("unknown job `{id}`")))
    }

    fn doctor_report(&self) -> Result<Reply, Problem> {
        self.jobs
            .last_report()
            .map(Reply::ok)
            .ok_or_else(|| Problem::new("J-E1005", 404, "no doctor report yet: run `jk doctor`"))
    }

    fn run_doctor(
        &self,
        who: &Principal,
        req: joker_api::model::DoctorRequest,
    ) -> Result<Reply, Problem> {
        if self.role_of(who) < Role::Admin {
            return Err(Problem::new(
                "J-E1002",
                403,
                "`jk doctor` requires the role `admin`",
            ));
        }
        let view = self.view();
        let mut skipped = Vec::new();
        let mut rt_length = None;
        for c in &req.checks {
            match c.as_str() {
                "base" => {}
                "rt" => {
                    let prod_live = view
                        .as_ref()
                        .is_some_and(|v| !v.plan.dev && !v.plan.outputs.is_empty());
                    if prod_live && !req.allow_live {
                        skipped.push(json!({
                            "id": "rt.cyclictest",
                            "status": "skip",
                            "detail": "outputs are active in prod and the measurement disturbs rt",
                            "help": "run again with --allow-live",
                        }));
                        continue;
                    }
                    let length = match &req.rt_duration {
                        Some(d) => wire::parse_duration(d).map_err(|e| {
                            bad("J-E1012", format!("`rt_duration`: {e}"))
                                .with("pointer", json!("/rt_duration"))
                        })?,
                        None => 60_000_000_000,
                    };
                    rt_length = Some(Duration::from_nanos(length));
                }
                "ml" | "network" => skipped.push(json!({
                    "id": c,
                    "status": "skip",
                    "detail": "nothing to check in 1.0",
                })),
                other => {
                    return Err(bad("J-E1012", format!("unknown check `{other}`"))
                        .with("pointer", json!("/checks")));
                }
            }
        }
        let inputs = crate::doctor::Inputs {
            running: view.map(|v| (v.plan, v.shared)),
            state_dir: self.state_dir.clone(),
            rt_cpus: lock_controller(&self.controller).active_rt_cpus(),
            now_mono_ns: self.now_mono(),
        };
        let job = self
            .jobs
            .start_doctor(&user(who), rt_length, skipped, inputs)
            .map_err(|e| Problem::new("J-E1000", 500, format!("cannot start the doctor: {e}")))?;
        let location = format!("/v1/jobs/{}", job["id"].as_str().unwrap_or_default());
        Ok(Reply {
            status: 202,
            headers: vec![("location", location)],
            ..Reply::ok(job)
        })
    }

    fn apps(&self) -> Result<Reply, Problem> {
        let states = lock_controller(&self.controller).app_states();
        let items: Vec<Value> = states.iter().map(app_json).collect();
        Ok(Reply::ok(json!({ "items": items })))
    }

    fn app(&self, name: &str) -> Result<Reply, Problem> {
        lock_controller(&self.controller)
            .app_states()
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|s| Reply::ok(app_json(s)))
            .ok_or_else(|| Problem::new("J-E1500", 404, format!("unknown app `{name}`")))
    }

    fn app_lifecycle(
        &self,
        who: &Principal,
        name: &str,
        op: joker_api::model::AppOp,
        req: ReasonBody,
    ) -> Result<Reply, Problem> {
        use joker_api::model::AppOp;
        if self.role_of(who) < Role::Operator {
            return Err(Problem::new(
                "J-E1002",
                403,
                format!("`{}` on an app requires the role `operator`", op.name()),
            ));
        }
        let reason = req.reason.filter(|r| !r.is_empty());
        if op == AppOp::Stop && reason.is_none() {
            return Err(
                bad("J-E1106", "stopping an app needs a reason").with("pointer", json!("/reason"))
            );
        }
        let mut c = lock_controller(&self.controller);
        match op {
            AppOp::Start => c.app_stop(name, false),
            AppOp::Stop => c.app_stop(name, true),
            AppOp::Restart => c.app_restart(name).map(|_| ()),
        }
        .map_err(problem)?;
        let mut m = Map::new();
        if let Some(generation) = c.active_generation() {
            m.insert("gen".into(), json!(generation));
        }
        m.insert("record".into(), json!("app"));
        m.insert("app".into(), json!(name));
        m.insert("op".into(), json!(op.name()));
        m.insert("principal".into(), json!(user(who)));
        if let Some(reason) = &reason {
            m.insert("reason".into(), json!(reason));
        }
        m.insert("ts".into(), json!(timestamp(utc_now_ns())));
        if let Err(e) = lock(&self.log).append(m, true) {
            eprintln!("jokerd: decision log: {e}");
        }
        c.app_states()
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|s| Reply::ok(app_json(s)))
            .ok_or_else(|| Problem::new("J-E1500", 404, format!("unknown app `{name}`")))
    }

    fn groups(&self) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let now = self.now_mono();
        let items: Vec<Value> = (0..v.plan.groups.len())
            .filter_map(|g| group_json(&v, g, now))
            .collect();
        Ok(Reply::ok(json!({ "items": items })))
    }

    fn group(&self, name: &str) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        v.plan
            .groups
            .iter()
            .position(|g| g.name == name)
            .and_then(|g| group_json(&v, g, self.now_mono()))
            .map(Reply::ok)
            .ok_or_else(|| Problem::new("J-E1005", 404, format!("unknown group `{name}`")))
    }

    fn metrics(&self) -> Result<Reply, Problem> {
        let view = self.view();
        let now = self.now_mono();
        let drivers: Vec<Value> = view.as_ref().map_or_else(Vec::new, |v| {
            devices_json(v)
                .into_iter()
                .map(|(name, mut d)| {
                    d["device"] = json!(name);
                    d
                })
                .collect()
        });
        let groups: Vec<Value> = match &view {
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
                        "consecutive_overruns": s.consecutive_overruns.get(),
                        "jitter": percentiles(&s.jitter, s.max_late_ns.get(), now),
                        "duration": percentiles(&s.duration, s.max_duration_ns.get(), now),
                    })
                })
                .collect(),
        };
        let envelope: Vec<Value> = view.as_ref().map_or_else(Vec::new, |v| {
            v.plan
                .outputs
                .iter()
                .filter_map(|o| {
                    let r = v.shared.output(v.plan.id(&o.path)?)?.results();
                    Some(json!({
                        "output": o.path,
                        "accepted": r.accepted,
                        "clamped": r.clamped,
                        "rejected": r.rejected,
                        "suppressed": r.suppressed,
                        "tripped": r.tripped,
                    }))
                })
                .collect()
        });
        let mut arbiter = Map::new();
        arbiter.insert("manual_active".into(), json!(self.manual.all(now).len()));
        let mut runtime = Map::new();
        if let Some(v) = &view {
            arbiter.insert(
                "leases_expiring".into(),
                json!(crate::runtime_signals::leases_expiring(&v.plan, &v.shared)),
            );
            let dropped: Map<String, Value> =
                crate::runtime_signals::dropped(&v.shared, Some(&self.logs))
                    .into_iter()
                    .map(|(queue, n)| (queue.to_owned(), json!(n)))
                    .collect();
            runtime.insert("dropped".into(), Value::Object(dropped));
        }
        Ok(Reply::ok(json!({
            "at": timestamp(utc_now_ns()),
            "groups": groups,
            "drivers": drivers,
            "envelope": envelope,
            "arbiter": arbiter,
            "runtime": runtime,
            "system": system_json(&self.state_dir),
            "ml": Value::Null,
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
            .filter_map(|s| self.sample_json(&v, s, now))
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
        let wanted = |name: &str| target.as_deref().is_none_or(|t| t == name);
        let now = self.now_mono();
        let mut c = lock_controller(&self.controller);
        let mut permit_of = |t: &str, a: &str, key: (&str, &str)| {
            let source = v
                .plan
                .permit_sources
                .get(&(key.0.to_owned(), key.1.to_owned()))?
                .clone();
            let value = c.permit_now(t, a, now);
            Some((source, value))
        };
        let mut items = Vec::new();
        for d in v.plan.devices.iter().filter(|d| wanted(&d.name)) {
            for a in &d.actions {
                let p = permit_of(&d.name, &a.name, (&d.name, &a.name));
                let p = p.as_ref().map(|(s, n)| (s.as_str(), *n));
                items.push(action_decl_json(&d.name, "device", &a.name, a, p));
                for member in &a.implements {
                    let p = permit_of(&d.name, member, (&d.name, &a.name));
                    let p = p.as_ref().map(|(s, n)| (s.as_str(), *n));
                    items.push(action_decl_json(&d.name, "instance", member, a, p));
                }
            }
        }
        for i in v
            .plan
            .instance_actions
            .iter()
            .filter(|i| wanted(&i.instance))
        {
            let Some(mut decl) = v
                .plan
                .devices
                .iter()
                .find(|d| d.name == i.device)
                .and_then(|d| d.actions.iter().find(|a| a.name == i.device_action))
                .cloned()
            else {
                continue;
            };
            if let Some(t) = i.timeout_ns {
                decl.timeout_ns = t;
            }
            if i.dedup_window_ns.is_some() {
                decl.dedup_window_ns = i.dedup_window_ns;
            }
            if let Some(r) = &i.role {
                decl.role.clone_from(r);
            }
            let p = permit_of(&i.instance, &i.action, (&i.instance, &i.action));
            let p = p.as_ref().map(|(s, n)| (s.as_str(), *n));
            items.push(action_decl_json(
                &i.instance,
                "instance",
                &i.action,
                &decl,
                p,
            ));
        }
        Ok(Reply::ok(json!({"items": items})))
    }

    fn invoke_action(
        &self,
        who: &Principal,
        target: &str,
        action: &str,
        idempotency_key: Option<&str>,
        request: InvokeActionRequest,
    ) -> Result<Reply, Problem> {
        let key = idempotency_key
            .ok_or_else(|| Problem::new("J-E1023", 400, "`Idempotency-Key` is required"))?;
        let timeout_ns = request
            .timeout
            .as_deref()
            .map(parse_duration)
            .transpose()
            .map_err(|e| bad("J-E1401", format!("`timeout`: {e}")))?;
        let role = self.role_of(who);
        let req = crate::actions::Request {
            key: key.to_owned(),
            args: request.args,
            timeout_ns,
            reason: request.reason,
            wait: request.wait,
            principal: user(who),
            role: Some(role),
        };
        let answer = crate::actions::invoke(&self.controller, &self.hub, target, action, req)
            .map_err(refused)?;
        let wait = request.wait
            && self
                .hub
                .get(&answer.id)
                .is_some_and(|i| !i.long || answer.replay);
        let inv = if wait {
            self.hub.wait(&answer.id, MAX_WAIT)
        } else {
            self.hub.get(&answer.id)
        }
        .ok_or_else(|| Problem::new("J-E1404", 404, "the invocation is no longer kept"))?;
        if let Some(e) = &inv.log_error {
            return Err(Problem::new(
                "J-E1000",
                500,
                format!(
                    "the action ended `{}` but its decision record was refused: {e}",
                    inv.state.name()
                ),
            ));
        }
        let body = inv.json(answer.replay);
        let mut reply = if inv.state.finished() {
            Reply::ok(body)
        } else {
            Reply::with_status(202, body)
                .with_header("location", format!("/v1/invocations/{}", inv.id))
        };
        if answer.replay {
            reply = reply.with_header("joker-idempotent-replay", "true");
        }
        Ok(reply)
    }

    fn invocations(&self, query: &Query) -> Result<Reply, Problem> {
        let target = one(query, "target");
        let state = one(query, "state");
        let limit = one(query, "limit")
            .map(|l| {
                l.parse::<usize>()
                    .map_err(|_| bad("J-E1012", "bad `limit`"))
            })
            .transpose()?
            .unwrap_or(100)
            .clamp(1, 1000);
        let items: Vec<Value> = self
            .hub
            .list(|i| {
                target.as_deref().is_none_or(|t| t == i.target)
                    && state.as_deref().is_none_or(|s| s == i.state.name())
            })
            .iter()
            .take(limit)
            .map(|i| i.json(false))
            .collect();
        Ok(Reply::ok(json!({"items": items, "next_cursor": null})))
    }

    fn invocation(&self, id: &str) -> Result<Reply, Problem> {
        self.hub
            .get(id)
            .map(|i| Reply::ok(i.json(false)))
            .ok_or_else(|| Problem::new("J-E1404", 404, format!("unknown invocation `{id}`")))
    }

    fn history(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        crate::queries::history(
            self.historian.store(),
            &v.plan,
            self.generation_ref(&v),
            query,
            utc_now_ns(),
        )
        .map(Reply::ok)
    }

    fn events(&self, query: &Query) -> Result<Reply, Problem> {
        crate::queries::events(query, utc_now_ns()).map(Reply::ok)
    }

    fn alarms(&self, query: &Query) -> Result<Reply, Problem> {
        let sel = values_of(query, "sel");
        let state = one(query, "state").unwrap_or_else(|| "all".into());
        if !["active", "unacked", "shelved", "suppressed", "all"].contains(&state.as_str()) {
            return Err(
                bad("J-E1012", format!("unknown state `{state}`")).with("param", json!("state"))
            );
        }
        let min = match one(query, "severity") {
            Some(s) => Some(crate::alarms::severity_of(&s).ok_or_else(|| {
                bad("J-E1012", format!("unknown severity `{s}`")).with("param", json!("severity"))
            })?),
            None => None,
        };
        let limit = one(query, "limit")
            .map(|l| {
                l.parse::<usize>()
                    .map_err(|_| bad("J-E1012", "bad `limit`"))
            })
            .transpose()?
            .unwrap_or(100)
            .clamp(1, 1000);
        let after = one(query, "cursor");
        let view = self.view();
        let live = view.as_ref().map(|v| (v.plan.as_ref(), v.shared.as_ref()));
        let mut items: Vec<(String, Value)> = self
            .alarms
            .list()
            .into_iter()
            .filter(|e| sel.is_empty() || sel.iter().any(|s| wire::matches(s, &e.id)))
            .filter(|e| min.is_none_or(|m| e.severity >= m))
            .filter(|e| match state.as_str() {
                "active" => e.active(),
                "unacked" => e.unacked(),
                "shelved" => e.shelved.is_some(),
                "suppressed" => false,
                _ => true,
            })
            .filter(|e| after.as_deref().is_none_or(|a| e.id.as_str() > a))
            .map(|e| (e.id.clone(), crate::alarms::alarm_json(&e, live)))
            .collect();
        let next = (items.len() > limit).then(|| items[limit - 1].0.clone());
        items.truncate(limit);
        let page: Vec<Value> = items.into_iter().map(|(_, v)| v).collect();
        Ok(Reply::ok(json!({"items": page, "next_cursor": next})))
    }

    fn alarm(&self, id: &str) -> Result<Reply, Problem> {
        self.alarm_reply(id)
    }

    fn ack_alarm(&self, who: &Principal, id: &str, req: AckRequest) -> Result<Reply, Problem> {
        self.alarm_command(who, id, AlarmAction::Ack(req.reason))
    }

    fn shelve_alarm(
        &self,
        who: &Principal,
        id: &str,
        req: ShelveRequest,
    ) -> Result<Reply, Problem> {
        let entry = self.alarm_entry(id)?;
        let ttl_text = req
            .ttl
            .ok_or_else(|| bad("J-E1201", "shelving needs a TTL").with("pointer", json!("/ttl")))?;
        let ttl_ns = parse_duration(&ttl_text)
            .map_err(|e| bad("J-E1012", e).with("pointer", json!("/ttl")))?;
        if ttl_ns == 0 {
            return Err(bad("J-E1201", "shelving needs a TTL").with("pointer", json!("/ttl")));
        }
        if ttl_ns > entry.max_shelve_ns {
            return Err(Problem::new(
                "J-E1202",
                422,
                format!(
                    "the TTL is above the alarm's max_shelve ({})",
                    duration(entry.max_shelve_ns)
                ),
            )
            .with("pointer", json!("/ttl")));
        }
        let reason = req.reason.filter(|r| !r.is_empty()).ok_or_else(|| {
            bad("J-E1013", "shelving needs a reason").with("pointer", json!("/reason"))
        })?;
        self.alarm_command(who, id, AlarmAction::Shelve { ttl_ns, reason })
    }

    fn unshelve_alarm(&self, who: &Principal, id: &str, req: AckRequest) -> Result<Reply, Problem> {
        self.alarm_command(who, id, AlarmAction::Unshelve(req.reason))
    }

    fn alarm_events(&self, query: &Query) -> Result<Reply, Problem> {
        crate::queries::alarm_events(
            self.historian.store(),
            self.historian.alarm_series(),
            query,
            utc_now_ns(),
        )
        .map(Reply::ok)
    }

    fn cancel_invocation(
        &self,
        who: &Principal,
        id: &str,
        _request: ReasonBody,
    ) -> Result<Reply, Problem> {
        if self.role_of(who) < Role::Operator {
            return Err(Problem::new(
                "J-E1002",
                403,
                "cancelling an invocation requires the role `operator`",
            ));
        }
        self.hub
            .cancel(id)
            .map(|i| Reply::ok(i.json(false)))
            .map_err(refused)
    }

    fn call_method(
        &self,
        who: &Principal,
        instance: &str,
        method: &str,
        req: MethodCallRequest,
    ) -> Result<Reply, Problem> {
        if self.role_of(who) < Role::Operator {
            return Err(Problem::new(
                "J-E1002",
                403,
                "calling a method requires the role `operator`",
            ));
        }
        let v = self.need_view()?;
        let (index, inst) = v
            .plan
            .instances
            .iter()
            .enumerate()
            .find(|(_, i)| i.name == instance)
            .ok_or_else(|| {
                Problem::new("J-E1410", 404, format!("`{instance}` is not an instance"))
            })?;
        let (number, m) = inst.method(method).ok_or_else(|| {
            Problem::new(
                "J-E1411",
                404,
                format!("`{instance}` has no method `{method}`"),
            )
        })?;
        let ttl_text = req.ttl.clone().ok_or_else(|| {
            bad("J-E1101", "a manual call needs a TTL (A10)").with("pointer", json!("/ttl"))
        })?;
        let ttl = parse_duration(&ttl_text)
            .map_err(|e| bad("J-E1012", e).with("pointer", json!("/ttl")))?;
        if ttl < 1_000_000_000 {
            return Err(bad("J-E1103", "the TTL is below 1 s").with("pointer", json!("/ttl")));
        }
        let outputs: Vec<OutputPlan> = m
            .writes
            .iter()
            .filter_map(|w| v.plan.output(w).cloned())
            .collect();
        if let Some(max) = outputs.iter().map(|o| o.max_manual_ns).min()
            && ttl > max
        {
            return Err(bad(
                "J-E1102",
                format!("the TTL is above max_manual ({})", duration(max)),
            )
            .with("pointer", json!("/ttl")));
        }
        let reason = req
            .reason
            .clone()
            .filter(|r| !r.is_empty())
            .ok_or_else(|| {
                bad("J-E1106", "a manual call needs a reason").with("pointer", json!("/reason"))
            })?;
        let level = Self::manual_level(req.level.as_deref())?;
        if let Some(unknown) = req
            .args
            .keys()
            .find(|k| !m.params.iter().any(|p| &p.name == *k))
        {
            return Err(
                bad("J-E1412", format!("`{method}` has no argument `{unknown}`"))
                    .with("pointer", json!(format!("/args/{unknown}"))),
            );
        }
        let mut args = [0u64; MAX_METHOD_ARGS];
        if m.params.len() > MAX_METHOD_ARGS {
            return Err(bad("J-E1412", "too many arguments"));
        }
        for (slot, p) in args.iter_mut().zip(&m.params) {
            let value = req.args.get(&p.name).ok_or_else(|| {
                bad("J-E1412", format!("missing argument `{}`", p.name))
                    .with("pointer", json!(format!("/args/{}", p.name)))
            })?;
            let signal = crate::plan::SignalPlan {
                path: p.name.clone(),
                ty: p.ty,
                unit: p.unit.clone(),
                direction: joker_core::registry::Direction::Input,
                group: None,
                max_age_ns: None,
                device: None,
                variants: Vec::new(),
                enum_name: None,
            };
            *slot = input_bits(&signal, value).map_err(|e| {
                bad("J-E1412", e.detail).with("pointer", json!(format!("/args/{}", p.name)))
            })?;
        }
        let wait_timeout = match &req.wait_timeout {
            Some(t) => Duration::from_nanos(parse_duration(t).map_err(|e| bad("J-E1012", e))?),
            None => Duration::from_secs(5),
        };
        if wait_timeout > MAX_WAIT {
            return Err(bad("J-E1012", "`wait_timeout` is above 30s")
                .with("pointer", json!("/wait_timeout")));
        }
        let group = inst.group.clone().ok_or_else(|| {
            Problem::new(
                "J-E1411",
                404,
                format!("`{instance}.{method}` writes no output of this node"),
            )
        })?;
        let principal = user(who);
        let now = self.now_mono();
        let mut taken = Vec::new();
        for o in &outputs {
            if let Some(h) = self.manual.get(&o.path, level, now)
                && h.principal != principal
            {
                if !req.takeover {
                    return Err(Problem::new(
                        "J-E1105",
                        409,
                        format!(
                            "manual:{level} of `{}` is held by {} until {}: {}",
                            o.path,
                            h.principal,
                            timestamp(h.until_utc_ns),
                            h.reason
                        ),
                    ));
                }
                taken.push(o.clone());
            }
        }
        let writer = v
            .plan
            .writer(&format!("user:manual:{level}"))
            .ok_or_else(|| Problem::new("J-E1000", 500, "manual writer missing"))?;
        let relinquish: Vec<Proposal> = taken
            .iter()
            .map(|o| Proposal {
                writer,
                signal: v.plan.id(&o.path).unwrap_or(joker_core::SignalId(0)),
                value: 0,
                flags: RELINQUISH,
                source: ProposalSource::Async,
                ticket: 0,
                ttl_ns: 0,
                intent: Intent::Write,
                task: NO_TASK,
            })
            .collect();
        self.stage(&v, &relinquish, &group)?;
        let n = self.tickets.number();
        let g = v
            .plan
            .groups
            .iter()
            .position(|gr| gr.name == group)
            .ok_or_else(|| Problem::new("J-E1000", 500, "owner group missing"))?;
        let inbox = v
            .shared
            .method_inboxes
            .get(g)
            .ok_or_else(|| Problem::new("J-E1000", 500, "method inbox missing"))?;
        inbox
            .push(MethodCall {
                writer,
                instance: u32::try_from(index).unwrap_or(u32::MAX),
                method: u16::try_from(number).unwrap_or(u16::MAX),
                args,
                n_args: u8::try_from(m.params.len()).unwrap_or(u8::MAX),
                source: ProposalSource::Async,
                ticket: n,
                task: NO_TASK,
                ttl_ns: ttl,
            })
            .map_err(|_| Problem::new("J-E1014", 503, "the owner group's inbox is full"))?;
        let since = utc_now_ns();
        let until = since.saturating_add(i64::try_from(ttl).unwrap_or(i64::MAX));
        let call = json!({"method": format!("{instance}.{method}"), "args": req.args});
        for o in &outputs {
            let stored = self.manual.set(
                &o.path,
                level,
                Occupant {
                    principal: principal.clone(),
                    value: call.clone(),
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
        }
        let id = joker_api::server::ulid(u128::try_from(since).unwrap_or(0));
        let mut writes: Vec<Value> = outputs
            .iter()
            .enumerate()
            .map(|(i, o)| {
                json!({
                    "id": joker_api::server::ulid(u128::try_from(since).unwrap_or(0) + 1 + i as u128),
                    "output": o.path,
                    "kind": "method",
                    "state": "queued",
                    "writer": principal,
                    "level": format!("manual:{level}"),
                    "ttl": duration(ttl),
                    "lease_until": timestamp(until),
                    "generation": {"gen": v.number, "hash": v.hash},
                })
            })
            .collect();
        let mut status = 202;
        if req.wait
            && let Some(result) = self.tickets.wait_method(&v.shared, n, wait_timeout)
        {
            if let Some(code) = result.error {
                return Err(Problem::new(
                    code,
                    422,
                    format!("`{instance}.{method}` expanded to nothing"),
                ));
            }
            let mut committed = 0;
            for _ in 0..result.writes {
                let Some(r) = self.tickets.wait(&v.shared, n, wait_timeout) else {
                    break;
                };
                let Some(path) = v.plan.path(r.proposal.signal).map(str::to_owned) else {
                    continue;
                };
                let Some(signal) = v.plan.signal(&path).cloned() else {
                    continue;
                };
                let applied = match r.applied {
                    joker_core::envelope::Applied::Relinquish => json!("relinquish"),
                    joker_core::envelope::Applied::Value(b) => wire::value(&signal, b),
                };
                if let Some(w) = writes.iter_mut().find(|w| w["output"] == json!(path)) {
                    w["state"] = json!("committed");
                    w["value"] = wire::value(&signal, r.proposal.value);
                    w["outcome"] = outcome_json(r.outcome, Some(applied));
                    w["tick"] = json!({"group": group, "seq": r.tick});
                    committed += 1;
                }
            }
            if committed == result.writes as usize {
                status = 200;
            }
        }
        let body = json!({
            "id": id,
            "instance": instance,
            "method": method,
            "writes": writes,
        });
        self.tickets.put(&id, body.clone());
        Ok(Reply::with_status(status, body))
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
        let hashes: std::collections::BTreeMap<u64, String> = lock_controller(&self.controller)
            .store()
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|g| (g.number, g.hash))
            .collect();
        let exact = !outputs.is_empty() && outputs.iter().all(|s| !s.contains('*'));
        let needles: Vec<Vec<u8>> = if exact {
            let aliases = self
                .view()
                .map(|v| v.plan.aliases.clone())
                .unwrap_or_default();
            outputs
                .iter()
                .map(|s| {
                    let path = aliases.get(s).unwrap_or(s);
                    format!("\"output\":{}", json!(path)).into_bytes()
                })
                .collect()
        } else {
            Vec::new()
        };
        let needles: Vec<&[u8]> = needles.iter().map(Vec::as_slice).collect();
        let mut items: Vec<(u64, Value)> = Vec::new();
        let _ = DecisionLog::scan_filtered(
            &self.state_dir.join("decisions.jsonl"),
            !asc,
            &needles,
            |e| {
                let seq = e["seq"].as_u64().unwrap_or(0);
                if cursor.is_some_and(|c| if asc { seq <= c } else { seq >= c }) {
                    return true;
                }
                let d = match e["record"].as_str() {
                    Some("output" | "write") => decision_json(&e, &hashes),
                    Some("write_ack") => ack_decision_json(&e, &hashes),
                    _ => None,
                };
                let Some(d) = d else {
                    return true;
                };
                let out = d["output"].as_str().unwrap_or("");
                let wanted = (outputs.is_empty() || outputs.iter().any(|s| wire::matches(s, out)))
                    && (outcomes.is_empty()
                        || outcomes
                            .iter()
                            .any(|o| d["outcome"]["outcome"] == *o.as_str()))
                    && (writers.is_empty()
                        || writers.iter().any(|w| d["writer"]["writer"] == *w.as_str()))
                    && (kinds.is_empty() || kinds.iter().any(|k| d["kind"] == *k.as_str()));
                if wanted {
                    items.push((seq, d));
                }
                items.len() <= limit
            },
        );
        let next = (items.len() > limit).then(|| items[limit - 1].0.to_string());
        let page: Vec<Value> = items.into_iter().take(limit).map(|(_, d)| d).collect();
        Ok(Reply::ok(json!({"items": page, "next_cursor": next})))
    }

    fn logs(&self, query: &Query) -> Result<Reply, Problem> {
        let source = match (one(query, "source"), one(query, "app")) {
            (Some(_), Some(_)) => {
                return Err(
                    bad("J-E1012", "`source` and `app` together").with("param", json!("app"))
                );
            }
            (s, None) => s,
            (None, Some(app)) => Some(format!("app:{app}")),
        };
        let min_level = match one(query, "level") {
            None => crate::logs::LogLevel::Trace,
            Some(l) => crate::logs::LogLevel::parse(&l)
                .ok_or_else(|| bad("J-E1012", "bad `level`").with("param", json!("level")))?,
        };
        let (from_ns, to_ns) = crate::queries::range(query, utc_now_ns(), None)?;
        let limit = one(query, "limit")
            .map(|l| {
                l.parse::<usize>()
                    .map_err(|_| bad("J-E1012", "bad `limit`").with("param", json!("limit")))
            })
            .transpose()?
            .unwrap_or(100)
            .clamp(1, 1000);
        let cursor = one(query, "cursor")
            .map(|c| {
                c.parse::<u64>()
                    .map_err(|_| Problem::new("J-E1032", 400, "bad cursor"))
            })
            .transpose()?;
        let (page, next) = self.logs.query(&crate::logs::LogFilter {
            source,
            min_level,
            from_ns,
            to_ns,
            ascending: one(query, "order").is_some_and(|o| o == "asc"),
            limit,
            cursor,
        });
        let items: Vec<Value> = page
            .iter()
            .map(|e| {
                let mut item = json!({
                    "seq": e.seq,
                    "ts": timestamp(e.ts_utc_ns),
                    "level": e.level.as_str(),
                    "source": e.source,
                    "msg": e.msg,
                });
                if !e.fields.is_empty() {
                    item["fields"] = e
                        .fields
                        .iter()
                        .map(|(k, v)| (k.clone(), json!(v)))
                        .collect::<Map<String, Value>>()
                        .into();
                }
                item
            })
            .collect();
        Ok(Reply::ok(json!({
            "items": items,
            "next_cursor": next.map(|n| n.to_string()),
        })))
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
        let mut artifacts = Vec::with_capacity(req.artifacts.len());
        for (i, a) in req.artifacts.iter().enumerate() {
            digest(&a.digest, &format!("/artifacts/{i}/digest"))?;
            artifact_kind(&a.kind, &format!("/artifacts/{i}/kind"))?;
            artifacts.push(GenArtifact {
                name: a.name.clone(),
                kind: a.kind.clone(),
                digest: a.digest.clone(),
            });
        }
        let g = {
            let mut c = lock_controller(&self.controller);
            c.propose_with(
                &manifest,
                &lock,
                &artifacts,
                req.message,
                dry_run,
                Some(user(who)),
            )
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

    fn missing_blobs(&self, req: MissingBlobsRequest) -> Result<Reply, Problem> {
        if req.digests.is_empty() || req.digests.len() > 1024 {
            return Err(
                bad("J-E1012", "between 1 and 1024 digests").with("pointer", json!("/digests"))
            );
        }
        let mut missing = Vec::new();
        for (i, d) in req.digests.iter().enumerate() {
            if !self.blobs.contains(&digest(d, &format!("/digests/{i}"))?) {
                missing.push(d.clone());
            }
        }
        Ok(Reply::ok(json!({ "missing": missing })))
    }

    fn has_blob(&self, text: &str) -> Result<bool, Problem> {
        Ok(self.blobs.contains(&digest(text, "/digest")?))
    }

    fn upload_blob(
        &self,
        _who: &Principal,
        text: &str,
        kind: Option<&str>,
        bytes: &[u8],
    ) -> Result<Reply, Problem> {
        let d = digest(text, "/digest")?;
        if let Some(k) = kind {
            artifact_kind(k, "/kind")?;
        }
        let existed = self.blobs.put(&d, bytes).map_err(|e| match e {
            PutError::Mismatch { actual } => Problem::new(
                "J-E1603",
                400,
                format!("the bytes are {actual}, not {text}"),
            ),
            PutError::TooLarge => Problem::new("J-E1606", 413, "a blob carries at most 8 MiB"),
            PutError::Io(e) => Problem::new("J-E1000", 500, format!("blob store: {e}")),
        })?;
        let mut body = json!({ "digest": text, "size": bytes.len(), "existed": existed });
        if let Some(k) = kind {
            body["kind"] = json!(k);
        }
        Ok(Reply::with_status(201, body))
    }

    fn generation(&self, selector: &str) -> Result<Reply, Problem> {
        let g = self.resolve_generation(selector)?;
        let mut r = Reply::ok(self.generation_json(&g));
        r.etag = Some(g.hash.clone());
        Ok(r)
    }

    fn generation_manifest(&self, selector: &str) -> Result<String, Problem> {
        let g = self.resolve_generation(selector)?;
        let c = lock_controller(&self.controller);
        c.store()
            .content(g.number)
            .map(|(manifest, _)| manifest)
            .map_err(|e| Problem::new("J-E1000", 500, e.to_string()))
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
    fn generation_ref(&self, v: &View) -> Value {
        json!({"gen": v.number, "hash": v.hash})
    }

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
        if with_sample && let Some(sample) = self.sample_json(v, s, now) {
            m.insert("sample".into(), sample);
        }
        Value::Object(m)
    }

    /// The JSON sample of a signal, from its wide slot for a `string` or
    /// `bytes` signal (ADR 0062) and from its scalar slot otherwise.
    fn sample_json(&self, v: &View, s: &SignalPlan, now: u64) -> Option<Value> {
        let id = v.plan.id(&s.path)?;
        if crate::shared::is_wide(s.ty) {
            let sample = v.shared.wide_slot(id)?.read(8).ok()?;
            return Some(wire::wide_sample(s, &sample, now, self.utc_origin_ns));
        }
        let sample = v.shared.slots.get(id.index())?.read(8).ok()?;
        Some(wire::sample(s, &sample, now, self.utc_origin_ns))
    }
}

/// The `system` section of `Metrics` (ref/09 §5.2), read when asked and
/// never on the tick: the first thermal zone's temperature, the Raspberry
/// Pi firmware's throttling flags, the memory in use and the free space
/// of the state directory's filesystem. A value the node cannot read is
/// left out.
fn system_json(state_dir: &std::path::Path) -> Value {
    let s = crate::system::sample(state_dir);
    let mut out = serde_json::Map::new();
    if let Some(t) = s.temp_celsius {
        out.insert("temp".into(), json!(format!("{t:.1} °C")));
    }
    if let Some(t) = s.throttled {
        out.insert("throttled".into(), json!(t));
    }
    if let Some(used) = s.memory_used_bytes {
        out.insert("memory_used".into(), json!(format!("{} KiB", used / 1024)));
    }
    if let Some(free) = s.disk_free_bytes {
        out.insert("disk_free".into(), json!(format!("{} KiB", free / 1024)));
    }
    Value::Object(out)
}

/// The declared `Envelope` of one output (ref/09 §5.6), and `reads`: the
/// signals its expressions and `requires_good` read, for `jk why`
/// (ADR 0091). A key the output does not declare is left out.
fn envelope_json(v: &View, o: &OutputPlan) -> Value {
    use joker_core::output::{OnInterlock, OnRange};
    let path_of = |id: joker_core::SignalId| {
        v.plan
            .signals
            .iter()
            .find(|s| v.plan.id(&s.path) == Some(id))
            .map(|s| s.path.clone())
    };
    let quantity = |x: f64| {
        if o.unit == "1" {
            format!("{x}")
        } else {
            format!("{x} {}", o.unit)
        }
    };
    let mut m = Map::new();
    if let Some((lo, hi)) = o.range {
        m.insert("range".into(), json!([quantity(lo), quantity(hi)]));
    }
    let on_range = match o.on_range {
        OnRange::Clamp => "clamp",
        OnRange::Reject => "reject",
        OnRange::Trip => "trip",
    };
    m.insert("on_range".into(), json!(on_range));
    if let Some(rate) = o.rate {
        m.insert("rate".into(), json!(format!("{}/s", quantity(rate))));
    }
    if let Some(band) = o.deadband {
        m.insert("deadband".into(), json!(quantity(band)));
    }
    if o.min_interval_ns > 0 {
        m.insert("min_interval".into(), json!(duration(o.min_interval_ns)));
    }
    if let Some(interlock) = &o.exprs.interlock {
        m.insert("interlock".into(), json!(interlock.source));
    }
    let on_interlock = match o.on_interlock {
        OnInterlock::Safe => "safe",
        OnInterlock::Hold => "hold",
    };
    m.insert("on_interlock".into(), json!(on_interlock));
    m.insert("permit_uncertain".into(), json!(o.permit_uncertain));
    let requires_good: Vec<String> = o
        .exprs
        .requires_good
        .iter()
        .filter_map(|id| path_of(*id))
        .collect();
    m.insert("requires_good".into(), json!(requires_good));
    if o.grace_ns > 0 {
        m.insert("grace".into(), json!(duration(o.grace_ns)));
    }
    m.insert("max_lease".into(), json!(duration(o.max_lease_ns)));
    m.insert("max_manual".into(), json!(duration(o.max_manual_ns)));
    let reads: Vec<String> = o.exprs.reads().into_iter().filter_map(path_of).collect();
    m.insert("reads".into(), json!(reads));
    Value::Object(m)
}

/// The `App` of ref/09 §5.14 for one of `Controller::app_states`: its
/// name, `running` or `stopped`, and its digest when it has one (ADR 0091).
fn app_json((name, stopped, digest): &(String, bool, Option<String>)) -> Value {
    let mut out = Map::new();
    out.insert("name".into(), json!(name));
    out.insert(
        "state".into(),
        json!(if *stopped { "stopped" } else { "running" }),
    );
    if let Some(d) = digest {
        out.insert("digest".into(), json!(d));
    }
    Value::Object(out)
}

/// The `Group` of ref/09 §5.2 for group `g`: its period, its rate-monotonic
/// priority (ADR 0089), its tasks with the measured WCET of each (the
/// p99.9 of its run times, ADR 0090) and its statistics. A field the node
/// does not know is left out.
fn group_json(v: &View, g: usize, now_mono_ns: u64) -> Option<Value> {
    let plan = v.plan.groups.get(g)?;
    let stats = v.shared.groups.get(g)?;
    let periods: Vec<u64> = v.plan.groups.iter().map(|p| p.period_ns).collect();
    let tasks: Vec<Value> = plan
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut task = Map::new();
            task.insert("app".into(), json!(t.app));
            task.insert("task".into(), json!(t.task));
            if let Some(p) = stats.tasks.get(i).and_then(|h| h.quantile(0.999)) {
                task.insert("wcet_measured_p999".into(), json!(duration(p)));
            }
            Value::Object(task)
        })
        .collect();
    let mut out = Map::new();
    out.insert("name".into(), json!(plan.name));
    out.insert("period".into(), json!(duration(plan.period_ns)));
    if let Some(p) = crate::cycle::rt_priorities(&periods).get(g) {
        out.insert("priority".into(), json!(p));
    }
    out.insert(
        "order".into(),
        json!(
            plan.tasks
                .iter()
                .map(|t| format!("{}.{}", t.app, t.task))
                .collect::<Vec<_>>()
        ),
    );
    out.insert("tasks".into(), Value::Array(tasks));
    out.insert(
        "stats".into(),
        json!({
            "ticks": stats.ticks.get(),
            "overruns": stats.overruns.get(),
            "consecutive_overruns": stats.consecutive_overruns.get(),
            "jitter": percentiles(&stats.jitter, stats.max_late_ns.get(), now_mono_ns),
            "duration": percentiles(&stats.duration, stats.max_duration_ns.get(), now_mono_ns),
        }),
    );
    Some(Value::Object(out))
}

/// The `Percentiles` of a histogram (ref/09 §5.2): `p50`, `p99` and
/// `p999` as the upper bound of their bucket, never above `max`, and the
/// `window` they cover since the first value; only `max` before it.
fn percentiles(h: &crate::shared::Histogram, max: u64, now_mono_ns: u64) -> Value {
    let mut m = Map::new();
    for (name, q) in [("p50", 0.5), ("p99", 0.99), ("p999", 0.999)] {
        if let Some(v) = h.quantile(q) {
            m.insert(name.into(), json!(duration(v.min(max))));
        }
    }
    m.insert("max".into(), json!(duration(max)));
    if let Some(first) = h.first_mono_ns() {
        m.insert(
            "window".into(),
            json!(duration(now_mono_ns.saturating_sub(first))),
        );
    }
    Value::Object(m)
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

/// The `id`, `ts` and `generation` of a decision-log entry, common to
/// every kind of `Decision`.
fn decision_head(
    e: &Value,
    hashes: &std::collections::BTreeMap<u64, String>,
) -> Option<Map<String, Value>> {
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
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(joker_api::server::ulid_from(ms, u128::from(seq))),
    );
    m.insert("ts".into(), json!(ts));
    if let Some(h) = hashes.get(&number) {
        m.insert("generation".into(), json!({"gen": number, "hash": h}));
    }
    Some(m)
}

/// A `write_ack` record as a `Decision` of kind `write_ack` (ADR 0065).
fn ack_decision_json(e: &Value, hashes: &std::collections::BTreeMap<u64, String>) -> Option<Value> {
    let mut m = decision_head(e, hashes)?;
    m.insert("output".into(), e["output"].clone());
    m.insert("kind".into(), json!("write_ack"));
    m.insert(
        "ack".into(),
        json!({
            "device": e["device"],
            "write_seq": e["write_seq"],
            "write_kind": e["kind"],
            "status": e["status"],
            "code": e["code"],
        }),
    );
    Some(Value::Object(m))
}

fn decision_json(e: &Value, hashes: &std::collections::BTreeMap<u64, String>) -> Option<Value> {
    let mut m = decision_head(e, hashes)?;
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
    m.insert("tick".into(), json!({"seq": e["tick"]}));
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
    fn an_action_decl_carries_its_arguments_role_timeout_and_default_dedup_window() {
        let mut decl = DeviceActionPlan {
            name: "set".into(),
            timeout_ns: 1_000_000_000,
            role: "operator".into(),
            long: true,
            ..DeviceActionPlan::default()
        };
        decl.args.insert("level".into(), "f64 %".into());
        decl.result.insert("offset".into(), "i64".into());
        let json = action_decl_json("pcs", "device", "set", &decl, Some(("pcs.ok", Some(true))));
        assert_eq!(json["target"], "pcs");
        assert_eq!(json["target_kind"], "device");
        assert_eq!(json["action"], "set");
        assert_eq!(json["role"], "operator");
        assert_eq!(json["timeout"], "1s");
        assert_eq!(json["long"], true);
        assert_eq!(json["dedup_window"], "1m");
        assert_eq!(json["args"]["level"], json!({"type": "f64", "unit": "%"}));
        assert_eq!(json["result"]["offset"]["type"], "i64");
        assert_eq!(json["permit"], "pcs.ok");
        assert_eq!(json["permit_now"], true);
    }

    #[test]
    fn a_declared_dedup_window_is_rendered_as_a_duration() {
        let decl = DeviceActionPlan {
            name: "reset_fault".into(),
            timeout_ns: 1_000_000_000,
            dedup_window_ns: Some(30_000_000_000),
            role: "admin".into(),
            ..DeviceActionPlan::default()
        };
        let json = action_decl_json("p1", "instance", "reset", &decl, None);
        assert_eq!(json["role"], "admin");
        assert_eq!(json["target_kind"], "instance");
        assert_eq!(json["dedup_window"], "30s");
        assert!(json.get("permit").is_none());
    }
}
