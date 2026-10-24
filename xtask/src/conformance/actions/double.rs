//! The register double of the `action` suites (ADR 0066 §21): the
//! equipment behind every device of the fixture, served through a driver
//! that the daemon's own io task runs, so the steps of an action go
//! through the real interpreter and reach these registers.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use joker_hal::cbor::{Cbor, Value as C};
use joker_hal::driver::{
    ActionCall, ActionOut, AddrRead, AddrWrite, CommittedWrite, ConnectOut, Driver, OpenArgs,
    RawSink, ReadBatch, WriteAcks,
};
use joker_hal::interpreter::kind_of_type;
use joker_hal::types::{
    ActionFailure, BatchFailure, DriverError, J0869, PointId, RawKind, RawSample, RawStatus,
    RawValue, WriteStatus,
};
use joker_hal::{Cx, Link};

/// A register value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reg {
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
}

impl Reg {
    /// The value of a TOML item, when it is a scalar.
    pub fn of(value: &toml::Value) -> Option<Self> {
        match value {
            toml::Value::Boolean(b) => Some(Self::Bool(*b)),
            toml::Value::Integer(i) => Some(Self::Int(*i)),
            toml::Value::Float(f) => Some(Self::Float(*f)),
            _ => None,
        }
    }

    fn raw(value: RawValue<'_>) -> Option<Self> {
        match value {
            RawValue::Bool(b) => Some(Self::Bool(b)),
            RawValue::Int(i) => Some(Self::Int(i)),
            RawValue::Uint(u) => i64::try_from(u).ok().map(Self::Int),
            RawValue::Float(f) => Some(Self::Float(f)),
            RawValue::Regs(r) => r
                .iter()
                .try_fold(0i64, |acc, w| {
                    acc.checked_mul(65536)?.checked_add(i64::from(*w))
                })
                .map(Self::Int),
            RawValue::None | RawValue::Bytes(_) | RawValue::Text(_) => None,
        }
    }

    fn as_i64(self) -> i64 {
        match self {
            Self::Bool(b) => i64::from(b),
            Self::Int(i) => i,
            Self::Float(f) => f as i64,
        }
    }

    /// Equality by value, across kinds.
    pub fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Float(a), b) | (b, Self::Float(a)) => (b.as_f64() - a).abs() < 1e-9,
            (a, b) => a.as_i64() == b.as_i64(),
        }
    }

    fn as_f64(self) -> f64 {
        match self {
            Self::Bool(b) => f64::from(u8::from(b)),
            Self::Int(i) => i as f64,
            Self::Float(f) => f,
        }
    }

    /// The value as text, for reports.
    pub fn text(self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
        }
    }
}

/// What a native action of the double does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Native {
    /// The result record.
    pub result: Vec<(String, Reg)>,
    /// The failure it ends with instead.
    pub failure: Option<String>,
    /// How long it runs before it answers, checking for cancellation.
    pub hold_ns: u64,
}

/// A register write that sets other registers.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    /// The address written.
    pub at: i64,
    /// The value written.
    pub value: Reg,
    /// What it sets.
    pub set: Vec<(i64, Reg)>,
}

/// One device's double, as the fixture declares it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spec {
    /// The device.
    pub device: String,
    /// The registers.
    pub registers: BTreeMap<i64, Reg>,
    /// The writes that set other registers.
    pub on_write: Vec<Effect>,
    /// The quality reported for reads of an address.
    pub quality: BTreeMap<i64, RawStatus>,
    /// The failure of reads and writes of an address.
    pub fail: BTreeMap<i64, BatchFailure>,
    /// Every `connect` fails.
    pub link_down: bool,
    /// The native actions, by name.
    pub native: BTreeMap<String, Native>,
}

/// The state of one device's double during a case.
#[derive(Debug, Default)]
pub struct State {
    /// The fixture's declaration.
    pub spec: Spec,
    /// The registers now.
    pub registers: BTreeMap<i64, Reg>,
    /// Every write received by address, in order.
    pub writes: Vec<(i64, Reg)>,
}

/// The state shared by a device's driver and the runner.
pub type Shared = Arc<Mutex<State>>;

fn locked(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The double's state of `spec`, ready for a case.
pub fn shared(spec: &Spec) -> Shared {
    Arc::new(Mutex::new(State {
        registers: spec.registers.clone(),
        spec: spec.clone(),
        writes: Vec::new(),
    }))
}

/// The driver over a [`State`].
#[derive(Debug)]
pub struct RegsDriver {
    state: Shared,
    points: BTreeMap<PointId, (i64, RawKind, u16)>,
    natives: BTreeMap<u32, String>,
}

fn at_of(addr: Cbor<'_>) -> Option<i64> {
    addr.get("at").ok().flatten()?.as_i64().ok()
}

fn words_of(reg: Reg, width: u16, words: &mut [u16; 4]) -> usize {
    let n = usize::from(width.clamp(1, 4));
    let mut v = reg.as_i64() as u64;
    if let Reg::Float(f) = reg
        && n == 2
    {
        v = u64::from((f as f32).to_bits());
    }
    for i in (0..n).rev() {
        words[i] = (v & 0xffff) as u16;
        v >>= 16;
    }
    n
}

impl RegsDriver {
    /// The driver of the device `request` describes, over `state`.
    pub fn new(state: Shared, request: &joker_hal::PlanRequest<'_>) -> Self {
        let points = request
            .points
            .iter()
            .filter_map(|p| Some((p.id, (at_of(p.addr)?, p.expect, p.width))))
            .collect();
        let natives = request
            .actions
            .iter()
            .map(|a| (a.id, a.name.to_owned()))
            .collect();
        Self {
            state,
            points,
            natives,
        }
    }

    fn push(&self, point: PointId, at: i64, expect: RawKind, width: u16, out: &mut RawSink<'_>) {
        let st = locked(&self.state);
        let status = st.spec.quality.get(&at).copied().unwrap_or(RawStatus::OK);
        let Some(reg) = st.registers.get(&at).copied() else {
            let _ = out.push(RawSample::failed(
                point,
                RawStatus::bad(joker_hal::Reason(12)),
            ));
            return;
        };
        drop(st);
        let mut words = [0u16; 4];
        let value = match expect {
            RawKind::Bool => RawValue::Bool(reg.as_i64() != 0),
            RawKind::Int => RawValue::Int(reg.as_i64()),
            RawKind::Uint => RawValue::Uint(reg.as_i64().max(0) as u64),
            RawKind::Float => RawValue::Float(reg.as_f64()),
            RawKind::Regs => {
                let n = words_of(reg, width, &mut words);
                RawValue::Regs(&words[..n])
            }
            RawKind::None | RawKind::Bytes | RawKind::Text => RawValue::None,
        };
        let mut sample = RawSample::good(point, value);
        sample.status = status;
        let _ = out.push(sample);
    }

    fn failure(&self, at: i64) -> Option<BatchFailure> {
        locked(&self.state).spec.fail.get(&at).copied()
    }
}

impl Driver for RegsDriver {
    fn open(_args: &OpenArgs<'_>, _link: Link) -> Result<Self, DriverError> {
        Err(DriverError::new(
            J0869,
            "the register double is built by the runner, not opened",
        ))
    }

    async fn connect(
        &mut self,
        _cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        if locked(&self.state).spec.link_down {
            Err(DriverError::new(J0869, "the double refuses to connect"))
        } else {
            Ok(())
        }
    }

    async fn read(
        &mut self,
        _cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        for point in batch.points {
            if let Some((at, expect, width)) = self.points.get(point).copied() {
                if let Some(f) = self.failure(at) {
                    return Err(f);
                }
                self.push(*point, at, expect, width, out);
            }
        }
        Ok(())
    }

    async fn write(
        &mut self,
        _cx: &mut Cx<'_>,
        writes: &[CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) {
        for i in 0..writes.len() {
            acks.set(i, WriteStatus::Ok, None);
        }
    }

    async fn read_addr(
        &mut self,
        _cx: &mut Cx<'_>,
        req: &AddrRead<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let at = at_of(req.addr).ok_or(BatchFailure::ConfigError("no `at` in the address"))?;
        if let Some(f) = self.failure(at) {
            return Err(f);
        }
        let expect = kind_of_type(req.type_name).unwrap_or(req.expect);
        self.push(0, at, expect, req.width, out);
        Ok(())
    }

    async fn write_addr(
        &mut self,
        _cx: &mut Cx<'_>,
        req: &AddrWrite<'_>,
    ) -> Result<(), BatchFailure> {
        let at = at_of(req.addr).ok_or(BatchFailure::ConfigError("no `at` in the address"))?;
        if let Some(f) = self.failure(at) {
            return Err(f);
        }
        let value = Reg::raw(req.value).ok_or(BatchFailure::ConfigError("not a scalar"))?;
        let mut st = locked(&self.state);
        st.writes.push((at, value));
        st.registers.insert(at, value);
        let effects: Vec<(i64, Reg)> = st
            .spec
            .on_write
            .iter()
            .filter(|e| e.at == at && e.value.same(value))
            .flat_map(|e| e.set.iter().copied())
            .collect();
        st.registers.extend(effects);
        Ok(())
    }

    async fn invoke(
        &mut self,
        cx: &mut Cx<'_>,
        call: &ActionCall<'_>,
        out: &mut ActionOut<'_>,
    ) -> Result<(), ActionFailure> {
        let name = self
            .natives
            .get(&call.action)
            .ok_or(ActionFailure::Unsupported)?;
        let native = locked(&self.state)
            .spec
            .native
            .get(name)
            .cloned()
            .ok_or(ActionFailure::Unsupported)?;
        let start = cx.now().mono_ns;
        let end = start.saturating_add(native.hold_ns);
        loop {
            if cx.cancelled() {
                return Err(ActionFailure::Cancelled);
            }
            let now = cx.now().mono_ns;
            if now >= end {
                break;
            }
            let done = (now - start).saturating_mul(1000) / native.hold_ns.max(1);
            cx.progress(u16::try_from(done).unwrap_or(1000), "holding");
            cx.sleep_until((now + 5_000_000).min(end)).await;
        }
        cx.progress(1000, "done");
        match native.failure.as_deref() {
            Some("timeout") => return Err(ActionFailure::Timeout),
            Some("invalid") => {
                return Err(ActionFailure::Invalid(DriverError::new(
                    J0869,
                    "the double refuses the arguments",
                )));
            }
            Some(_) => {
                return Err(ActionFailure::Device(DriverError::new(
                    J0869,
                    "the double fails",
                )));
            }
            None => {}
        }
        let mut entries: Vec<(String, C)> = native
            .result
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    match *v {
                        Reg::Bool(b) => C::Bool(b),
                        Reg::Int(i) => C::Int(i),
                        Reg::Float(f) => C::Float(f),
                    },
                )
            })
            .collect();
        if let Ok(entries_in) = call.args.entries() {
            for (k, v) in entries_in.flatten() {
                if let Ok(i) = v.as_i64() {
                    entries.push((format!("echo_{k}"), C::Int(i)));
                }
            }
        }
        out.result(&C::Map(entries).encode())
            .map_err(|_| ActionFailure::Device(DriverError::new(J0869, "result too large")))
    }
}
