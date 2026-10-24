//! The Wasm apps of a node's generations (ADR 0067): each app arrives as a
//! `joker.app.v1+wasm` blob of the generation, optionally with the
//! `joker.app.v1+cwasm` that `jk` precompiled for the node's triple, and
//! runs from the AOT cache (`aot/<engine>/<hex>.cwasm`, ref/02 §2.2.4).
//!
//! A `.cwasm` that the node's engine accepts is installed in the cache; one
//! built for another engine is left out. In `prod` an app with nothing in
//! the cache is refused (`J-E0010`); in `dev` the node compiles it once and
//! fills the cache (ADR 0052).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use joker_host::{AotCache, AppComponent, HostEngine, HostError, RunMode, RuntimeMode};

use crate::blobs::{Blobs, parse_digest};
use crate::control::ControlError;
use crate::generations::GenArtifact;
use crate::wasm::{Traps, WasmTasks};

/// The media type of an app component.
pub const APP_WASM: &str = "joker.app.v1+wasm";

/// The media type of an app precompiled for one engine.
pub const APP_CWASM: &str = "joker.app.v1+cwasm";

/// The prefix of the artifact name of an app (`apps.<app>`, ref/09 §5.13).
pub const APP_PREFIX: &str = "apps.";

type Loaded = BTreeMap<([u8; 32], bool), Arc<AppComponent>>;

/// The apps of a node: its engine, its blob store and its AOT cache.
pub struct NodeApps {
    engine: Arc<HostEngine>,
    blobs: Blobs,
    aot: AotCache,
    traps: Arc<Traps>,
    loaded: Mutex<Loaded>,
}

impl fmt::Debug for NodeApps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeApps")
            .field("engine", self.engine.id())
            .finish_non_exhaustive()
    }
}

fn error(code: &'static str, status: u16, detail: impl Into<String>) -> ControlError {
    ControlError {
        code,
        status,
        detail: detail.into(),
        errors: Vec::new(),
    }
}

fn invalid(app: &str, e: &HostError) -> ControlError {
    let mut err = error("J-E1601", 422, "the generation did not pass validation");
    err.errors
        .push((e.code().to_owned(), format!("app `{app}`: {}", e.message())));
    err
}

/// Whether an effective manifest runs in `prod` (`[project] mode`, whose
/// default is `prod`, ref/01 §5.1).
pub fn is_prod(manifest: &str) -> bool {
    manifest
        .parse::<toml::Table>()
        .ok()
        .and_then(|t| {
            t.get("project")
                .and_then(|p| p.get("mode"))
                .and_then(toml::Value::as_str)
                .map(|m| m != "dev")
        })
        .unwrap_or(true)
}

struct AppFiles<'a> {
    wasm: Option<&'a GenArtifact>,
    cwasm: Vec<&'a GenArtifact>,
}

impl NodeApps {
    /// The apps of the state directory `state_dir`, on this machine's
    /// engine.
    pub fn open(state_dir: &Path) -> Result<Self, HostError> {
        Ok(Self::with_engine(
            Arc::new(HostEngine::native()?),
            state_dir,
        ))
    }

    /// The apps of `state_dir` on `engine`.
    pub fn with_engine(engine: Arc<HostEngine>, state_dir: &Path) -> Self {
        Self {
            engine,
            blobs: Blobs::new(state_dir),
            aot: AotCache::new(state_dir.join("aot")),
            traps: Arc::default(),
            loaded: Mutex::new(BTreeMap::new()),
        }
    }

    /// The engine the apps run on.
    pub fn engine(&self) -> &HostEngine {
        &self.engine
    }

    /// Where the cycle tasks of every generation report their traps.
    pub fn traps(&self) -> Arc<Traps> {
        Arc::clone(&self.traps)
    }

    /// The Wasm apps of a generation, loaded from the AOT cache after
    /// installing the `.cwasm` that came with it: the provider its manifest
    /// is checked and its tasks are instantiated with. Every artifact must
    /// be in the blob store (`J-E1602`), and `prod` refuses an app the cache
    /// does not hold (`J-E0010`).
    pub fn provider(
        &self,
        artifacts: &[GenArtifact],
        prod: bool,
    ) -> Result<WasmTasks, ControlError> {
        let apps = self.app_files(artifacts)?;
        let missing: Vec<&str> = artifacts
            .iter()
            .filter(|a| parse_digest(&a.digest).is_none_or(|d| !self.blobs.contains(&d)))
            .map(|a| a.digest.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(error(
                "J-E1602",
                422,
                format!("missing blobs: {}", missing.join(", ")),
            ));
        }
        let mode = if prod {
            RuntimeMode::Prod
        } else {
            RuntimeMode::Dev
        };
        let mut tasks = WasmTasks::with_engine(Arc::clone(&self.engine), RunMode::Normal)
            .with_traps(Arc::clone(&self.traps));
        for (app, files) in apps {
            let Some(wasm) = files.wasm else {
                return Err(error(
                    "J-E1012",
                    400,
                    format!("`{APP_PREFIX}{app}` has a `{APP_CWASM}` and no `{APP_WASM}`"),
                ));
            };
            let component = self.component(app, wasm, &files.cwasm, mode)?;
            let name = tasks.insert(component);
            if name != app {
                let mut err = error("J-E1601", 422, "the generation did not pass validation");
                err.errors.push((
                    "J-E1601".to_owned(),
                    format!("the component of `{APP_PREFIX}{app}` is app `{name}`"),
                ));
                return Err(err);
            }
        }
        Ok(tasks)
    }

    fn app_files<'a>(
        &self,
        artifacts: &'a [GenArtifact],
    ) -> Result<BTreeMap<&'a str, AppFiles<'a>>, ControlError> {
        let mut apps: BTreeMap<&str, AppFiles<'_>> = BTreeMap::new();
        for a in artifacts {
            if a.kind != APP_WASM && a.kind != APP_CWASM {
                continue;
            }
            let Some(app) = a.name.strip_prefix(APP_PREFIX).filter(|n| !n.is_empty()) else {
                return Err(error(
                    "J-E1012",
                    400,
                    format!(
                        "an app artifact is named `{APP_PREFIX}<app>`, not `{}`",
                        a.name
                    ),
                ));
            };
            let files = apps.entry(app).or_insert(AppFiles {
                wasm: None,
                cwasm: Vec::new(),
            });
            if a.kind == APP_WASM {
                if files.wasm.is_some() {
                    return Err(error(
                        "J-E1012",
                        400,
                        format!("`{}` has two `{APP_WASM}` artifacts", a.name),
                    ));
                }
                files.wasm = Some(a);
            } else {
                files.cwasm.push(a);
            }
        }
        Ok(apps)
    }

    fn blob(&self, a: &GenArtifact) -> Result<Vec<u8>, ControlError> {
        let digest = parse_digest(&a.digest)
            .ok_or_else(|| error("J-E1012", 400, format!("bad digest `{}`", a.digest)))?;
        self.blobs
            .get(&digest)
            .map_err(|e| error("J-E1602", 422, format!("blob {}: {e}", a.digest)))
    }

    fn component(
        &self,
        app: &str,
        wasm: &GenArtifact,
        cwasm: &[&GenArtifact],
        mode: RuntimeMode,
    ) -> Result<Arc<AppComponent>, ControlError> {
        let digest = parse_digest(&wasm.digest)
            .ok_or_else(|| error("J-E1012", 400, format!("bad digest `{}`", wasm.digest)))?;
        let key = (digest, mode == RuntimeMode::Prod);
        if let Some(c) = self
            .loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Ok(Arc::clone(c));
        }
        let bytes = self.blob(wasm)?;
        let mut refused = Vec::new();
        if !self.aot.contains(&self.engine, &bytes) {
            for c in cwasm {
                match self.aot.accept(&self.engine, &bytes, &self.blob(c)?) {
                    Ok(_) => break,
                    Err(e) => refused.push(format!("{}: {}", c.digest, e.message())),
                }
            }
        }
        let component = match self.aot.load(&self.engine, &bytes, mode) {
            Ok(c) => Arc::new(c),
            Err(e) if refused.is_empty() => return Err(invalid(app, &e)),
            Err(e) => {
                let detail = format!(
                    "{}; the engine {} refused {}",
                    e.message(),
                    self.engine.id(),
                    refused.join(", ")
                );
                return Err(invalid(app, &HostError::new(e.code(), detail)));
            }
        };
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, Arc::clone(&component));
        Ok(component)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_mode_is_prod() {
        assert!(is_prod("[project]\nname = \"x\"\n"));
        assert!(is_prod("[project]\nmode = \"prod\"\n"));
        assert!(!is_prod("[project]\nmode = \"dev\"\n"));
    }

    fn artifact(name: &str, kind: &str, bytes: &[u8]) -> GenArtifact {
        GenArtifact {
            name: name.to_owned(),
            kind: kind.to_owned(),
            digest: crate::blobs::digest_of(bytes),
        }
    }

    #[test]
    fn a_generation_without_its_blobs_is_refused_with_the_missing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let apps = NodeApps::open(dir.path()).unwrap();
        let a = artifact("apps.light", APP_WASM, b"not uploaded");
        let err = apps.provider(std::slice::from_ref(&a), false).unwrap_err();
        assert_eq!(err.code, "J-E1602");
        assert!(err.detail.contains(&a.digest), "{}", err.detail);
    }

    #[test]
    fn app_artifacts_are_named_after_their_app() {
        let dir = tempfile::tempdir().unwrap();
        let apps = NodeApps::open(dir.path()).unwrap();
        let bad = artifact("light", APP_WASM, b"x");
        assert_eq!(apps.provider(&[bad], false).unwrap_err().code, "J-E1012");
        let alone = artifact("apps.light", APP_CWASM, b"x");
        let digest = parse_digest(&alone.digest).unwrap();
        Blobs::new(dir.path()).put(&digest, b"x").unwrap();
        assert_eq!(apps.provider(&[alone], false).unwrap_err().code, "J-E1012");
    }

    #[test]
    fn prod_refuses_an_app_without_an_aot_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let apps = NodeApps::open(dir.path()).unwrap();
        let wasm = artifact("apps.light", APP_WASM, b"a component");
        let digest = parse_digest(&wasm.digest).unwrap();
        Blobs::new(dir.path()).put(&digest, b"a component").unwrap();
        let err = apps
            .provider(std::slice::from_ref(&wasm), true)
            .unwrap_err();
        assert_eq!(err.code, "J-E1601");
        assert_eq!(err.errors[0].0, "J-E0010", "{:?}", err.errors);
        let cwasm = artifact("apps.light", APP_CWASM, b"not native code");
        let digest = parse_digest(&cwasm.digest).unwrap();
        Blobs::new(dir.path())
            .put(&digest, b"not native code")
            .unwrap();
        let err = apps.provider(&[wasm, cwasm.clone()], true).unwrap_err();
        assert_eq!(err.errors[0].0, "J-E0010", "{:?}", err.errors);
        assert!(err.errors[0].1.contains(&cwasm.digest), "{:?}", err.errors);
    }

    #[test]
    fn other_artifacts_are_not_apps() {
        let dir = tempfile::tempdir().unwrap();
        let apps = NodeApps::open(dir.path()).unwrap();
        let profile = artifact("profiles/x@1.profile.toml", "joker.profile.v1+toml", b"x");
        let digest = parse_digest(&profile.digest).unwrap();
        Blobs::new(dir.path()).put(&digest, b"x").unwrap();
        let tasks = apps.provider(&[profile], true).unwrap();
        assert_eq!(tasks.names().count(), 0);
    }
}
