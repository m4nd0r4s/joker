//! The wasmtime engine of the app instances (docs/01 §9).
//!
//! Every engine, native or cross, is configured for an explicit target triple
//! with the baseline CPU features of that triple: the engine of a node and the
//! cross engine of the laptop that compiles for it are then the same engine,
//! with the same [`EngineId`], and a `.cwasm` built on one loads on the other
//! (docs/07 §1, principle 8; ADR 0052).

use core::fmt;
use core::hash::{Hash, Hasher};

use wasmtime::{Config, Engine, OptLevel};

use crate::error::{HostError, codes};

/// The wasmtime version this crate pins (`=49.0.4` in the workspace).
pub const WASMTIME_VERSION: &str = "49.0.4";

/// The identity of an engine for the AOT cache: wasmtime version, target
/// triple and a digest of every setting that changes the compiled code.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EngineId(String);

impl EngineId {
    /// The identity as a path segment (`wasmtime-49.0.4-aarch64-unknown-linux-gnu-…`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The engine that compiles and runs app components.
#[derive(Clone)]
pub struct HostEngine {
    engine: Engine,
    triple: String,
    id: EngineId,
    native: bool,
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
        let mut engine = Self::build(&triple)?;
        engine.native = true;
        Ok(engine)
    }

    /// A cross engine for `triple` (`aarch64-unknown-linux-gnu`): it only
    /// precompiles, unless `triple` is this machine's.
    pub fn for_target(triple: &str) -> Result<Self, HostError> {
        let mut engine = Self::build(triple)?;
        engine.native = host_triple().is_ok_and(|h| h == triple);
        Ok(engine)
    }

    fn build(triple: &str) -> Result<Self, HostError> {
        let mut config = Config::new();
        config
            .target(triple)
            .map_err(|e| HostError::new(codes::AOT_UNAVAILABLE, format!("target {triple}: {e}")))?;
        config
            .wasm_component_model(true)
            .wasm_component_model_async(true)
            .wasm_relaxed_simd(false)
            .cranelift_nan_canonicalization(true)
            .cranelift_opt_level(OptLevel::Speed)
            .epoch_interruption(true)
            .parallel_compilation(true);
        let engine = Engine::new(&config).map_err(|e| {
            HostError::new(codes::AOT_UNAVAILABLE, format!("engine for {triple}: {e}"))
        })?;
        let mut hasher = Fnv1a::default();
        engine.precompile_compatibility_hash().hash(&mut hasher);
        let id = EngineId(format!(
            "wasmtime-{WASMTIME_VERSION}-{triple}-{:016x}",
            hasher.finish()
        ));
        Ok(Self {
            engine,
            triple: triple.to_owned(),
            id,
            native: false,
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
    pub fn precompile(&self, wasm: &[u8]) -> Result<Vec<u8>, HostError> {
        self.engine
            .precompile_component(wasm)
            .map_err(|e| HostError::new(codes::COMPONENT_INVALID, format!("{e}")))
    }

    /// Advances the epoch by one. The supervisor calls this every 100 µs
    /// (docs/01 §4); deadlines are counted in these ticks.
    pub fn increment_epoch(&self) {
        self.engine.increment_epoch();
    }

    pub(crate) fn wasmtime(&self) -> &Engine {
        &self.engine
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
        assert!(native.id().as_str().starts_with("wasmtime-49.0.4-"));
    }

    #[test]
    fn engines_for_the_bench_targets() {
        let pi = HostEngine::for_target("aarch64-unknown-linux-gnu").unwrap();
        let ci = HostEngine::for_target("x86_64-unknown-linux-gnu").unwrap();
        assert_ne!(pi.id(), ci.id());
        assert!(pi.id().as_str().contains("aarch64-unknown-linux-gnu"));
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
