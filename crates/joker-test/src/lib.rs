//! `joker-test`: the test harness of Joker apps (ref/04 §6), with a virtual
//! clock and the real core.
//!
//! This version is the native mode of ref/04 §6.1 (`cargo test`, docs/13
//! task 19): the app is linked into the test binary, its cycle tasks run in
//! process through the same `init` and `on-cycle` exports a component has,
//! over the same packed image and proposal buffer (ref/04 §3), and its
//! proposals go through the real `joker-core`: process image, arbiter,
//! envelope and commit. Time is virtual: ten seconds of ticks run in
//! milliseconds, and the same test gives the same trace every time.
//!
//! ```ignore
//! use joker_test::prelude::*;
//!
//! let mut h = Harness::<Light>::new().group("blink", 20.ms()).param("half_period", 500.0);
//! h.run_for(10.s());
//! assert!(h.output("led").toggles_every(500.ms()));
//! ```
//!
//! The harness serves the cycle subset of the SDK: inputs, outputs and
//! parameters of scalar types, cycle tasks of one group, constant `range`
//! and `rate` envelopes, one writer level and its lease. Running the real
//! `.wasm` component (`jk test`), async tasks, devices, events and actions
//! come with the SDK features they test.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "a harness reports a misuse of the test by failing it, with the reason"
)]

mod harness;
mod series;
mod time;
mod trace;
mod value;

pub use harness::{Envelope, Harness, TaskError};
pub use series::Series;
pub use trace::{Observed, OutputTrace};
pub use value::Value;

/// What a test imports: `use joker_test::prelude::*;`.
pub mod prelude {
    pub use crate::{Envelope, Harness, Observed, OutputTrace, Series, TaskError, Value};
    pub use joker::QualityReason::*;
    pub use joker::{ClockQuality, Duration, DurationExt, Quality, QualityReason};
}
