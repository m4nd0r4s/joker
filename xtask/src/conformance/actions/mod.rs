//! The `action` suites (ADR 0066 §20–§24): device and instance actions
//! of the runtime, run against the daemon's own controller, invocation
//! hub and io tasks, with a register double in place of each driver, and
//! again with the real `modbus-tcp` driver against a wire double (ADR
//! 0073).

pub mod double;
pub mod wire;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use joker_core::{ClockQuality, QualityCode, QualityReason};
use joker_driver_modbus::{ModbusTcp, ModbusTcpPlanner};
use joker_hal::cbor::Cbor;
use joker_hal::std_host::{TokioHost, TokioTcpConnector};
use joker_hal::types::{BatchFailure, BatchKind, Direction, Poll, RawStatus, Reason};
use joker_hal::{ActionSpec, Driver, Link, OpenArgs, PlanRequest, Planner, TcpEndpoint};
use jokerd::actions::{Answer, Hub, MAX_WAIT, Refusal, Request, Role};
use jokerd::control::{Config, Controller};
use jokerd::decisions::DecisionLog;
use jokerd::devices::{DeviceSet, Devices};
use jokerd::drivers::{ActionCall, point_targets, run_driver};
use jokerd::pacer::{Monotonic, SystemPacer};
use jokerd::plan::Plan;
use serde_json::{Map, Value as Json, json};
use toml::{Table, Value};

use self::double::{Effect, Native, Reg, RegsDriver, Spec};
use super::model::Diagnostic;
use super::report::{CaseReport, Status, SuiteReport};

/// The suite kind of this area.
pub const KIND: &str = "action";

/// Whether `text` is a suite of this area (`[suite] kind = "action"`).
pub fn is_action_suite(text: &str) -> bool {
    text.parse::<Table>().ok().is_some_and(|t| {
        t.get("suite")
            .and_then(|s| s.get("kind"))
            .and_then(Value::as_str)
            == Some(KIND)
    })
}

#[derive(Debug)]
struct Suite {
    id: String,
    spec: Vec<String>,
    manifest: String,
    profiles: Vec<(String, String)>,
    doubles: Vec<Spec>,
    cases: Vec<Case>,
}

/// Which double a case runs against (ADR 0073).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The register double in place of every driver (ADR 0066 §21).
    Register,
    /// The real driver of every `modbus-tcp` device, against the wire
    /// double; the register double for the others.
    Wire,
}

impl Mode {
    fn suffix(self) -> &'static str {
        match self {
            Self::Register => "",
            Self::Wire => "@wire",
        }
    }
}

#[derive(Debug)]
struct Case {
    id: String,
    name: Option<String>,
    spec: Vec<String>,
    doubles: Vec<Mode>,
    steps: Vec<Step>,
    expect: Vec<(String, Expect)>,
}

fn doubles_of(t: &Table, at: &str) -> Result<Vec<Mode>, Diagnostic> {
    let Some(v) = t.get("doubles") else {
        return Ok(vec![Mode::Register, Mode::Wire]);
    };
    let list = v
        .as_array()
        .ok_or_else(|| diag(at, "`doubles` is a list of `register` and `wire`"))?;
    let mut modes = Vec::new();
    for item in list {
        let mode = match item.as_str() {
            Some("register") => Mode::Register,
            Some("wire") => Mode::Wire,
            _ => return Err(diag(at, "`doubles` is a list of `register` and `wire`")),
        };
        if !modes.contains(&mode) {
            modes.push(mode);
        }
    }
    if modes.is_empty() {
        return Err(diag(at, "`doubles` names at least one double"));
    }
    Ok(modes)
}

#[derive(Debug)]
enum Step {
    Invoke {
        at_ns: u64,
        target: String,
        action: String,
        reference: String,
        key: String,
        args: Map<String, Json>,
        timeout_ns: Option<u64>,
        reason: Option<String>,
        wait: bool,
        role: Role,
    },
    Cancel {
        at_ns: u64,
        reference: String,
    },
    Settle {
        at_ns: u64,
    },
}

#[derive(Debug, Default)]
struct InvocationExpect {
    reference: String,
    state: Option<String>,
    result: Option<Json>,
    error: Option<String>,
    code: Option<String>,
    message: Option<String>,
    replay: Option<bool>,
    rejected: Option<String>,
    progress: Option<f64>,
}

#[derive(Debug, Default)]
struct DecisionExpect {
    reference: String,
    outcome: Option<String>,
    kind: Option<String>,
    code: Option<String>,
    duplicate: Option<bool>,
    count: Option<usize>,
    instance: Option<String>,
    device: Option<String>,
}

#[derive(Debug)]
enum Expect {
    Invocation(InvocationExpect),
    Decision(DecisionExpect),
    Writes {
        device: String,
        equals: Vec<(i64, Reg)>,
    },
    Register {
        device: String,
        at: i64,
        equals: Reg,
    },
}

fn diag(location: impl Into<String>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: "J0887",
        location: location.into(),
        message: message.into(),
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn text(t: &Table, key: &str) -> Option<String> {
    t.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn duration(t: &Table, key: &str, at: &str) -> Result<Option<u64>, Diagnostic> {
    match t.get(key) {
        None => Ok(None),
        Some(v) => {
            let s = v
                .as_str()
                .ok_or_else(|| diag(at, format!("`{key}` is a duration")))?;
            super::literal::duration_ns(s)
                .map(Some)
                .map_err(|e| diag(at, format!("`{key}`: {}", e.message)))
        }
    }
}

fn json_of(v: &Value) -> Json {
    match v {
        Value::String(s) => json!(s),
        Value::Integer(i) => json!(i),
        Value::Float(f) => json!(f),
        Value::Boolean(b) => json!(b),
        Value::Datetime(d) => json!(d.to_string()),
        Value::Array(a) => Json::Array(a.iter().map(json_of).collect()),
        Value::Table(t) => Json::Object(t.iter().map(|(k, v)| (k.clone(), json_of(v))).collect()),
    }
}

fn status_of(literal: &str, at: &str) -> Result<RawStatus, Diagnostic> {
    let (code, reason) = match literal.split_once('(') {
        Some((c, r)) => (c, r.trim_end_matches(')')),
        None => (literal, "none"),
    };
    let code = match code {
        "good" => QualityCode::Good,
        "uncertain" => QualityCode::Uncertain,
        "bad" => QualityCode::Bad,
        _ => return Err(diag(at, format!("`{literal}` is not a quality"))),
    };
    let reason = (0..=u8::MAX)
        .find(|r| QualityReason::from_u8(*r).name() == Some(reason))
        .ok_or_else(|| diag(at, format!("`{reason}` is not a quality reason")))?;
    Ok(RawStatus {
        code,
        reason: Reason(reason),
    })
}

fn address(key: &str, at: &str) -> Result<i64, Diagnostic> {
    key.parse()
        .map_err(|_| diag(at, format!("`{key}` is not a register address")))
}

fn reg(v: &Value, at: &str) -> Result<Reg, Diagnostic> {
    Reg::of(v).ok_or_else(|| diag(at, "a register holds a boolean, an integer or a float"))
}

fn double_of(t: &Table, at: &str) -> Result<Spec, Diagnostic> {
    let device = text(t, "device").ok_or_else(|| diag(at, "`device` is required"))?;
    let mut spec = Spec {
        device,
        ..Spec::default()
    };
    if let Some(regs) = t.get("registers").and_then(Value::as_table) {
        for (k, v) in regs {
            spec.registers.insert(address(k, at)?, reg(v, at)?);
        }
    }
    for effect in t
        .get("on_write")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let e = effect
            .as_table()
            .ok_or_else(|| diag(at, "`on_write` holds tables"))?;
        let mut set = Vec::new();
        for (k, v) in e.get("set").and_then(Value::as_table).into_iter().flatten() {
            set.push((address(k, at)?, reg(v, at)?));
        }
        spec.on_write.push(Effect {
            at: e
                .get("at")
                .and_then(Value::as_integer)
                .ok_or_else(|| diag(at, "`on_write.at` is an integer"))?,
            value: reg(
                e.get("value")
                    .ok_or_else(|| diag(at, "`on_write.value` is required"))?,
                at,
            )?,
            set,
        });
    }
    for (k, v) in t
        .get("quality")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
    {
        let literal = v
            .as_str()
            .ok_or_else(|| diag(at, "a quality is a literal"))?;
        spec.quality
            .insert(address(k, at)?, status_of(literal, at)?);
    }
    for (k, v) in t
        .get("fail")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
    {
        let failure = match v.as_str() {
            Some("device_fault") => BatchFailure::DeviceFault("the double faults"),
            Some("timeout") => BatchFailure::Timeout,
            Some("comm_failure") => BatchFailure::CommFailure("the double is cut"),
            _ => {
                return Err(diag(
                    at,
                    "`fail` is `device_fault`, `timeout` or `comm_failure`",
                ));
            }
        };
        spec.fail.insert(address(k, at)?, failure);
    }
    spec.link_down = text(t, "link").as_deref() == Some("down");
    for (name, n) in t
        .get("native")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
    {
        let n = n
            .as_table()
            .ok_or_else(|| diag(at, "`native.<action>` is a table"))?;
        let mut native = Native {
            failure: text(n, "failure"),
            hold_ns: duration(n, "hold", at)?.unwrap_or(0),
            ..Native::default()
        };
        for (k, v) in n
            .get("result")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
        {
            native.result.push((k.clone(), reg(v, at)?));
        }
        spec.native.insert(name.clone(), native);
    }
    Ok(spec)
}

fn step_of(t: &Table, at: &str) -> Result<Step, Diagnostic> {
    let at_ns = duration(t, "at", at)?.ok_or_else(|| diag(at, "`at` is required"))?;
    if let Some(invoke) = text(t, "invoke") {
        let (target, action) = invoke
            .split_once('.')
            .ok_or_else(|| diag(at, "`invoke` is `<target>.<action>`"))?;
        let key = text(t, "key").ok_or_else(|| diag(at, "`key` is required"))?;
        let role = match text(t, "role") {
            None => Role::Operator,
            Some(r) => Role::parse(&r).ok_or_else(|| diag(at, format!("`{r}` is not a role")))?,
        };
        let args = match t.get("args").map(json_of) {
            None => Map::new(),
            Some(Json::Object(m)) => m,
            Some(_) => return Err(diag(at, "`args` is a table")),
        };
        return Ok(Step::Invoke {
            at_ns,
            target: target.to_owned(),
            action: action.to_owned(),
            reference: text(t, "ref").unwrap_or_else(|| key.clone()),
            key,
            args,
            timeout_ns: duration(t, "timeout", at)?,
            reason: text(t, "reason"),
            wait: t.get("wait").and_then(Value::as_bool).unwrap_or(true),
            role,
        });
    }
    if let Some(reference) = text(t, "cancel") {
        return Ok(Step::Cancel { at_ns, reference });
    }
    if t.get("settle").and_then(Value::as_bool) == Some(true) {
        return Ok(Step::Settle { at_ns });
    }
    Err(diag(at, "an action is `invoke`, `cancel` or `settle`"))
}

fn expect_of(t: &Table, at: &str) -> Result<Expect, Diagnostic> {
    if let Some(reference) = text(t, "invocation") {
        return Ok(Expect::Invocation(InvocationExpect {
            reference,
            state: text(t, "state"),
            result: t.get("result").map(json_of),
            error: text(t, "error"),
            code: text(t, "code"),
            message: text(t, "message"),
            replay: t.get("replay").and_then(Value::as_bool),
            rejected: text(t, "rejected"),
            progress: t
                .get("progress")
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))),
        }));
    }
    if let Some(reference) = text(t, "decision") {
        return Ok(Expect::Decision(DecisionExpect {
            reference,
            outcome: text(t, "outcome"),
            kind: text(t, "kind"),
            code: text(t, "code"),
            duplicate: t.get("duplicate").and_then(Value::as_bool),
            count: t
                .get("count")
                .and_then(Value::as_integer)
                .and_then(|c| usize::try_from(c).ok()),
            instance: text(t, "instance"),
            device: text(t, "device"),
        }));
    }
    if let Some(device) = text(t, "writes") {
        let mut equals = Vec::new();
        for pair in t
            .get("equals")
            .and_then(Value::as_array)
            .ok_or_else(|| diag(at, "`writes` needs `equals = [[at, value], …]`"))?
        {
            let pair = pair
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or_else(|| diag(at, "each write is `[at, value]`"))?;
            let a = pair[0]
                .as_integer()
                .ok_or_else(|| diag(at, "a write's address is an integer"))?;
            equals.push((a, reg(&pair[1], at)?));
        }
        return Ok(Expect::Writes { device, equals });
    }
    if let Some(device) = text(t, "register") {
        return Ok(Expect::Register {
            device,
            at: t
                .get("at")
                .and_then(Value::as_integer)
                .ok_or_else(|| diag(at, "`register` needs `at`"))?,
            equals: reg(
                t.get("equals")
                    .ok_or_else(|| diag(at, "`register` needs `equals`"))?,
                at,
            )?,
        });
    }
    Err(diag(
        at,
        "an expectation is `invocation`, `decision`, `writes` or `register`",
    ))
}

fn load(text_in: &str) -> Result<Suite, Vec<Diagnostic>> {
    let root: Table = text_in
        .parse()
        .map_err(|e: toml::de::Error| vec![diag("file", e.message().to_owned())])?;
    let one = |d: Diagnostic| vec![d];
    if root.get("schema").and_then(Value::as_integer) != Some(1) {
        return Err(one(diag("schema", "`schema = 1` is required")));
    }
    let suite = root
        .get("suite")
        .and_then(Value::as_table)
        .ok_or_else(|| one(diag("suite", "`[suite]` is required")))?;
    let id = text(suite, "id").ok_or_else(|| one(diag("suite", "`id` is required")))?;
    if strings(suite.get("hosts")) != ["linux"] {
        return Err(one(diag(
            "suite",
            "an `action` suite runs on `hosts = [\"linux\"]` only (ADR 0066 §20)",
        )));
    }
    let fixture = root
        .get("fixture")
        .and_then(Value::as_table)
        .ok_or_else(|| one(diag("fixture", "`[fixture]` is required")))?;
    let manifest =
        text(fixture, "manifest").ok_or_else(|| one(diag("fixture", "`manifest` is required")))?;
    let profiles = fixture
        .get("profiles")
        .and_then(Value::as_table)
        .map(|t| {
            t.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let mut doubles = Vec::new();
    for (i, d) in root
        .get("double")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let at = format!("double[{i}]");
        let t = d
            .as_table()
            .ok_or_else(|| one(diag(&at, "`[[double]]` is a table")))?;
        doubles.push(double_of(t, &at).map_err(one)?);
    }
    let mut cases = Vec::new();
    let mut errors = Vec::new();
    for (i, c) in root
        .get("case")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let at = format!("case[{i}]");
        let Some(t) = c.as_table() else {
            errors.push(diag(&at, "`[[case]]` is a table"));
            continue;
        };
        let Some(case_id) = text(t, "id") else {
            errors.push(diag(&at, "`id` is required"));
            continue;
        };
        let mut steps = Vec::new();
        for (j, s) in t
            .get("do")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let where_ = format!("{at}.do[{j}]");
            match s.as_table().map(|s| step_of(s, &where_)) {
                Some(Ok(step)) => steps.push(step),
                Some(Err(e)) => errors.push(e),
                None => errors.push(diag(where_, "an action is a table")),
            }
        }
        let mut expect = Vec::new();
        for (j, e) in t
            .get("expect")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let where_ = format!("{at}.expect[{j}]");
            match e.as_table().map(|e| expect_of(e, &where_)) {
                Some(Ok(x)) => expect.push((e.to_string(), x)),
                Some(Err(err)) => errors.push(err),
                None => errors.push(diag(where_, "an expectation is a table")),
            }
        }
        if expect.is_empty() {
            errors.push(diag(&at, "`expect` is required"));
        }
        let doubles = match doubles_of(t, &at) {
            Ok(d) => d,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        cases.push(Case {
            id: case_id,
            name: text(t, "name"),
            spec: strings(t.get("spec")),
            doubles,
            steps,
            expect,
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Suite {
        id,
        spec: strings(suite.get("spec")),
        manifest,
        profiles,
        doubles,
        cases,
    })
}

struct DoubleDevices {
    handle: tokio::runtime::Handle,
    doubles: BTreeMap<String, double::Shared>,
    mode: Mode,
}

struct DoubleSet {
    stop: tokio::sync::watch::Sender<bool>,
    senders: BTreeMap<String, tokio::sync::mpsc::Sender<ActionCall>>,
}

impl DeviceSet for DoubleSet {
    fn stop(self: Box<Self>) {
        let _ = self.stop.send(true);
    }

    fn action_sender(&self, device: &str) -> Option<tokio::sync::mpsc::Sender<ActionCall>> {
        self.senders.get(device).cloned()
    }
}

fn read_plan(request: &PlanRequest<'_>) -> joker_hal::plan::Plan {
    let mut by_period: BTreeMap<u64, Vec<u32>> = BTreeMap::new();
    for p in request.points {
        if let (Direction::Input, Poll::Period(ns)) = (p.direction, p.poll) {
            by_period.entry(ns).or_default().push(p.id);
        }
    }
    let mut plan = joker_hal::plan::Plan::new();
    for (period, points) in by_period {
        plan.push(0, BatchKind::Poll, period, &points, 0, 0, &[]);
    }
    plan
}

impl Devices for DoubleDevices {
    fn start(&self, plan: &Arc<Plan>, shared: &Arc<jokerd::shared::Shared>) -> Box<dyn DeviceSet> {
        let (stop, stop_rx) = tokio::sync::watch::channel(false);
        let host = Arc::new(TokioHost::new(ClockQuality::Synced));
        let mut senders = BTreeMap::new();
        for (index, device) in plan.devices.iter().enumerate() {
            let state = self.doubles.get(&device.name).cloned().unwrap_or_else(|| {
                double::shared(&Spec {
                    device: device.name.clone(),
                    ..Spec::default()
                })
            });
            let (tx, rx) = tokio::sync::mpsc::channel(jokerd::actions::QUEUE_DEPTH);
            senders.insert(device.name.clone(), tx);
            if self.mode == Mode::Wire && device.driver == "modbus-tcp" {
                start_wire(
                    &self.handle,
                    state,
                    device.clone(),
                    Arc::clone(plan),
                    index,
                    Arc::clone(shared),
                    Arc::clone(&host),
                    rx,
                    stop_rx.clone(),
                );
                continue;
            }
            let device = device.clone();
            let plan = Arc::clone(plan);
            let shared = Arc::clone(shared);
            let host = Arc::clone(&host);
            let mut stop = stop_rx.clone();
            self.handle.spawn(async move {
                let owned = jokerd::bridge::owned_request(&device);
                let views = owned.views();
                let specs: Vec<ActionSpec<'_>> = device
                    .actions
                    .iter()
                    .filter_map(|a| {
                        Some(ActionSpec {
                            id: a.native_id,
                            device: 0,
                            name: &a.name,
                            native: Cbor::new(a.native.as_deref()?),
                            timeout_ns: a.timeout_ns,
                            long: a.long,
                        })
                    })
                    .collect();
                let request = PlanRequest {
                    actions: &specs,
                    ..views.request()
                };
                let hal_plan = read_plan(&request);
                let driver = RegsDriver::new(state, &request);
                let targets = point_targets(&plan, index);
                run_driver(
                    driver,
                    &*host,
                    request,
                    hal_plan.view(),
                    device.timeout_ns,
                    &targets,
                    &shared,
                    index,
                    device.fallback_keepalive_ns,
                    rx,
                    move || *stop.borrow_and_update(),
                )
                .await;
            });
        }
        Box::new(DoubleSet { stop, senders })
    }
}

fn wire_listener(state: &double::Shared) -> std::io::Result<(std::net::TcpListener, String)> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?.to_string();
    let down = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .spec
        .link_down;
    if down {
        drop(listener);
        let refused = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = refused.local_addr()?.to_string();
        drop(refused);
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            address,
        ));
    }
    Ok((listener, address))
}

#[allow(
    clippy::too_many_arguments,
    reason = "the same pieces `run_driver` takes for one device, plus the double it serves"
)]
fn start_wire(
    handle: &tokio::runtime::Handle,
    state: double::Shared,
    device: jokerd::bridge::DevicePlan,
    plan: Arc<Plan>,
    index: usize,
    shared: Arc<jokerd::shared::Shared>,
    host: Arc<TokioHost>,
    rx: tokio::sync::mpsc::Receiver<ActionCall>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let address = match wire_listener(&state) {
        Ok((listener, address)) => {
            handle.spawn(wire::serve_on(listener, state));
            address
        }
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => e.to_string(),
        Err(e) => {
            eprintln!("wire double of `{}`: {e}", device.name);
            return;
        }
    };
    handle.spawn(async move {
        let owned = jokerd::bridge::owned_request(&device);
        let views = owned.views();
        let request = views.request();
        let hal_plan = match ModbusTcpPlanner::plan(&request) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("plan of `{}`: {e:?}", device.name);
                return;
            }
        };
        let link = Link::Tcp(TcpEndpoint::new(TokioTcpConnector::to(&address)));
        let driver = match ModbusTcp::open(&OpenArgs::new(request, hal_plan.view()), link) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("open of `{}`: {e:?}", device.name);
                return;
            }
        };
        let targets = point_targets(&plan, index);
        run_driver(
            driver,
            &*host,
            request,
            hal_plan.view(),
            device.timeout_ns,
            &targets,
            &shared,
            index,
            device.fallback_keepalive_ns,
            rx,
            move || *stop.borrow_and_update(),
        )
        .await;
    });
}

enum Observed {
    Answer(Answer),
    Refused(Refusal),
}

struct Run {
    hub: Arc<Hub>,
    observed: BTreeMap<String, Observed>,
    records: Vec<Json>,
    doubles: BTreeMap<String, double::Shared>,
}

/// A directory of its own for one case, removed with it.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "joker-action-case-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn profile_path(dir: &Path, reference: &str) -> Option<std::path::PathBuf> {
    let (vendor, model) = reference.split_once('/')?;
    Some(
        dir.join("profiles")
            .join(vendor)
            .join(format!("{model}.profile.toml")),
    )
}

fn run_case(suite: &Suite, case: &Case, mode: Mode, dir: &Path) -> Result<Run, String> {
    for (reference, text) in &suite.profiles {
        let path = profile_path(dir, reference)
            .ok_or_else(|| format!("`{reference}` is not `<vendor>/<model>@<major>`"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
    }
    let doubles: BTreeMap<String, double::Shared> = suite
        .doubles
        .iter()
        .map(|d| (d.device.clone(), double::shared(d)))
        .collect();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.join("decisions.jsonl")).map_err(|e| e.to_string())?,
    ));
    let clock = Monotonic::start();
    let devices = Arc::new(DoubleDevices {
        handle: runtime.handle().clone(),
        doubles: doubles.clone(),
        mode,
    });
    let mut controller = Controller::new(
        dir,
        log,
        Arc::new(jokerd::native::NativeTasks),
        devices,
        Box::new(move || Box::new(SystemPacer::new(clock, ClockQuality::Unsynced))),
        Config {
            project_root: dir.to_path_buf(),
            ..Config::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let g = controller
        .propose(&suite.manifest, "", None, false)
        .map_err(|e| format!("{e}: {:?}", e.errors))?;
    controller
        .activate(g.number, None, None)
        .map_err(|e| format!("{e}: {:?}", e.errors))?;
    let hub = controller.actions();
    let controller = Arc::new(Mutex::new(controller));
    let start = Instant::now();
    let mut observed = BTreeMap::new();
    for step in &case.steps {
        let at_ns = match step {
            Step::Invoke { at_ns, .. } | Step::Cancel { at_ns, .. } | Step::Settle { at_ns } => {
                *at_ns
            }
        };
        let due = start + Duration::from_nanos(at_ns);
        if let Some(left) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(left);
        }
        match step {
            Step::Invoke {
                target,
                action,
                reference,
                key,
                args,
                timeout_ns,
                reason,
                wait,
                role,
                ..
            } => {
                let request = Request {
                    key: key.clone(),
                    args: args.clone(),
                    timeout_ns: *timeout_ns,
                    reason: reason.clone(),
                    wait: *wait,
                    principal: "user:conformance".into(),
                    role: Some(*role),
                };
                let result = jokerd::actions::invoke(&controller, &hub, target, action, request);
                let entry = match result {
                    Ok(answer) => {
                        let long = hub.get(&answer.id).is_some_and(|i| i.long);
                        if *wait && !long {
                            hub.wait(&answer.id, MAX_WAIT);
                        }
                        Observed::Answer(answer)
                    }
                    Err(r) => Observed::Refused(r),
                };
                observed.insert(reference.clone(), entry);
            }
            Step::Cancel { reference, .. } => {
                if let Some(Observed::Answer(a)) = observed.get(reference) {
                    let _ = hub.cancel(&a.id);
                }
            }
            Step::Settle { .. } => {
                hub.settle(MAX_WAIT);
            }
        }
    }
    hub.settle(MAX_WAIT);
    jokerd::daemon::lock_controller(&controller).shutdown();
    runtime.shutdown_timeout(Duration::from_secs(2));
    let records = DecisionLog::read_all(&dir.join("decisions.jsonl"))
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|r| r["record"] == "action")
        .collect();
    Ok(Run {
        hub,
        observed,
        records,
        doubles,
    })
}

fn same_json(expected: &Json, actual: &Json) -> bool {
    match (expected, actual) {
        (Json::Number(a), Json::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(a), Some(b)) => (a - b).abs() < 1e-9,
            _ => a == b,
        },
        (Json::Object(a), Json::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|w| same_json(v, w)))
        }
        (Json::Array(a), Json::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_json(x, y))
        }
        (a, b) => a == b,
    }
}

fn check(field: &str, expected: Option<&str>, actual: Option<&str>, out: &mut Vec<String>) {
    if let Some(e) = expected
        && actual != Some(e)
    {
        out.push(format!(
            "{field}: expected `{e}`, got `{}`",
            actual.unwrap_or("none")
        ));
    }
}

fn evaluate(run: &Run, expect: &Expect) -> Vec<String> {
    let mut out = Vec::new();
    match expect {
        Expect::Invocation(x) => match run.observed.get(&x.reference) {
            None => out.push(format!("no invocation `{}` in the case", x.reference)),
            Some(Observed::Refused(r)) => match &x.rejected {
                Some(code) if code == r.code => {}
                _ => out.push(format!("refused with {}: {}", r.code, r.detail)),
            },
            Some(Observed::Answer(a)) => {
                if let Some(code) = &x.rejected {
                    out.push(format!("expected a refusal with {code}, it was admitted"));
                    return out;
                }
                let Some(inv) = run.hub.get(&a.id) else {
                    out.push("the invocation is no longer kept".into());
                    return out;
                };
                let j = inv.json(a.replay);
                check("state", x.state.as_deref(), j["state"].as_str(), &mut out);
                check(
                    "error",
                    x.error.as_deref(),
                    j["error"]["kind"].as_str(),
                    &mut out,
                );
                check(
                    "code",
                    x.code.as_deref(),
                    j["error"]["code"].as_str(),
                    &mut out,
                );
                if let Some(m) = &x.message
                    && !j["error"]["message"]
                        .as_str()
                        .is_some_and(|s| s.contains(m.as_str()))
                {
                    out.push(format!(
                        "message: expected to contain `{m}`, got {}",
                        j["error"]
                    ));
                }
                if let Some(r) = x.replay
                    && j["replay"].as_bool() != Some(r)
                {
                    out.push(format!("replay: expected {r}, got {}", j["replay"]));
                }
                if let Some(r) = &x.result
                    && !same_json(r, &j["result"])
                {
                    out.push(format!("result: expected {r}, got {}", j["result"]));
                }
                if let Some(p) = x.progress
                    && !j["progress"]["fraction"]
                        .as_f64()
                        .is_some_and(|f| (f - p).abs() < 1e-9)
                {
                    out.push(format!("progress: expected {p}, got {}", j["progress"]));
                }
            }
        },
        Expect::Decision(x) => {
            let (own, original) = match run.observed.get(&x.reference) {
                None => {
                    out.push(format!("no invocation `{}` in the case", x.reference));
                    return out;
                }
                Some(Observed::Answer(a)) => (Some(a.record.clone()), Some(a.id.clone())),
                Some(Observed::Refused(r)) => (r.invocation.clone(), None),
            };
            let mine: Vec<&Json> = run
                .records
                .iter()
                .filter(|r| r["invocation"].as_str() == own.as_deref())
                .collect();
            if let Some(n) = x.count
                && mine.len() != n
            {
                out.push(format!("count: expected {n} records, got {}", mine.len()));
            }
            let Some(record) = mine.last() else {
                out.push("no decision record".into());
                return out;
            };
            check(
                "outcome",
                x.outcome.as_deref(),
                record["outcome"].as_str(),
                &mut out,
            );
            check("kind", x.kind.as_deref(), record["kind"].as_str(), &mut out);
            check("code", x.code.as_deref(), record["code"].as_str(), &mut out);
            check(
                "instance",
                x.instance.as_deref(),
                record["instance"].as_str(),
                &mut out,
            );
            check(
                "device",
                x.device.as_deref(),
                record["device"].as_str(),
                &mut out,
            );
            if let Some(d) = x.duplicate {
                let names_original = record["duplicate_of"].as_str().is_some()
                    && record["duplicate_of"].as_str() == original.as_deref();
                if names_original != d {
                    out.push(format!(
                        "duplicate: expected {d}, got duplicate_of {}",
                        record["duplicate_of"]
                    ));
                }
            }
        }
        Expect::Writes { device, equals } => {
            let writes = run
                .doubles
                .get(device)
                .map(|d| {
                    d.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .writes
                        .clone()
                })
                .unwrap_or_default();
            let same = writes.len() == equals.len()
                && writes
                    .iter()
                    .zip(equals)
                    .all(|((a, v), (b, w))| a == b && v.same(*w));
            if !same {
                let show = |w: &[(i64, Reg)]| {
                    w.iter()
                        .map(|(a, v)| format!("[{a}, {}]", v.text()))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                out.push(format!(
                    "writes: expected [{}], got [{}]",
                    show(equals),
                    show(&writes)
                ));
            }
        }
        Expect::Register { device, at, equals } => {
            let value = run.doubles.get(device).and_then(|d| {
                d.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .registers
                    .get(at)
                    .copied()
            });
            if !value.is_some_and(|v| v.same(*equals)) {
                out.push(format!(
                    "register {device}.{at}: expected {}, got {}",
                    equals.text(),
                    value.map_or("none".into(), Reg::text)
                ));
            }
        }
    }
    out
}

fn failure(index: usize, source: &str, message: String) -> super::eval::Failure {
    super::eval::Failure {
        index,
        source: source.to_owned(),
        tick: 0,
        at: "the end of the case".to_owned(),
        subject: "action".to_owned(),
        expected: source.to_owned(),
        actual: message,
    }
}

/// Runs one `action` suite file.
pub fn run_file(path: &Path, text_in: &str, filters: &[String]) -> SuiteReport {
    let file = path.display().to_string();
    let suite = match load(text_in) {
        Ok(s) => s,
        Err(diagnostics) => {
            return SuiteReport {
                id: None,
                file,
                diagnostics,
                cases: Vec::new(),
            };
        }
    };
    let mut cases = Vec::new();
    for (case, mode) in suite
        .cases
        .iter()
        .flat_map(|c| c.doubles.iter().map(move |m| (c, *m)))
    {
        let id = format!("{}.{}{}", suite.id, case.id, mode.suffix());
        if !super::selected(&id, filters) {
            continue;
        }
        let mut report = CaseReport {
            id,
            name: case.name.clone(),
            spec: if case.spec.is_empty() {
                suite.spec.clone()
            } else {
                case.spec.clone()
            },
            status: Status::Pass,
            known: None,
            missing: Vec::new(),
            diagnostics: Vec::new(),
            failures: Vec::new(),
            ticks: 0,
            digest: None,
        };
        let outcome = Scratch::new()
            .map_err(|e| e.to_string())
            .and_then(|dir| run_case(&suite, case, mode, &dir.0));
        match outcome {
            Err(e) => {
                report.status = Status::Error;
                report.diagnostics.push(diag("fixture", e));
            }
            Ok(run) => {
                for (i, (source, expect)) in case.expect.iter().enumerate() {
                    for message in evaluate(&run, expect) {
                        report.failures.push(failure(i, source, message));
                    }
                }
                if !report.failures.is_empty() {
                    report.status = Status::Fail;
                }
            }
        }
        cases.push(report);
    }
    SuiteReport {
        id: Some(suite.id),
        file,
        diagnostics: Vec::new(),
        cases,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
schema = 1

[suite]
kind  = "action"
id    = "actions.t"
hosts = ["linux"]

[fixture]
manifest = "x"

[[double]]
device    = "pcs"
registers = { "1" = 2 }
quality   = { "1" = "uncertain(stale)" }

[[case]]
id     = "c"
do     = [{ at = "0ms", invoke = "pcs.a", key = "k" }]
expect = [{ invocation = "k", state = "succeeded" }]
"#;

    #[test]
    fn an_action_suite_is_recognised_by_its_kind() {
        assert!(is_action_suite(MINIMAL));
        assert!(!is_action_suite("schema = 1\n[suite]\nkind = \"core\"\n"));
        assert!(!is_action_suite("not toml ["));
    }

    #[test]
    fn a_minimal_suite_loads() {
        let suite = load(MINIMAL).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(suite.id, "actions.t");
        assert_eq!(suite.cases.len(), 1);
        assert_eq!(suite.doubles[0].registers.get(&1), Some(&Reg::Int(2)));
        assert_eq!(
            suite.doubles[0].quality.get(&1).map(|s| s.code),
            Some(QualityCode::Uncertain)
        );
    }

    #[test]
    fn a_suite_for_another_host_or_with_a_bad_case_is_malformed() {
        let mcu = MINIMAL.replace("hosts = [\"linux\"]", "hosts = [\"linux\", \"mcu\"]");
        assert!(load(&mcu).is_err_and(|d| d.iter().all(|d| d.code == "J0887")));
        let bad = MINIMAL.replace("invoke = \"pcs.a\"", "invoke = \"pcs\"");
        assert!(load(&bad).is_err());
        let no_expect = MINIMAL.replace(
            "expect = [{ invocation = \"k\", state = \"succeeded\" }]",
            "",
        );
        assert!(load(&no_expect).is_err());
    }

    #[test]
    fn a_case_runs_against_both_doubles_unless_it_names_them() {
        let both = load(MINIMAL).unwrap();
        assert_eq!(both.cases[0].doubles, [Mode::Register, Mode::Wire]);
        let one = MINIMAL.replace("id     = \"c\"", "id     = \"c\"\ndoubles = [\"register\"]");
        assert_eq!(load(&one).unwrap().cases[0].doubles, [Mode::Register]);
        for bad in ["[]", "[\"bus\"]", "\"wire\""] {
            let text = MINIMAL.replace(
                "id     = \"c\"",
                &format!("id     = \"c\"\ndoubles = {bad}"),
            );
            assert!(load(&text).is_err_and(|d| d.iter().all(|d| d.code == "J0887")));
        }
    }

    #[test]
    fn json_values_compare_numerically() {
        assert!(same_json(&json!({"a": 1}), &json!({"a": 1.0})));
        assert!(!same_json(&json!({"a": 1}), &json!({"a": 1, "b": 2})));
        assert!(same_json(&json!([1, "x"]), &json!([1.0, "x"])));
    }
}
