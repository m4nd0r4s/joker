//! The AOT cache (docs/01 §9, ref/02 §2: `aot/<engine>/<sha256>.cwasm`).
//!
//! The key is the engine identity, which covers the wasmtime version, the
//! target triple and the code-generation settings, and the SHA-256 of the
//! `.wasm`. `jk build --target` fills it on the laptop with a cross engine;
//! the node only reads it in production (docs/07 §1, principle 8).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::component::{self, AppComponent, RuntimeMode};
use crate::engine::{EngineId, HostEngine};
use crate::error::{HostError, codes};
use crate::{ffi, meta};

/// A directory of precompiled components.
#[derive(Clone, Debug)]
pub struct AotCache {
    root: PathBuf,
}

fn io(path: &Path, e: std::io::Error) -> HostError {
    HostError::new(codes::AOT_UNAVAILABLE, format!("{}: {e}", path.display()))
}

impl AotCache {
    /// A cache rooted at `root` (`/var/lib/joker/aot` on a node).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Where the artifact of `digest` for `engine` lives.
    pub fn path(&self, engine: &EngineId, digest: &[u8; 32]) -> PathBuf {
        self.root
            .join(engine.as_str())
            .join(format!("{}.cwasm", component::hex(digest)))
    }

    /// Compiles `wasm` with `engine` (native or cross) and stores the result.
    pub fn install(&self, engine: &HostEngine, wasm: &[u8]) -> Result<PathBuf, HostError> {
        meta::read(wasm)?;
        let cwasm = engine.precompile(wasm)?;
        self.store(engine.id(), &component::digest(wasm), &cwasm)
    }

    /// Stores an artifact compiled elsewhere for `engine`, atomically.
    pub fn store(
        &self,
        engine: &EngineId,
        digest: &[u8; 32],
        cwasm: &[u8],
    ) -> Result<PathBuf, HostError> {
        let path = self.path(engine, digest);
        let dir = path.parent().unwrap_or(&self.root);
        fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        let tmp = path.with_extension("cwasm.tmp");
        let mut file = fs::File::create(&tmp).map_err(|e| io(&tmp, e))?;
        file.write_all(cwasm).map_err(|e| io(&tmp, e))?;
        file.sync_all().map_err(|e| io(&tmp, e))?;
        fs::rename(&tmp, &path).map_err(|e| io(&path, e))?;
        Ok(path)
    }

    /// True when the artifact of `wasm` for `engine` is in the cache.
    pub fn contains(&self, engine: &HostEngine, wasm: &[u8]) -> bool {
        self.path(engine.id(), &component::digest(wasm)).is_file()
    }

    /// Loads `wasm` from its artifact. On a miss, `dev` compiles and stores
    /// it; `prod` refuses with `J-E0010`.
    pub fn load(
        &self,
        engine: &HostEngine,
        wasm: &[u8],
        mode: RuntimeMode,
    ) -> Result<AppComponent, HostError> {
        if !engine.is_native() {
            return Err(HostError::new(
                codes::AOT_UNAVAILABLE,
                format!("{} cannot run code for {}", engine.id(), engine.triple()),
            ));
        }
        let digest = component::digest(wasm);
        let path = self.path(engine.id(), &digest);
        if !path.is_file() {
            if mode == RuntimeMode::Prod {
                return Err(HostError::new(
                    codes::AOT_UNAVAILABLE,
                    format!("no AOT artifact at {}", path.display()),
                ));
            }
            self.install(engine, wasm)?;
        }
        let meta = meta::read(wasm)?;
        let cwasm = fs::read(&path).map_err(|e| io(&path, e))?;
        let component = ffi::deserialize_component(engine.wasmtime(), &cwasm)?;
        AppComponent::checked(engine, component, meta, digest, mode)
    }
}
