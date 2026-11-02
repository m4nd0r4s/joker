//! What the async view asks of the host: in a component, the `async-io`
//! imports of the world (ref/04 §2.4); in the native test harness, the
//! harness itself (ref/04 §6.1), which installs its implementation with
//! [`install`].

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::Cell;
use core::future::Future;
use core::pin::Pin;

use critical_section::Mutex;
use joker_sdk_sys::joker::runtime::async_io::{Aggregate, HistoryQuery};
use joker_sdk_sys::joker::runtime::types;

use crate::time::{ClockQuality, Instant, MonoNanos, UtcNanos};
use crate::value::{Quality, Sample};

/// A future the host resolves.
pub type HostFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// The `async-io` interface, as the async view uses it.
pub trait TaskHost: Sync {
    /// The current sample of a port's slot.
    fn read(&self, port: u32) -> types::Sample;
    /// The current value of a parameter.
    fn param(&self, port: u32) -> types::Value;
    /// A proposal, without waiting.
    fn set(&self, port: u32, v: types::Value);
    /// A proposal that resolves after the commit of the owning group.
    fn write(
        &self,
        port: u32,
        v: types::Value,
    ) -> HostFuture<Result<types::WriteOutcome, types::Error>>;
    /// Releases this writer's level.
    fn relinquish(&self, port: u32);
    /// The outcome of this writer's last proposal.
    fn last_outcome(&self, port: u32) -> Option<types::WriteOutcome>;
    /// Writes a published signal, an alarm condition or an `Internal<T>`.
    fn publish(&self, port: u32, v: types::Value);
    /// Adds to a counter.
    fn counter_add(&self, port: u32, delta: u64);
    /// Emits an event.
    fn emit(&self, port: u32, payload: Vec<u8>);
    /// Reads the historian.
    fn history(&self, q: HistoryQuery) -> HostFuture<Result<Vec<types::Sample>, types::Error>>;
    /// The current instant.
    fn now(&self) -> types::Instant;
    /// Resolves at the given monotonic instant.
    fn sleep_until(&self, mono_ns: u64) -> HostFuture<()>;
    /// A number of the instance generator (ref/04 §2.7).
    fn random(&self) -> u64;
}

static HOST: Mutex<Cell<Option<&'static dyn TaskHost>>> = Mutex::new(Cell::new(None));

/// Installs the host of the native test harness.
pub fn install(host: &'static dyn TaskHost) {
    critical_section::with(|cs| HOST.borrow(cs).set(Some(host)));
}

/// The host of the async view.
pub(crate) fn host() -> &'static dyn TaskHost {
    match critical_section::with(|cs| HOST.borrow(cs).get()) {
        Some(h) => h,
        None => &Imports,
    }
}

/// The `async-io` imports of the component. Outside Wasm, without a
/// harness, there is no host: reads are `bad(not_connected)` and writes go
/// nowhere.
#[derive(Debug)]
struct Imports;

#[cfg(target_arch = "wasm32")]
impl TaskHost for Imports {
    fn read(&self, port: u32) -> types::Sample {
        joker_sdk_sys::joker::runtime::async_io::read(port)
    }
    fn param(&self, port: u32) -> types::Value {
        joker_sdk_sys::joker::runtime::async_io::param(port)
    }
    fn set(&self, port: u32, v: types::Value) {
        joker_sdk_sys::joker::runtime::async_io::set(port, &v);
    }
    fn write(
        &self,
        port: u32,
        v: types::Value,
    ) -> HostFuture<Result<types::WriteOutcome, types::Error>> {
        Box::pin(async move { joker_sdk_sys::joker::runtime::async_io::write(port, v).await })
    }
    fn relinquish(&self, port: u32) {
        joker_sdk_sys::joker::runtime::async_io::relinquish(port);
    }
    fn last_outcome(&self, port: u32) -> Option<types::WriteOutcome> {
        joker_sdk_sys::joker::runtime::async_io::last_outcome(port)
    }
    fn publish(&self, port: u32, v: types::Value) {
        joker_sdk_sys::joker::runtime::async_io::publish(port, &v);
    }
    fn counter_add(&self, port: u32, delta: u64) {
        joker_sdk_sys::joker::runtime::async_io::counter_add(port, delta);
    }
    fn emit(&self, port: u32, payload: Vec<u8>) {
        joker_sdk_sys::joker::runtime::async_io::emit(port, &payload);
    }
    fn history(&self, q: HistoryQuery) -> HostFuture<Result<Vec<types::Sample>, types::Error>> {
        Box::pin(async move { joker_sdk_sys::joker::runtime::async_io::history(q).await })
    }
    fn now(&self) -> types::Instant {
        joker_sdk_sys::joker::runtime::async_io::now()
    }
    fn sleep_until(&self, mono_ns: u64) -> HostFuture<()> {
        Box::pin(async move { joker_sdk_sys::joker::runtime::async_io::sleep_until(mono_ns).await })
    }
    fn random(&self) -> u64 {
        joker_sdk_sys::joker::runtime::async_io::random_u64()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TaskHost for Imports {
    fn read(&self, _port: u32) -> types::Sample {
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
    fn param(&self, _port: u32) -> types::Value {
        types::Value::Uint(0)
    }
    fn set(&self, _port: u32, _v: types::Value) {}
    fn write(
        &self,
        _port: u32,
        _v: types::Value,
    ) -> HostFuture<Result<types::WriteOutcome, types::Error>> {
        Box::pin(async { Err(no_host()) })
    }
    fn relinquish(&self, _port: u32) {}
    fn last_outcome(&self, _port: u32) -> Option<types::WriteOutcome> {
        None
    }
    fn publish(&self, _port: u32, _v: types::Value) {}
    fn counter_add(&self, _port: u32, _delta: u64) {}
    fn emit(&self, _port: u32, _payload: Vec<u8>) {}
    fn history(&self, _q: HistoryQuery) -> HostFuture<Result<Vec<types::Sample>, types::Error>> {
        Box::pin(async { Err(no_host()) })
    }
    fn now(&self) -> types::Instant {
        types::Instant {
            mono_ns: 0,
            utc_ns: 0,
            clock: types::ClockQuality::Unsynced,
        }
    }
    fn sleep_until(&self, _mono_ns: u64) -> HostFuture<()> {
        Box::pin(async {})
    }
    fn random(&self) -> u64 {
        0
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn no_host() -> types::Error {
    types::Error {
        code: alloc::string::String::from("J-E0601"),
        message: alloc::string::String::from("no host outside a component or the test harness"),
    }
}

/// The WIT value of a scalar of signal type `ty` (ref/04 §2.2).
pub fn wire(ty: u8, bits: u64) -> types::Value {
    match ty {
        0 => types::Value::Boolean(bits != 0),
        1 => types::Value::Int(bits as i64),
        3 | 4 => types::Value::Float(f64::from_bits(bits)),
        _ => types::Value::Uint(bits),
    }
}

/// The bits of a scalar WIT value (ref/00 §4.1); 0 for text and blobs.
pub fn bits(v: &types::Value) -> u64 {
    match v {
        types::Value::Boolean(b) => u64::from(*b),
        types::Value::Int(i) => *i as u64,
        types::Value::Uint(u) => *u,
        types::Value::Float(f) => f.to_bits(),
        types::Value::Text(_) | types::Value::Blob(_) => 0,
    }
}

/// The SDK quality of a WIT quality.
pub fn quality(q: &types::Quality) -> Quality {
    Quality::from_codes(q.code as u8, q.reason as u8)
}

/// The SDK instant of a WIT instant.
pub fn instant(i: &types::Instant) -> Instant {
    Instant {
        mono: MonoNanos(i.mono_ns),
        utc: UtcNanos(i.utc_ns),
        clock: ClockQuality::from_u8(i.clock as u8),
    }
}

/// The SDK sample of a WIT sample, with the value built by `value`.
pub fn sample<T>(s: &types::Sample, value: impl FnOnce(&types::Value) -> T) -> Sample<T> {
    Sample {
        value: value(&s.value),
        quality: quality(&s.quality),
        ts_src: s.ts_src.map(UtcNanos),
        ts_hw: s.ts_hw.map(MonoNanos),
        ts_acq: MonoNanos(s.ts_acq),
        seq: s.seq,
    }
}

/// The aggregate of a historian query.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Agg {
    /// Every stored sample (`step` is ignored).
    Raw,
    /// Mean per step.
    Mean,
    /// Minimum per step.
    Min,
    /// Maximum per step.
    Max,
    /// First per step.
    First,
    /// Last per step.
    Last,
    /// Count per step (`uint`).
    Count,
}

impl Agg {
    pub(crate) fn wire(self) -> Aggregate {
        match self {
            Agg::Raw => Aggregate::Raw,
            Agg::Mean => Aggregate::Mean,
            Agg::Min => Aggregate::Min,
            Agg::Max => Aggregate::Max,
            Agg::First => Aggregate::First,
            Agg::Last => Aggregate::Last,
            Agg::Count => Aggregate::Count,
        }
    }
}
