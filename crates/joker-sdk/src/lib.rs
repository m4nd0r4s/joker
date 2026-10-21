//! The Joker Rust SDK (ref/04 §4), imported as `joker`:
//!
//! ```toml
//! [dependencies]
//! joker = { package = "joker-sdk", version = "0.1" }
//! ```
//!
//! An app is a struct with `#[joker::app]` whose fields are typed ports, and
//! cycle tasks with `#[joker::cycle]`. Built for `wasm32-wasip2`, the crate
//! is a component of the world `joker:runtime@0.1.0` with its `joker:meta`
//! section; on other targets the macros generate no exports.
//!
//! This version serves the cycle subset of M0: inputs, outputs and
//! parameters of scalar types, and cycle tasks. Reads return `Result` unless
//! the sample is `Good` (docs/16 §3.1).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod error;
mod io;
mod runtime;
mod time;
mod value;

pub use error::{Code, Error, QualityError, Result};
pub use io::{App, CycleInput, CycleIo, CycleOutput, Input, Output, Param, ParamHandle};
pub use joker_sdk_macros::{app, cycle};
pub use time::{ClockQuality, Duration, DurationExt, Instant, MonoNanos, UtcNanos};
pub use value::{
    ClampKind, Level, PortValue, Quality, QualityCode, QualityReason, RejectReason, Sample,
    WriteOutcome,
};

/// What an app file imports: `use joker::prelude::*;`.
pub mod prelude {
    pub use crate::{
        App, ClampKind, ClockQuality, CycleInput, CycleIo, CycleOutput, Duration, DurationExt,
        Error, Input, Instant, Level, MonoNanos, Output, Param, ParamHandle, PortValue, Quality,
        QualityCode, QualityError, QualityReason, RejectReason, Result, Sample, UtcNanos,
        WriteOutcome,
    };
}

#[doc(hidden)]
pub mod __private {
    pub use crate::io::{CycleCx, Layout, PortDecl, PortKind, ProposalBuffer, TaskCx, new_state};
    pub use crate::runtime::{AppEntry, CycleTask, Runtime};
    pub use inventory;
    pub use joker_sdk_sys as sys;
}
