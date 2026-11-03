//! The wasmtime engine of the app instances (docs/01 §9).
//!
//! Every engine, native or cross, is configured for an explicit target triple
//! with the baseline CPU features of that triple: the engine of a node and the
//! cross engine of the laptop that compiles for it are then the same engine,
//! with the same [`EngineId`], and a `.cwasm` built on one loads on the other
//! (docs/07 §1, principle 8; ADR 0052).

use core::fmt;
use core::hash::{Hash, Hasher};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

#[cfg(feature = "compiler")]
use wasmtime::OptLevel;
use wasmtime::{Config, Engine, InstanceAllocationStrategy, PoolingAllocationConfig};

use crate::error::{HostError, codes};

/// The wasmtime version this crate pins (`=48.0.8` in the workspace).
pub const WASMTIME_VERSION: &str = "48.0.8";

/// The identity of an engine for the AOT cache: wasmtime version, target
/// triple and a digest of every setting that changes the compiled code.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EngineId(String);

impl EngineId {
    /// The identity as a path segment (`wasmtime-48.0.8-aarch64-unknown-linux-gnu-…`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How often the epoch ticker advances the epoch (docs/01 §4, ADR 0102).
pub const EPOCH_PERIOD: Duration = Duration::from_micros(100);

/// The linear memory reserved for each instance on a 32-bit node: the
/// default cycle `memory.max` of a Rust app (docs/01 §9, ADR 0107). It is
/// part of the engine identity, so a node and the laptop that compiles for
/// it use the same value.
pub const MEMORY_RESERVATION_32: u64 = 16 * 1024 * 1024;

/// Core instances a component may hold in a pool slot: the app's module and
/// the shim modules the component model adds.
const CORE_INSTANCES_PER_COMPONENT: u32 = 8;

/// Linear memories a component may hold in a pool slot.
const MEMORIES_PER_COMPONENT: u32 = 2;

/// Tables a component may hold in a pool slot.
const TABLES_PER_COMPONENT: u32 = 4;

/// Elements of one pooled table.
const TABLE_ELEMENTS: usize = 10_000;

/// The size of the instance pool of a node's engine (docs/01 §9, ADR
/// 0107): how many app instances can live at once, each with its linear
/// memory and its tables reserved when the engine is created. A generation
/// switch of an app holds two instances of each of its tasks for a moment
/// (the old one serving and the new one warming up), so the default leaves
/// room for that.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PoolConfig {
    /// Component instances alive at once.
    pub instances: u32,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            instances: if cfg!(target_pointer_width = "32") {
                16
            } else {
                32
            },
        }
    }
}

impl PoolConfig {
    /// The linear memories of the pool.
    pub fn memories(&self) -> u32 {
        self.instances
    }

    /// The tables of the pool.
    pub fn tables(&self) -> u32 {
        self.instances.saturating_mul(2)
    }

    /// The address space the pool reserves for linear memories and tables
    /// on an engine whose memories reserve `reservation` bytes with a
    /// `guard` after each: reserved, not resident (ADR 0107).
    pub fn reserved_bytes(&self, reservation: u64, guard: u64) -> u64 {
        let memories = u64::from(self.memories()) * (reservation + guard) + guard;
        let tables = u64::from(self.tables()) * (TABLE_ELEMENTS as u64) * 8;
        memories + tables
    }

    fn allocation(&self, max_memory_size: u64) -> PoolingAllocationConfig {
        let mut pool = PoolingAllocationConfig::new();
        pool.total_component_instances(self.instances)
            .total_core_instances(self.instances.saturating_mul(CORE_INSTANCES_PER_COMPONENT))
            .max_core_instances_per_component(CORE_INSTANCES_PER_COMPONENT)
            .total_memories(self.memories())
            .max_memories_per_component(MEMORIES_PER_COMPONENT)
            .max_memories_per_module(1)
            .max_memory_size(usize::try_from(max_memory_size).unwrap_or(usize::MAX))
            .total_tables(self.tables())
            .max_tables_per_component(TABLES_PER_COMPONENT)
            .max_tables_per_module(2)
            .table_elements(TABLE_ELEMENTS)
            .total_stacks(0)
            .max_unused_warm_slots(0);
        pool
    }
}

/// What the pool of an engine holds now.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PoolUsage {
    /// Component instances alive.
    pub instances: u64,
    /// Linear memories in use.
    pub memories: usize,
    /// Component instances the pool holds at most.
    pub capacity: u32,
    /// Address space reserved for memories and tables, bytes.
    pub reserved_bytes: u64,
}

/// The engine that compiles and runs app components.
#[derive(Clone)]
pub struct HostEngine {
    engine: Engine,
    triple: String,
    id: EngineId,
    native: bool,
    pool: Option<PoolConfig>,
    ticking: Arc<AtomicBool>,
    created: Arc<AtomicU64>,
}

impl fmt::Debug for HostEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostEngine")
            .field("id", &self.id)
            .field("native", &self.native)
            .finish_non_exhaustive()
    }
}

impl HostEngine {
    /// The engine of this machine, which can compile and run components.
    pub fn native() -> Result<Self, HostError> {
        let triple = host_triple()?;
        let mut engine = Self::build(&triple, None)?;
        engine.native = true;
        Ok(engine)
    }

    /// The engine of a node: [`HostEngine::native`], with every instance
    /// taken from a pool reserved here, at startup (docs/01 §9, ADR 0107).
    /// Instantiating past the pool fails with `J-E0909`.
    pub fn pooled(pool: PoolConfig) -> Result<Self, HostError> {
        let triple = host_triple()?;
        let mut engine = Self::build(&triple, Some(pool))?;
        engine.native = true;
        Ok(engine)
    }

    /// A cross engine for `triple` (`aarch64-unknown-linux-gnu`): it only
    /// precompiles, unless `triple` is this machine's.
    pub fn for_target(triple: &str) -> Result<Self, HostError> {
        let mut engine = Self::build(triple, None)?;
        engine.native = host_triple().is_ok_and(|h| h == triple);
        Ok(engine)
    }

    fn build(triple: &str, pool: Option<PoolConfig>) -> Result<Self, HostError> {
        let mut config = Config::new();
        let target = compile_target(triple);
        config
            .target(target)
            .map_err(|e| HostError::new(codes::AOT_UNAVAILABLE, format!("target {triple}: {e}")))?;
        let (reservation, guard) = memory_layout(target);
        if let Some((reservation, guard)) = reservation.zip(guard) {
            config
                .memory_reservation(reservation)
                .memory_guard_size(guard);
        }
        if let Some(pool) = pool {
            let max = reservation.unwrap_or(DEFAULT_RESERVATION_64);
            config.allocation_strategy(InstanceAllocationStrategy::Pooling(pool.allocation(max)));
        }
        config
            .wasm_component_model(true)
            .wasm_component_model_async(true)
            .wasm_relaxed_simd(false)
            .epoch_interruption(true);
        #[cfg(feature = "compiler")]
        config
            .cranelift_nan_canonicalization(true)
            .cranelift_opt_level(OptLevel::Speed)
            .parallel_compilation(true);
        let engine = Engine::new(&config).map_err(|e| {
            HostError::new(codes::AOT_UNAVAILABLE, format!("engine for {triple}: {e}"))
        })?;
        let mut hasher = Fnv1a::default();
        hasher.write(settings(target, reservation, guard).as_bytes());
        let id = EngineId(format!(
            "wasmtime-{WASMTIME_VERSION}-{triple}-{:016x}",
            hasher.finish()
        ));
        Ok(Self {
            engine,
            triple: triple.to_owned(),
            id,
            native: false,
            pool,
            ticking: Arc::new(AtomicBool::new(false)),
            created: Arc::new(AtomicU64::new(0)),
        })
    }

    /// The cycle instances created on this engine so far, `init` included.
    pub fn instances_created(&self) -> u64 {
        self.created.load(Ordering::Relaxed)
    }

    pub(crate) fn count_instance(&self) {
        self.created.fetch_add(1, Ordering::Relaxed);
    }

    /// What the instance pool holds now; `None` on an engine without one.
    pub fn pool_usage(&self) -> Option<PoolUsage> {
        let pool = self.pool?;
        let metrics = self.engine.pooling_allocator_metrics()?;
        let (reservation, guard) = memory_layout(compile_target(&self.triple));
        Some(PoolUsage {
            instances: metrics.component_instances(),
            memories: metrics.memories(),
            capacity: pool.instances,
            reserved_bytes: pool.reserved_bytes(
                reservation.unwrap_or(DEFAULT_RESERVATION_64),
                guard.unwrap_or(DEFAULT_GUARD_64),
            ),
        })
    }

    /// The cache identity of this engine.
    pub fn id(&self) -> &EngineId {
        &self.id
    }

    /// The target triple.
    pub fn triple(&self) -> &str {
        &self.triple
    }

    /// True when this engine can run what it compiles.
    pub fn is_native(&self) -> bool {
        self.native
    }

    /// Compiles a component to a `.cwasm` for this engine's target.
    /// Without the `compiler` feature it fails with `J-E0010` (ADR 0109).
    pub fn precompile(&self, wasm: &[u8]) -> Result<Vec<u8>, HostError> {
        #[cfg(feature = "compiler")]
        return self
            .engine
            .precompile_component(wasm)
            .map_err(|e| HostError::new(codes::COMPONENT_INVALID, format!("{e}")));
        #[cfg(not(feature = "compiler"))]
        {
            let _ = wasm;
            Err(no_compiler())
        }
    }

    /// True when this build can compile components (feature `compiler`).
    pub fn can_compile(&self) -> bool {
        cfg!(feature = "compiler")
    }

    /// Advances the epoch by one. The supervisor calls this every 100 µs
    /// (docs/01 §4); deadlines are counted in these ticks.
    pub fn increment_epoch(&self) {
        self.engine.increment_epoch();
    }

    /// Starts the thread `rt/epoch` that advances the epoch every
    /// [`EPOCH_PERIOD`] while this engine lives (ADR 0102); `setup` runs
    /// first on that thread, to raise its priority. A second call does
    /// nothing.
    pub fn start_epoch_ticker(
        &self,
        setup: impl FnOnce() + Send + 'static,
    ) -> Result<(), HostError> {
        if self.ticking.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let weak = self.engine.weak();
        let started = std::thread::Builder::new()
            .name("rt/epoch".into())
            .spawn(move || {
                setup();
                while let Some(engine) = weak.upgrade() {
                    engine.increment_epoch();
                    drop(engine);
                    std::thread::sleep(EPOCH_PERIOD);
                }
            });
        if let Err(e) = started {
            self.ticking.store(false, Ordering::Release);
            return Err(HostError::new(
                codes::INIT_FAILED,
                format!("cannot start the epoch ticker: {e}"),
            ));
        }
        Ok(())
    }

    pub(crate) fn wasmtime(&self) -> &Engine {
        &self.engine
    }
}

/// wasmtime's linear memory reservation on 64-bit targets: 4 GiB, so that
/// Cranelift needs no bounds checks.
const DEFAULT_RESERVATION_64: u64 = 1 << 32;

/// wasmtime's guard after each linear memory on 64-bit targets.
const DEFAULT_GUARD_64: u64 = 32 << 20;

/// The guard after each linear memory on 32-bit targets: wasmtime's.
const GUARD_32: u64 = 64 << 10;

/// The linear memory reservation and guard of a compilation target, when
/// they differ from wasmtime's: on 32-bit targets, [`MEMORY_RESERVATION_32`]
/// (docs/01 §9, ADR 0107).
/// What the engine identity digests (ADR 0109): every setting this crate
/// gives wasmtime that changes the compiled code, as fixed text. It does not
/// depend on whether the build has a compiler, so a node without one and
/// the laptop that compiles for it have the same identity.
fn settings(target: &str, reservation: Option<u64>, guard: Option<u64>) -> String {
    format!(
        "target={target};memory-reservation={reservation:?};memory-guard={guard:?};\
         component-model;component-model-async;relaxed-simd=off;\
         nan-canonicalization;opt-level=speed;epoch-interruption"
    )
}

/// The error of a build without the `compiler` feature asked to compile.
#[cfg(not(feature = "compiler"))]
pub(crate) fn no_compiler() -> HostError {
    HostError::new(
        codes::AOT_UNAVAILABLE,
        "this build of the runtime has no compiler (feature `compiler`): \
         the app needs a .cwasm precompiled by jk",
    )
}

fn memory_layout(target: &str) -> (Option<u64>, Option<u64>) {
    if target == "pulley32" {
        (Some(MEMORY_RESERVATION_32), Some(GUARD_32))
    } else {
        (None, None)
    }
}

/// The wasmtime compilation target of a node triple: Cranelift's own triple,
/// or the Pulley interpreter on 32-bit Arm, which Cranelift does not support
/// (docs/01 §9 and §22).
fn compile_target(triple: &str) -> &str {
    if triple.starts_with("arm-") || triple.starts_with("armv") {
        "pulley32"
    } else {
        triple
    }
}

/// The target triple of this machine, among the ones a node can be.
pub fn host_triple() -> Result<String, HostError> {
    let arch = std::env::consts::ARCH;
    let triple = match std::env::consts::OS {
        "linux" if arch == "arm" => "armv7-unknown-linux-gnueabihf".to_owned(),
        "linux" if cfg!(target_env = "musl") => format!("{arch}-unknown-linux-musl"),
        "linux" => format!("{arch}-unknown-linux-gnu"),
        "macos" => format!("{arch}-apple-darwin"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        os => {
            return Err(HostError::new(
                codes::AOT_UNAVAILABLE,
                format!("no engine for {arch} on {os}"),
            ));
        }
    };
    Ok(triple)
}

/// FNV-1a: a hasher whose output is fixed across Rust releases, unlike the
/// standard library's.
#[derive(Debug)]
struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fnv1a {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_cross_engines_for_the_same_triple_are_one_engine() {
        let native = HostEngine::native().unwrap();
        let cross = HostEngine::for_target(native.triple()).unwrap();
        assert_eq!(native.id(), cross.id());
        assert!(cross.is_native());
        assert!(native.id().as_str().starts_with("wasmtime-48.0.8-"));
    }

    #[test]
    fn engines_for_the_bench_targets() {
        let pi = HostEngine::for_target("aarch64-unknown-linux-gnu").unwrap();
        let ci = HostEngine::for_target("x86_64-unknown-linux-gnu").unwrap();
        assert_ne!(pi.id(), ci.id());
        assert!(pi.id().as_str().contains("aarch64-unknown-linux-gnu"));
    }

    #[test]
    fn armv7_and_armv6_nodes_get_a_pulley_engine() {
        let empty_component = b"\0asm\x0d\0\x01\0";
        for triple in [
            "armv7-unknown-linux-gnueabihf",
            "arm-unknown-linux-musleabihf",
        ] {
            let engine = HostEngine::for_target(triple).unwrap();
            assert!(engine.id().as_str().contains(triple));
            assert!(!engine.is_native());
            #[cfg(feature = "compiler")]
            engine.precompile(empty_component).unwrap();
            #[cfg(not(feature = "compiler"))]
            assert_eq!(
                engine.precompile(empty_component).unwrap_err().code(),
                codes::AOT_UNAVAILABLE
            );
        }
    }

    #[test]
    fn the_identity_does_not_depend_on_the_compiler() {
        let pi = HostEngine::for_target("arm-unknown-linux-musleabihf").unwrap();
        assert_eq!(
            pi.id().as_str(),
            "wasmtime-48.0.8-arm-unknown-linux-musleabihf-3180e7308214de33"
        );
        assert_eq!(pi.can_compile(), cfg!(feature = "compiler"));
    }

    #[test]
    fn the_pinned_version_is_the_locked_one() {
        let lock =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
                .unwrap();
        let entry = lock
            .split("[[package]]")
            .find(|p| p.contains("name = \"wasmtime\"\n"))
            .unwrap();
        assert!(entry.contains(&format!("version = \"{WASMTIME_VERSION}\"")));
    }
}
