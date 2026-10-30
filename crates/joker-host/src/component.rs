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

/// The worlds a component may declare on this runtime, newest first: the
/// worlds of [`WORLD`]'s adapter window that have been published (ADR
/// 0098). `GET /v1/version` reports them as `wit.supported`.
pub const SUPPORTED_WORLDS: &[&str] = &[WORLD];

/// Whether a runtime serving `served` loads a component of `world`, both
/// `(major, minor, patch)` (ref/04 §2.8, ADR 0098): a world with a newer
/// major or minor is `J-E0001`; before 1.0 the window is the served minor
/// and the one before it, from 1.0 on every minor of the served major up to
/// its own; anything else is `J-E0002`. The patch never decides.
pub fn window(served: (u64, u64, u64), world: (u64, u64, u64)) -> Result<(), &'static str> {
    let (s_major, s_minor, _) = served;
    let (w_major, w_minor, _) = world;
    if (w_major, w_minor) > (s_major, s_minor) {
        return Err(codes::WORLD_TOO_NEW);
    }
    let inside = if s_major == 0 {
        w_major == 0 && w_minor + 1 >= s_minor
    } else {
        w_major == s_major
    };
    if inside {
        Ok(())
    } else {
        Err(codes::OUTSIDE_WINDOW)
    }
}

fn check_meta(meta: &AppMeta, mode: RuntimeMode) -> Result<(), HostError> {
    let served = meta::parse_world(WORLD).unwrap_or_default();
    let Some(world) = meta::parse_world(&meta.world) else {
        return Err(HostError::new(codes::META_INVALID, "invalid world"));
    };
    match window(served, world) {
        Ok(()) => {}
        Err(code) if code == codes::WORLD_TOO_NEW => {
            return Err(HostError::new(
                code,
                format!("{} is newer than {WORLD}", meta.world),
            ));
        }
        Err(code) => {
            return Err(HostError::new(
                code,
                format!("{} is outside the window of {WORLD}", meta.world),
            ));
        }
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

/// The unstable feature an import of `interface` with `exports` uses, if
/// any (ref/04 §2.9): a whole unstable interface, or one unstable function
/// of a stable one.
pub fn unstable_import(interface: &str, exports: &[&str]) -> Option<&'static str> {
    UNSTABLE
        .iter()
        .find(|(iface, func, _)| *iface == interface && (func.is_empty() || exports.contains(func)))
        .map(|(_, _, feature)| *feature)
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
                let exports: Vec<&str> = match &item.ty {
                    ComponentItem::ComponentInstance(inst) => {
                        inst.exports(engine.wasmtime()).map(|(n, _)| n).collect()
                    }
                    _ => Vec::new(),
                };
                if let Some(feature) = unstable_import(interface, &exports)
                    && mode == RuntimeMode::Prod
                {
                    return Err(HostError::new(
                        codes::UNSTABLE_FEATURE,
                        format!("{name} uses the unstable feature {feature}"),
                    ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_before_1_0_is_the_served_minor_and_the_one_before() {
        let served = (0, 2, 0);
        assert_eq!(window(served, (0, 2, 0)), Ok(()));
        assert_eq!(window(served, (0, 2, 7)), Ok(()));
        assert_eq!(window(served, (0, 1, 3)), Ok(()));
        assert_eq!(window(served, (0, 0, 9)), Err(codes::OUTSIDE_WINDOW));
        assert_eq!(window(served, (0, 3, 0)), Err(codes::WORLD_TOO_NEW));
        assert_eq!(window(served, (1, 0, 0)), Err(codes::WORLD_TOO_NEW));
    }

    #[test]
    fn from_1_0_every_minor_of_the_major_is_served() {
        let served = (1, 3, 0);
        assert_eq!(window(served, (1, 0, 0)), Ok(()));
        assert_eq!(window(served, (1, 3, 2)), Ok(()));
        assert_eq!(window(served, (1, 4, 0)), Err(codes::WORLD_TOO_NEW));
        assert_eq!(window(served, (0, 9, 0)), Err(codes::OUTSIDE_WINDOW));
        assert_eq!(window(served, (2, 0, 0)), Err(codes::WORLD_TOO_NEW));
    }

    #[test]
    fn the_served_world_is_in_its_own_window() {
        let served = meta::parse_world(WORLD).unwrap_or_default();
        for world in SUPPORTED_WORLDS {
            let parsed = meta::parse_world(world).unwrap_or_default();
            assert_eq!(window(served, parsed), Ok(()), "{world}");
        }
    }

    #[test]
    fn unstable_imports_are_named_by_their_feature() {
        assert_eq!(unstable_import("ml", &["infer"]), Some("ml"));
        assert_eq!(unstable_import("frames", &[]), Some("streams"));
        assert_eq!(unstable_import("blocks", &["next"]), Some("blocks"));
        assert_eq!(
            unstable_import("cycle-io", &["get", "write-at"]),
            Some("scheduled-writes")
        );
        assert_eq!(unstable_import("async-io", &["propose"]), Some("intents"));
        assert_eq!(unstable_import("cycle-io", &["get", "write"]), None);
        assert_eq!(unstable_import("kv", &["get"]), None);
    }
}
