//! The `sim` driver (tier N, ref/01 appendix A): one simulated device per
//! instance, served from its model.
//!
//! The device's `config` carries the model and its parameters (ADR 0051):
//! `model` is the source text of the `*.sim.toml` file, which the core reads
//! when it compiles the generation, and `params` the device's `params`
//! table. Each point's address is `{ signal = "<name>" }`.

use std::collections::BTreeMap;
use std::string::String;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::vec::Vec;

use joker_hal::descriptor::INTERFACE;
use joker_hal::plan::Plan;
use joker_hal::types::{Diagnostic, J0809, J0852, J0853};
use joker_hal::{
    BatchFailure, BatchKind, BusKind, Capabilities, Capability, Cbor, CommittedWrite, ConnectOut,
    Cx, Descriptor, DeviceInfo, Direction, Driver, DriverError, HealthOut, InstanceMode, Link,
    OpenArgs, PlanRequest, Planner, PointId, Poll, RawKind, RawSample, RawSink, RawValue,
    ReadBatch, Tier, WriteAcks, WriteStatus,
};

use crate::literal::Literal;
use crate::model::{Model, SimDirection, SimType, parse};
use crate::plant::{Plant, Reading, fnv1a};

/// The descriptor of `sim`.
pub const DESCRIPTOR: Descriptor = Descriptor {
    name: "sim",
    version: "0.1.0",
    tier: Tier::N,
    protocols: &["sim"],
    buses: &[BusKind::None],
    instance: InstanceMode::Device,
    capabilities: Capabilities::of(&[Capability::Write]),
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
        SimType::U64 => RawKind::Uint,
        SimType::F64 | SimType::F32 => RawKind::Float,
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

/// Reads the model and parameters of a device `config`.
pub fn model_of(config: Cbor<'_>, device: &str) -> Result<(Model, u64), DriverError> {
    let bad = |m: &str| DriverError::new(J0852, m);
    let source = config
        .get("model")
        .map_err(|_| bad("config is not a map"))?
        .ok_or_else(|| bad("a sim device needs `model`"))?
        .as_str()
        .map_err(|_| bad("`model` must be the model's source text"))?;
    let mut params = BTreeMap::new();
    if let Some(p) = config
        .get("params")
        .map_err(|_| bad("config is not a map"))?
    {
        for entry in p.entries().map_err(|_| bad("`params` must be a table"))? {
            let (k, v) = entry.map_err(|_| bad("`params` must be a table"))?;
            params.insert(
                String::from(k),
                param(v).ok_or_else(|| bad("invalid parameter value"))?,
            );
        }
    }
    let model =
        parse(source, &params).map_err(|e| DriverError::new(e.code, &std::format!("{e}")))?;
    Ok((model, fnv1a(device)))
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

/// The runtime half of `sim`.
#[derive(Debug)]
pub struct SimDriver {
    plant: Arc<Mutex<Plant>>,
    points: Vec<(PointId, usize, SimType)>,
    reads: u64,
    writes: u64,
}

impl SimDriver {
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
        SimType::U64 => RawValue::Uint(v.round_ties_even().clamp(0.0, u64::MAX as f64) as u64),
        SimType::F64 => RawValue::Float(v),
        SimType::F32 => RawValue::Float(f64::from(v as f32)),
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

impl Driver for SimDriver {
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
        Ok(Self {
            plant: Arc::new(Mutex::new(Plant::new(model, seed))),
            points,
            reads: 0,
            writes: 0,
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
            let mut latency = plant.model().latency_ns;
            for &point in batch.points {
                let Some(&(_, i, ty)) = self.points.iter().find(|p| p.0 == point) else {
                    continue;
                };
                latency = latency.max(plant.model().latency_ns + plant.delay_ns(now, i));
                let sample = match plant.read(now, i) {
                    Reading::Down => return Err(BatchFailure::CommFailure("simulated disconnect")),
                    Reading::Value(Some(v), status) => RawSample {
                        status,
                        ts_hw: Some(now),
                        ..RawSample::good(point, to_raw(ty, v))
                    },
                    Reading::Value(None, status) => RawSample::failed(point, status),
                };
                samples.push(sample);
            }
            (samples, latency)
        };
        if latency > 0 {
            cx.sleep_until(now + latency).await;
        }
        for s in samples {
            let _ = out.push(s);
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
            let status = match (self.signal(w.point), from_raw(w.value)) {
                _ if down => WriteStatus::NotConnected,
                (Some((i, _)), Some(v)) => {
                    plant.apply(i, v);
                    self.writes += 1;
                    WriteStatus::Ok
                }
                _ => WriteStatus::Unsupported,
            };
            acks.set(k, status, Some(now));
        }
    }

    fn health(&self, out: &mut HealthOut<'_>) {
        out.counter("reads", self.reads);
        out.counter("writes", self.writes);
    }
}
