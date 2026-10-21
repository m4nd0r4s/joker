//! The virtualized WASI of a cycle instance (ref/04 §2.7): the clocks read the
//! latch instant of the tick, randomness is the instance generator, and there
//! is no environment, argument, stdin, filesystem or network.

use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use core::time::Duration;
use std::sync::Arc;

use wasmtime_wasi::{HostMonotonicClock, HostWallClock, WasiCtx};

use crate::rng::{SharedRng, Xoshiro};

/// The latch instant of the current tick, shared with the WASI clocks.
#[derive(Debug, Default)]
pub(crate) struct LatchTime {
    mono_ns: AtomicU64,
    utc_ns: AtomicI64,
}

impl LatchTime {
    pub(crate) fn set(&self, mono_ns: u64, utc_ns: i64) {
        self.mono_ns.store(mono_ns, Ordering::Relaxed);
        self.utc_ns.store(utc_ns, Ordering::Relaxed);
    }
}

struct LatchClock(Arc<LatchTime>);

impl HostMonotonicClock for LatchClock {
    fn resolution(&self) -> u64 {
        1
    }

    fn now(&self) -> u64 {
        self.0.mono_ns.load(Ordering::Relaxed)
    }
}

impl HostWallClock for LatchClock {
    fn resolution(&self) -> Duration {
        Duration::from_nanos(1)
    }

    fn now(&self) -> Duration {
        let utc = self.0.utc_ns.load(Ordering::Relaxed);
        Duration::from_nanos(u64::try_from(utc).unwrap_or(0))
    }
}

pub(crate) fn cycle_ctx(latch: &Arc<LatchTime>, rng: &Arc<Xoshiro>, seed: u64) -> WasiCtx {
    WasiCtx::builder()
        .monotonic_clock(LatchClock(latch.clone()))
        .wall_clock(LatchClock(latch.clone()))
        .secure_random(SharedRng(rng.clone()))
        .insecure_random(SharedRng(rng.clone()))
        .insecure_random_seed(u128::from(seed))
        .allow_tcp(false)
        .allow_udp(false)
        .allow_ip_name_lookup(false)
        .build()
}
