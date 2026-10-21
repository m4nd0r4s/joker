//! A loaded app component: compiled or deserialized code, its `joker:meta`
//! and the checks made at load (ref/04 §2.7–§2.9, §3.1, §5.4).

use core::fmt;

use sha2::{Digest, Sha256};
use wasmtime::component::Component;
use wasmtime::component::types::ComponentItem;

use crate::engine::HostEngine;
use crate::error::{HostError, codes};
use crate::meta::{self, AppMeta};

/// The world this runtime serves (ref/04 §2.9).
pub const WORLD: &str = "joker:runtime@0.1.0";

/// How strict the runtime is (`[project] mode`, ref/01).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RuntimeMode {
    /// Development: unstable features load, compiling on the node is allowed.
    Dev,
    /// Production: unstable features are refused (`J-E0003`), and only AOT
    /// artifacts load (docs/01 §9).
    Prod,
}

/// The unstable items of the world and their feature (ref/04 §2.9).
const UNSTABLE: &[(&str, &str, &str)] = &[
    ("cycle-io", "write-at", "scheduled-writes"),
    ("async-io", "write-at", "scheduled-writes"),
    ("async-io", "propose", "intents"),
    ("ml", "", "ml"),
    ("frames", "", "streams"),
    ("blocks", "", "blocks"),
];

/// The joker:runtime interfaces a component may import.
const RUNTIME_INTERFACES: &[&str] = &[
    "types", "cycle-io", "async-io", "kv", "secrets", "log", "ml", "frames", "blocks",
];

/// The WASI packages a component may import (ref/04 §2.7, ADR 0052).
const WASI_PACKAGES: &[&str] = &[
    "io",
    "clocks",
    "random",
    "cli",
    "filesystem",
    "sockets",
    "http",
];

/// An app component ready to instantiate.
#[derive(Clone)]
pub struct AppComponent {
    pub(crate) component: Component,
    pub(crate) meta: AppMeta,
    digest: [u8; 32],
}

impl fmt::Debug for AppComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppComponent")
            .field("app", &self.meta.name)
            .field("version", &self.meta.version)
            .field("digest", &hex(&self.digest))
            .finish_non_exhaustive()
    }
}

impl AppComponent {
    /// Compiles a component with the engine (JIT). Only for `dev` and tests:
    /// production loads AOT artifacts through [`crate::AotCache`].
    pub fn compile(engine: &HostEngine, wasm: &[u8], mode: RuntimeMode) -> Result<Self, HostError> {
        let meta = meta::read(wasm)?;
        let component = Component::new(engine.wasmtime(), wasm)
            .map_err(|e| HostError::new(codes::COMPONENT_INVALID, format!("{e}")))?;
        Self::checked(engine, component, meta, digest(wasm), mode)
    }

    pub(crate) fn checked(
        engine: &HostEngine,
        component: Component,
        meta: AppMeta,
        digest: [u8; 32],
        mode: RuntimeMode,
    ) -> Result<Self, HostError> {
        check_meta(&meta, mode)?;
        check_imports(engine, &component, &meta, mode)?;
        Ok(Self {
            component,
            meta,
            digest,
        })
    }

    /// The merged `joker:meta`.
    pub fn meta(&self) -> &AppMeta {
        &self.meta
    }

    /// SHA-256 of the `.wasm` bytes.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

/// SHA-256 of a component, the content address of the artifact store.
pub fn digest(wasm: &[u8]) -> [u8; 32] {
    Sha256::digest(wasm).into()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn check_meta(meta: &AppMeta, mode: RuntimeMode) -> Result<(), HostError> {
    let served = meta::parse_world(WORLD).unwrap_or_default();
    let Some(world) = meta::parse_world(&meta.world) else {
        return Err(HostError::new(codes::META_INVALID, "invalid world"));
    };
    if world > served {
        return Err(HostError::new(
            codes::WORLD_TOO_NEW,
            format!("{} is newer than {WORLD}", meta.world),
        ));
    }
    if world != served {
        return Err(HostError::new(
            codes::OUTSIDE_WINDOW,
            format!("{} is outside the window of {WORLD}", meta.world),
        ));
    }
    if meta.cycle_abi != u64::from(crate::abi::ABI_VERSION) {
        return Err(HostError::new(
            codes::OUTSIDE_WINDOW,
            format!("cycle ABI {} is not served", meta.cycle_abi),
        ));
    }
    if mode == RuntimeMode::Prod && !meta.features.is_empty() {
        return Err(HostError::new(
            codes::UNSTABLE_FEATURE,
            format!("unstable features {:?}", meta.features),
        ));
    }
    Ok(())
}

fn check_imports(
    engine: &HostEngine,
    component: &Component,
    meta: &AppMeta,
    mode: RuntimeMode,
) -> Result<(), HostError> {
    let version = meta.world.strip_prefix("joker:runtime").unwrap_or_default();
    let ty = component.component_type();
    for (name, item) in ty.imports(engine.wasmtime()) {
        let (package, rest) = name.split_once('/').unwrap_or((name, ""));
        let (interface, iface_version) = rest.split_once('@').map_or((rest, ""), |(i, v)| (i, v));
        match package {
            "joker:runtime" => {
                if !RUNTIME_INTERFACES.contains(&interface) {
                    return Err(HostError::new(codes::IMPORT_NOT_ALLOWED, name.to_owned()));
                }
                if format!("@{iface_version}") != version {
                    return Err(HostError::new(
                        codes::META_WORLD_MISMATCH,
                        format!("imports {name} but joker:meta says {}", meta.world),
                    ));
                }
                for (iface, func, feature) in UNSTABLE {
                    if *iface != interface {
                        continue;
                    }
                    let uses = func.is_empty()
                        || match &item.ty {
                            ComponentItem::ComponentInstance(inst) => {
                                inst.exports(engine.wasmtime()).any(|(n, _)| n == *func)
                            }
                            _ => false,
                        };
                    if uses && mode == RuntimeMode::Prod {
                        return Err(HostError::new(
                            codes::UNSTABLE_FEATURE,
                            format!("{name} uses the unstable feature {feature}"),
                        ));
                    }
                }
            }
            p if p
                .strip_prefix("wasi:")
                .is_some_and(|w| WASI_PACKAGES.contains(&w)) => {}
            _ => return Err(HostError::new(codes::IMPORT_NOT_ALLOWED, name.to_owned())),
        }
    }
    Ok(())
}
