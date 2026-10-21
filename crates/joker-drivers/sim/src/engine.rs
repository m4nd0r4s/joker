//! The `joker:sim/plant` world (ref/03 §2.4.10) as the driver sees it.
//!
//! The driver does not embed a Wasm runtime: the node's Wasm host
//! (`joker-host`) implements [`PlantEngine`] and the node opens the driver
//! as `SimDriver<ThatEngine>`. A build without an engine refuses a model
//! with `[plant]` when the device opens (ADR 0054).

use std::boxed::Box;
use std::string::String;
use std::vec::Vec;

/// One instantiated plant component: the `step` export.
pub trait PlantInstance: Send + core::fmt::Debug {
    /// Advances the plant by `dt_ns` of virtual time. `inputs` follow the
    /// order of `[plant] inputs` and the result must follow `[plant]
    /// outputs`; `bool` is 0.0 or 1.0 and an enum is its variant value.
    fn step(&mut self, dt_ns: u64, inputs: &[f64]) -> Result<Vec<f64>, String>;
}

/// What instantiates plant components: the `init` export and the engine.
pub trait PlantEngine: 'static {
    /// Instantiates `component` (the bytes of the `.wasm`) and calls `init`
    /// with the `[plant] config` in deterministic CBOR and the model's seed.
    /// The component has no imports.
    fn instantiate(
        component: &[u8],
        config: &[u8],
        seed: u64,
    ) -> Result<Box<dyn PlantInstance>, String>;
}

/// The engine of a build without a Wasm runtime: every `[plant]` fails.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoEngine;

impl PlantEngine for NoEngine {
    fn instantiate(_: &[u8], _: &[u8], _: u64) -> Result<Box<dyn PlantInstance>, String> {
        Err(String::from(
            "this build has no Wasm engine for `[plant]`; it comes with the node's Wasm host",
        ))
    }
}
