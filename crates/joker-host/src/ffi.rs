//! The one unsafe boundary of the host: turning `.cwasm` bytes back into
//! native code (AGENTS.md, invariant 10).
//!
//! Wasmtime cannot check that precompiled bytes are what its compiler wrote:
//! loading arbitrary bytes is undefined behaviour. The host only loads files
//! of the AOT cache, which it wrote itself or installed from a generation
//! whose artifacts are signed (ADR 0025) or, until signatures are checked,
//! were uploaded by an admin of the local API, who may already replace the
//! runtime itself (ADR 0067), under the engine's own
//! [`EngineId`] and the SHA-256 of the component they were compiled from,
//! after wasmtime has checked their header against the engine
//! ([`wasmtime::Engine::detect_precompiled`]).
//!
//! [`EngineId`]: crate::engine::EngineId

#![allow(unsafe_code)]

use wasmtime::component::Component;
use wasmtime::{Engine, Precompiled};

use crate::error::{HostError, codes};

/// Loads a component precompiled by [`crate::HostEngine::precompile`] for the
/// same engine.
///
/// Sound because the bytes come from the AOT cache, produced by
/// `Engine::precompile_component` of an engine with the same id, and wasmtime
/// checks their version and settings against `engine` before use.
pub(crate) fn deserialize_component(engine: &Engine, cwasm: &[u8]) -> Result<Component, HostError> {
    if Engine::detect_precompiled(cwasm) != Some(Precompiled::Component) {
        return Err(HostError::new(
            codes::AOT_UNAVAILABLE,
            "the AOT artifact is not a precompiled component",
        ));
    }
    unsafe { Component::deserialize(engine, cwasm) }
        .map_err(|e| HostError::new(codes::AOT_UNAVAILABLE, format!("{e}")))
}
