//! The `sim` driver (tier N, ref/01 appendix A): one simulated device per
//! instance, served from its model.
//!
//! The device's `config` carries the model and what it refers to (ADR 0051,
//! ADR 0054): `model` is the source text of the `*.sim.toml` file, `params`
//! the device's `params` table, `profile` the source of the profile named by
//! `[model] profile`, `files` the bytes of the files the model names (by the
//! path as written in it) and `seed` a scenario's seed override. Each
//! point's address is `{ signal = "<name>" }`; the model's `[actions]` are
//! native actions.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::string::String;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::vec::Vec;

use joker_hal::descriptor::INTERFACE;
use joker_hal::plan::Plan;
use joker_hal::types::{Diagnostic, J0809, J0852, J0853};
use joker_hal::{
    ActionCall, ActionFailure, ActionOut, BatchFailure, BatchKind, BusKind, Capabilities,
    Capability, Cbor, CommittedWrite, ConnectOut, Cx, Descriptor, DeviceInfo, Direction, Driver,
    DriverError, HealthOut, InstanceMode, Link, OpenArgs, PlanRequest, Planner, PointId, Poll,
    RawKind, RawSample, RawSink, RawValue, ReadBatch, Tier, WriteAcks, WriteStatus,
};

use crate::codes::{J0821, J0822, J0837};
use crate::engine::{NoEngine, PlantEngine};
use crate::literal::Literal;
use crate::model::{ArgType, Inputs, Model, SimDirection, SimType, Step, StepValue, parse_with};
use crate::plant::{Plant, Reading, fnv1a};

/// The descriptor of `sim`.
pub const DESCRIPTOR: Descriptor = Descriptor {
    name: "sim",
    version: "0.2.0",
    tier: Tier::N,
    protocols: &["sim"],
    buses: &[BusKind::None],
    instance: InstanceMode::Device,
    capabilities: Capabilities::of(&[Capability::Write, Capability::NativeActions]),
    interface: INTERFACE,
    micro: false,
    license: "Apache-2.0 OR MIT",
    description: "Simulated plant from a *.sim.toml model (ref/03 §2.4)",
};

/// The raw kind a signal type is delivered as.
pub const fn raw_kind(ty: SimType) -> RawKind {
    match ty {
        SimType::Bool => RawKind::Bool,
        SimType::I64 => RawKind::Int,
        SimType::U64 | SimType::Enum(_) | SimType::Bits(_) => RawKind::Uint,
        SimType::F64 | SimType::F32 => RawKind::Float,
        SimType::String => RawKind::Text,
        SimType::Bytes => RawKind::Bytes,
    }
}

fn param(value: Cbor<'_>) -> Option<Literal> {
    if let Ok(b) = value.as_bool() {
        return Some(Literal::Bool(b));
    }
    if let Ok(t) = value.as_str() {
        return Some(Literal::Text(String::from(t)));
    }
    value.as_f64().ok().map(Literal::Number)
}

/// The model and the seed of a device `config`; `device` names the device
/// for the default seed (ref/03 §2.4.2).
pub fn model_of(config: Cbor<'_>, device: &str) -> Result<(Model, u64), DriverError> {
    let bad = |m: &str| DriverError::new(J0852, m);
    let get = |key: &str| config.get(key).map_err(|_| bad("config is not a map"));
    let source = get("model")?
        .ok_or_else(|| bad("a sim device needs `model`"))?
        .as_str()
        .map_err(|_| bad("`model` must be the model's source text"))?;
    let mut inputs = Inputs::default();
    if let Some(p) = get("params")? {
        for entry in p.entries().map_err(|_| bad("`params` must be a table"))? {
            let (k, v) = entry.map_err(|_| bad("`params` must be a table"))?;
            inputs.params.insert(
                String::from(k),
                param(v).ok_or_else(|| bad("invalid parameter value"))?,
            );
        }
    }
    if let Some(p) = get("profile")? {
        let text = p
            .as_str()
            .map_err(|_| bad("`profile` must be the profile's source text"))?;
        inputs.profile = Some(String::from(text));
    }
    if let Some(f) = get("files")? {
        for entry in f.entries().map_err(|_| bad("`files` must be a table"))? {
            let (k, v) = entry.map_err(|_| bad("`files` must be a table"))?;
            let bytes = v
                .as_byte_string()
                .or_else(|_| v.as_str().map(str::as_bytes))
                .map_err(|_| bad("a file is a byte or text string"))?;
            inputs.files.insert(String::from(k), bytes.to_vec());
        }
    }
    let model =
        parse_with(source, &inputs).map_err(|e| DriverError::new(e.code, &std::format!("{e}")))?;
    let seed = match get("seed")? {
        Some(s) => s.as_u64().map_err(|_| bad("`seed` is a u64"))?,
        None => model.seed.unwrap_or_else(|| fnv1a(device)),
    };
    Ok((model, seed))
}

fn component<'a>(config: Cbor<'a>, path: &str) -> Option<&'a [u8]> {
    let file = config.get("files").ok()??.get(path).ok()??;
    file.as_byte_string()
        .or_else(|_| file.as_str().map(str::as_bytes))
        .ok()
}

fn point_signal<'a>(model: &Model, addr: Cbor<'a>) -> Option<(usize, &'a str)> {
    let name = addr.get("signal").ok()??.as_str().ok()?;
    Some((model.signal(name)?, name))
}

/// The plan half of `sim`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SimPlanner;

impl Planner for SimPlanner {
    const DESCRIPTOR: &'static Descriptor = &DESCRIPTOR;

    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>> {
        let mut errors = Vec::new();
        let mut models = BTreeMap::new();
        for d in req.devices {
            match model_of(d.config, d.name) {
                Ok((m, _)) => {
                    models.insert(d.index, m);
                }
                Err(e) => errors.push(Diagnostic::general(e.code, "invalid sim model or params")),
            }
        }
        for a in req.actions {
            if models
                .get(&a.device)
                .is_some_and(|m| m.action(a.name).is_none())
            {
                errors.push(Diagnostic::general(J0821, "the model has no such action"));
            }
        }
        let mut groups: BTreeMap<(u16, u64, bool), Vec<PointId>> = BTreeMap::new();
        for p in req.points {
            let Some(model) = models.get(&p.device) else {
                continue;
            };
            let Some((i, _)) = point_signal(model, p.addr) else {
                errors.push(Diagnostic::at(
                    J0853,
                    p.id,
                    "address must be { signal = <a signal of the model> }",
                ));
                continue;
            };
            let s = &model.signals[i];
            let dir_ok = matches!(
                (s.direction, p.direction),
                (SimDirection::Input, Direction::Input)
                    | (SimDirection::Output, Direction::Output)
                    | (SimDirection::Param, Direction::Param)
            );
            if !dir_ok || p.expect != raw_kind(s.ty) {
                errors.push(Diagnostic::at(
                    J0809,
                    p.id,
                    "point type or direction differs from the model",
                ));
                continue;
            }
            match p.poll {
                Poll::Period(ns) if p.direction != Direction::Output => {
                    groups.entry((p.device, ns, false)).or_default().push(p.id);
                }
                Poll::OnDemand if p.direction != Direction::Output => {
                    groups.entry((p.device, 0, true)).or_default().push(p.id);
                }
                Poll::Push => errors.push(Diagnostic::at(J0809, p.id, "sim points are polled")),
                _ => {}
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut plan = Plan::new();
        for ((device, period, on_demand), points) in groups {
            let kind = if on_demand {
                BatchKind::OnDemand
            } else {
                BatchKind::Poll
            };
            plan.push(device, kind, period, &points, 0, 0, &[]);
        }
        Ok(plan)
    }
}

/// A handle on the plant, for the `jk dev` panel and scenarios.
#[derive(Clone, Debug)]
pub struct SimHandle(Arc<Mutex<Plant>>);

impl SimHandle {
    /// Locks the plant. The lock lives in the `io` domain only.
    pub fn plant(&self) -> MutexGuard<'_, Plant> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The runtime half of `sim`, with the Wasm engine `E` for `[plant]`.
pub struct SimDriver<E: PlantEngine = NoEngine> {
    plant: Arc<Mutex<Plant>>,
    points: Vec<(PointId, usize, SimType)>,
    actions: Vec<(u32, usize, u64)>,
    reads: u64,
    writes: u64,
    invocations: u64,
    engine: PhantomData<fn() -> E>,
}

impl<E: PlantEngine> core::fmt::Debug for SimDriver<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SimDriver")
            .field("points", &self.points)
            .field("actions", &self.actions)
            .field("reads", &self.reads)
            .field("writes", &self.writes)
            .field("invocations", &self.invocations)
            .finish_non_exhaustive()
    }
}

impl<E: PlantEngine> SimDriver<E> {
    /// The handle on this instance's plant.
    pub fn handle(&self) -> SimHandle {
        SimHandle(self.plant.clone())
    }

    fn plant(&self) -> MutexGuard<'_, Plant> {
        self.plant.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn signal(&self, point: PointId) -> Option<(usize, SimType)> {
        self.points
            .iter()
            .find(|p| p.0 == point)
            .map(|p| (p.1, p.2))
    }
}

fn to_raw(ty: SimType, v: f64) -> RawValue<'static> {
    match ty {
        SimType::Bool => RawValue::Bool(v != 0.0),
        SimType::I64 => {
            RawValue::Int(v.round_ties_even().clamp(i64::MIN as f64, i64::MAX as f64) as i64)
        }
        SimType::U64 | SimType::Enum(_) | SimType::Bits(_) => {
            RawValue::Uint(v.round_ties_even().clamp(0.0, u64::MAX as f64) as u64)
        }
        SimType::F64 => RawValue::Float(v),
        SimType::F32 => RawValue::Float(f64::from(v as f32)),
        SimType::String | SimType::Bytes => RawValue::None,
    }
}

fn from_raw(value: RawValue<'_>) -> Option<f64> {
    match value {
        RawValue::Bool(b) => Some(f64::from(u8::from(b))),
        RawValue::Int(i) => Some(i as f64),
        RawValue::Uint(u) => Some(u as f64),
        RawValue::Float(f) => Some(f),
        _ => None,
    }
}

#[derive(Debug)]
enum Delivered {
    Num(SimType, f64),
    Text(SimType, Vec<u8>),
    Failed(joker_hal::RawStatus),
}

type Entry<'a> = (Option<u64>, Option<String>, Cbor<'a>);

fn map_entries(args: Cbor<'_>) -> Result<Vec<Entry<'_>>, ()> {
    let bytes = args.as_bytes();
    let first = *bytes.first().ok_or(())?;
    if first >> 5 != 5 {
        return Err(());
    }
    let info = first & 0x1f;
    let (count, mut at) = match info {
        0..=23 => (u64::from(info), 1),
        24..=27 => {
            let n = 1usize << (info - 24);
            let raw = bytes.get(1..1 + n).ok_or(())?;
            (raw.iter().fold(0u64, |a, b| a << 8 | u64::from(*b)), 1 + n)
        }
        _ => return Err(()),
    };
    let mut out = Vec::new();
    for _ in 0..count {
        let (k, rest) = Cbor::split(bytes.get(at..).ok_or(())?).map_err(|_| ())?;
        let (v, rest2) = Cbor::split(rest).map_err(|_| ())?;
        at = bytes.len() - rest2.len();
        out.push((k.as_u64().ok(), k.as_str().ok().map(String::from), v));
    }
    Ok(out)
}

enum ArgValue {
    Num(f64),
    Text(Vec<u8>),
}

fn invalid(m: &str) -> ActionFailure {
    ActionFailure::Invalid(DriverError::new(J0822, m))
}

fn arguments(
    model: &Model,
    decl: &[(String, ArgType)],
    args: Cbor<'_>,
) -> Result<BTreeMap<String, ArgValue>, ActionFailure> {
    let mut out = BTreeMap::new();
    if decl.is_empty() && (args.as_bytes().is_empty() || args.is_null()) {
        return Ok(out);
    }
    let entries = map_entries(args).map_err(|_| invalid("arguments are a record"))?;
    for (pos, name, v) in entries {
        let (n, ty) = match (pos, name) {
            (Some(p), _) => decl.get(p as usize),
            (None, Some(n)) => decl.iter().find(|(d, _)| *d == n),
            _ => None,
        }
        .ok_or_else(|| invalid("unknown argument"))?;
        let value = match ty {
            ArgType::Bool => ArgValue::Num(f64::from(u8::from(
                v.as_bool().map_err(|_| invalid("expected a boolean"))?,
            ))),
            ArgType::Num(..) => {
                ArgValue::Num(v.as_f64().map_err(|_| invalid("expected a number"))?)
            }
            ArgType::Enum(e) => {
                let en = &model.enums[*e];
                let x = match (v.as_str(), v.as_u64()) {
                    (Ok(t), _) => en.value(t),
                    (_, Ok(u)) => en.variant(u as i64).map(|_| u as i64),
                    _ => None,
                }
                .ok_or_else(|| invalid("not a variant"))?;
                ArgValue::Num(x as f64)
            }
            ArgType::String => ArgValue::Text(
                v.as_str()
                    .map_err(|_| invalid("expected a string"))?
                    .as_bytes()
                    .to_vec(),
            ),
        };
        out.insert(n.clone(), value);
    }
    if decl.iter().any(|(n, _)| !out.contains_key(n)) {
        return Err(invalid("missing argument"));
    }
    Ok(out)
}

fn arg_in(model: &Model, decl: &[(String, ArgType)], name: &str, v: f64, signal: usize) -> f64 {
    let target = model.signals[signal].unit;
    match decl.iter().find(|(n, _)| n == name) {
        Some((_, ArgType::Num(_, unit))) => unit.convert(v, &target).unwrap_or(v),
        _ => v,
    }
}

impl<E: PlantEngine> Driver for SimDriver<E> {
    fn open(args: &OpenArgs<'_>, _link: Link) -> Result<Self, DriverError> {
        let device = args
            .device(0)
            .ok_or_else(|| DriverError::new(J0852, "a sim instance has one device"))?;
        let (model, seed) = model_of(device.config, device.name)?;
        let mut points = Vec::new();
        for p in args.request().points {
            let (i, _) = point_signal(&model, p.addr).ok_or_else(|| {
                DriverError::new(
                    J0853,
                    "address must be { signal = <a signal of the model> }",
                )
            })?;
            points.push((p.id, i, model.signals[i].ty));
        }
        let actions = args
            .request()
            .actions
            .iter()
            .filter_map(|a| model.action(a.name).map(|i| (a.id, i, a.timeout_ns)))
            .collect();
        let instance = match &model.plant {
            None => None,
            Some(spec) => {
                let bytes = component(device.config, &spec.component).ok_or_else(|| {
                    DriverError::new(J0837, "the `[plant]` component was not provided")
                })?;
                Some(
                    E::instantiate(bytes, &spec.config, seed)
                        .map_err(|m| DriverError::new(J0837, &m))?,
                )
            }
        };
        let mut plant = Plant::seeded(model, seed);
        if let Some(i) = instance {
            plant.attach(i);
        }
        Ok(Self {
            plant: Arc::new(Mutex::new(plant)),
            points,
            actions,
            reads: 0,
            writes: 0,
            invocations: 0,
            engine: PhantomData,
        })
    }

    async fn connect(
        &mut self,
        cx: &mut Cx<'_>,
        out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        let now = cx.now().mono_ns;
        let mut plant = self.plant();
        plant.start(now);
        let description = plant.model().description.clone();
        drop(plant);
        out.device(
            0,
            &DeviceInfo {
                model: description.as_deref(),
                ..DeviceInfo::default()
            },
            None,
        );
        Ok(())
    }

    async fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let now = cx.now().mono_ns;
        self.reads += 1;
        let (samples, latency) = {
            let mut plant = self.plant.lock().unwrap_or_else(PoisonError::into_inner);
            plant.advance_to(now);
            if plant.disconnected(now) {
                return Err(BatchFailure::CommFailure("simulated disconnect"));
            }
            let mut samples = Vec::with_capacity(batch.points.len());
            let mut latency = 0;
            for &point in batch.points {
                let Some(&(_, i, ty)) = self.points.iter().find(|p| p.0 == point) else {
                    continue;
                };
                latency = latency.max(plant.latency_ns(i) + plant.delay_ns(now, i));
                let ts = plant.edge_at(now, i).unwrap_or(now);
                let delivered = match plant.read(now, i) {
                    Reading::Down => return Err(BatchFailure::CommFailure("simulated disconnect")),
                    Reading::Value(None, status) => Delivered::Failed(status),
                    Reading::Value(Some(_), status) if ty.is_text() => {
                        let text = plant.text(i).unwrap_or(&[]).to_vec();
                        samples.push((point, status, ts, Delivered::Text(ty, text)));
                        continue;
                    }
                    Reading::Value(Some(v), status) => {
                        samples.push((point, status, ts, Delivered::Num(ty, v)));
                        continue;
                    }
                };
                samples.push((point, joker_hal::RawStatus::OK, ts, delivered));
            }
            (samples, latency)
        };
        if latency > 0 {
            cx.sleep_until(now + latency).await;
        }
        for (point, status, ts, d) in &samples {
            let value = match d {
                Delivered::Num(ty, v) => to_raw(*ty, *v),
                Delivered::Text(SimType::String, t) => {
                    RawValue::Text(core::str::from_utf8(t).unwrap_or(""))
                }
                Delivered::Text(_, t) => RawValue::Bytes(t),
                Delivered::Failed(status) => {
                    let _ = out.push(RawSample::failed(*point, *status));
                    continue;
                }
            };
            let _ = out.push(RawSample {
                status: *status,
                ts_hw: Some(*ts),
                ..RawSample::good(*point, value)
            });
        }
        Ok(())
    }

    async fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) {
        let now = cx.now().mono_ns;
        let mut plant = self.plant.lock().unwrap_or_else(PoisonError::into_inner);
        plant.advance_to(now);
        let down = plant.disconnected(now);
        for (k, w) in writes.iter().enumerate() {
            let status = match (self.signal(w.point), w.value) {
                _ if down => WriteStatus::NotConnected,
                (Some((i, SimType::String)), RawValue::Text(t)) => {
                    plant.apply_text(i, t.as_bytes());
                    self.writes += 1;
                    WriteStatus::Ok
                }
                (Some((i, SimType::Bytes)), RawValue::Bytes(b)) => {
                    plant.apply_text(i, b);
                    self.writes += 1;
                    WriteStatus::Ok
                }
                (Some((i, ty)), value) if !ty.is_text() => match from_raw(value) {
                    Some(v) => {
                        plant.apply(i, v);
                        self.writes += 1;
                        WriteStatus::Ok
                    }
                    None => WriteStatus::Unsupported,
                },
                _ => WriteStatus::Unsupported,
            };
            acks.set(k, status, Some(now));
        }
    }

    async fn invoke(
        &mut self,
        cx: &mut Cx<'_>,
        call: &ActionCall<'_>,
        out: &mut ActionOut<'_>,
    ) -> Result<(), ActionFailure> {
        let Some(&(_, index, manifest_timeout)) = self.actions.iter().find(|a| a.0 == call.action)
        else {
            return Err(ActionFailure::Unsupported);
        };
        self.invocations += 1;
        let start = cx.now().mono_ns;
        let (action, model) = {
            let plant = self.plant();
            let model = plant.model().clone();
            (model.actions[index].clone(), model)
        };
        let args = arguments(&model, &action.args, call.args)?;
        let timeout = if manifest_timeout > 0 {
            manifest_timeout
        } else {
            action.timeout_ns
        };
        let deadline = start.saturating_add(timeout);
        for step in &action.steps {
            match step {
                Step::Wait(ns) => {
                    let until = cx.now().mono_ns.saturating_add(*ns);
                    if until > deadline {
                        cx.sleep_until(deadline).await;
                        return Err(ActionFailure::Timeout);
                    }
                    cx.sleep_until(until).await;
                }
                Step::FailIf {
                    signal,
                    equals,
                    message,
                } => {
                    let now = cx.now().mono_ns;
                    let mut plant = self.plant();
                    plant.advance_to(now);
                    if plant.disconnected(now) {
                        return Err(ActionFailure::NotConnected);
                    }
                    let hit = match equals {
                        StepValue::Num(x) => plant.value(*signal) == Some(*x),
                        StepValue::Text(t) => plant.text(*signal) == Some(t.as_slice()),
                        StepValue::Arg(a) => match args.get(a) {
                            Some(ArgValue::Num(x)) => {
                                let x = arg_in(&model, &action.args, a, *x, *signal);
                                plant.value(*signal) == Some(x)
                            }
                            Some(ArgValue::Text(t)) => plant.text(*signal) == Some(t.as_slice()),
                            None => false,
                        },
                    };
                    if hit {
                        return Err(ActionFailure::Device(DriverError::new(J0821, message)));
                    }
                }
                Step::Set { values, release } => {
                    let now = cx.now().mono_ns;
                    let mut plant = self.plant();
                    plant.advance_to(now);
                    if plant.disconnected(now) {
                        return Err(ActionFailure::NotConnected);
                    }
                    for (signal, v) in values {
                        if *release {
                            plant.release(*signal);
                            continue;
                        }
                        match v {
                            StepValue::Num(x) => plant.set(*signal, *x),
                            StepValue::Text(t) => plant.set_text(*signal, t),
                            StepValue::Arg(a) => match args.get(a) {
                                Some(ArgValue::Num(x)) => {
                                    let x = arg_in(&model, &action.args, a, *x, *signal);
                                    plant.set(*signal, x);
                                }
                                Some(ArgValue::Text(t)) => plant.set_text(*signal, t),
                                None => {}
                            },
                        }
                    }
                }
            }
        }
        out.result(&[0xa0])
            .map_err(|_| ActionFailure::Device(DriverError::new(J0821, "result too large")))
    }

    fn health(&self, out: &mut HealthOut<'_>) {
        out.counter("reads", self.reads);
        out.counter("writes", self.writes);
        out.counter("invocations", self.invocations);
    }
}
