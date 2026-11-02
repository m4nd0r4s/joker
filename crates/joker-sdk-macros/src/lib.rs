//! The macros of the Joker Rust SDK (ref/04 §4.2–§4.3, §4.12): the app,
//! task and state attributes and the `Record`, `Event` and `Enum` derives.
//! Use them through `joker-sdk` (`joker::app`, `joker::cycle`, …); the
//! generated code refers to the crate as `::joker`.
//!
//! Each macro emits its `joker:meta` fragment (ref/04 §5.2) and reports a
//! misuse as a compilation error with its code (ref/04 §4.16) on the
//! offending field or argument.

mod app;
mod cbor;
mod derive;
mod task;
mod util;

use proc_macro::TokenStream;

fn emit(r: syn::Result<proc_macro2::TokenStream>) -> TokenStream {
    match r {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Declares the app struct (ref/04 §4.2).
///
/// ```ignore
/// #[joker::app(name = "light", version = "0.1.0")]
/// pub struct Light {
///     #[output]
///     led: Output<bool>,
///     #[param(unit = "ms", range = "20..5000", default = 500.0)]
///     half_period: Param<f64>,
/// }
/// ```
///
/// Arguments: `name`, `version`, `requires_clock`, `secrets`,
/// `optional_secrets` and `proposal_buffer`. Each field has one port
/// attribute: `#[input]`, `#[output]` (with `event` for an `EventOutput<E>`),
/// `#[param]`, `#[internal(writer = "…")]`, `#[publish]` or `#[alarm]`.
#[proc_macro_attribute]
pub fn app(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(app::expand(attr.into(), item.into()))
}

/// Declares a cycle task (ref/04 §4.3.1).
///
/// ```ignore
/// #[joker::cycle(period = "20ms", wcet = "5us")]
/// fn blink(io: &mut CycleIo<Light>, st: &mut Blink) -> Result<()> { … }
/// ```
///
/// `period` is required; `wcet` must not exceed it. The state type, when
/// there is one, starts at `Default` and is created at `init`.
#[proc_macro_attribute]
pub fn cycle(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(task::expand_cycle(attr.into(), item.into()))
}

/// Declares an async task (ref/04 §4.3.2) with exactly one trigger:
/// `every = "15m"` (and `jitter`), `on = "port"` or `on = ["a", "b"]`,
/// `on_event = "port"`, `at = "cron: …"` or `startup`.
///
/// ```ignore
/// #[joker::task(on = "soc")]
/// async fn watch_soc(io: &TaskIo<PeakShaver>, s: Sample<f64>) -> Result<()> { … }
/// ```
#[proc_macro_attribute]
pub fn task(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(task::expand_task(attr.into(), item.into()))
}

/// The check of a new generation's parameters (ref/04 §4.3.4):
/// `fn(&ParamChanges) -> Result<()>`; an `Err` rejects the generation for
/// this app.
#[proc_macro_attribute]
pub fn on_params(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(task::expand_on_params(attr.into(), item.into()))
}

/// The state type of a task (ref/04 §4.2, §4.12): `persist` makes it
/// survive the hot swap, `version = N` is its schema version.
#[proc_macro_attribute]
pub fn state(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(task::expand_state(attr.into(), item.into()))
}

/// A custom migration of a persisted state type (ref/04 §4.12):
/// `#[joker::migrate(Ctl)] fn m(prev: StateBlob<'_>, from: &str) -> Result<Ctl>`.
#[proc_macro_attribute]
pub fn migrate(attr: TokenStream, item: TokenStream) -> TokenStream {
    emit(task::expand_migrate(attr.into(), item.into()))
}

/// A record of the app (ref/04 §4.10): deterministic CBOR, a map with the
/// field positions as keys, and its schema in `joker:meta`.
#[proc_macro_derive(Record)]
pub fn record(input: TokenStream) -> TokenStream {
    emit(derive::expand_record(input.into(), false))
}

/// An event record: a `Record` with the event marker.
#[proc_macro_derive(Event)]
pub fn event(input: TokenStream) -> TokenStream {
    emit(derive::expand_record(input.into(), true))
}

/// An enum without data (ref/04 §4.2): its values, a port type, a record
/// field, and `Clone`, `Copy`, `PartialEq`, `Eq` and `Debug`.
#[proc_macro_derive(Enum)]
pub fn enum_(input: TokenStream) -> TokenStream {
    emit(derive::expand_enum(input.into()))
}
