//! The Joker Rust SDK (ref/04 §4), imported as `joker`:
//!
//! ```toml
//! [dependencies]
//! joker = { package = "joker-sdk", version = "0.1" }
//! ```
//!
//! An app is a struct with `#[joker::app]` whose fields are typed ports,
//! with cycle tasks (`#[joker::cycle]`), which read the latch of a tick and
//! propose without allocating, and async tasks (`#[joker::task]`), which run
//! on a trigger and may wait. Built for `wasm32-wasip2`, the crate is a
//! component of the world `joker:runtime@0.1.0` with its `joker:meta`
//! section; on other targets the macros generate no exports, and the test
//! harness `joker-test` drives the app in process.
//!
//! Reads return `Result` unless the sample is `Good` (docs/16 §3.1): a `Bad`
//! sample never becomes a number.

#![no_std]

extern crate alloc;
#[cfg(any(test, target_family = "wasm"))]
extern crate std;

mod bounded;
#[doc(hidden)]
pub mod cbor;
mod error;
mod handles;
mod host;
mod io;
pub mod log;
mod record;
mod runtime;
mod state;
mod time;
mod value;
pub mod view;

pub use bounded::{Bits, BoundedBytes, BoundedString, BoundedVec};
pub use error::{Code, Error, QualityError, Result};
pub use handles::{
    AlarmHandle, CounterHandle, CycleInput, CycleInternal, CycleOutput, EventOutputHandle, History,
    InputHandle, InternalHandle, OutputHandle, ParamHandle, PublishedHandle, TaskInput,
    TaskInternal, TaskOutput, WriteTicket,
};
pub use host::Agg;
pub use io::{
    Alarm, App, Counter, CycleIo, EventOutput, Input, Internal, Output, Param, Published, TaskIo,
};
pub use joker_sdk_macros::{Enum, Event, Record, app, cycle, migrate, on_params, state, task};
pub use record::{Enum, Event, Field, Record};
pub use runtime::{Change, Delivery};
pub use state::{ParamChanges, Persist, Shared, State, StateBlob};
pub use time::{ClockQuality, Duration, DurationExt, Instant, MonoNanos, UtcNanos, UtcSeconds};
pub use value::{
    AlarmState, ClampKind, Level, PortValue, Quality, QualityCode, QualityReason, RejectReason,
    Sample, Value, WriteOutcome,
};

/// What an app file imports: `use joker::prelude::*;`.
pub mod prelude {
    pub use crate::log;
    pub use crate::{
        Agg, Alarm, AlarmHandle, AlarmState, App, Bits, BoundedBytes, BoundedString, BoundedVec,
        Change, ClampKind, ClockQuality, Counter, CounterHandle, CycleInput, CycleInternal,
        CycleIo, CycleOutput, Delivery, Duration, DurationExt, Enum, Error, Event, EventOutput,
        EventOutputHandle, History, Input, Instant, Internal, Level, MonoNanos, Output, Param,
        ParamChanges, ParamHandle, PortValue, Published, PublishedHandle, Quality, QualityCode,
        QualityError, QualityReason, Record, RejectReason, Result, Sample, Shared, StateBlob,
        TaskInput, TaskInternal, TaskIo, TaskOutput, UtcNanos, UtcSeconds, Value, WriteOutcome,
    };
}

#[doc(hidden)]
pub mod __private {
    pub use crate::host::{HostFuture, TaskHost, bits, install as install_host, wire};
    pub use crate::io::{
        CycleCx, CycleRun, Layout, PortDecl, PortKind, ProposalBuffer, Src, TaskCx, is_crate_root,
        new_state, port_index,
    };
    pub use crate::runtime::{
        AppEntry, AsyncTask, CycleTask, Migration, OnParams, Runtime, StateCodec, TaskArgs,
        TaskFuture, TriggerDecl, boxed, load_field, load_state, migrated, new_shared, save_state,
        task_io,
    };
    pub use crate::value::sealed::Sealed;
    pub use inventory;
    pub use joker_sdk_sys as sys;
}
