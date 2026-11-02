//! The `async-io` host of the native harness (ref/04 §6.1): the async
//! tasks read the current slots, propose into an inbox the next tick drains
//! (docs/01 §4, step 2), read the historian from series, and sleep on the
//! virtual clock.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

use joker::__private::{HostFuture, TaskHost};
use joker_core::ValueType;
use joker_sdk_sys::joker::runtime::async_io::{Aggregate, HistoryQuery};
use joker_sdk_sys::joker::runtime::types;

use crate::series::Series;
use crate::value::Value;

/// What an async task did that the next tick applies.
#[derive(Clone, Debug)]
pub(crate) enum Op {
    /// A proposal (`set`, `write`).
    Set(u32, u64),
    /// A relinquish.
    Relinquish(u32),
    /// A `publish`: an `Internal<T>`, an alarm condition.
    Publish(u32, types::Value),
    /// An event.
    Emit(u32, Vec<u8>),
}

/// The state the harness and the async tasks share.
#[derive(Debug, Default)]
pub(crate) struct HostState {
    pub tick: u64,
    pub now_mono_ns: u64,
    pub now_utc_ns: i64,
    pub clock: Option<types::ClockQuality>,
    pub start_utc_ns: i64,
    pub samples: Vec<Option<types::Sample>>,
    pub params: Vec<u64>,
    pub types: Vec<Option<ValueType>>,
    pub history: BTreeMap<u32, Series>,
    pub outcomes: BTreeMap<u32, (u64, types::WriteOutcome)>,
    pub ops: Vec<Op>,
    pub rng: u64,
}

/// The host the harness installs for the async view.
#[derive(Debug, Clone)]
pub(crate) struct Host(pub Arc<Mutex<HostState>>);

impl Host {
    pub(crate) fn lock(&self) -> MutexGuard<'_, HostState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn not_connected() -> types::Sample {
    types::Sample {
        value: types::Value::Uint(0),
        quality: types::Quality {
            code: types::QualityCode::Bad,
            reason: types::QualityReason::NotConnected,
        },
        ts_src: None,
        ts_hw: None,
        ts_acq: 0,
        seq: 0,
    }
}

/// A WIT value of a harness value.
pub(crate) fn wire(v: Value, ty: ValueType) -> types::Value {
    match ty {
        ValueType::Bool => types::Value::Boolean(v.as_f64() != 0.0),
        ValueType::F64 | ValueType::F32 => types::Value::Float(v.as_f64()),
        ValueType::I64 => types::Value::Int(v.to_bits(ty) as i64),
        _ => types::Value::Uint(v.to_bits(ty)),
    }
}

fn wire_quality(q: joker::Quality) -> types::Quality {
    let code = match q.code {
        joker::QualityCode::Good => types::QualityCode::Good,
        joker::QualityCode::Uncertain => types::QualityCode::Uncertain,
        joker::QualityCode::Bad => types::QualityCode::Bad,
    };
    let reason = match q.reason as u8 {
        0 => types::QualityReason::None,
        1 => types::QualityReason::Overridden,
        2 => types::QualityReason::LocalOverride,
        3 => types::QualityReason::Stale,
        4 => types::QualityReason::NotConfirmed,
        5 => types::QualityReason::Substituted,
        6 => types::QualityReason::OutOfDistribution,
        7 => types::QualityReason::SensorCal,
        8 => types::QualityReason::ClockUnsynced,
        9 => types::QualityReason::Initial,
        10 => types::QualityReason::Frozen,
        11 => types::QualityReason::PartialWrite,
        12 => types::QualityReason::CommFailure,
        13 => types::QualityReason::DeviceFault,
        14 => types::QualityReason::OutOfRange,
        15 => types::QualityReason::ConfigError,
        16 => types::QualityReason::NotConnected,
        17 => types::QualityReason::Nan,
        18 => types::QualityReason::Tripped,
        _ => types::QualityReason::Deadline,
    };
    types::Quality { code, reason }
}

impl HostState {
    /// One sample per step of `q`, from the series of the port; the times
    /// before the start take the series' first value.
    fn history(&self, q: &HistoryQuery) -> Result<Vec<types::Sample>, types::Error> {
        let fail = |code: &str, message: &str| types::Error {
            code: code.to_owned(),
            message: message.to_owned(),
        };
        let series = self
            .history
            .get(&q.port)
            .ok_or_else(|| fail("J-E0403", "the signal has no recording in the harness"))?;
        let ty = self
            .types
            .get(q.port as usize)
            .copied()
            .flatten()
            .unwrap_or(ValueType::F64);
        if q.to_utc_ns < q.from_utc_ns || (q.step_ns == 0 && q.agg != Aggregate::Raw) {
            return Err(fail("J-E0401", "invalid interval"));
        }
        let step = if q.step_ns == 0 {
            1_000_000_000
        } else {
            q.step_ns
        };
        let span = u64::try_from(q.to_utc_ns - q.from_utc_ns).unwrap_or(u64::MAX);
        let n = span / step;
        if n > 10_000 {
            return Err(fail("J-E0402", "more than 10 000 points"));
        }
        let mut out = Vec::with_capacity(n as usize);
        for k in 0..n {
            let utc = q.from_utc_ns.saturating_add((k * step) as i64);
            let t = u64::try_from(utc.saturating_sub(self.start_utc_ns)).unwrap_or(0);
            out.push(types::Sample {
                value: wire(series.value_at(t), ty),
                quality: wire_quality(series.quality_at(t)),
                ts_src: None,
                ts_hw: None,
                ts_acq: t,
                seq: k,
            });
        }
        Ok(out)
    }
}

struct Sleep {
    host: Host,
    until: u64,
}

impl Future for Sleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.host.lock().now_mono_ns >= self.until {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

struct Commit {
    host: Host,
    port: u32,
    after: u64,
}

impl Future for Commit {
    type Output = Result<types::WriteOutcome, types::Error>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        let state = self.host.lock();
        match state.outcomes.get(&self.port) {
            Some((tick, o)) if *tick > self.after => Poll::Ready(Ok(o.clone())),
            _ => Poll::Pending,
        }
    }
}

impl TaskHost for Host {
    fn read(&self, port: u32) -> types::Sample {
        self.lock()
            .samples
            .get(port as usize)
            .cloned()
            .flatten()
            .unwrap_or_else(not_connected)
    }

    fn param(&self, port: u32) -> types::Value {
        let state = self.lock();
        let bits = state.params.get(port as usize).copied().unwrap_or(0);
        let ty = state
            .types
            .get(port as usize)
            .copied()
            .flatten()
            .unwrap_or(ValueType::F64);
        wire(Value::from_bits(bits, ty), ty)
    }

    fn set(&self, port: u32, v: types::Value) {
        self.lock()
            .ops
            .push(Op::Set(port, joker::__private::bits(&v)));
    }

    fn write(
        &self,
        port: u32,
        v: types::Value,
    ) -> HostFuture<Result<types::WriteOutcome, types::Error>> {
        let after = {
            let mut state = self.lock();
            state.ops.push(Op::Set(port, joker::__private::bits(&v)));
            state.tick
        };
        Box::pin(Commit {
            host: self.clone(),
            port,
            after,
        })
    }

    fn relinquish(&self, port: u32) {
        self.lock().ops.push(Op::Relinquish(port));
    }

    fn last_outcome(&self, port: u32) -> Option<types::WriteOutcome> {
        self.lock().outcomes.get(&port).map(|(_, o)| o.clone())
    }

    fn publish(&self, port: u32, v: types::Value) {
        self.lock().ops.push(Op::Publish(port, v));
    }

    fn counter_add(&self, _port: u32, _delta: u64) {
        panic!("counters are not served by the native harness yet");
    }

    fn emit(&self, port: u32, payload: Vec<u8>) {
        self.lock().ops.push(Op::Emit(port, payload));
    }

    fn history(&self, q: HistoryQuery) -> HostFuture<Result<Vec<types::Sample>, types::Error>> {
        let result = self.lock().history(&q);
        Box::pin(async move { result })
    }

    fn now(&self) -> types::Instant {
        let state = self.lock();
        types::Instant {
            mono_ns: state.now_mono_ns,
            utc_ns: state.now_utc_ns,
            clock: state.clock.unwrap_or(types::ClockQuality::Synced),
        }
    }

    fn sleep_until(&self, mono_ns: u64) -> HostFuture<()> {
        Box::pin(Sleep {
            host: self.clone(),
            until: mono_ns,
        })
    }

    fn random(&self) -> u64 {
        let mut state = self.lock();
        state.rng = state.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// The WIT sample of a harness sample.
pub(crate) fn sample(
    value: types::Value,
    q: joker::Quality,
    seq: u64,
    ts_acq: u64,
) -> types::Sample {
    types::Sample {
        value,
        quality: wire_quality(q),
        ts_src: None,
        ts_hw: None,
        ts_acq,
        seq,
    }
}
