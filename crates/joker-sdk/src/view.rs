//! The two views of an app (ref/04 §4.4): the port handles are generic over
//! the view, and an operation of the other view exists with a bound only the
//! right view satisfies, so using it is a compilation error with its code
//! (`J-E0701`, `J-E0702`) and not a "method not found".

mod sealed {
    pub trait Sealed {}
}

/// The cycle view (`CycleIo`).
#[derive(Clone, Copy, Debug)]
pub struct Cycle;

/// The async view (`TaskIo`).
#[derive(Clone, Copy, Debug)]
pub struct Task;

impl sealed::Sealed for Cycle {}
impl sealed::Sealed for Task {}

/// A view: [`Cycle`] or [`Task`].
pub trait View: sealed::Sealed + Copy + 'static {}

impl View for Cycle {}
impl View for Task {}

/// An operation only async tasks have.
#[diagnostic::on_unimplemented(
    message = "J-E0701: `{A}` is only available in async tasks (TaskIo)",
    label = "cycle tasks cannot wait for a commit or for the historian",
    note = "a cycle task that needs this publishes an event; an async task acts on it (doc 02 §8)"
)]
pub trait AsyncApi<A>: View {}

impl<A> AsyncApi<A> for Task {}

/// An operation only cycle tasks have.
#[diagnostic::on_unimplemented(
    message = "J-E0701: `{A}` is only available in cycle tasks (CycleIo)",
    label = "an async task reads the current slot, not the latch of a tick",
    note = "the latch facts of a tick (changed, pending) exist only inside a cycle task"
)]
pub trait CycleApi<A>: View {}

impl<A> CycleApi<A> for Cycle {}

/// The cycle domain writes the `Internal<T>` (`#[internal(writer = "cycle")]`).
#[derive(Clone, Copy, Debug)]
pub struct CycleWriter;

/// The async domain writes the `Internal<T>` (the default).
#[derive(Clone, Copy, Debug)]
pub struct AsyncWriter;

/// The view `Self` is the declared writer `W` of an `Internal<T>`.
#[diagnostic::on_unimplemented(
    message = "J-E0702: this internal is written by the {W} domain; declare #[internal(writer = \"…\")] to change it",
    label = "`set` from a view that is not the declared writer",
    note = "an Internal<T> has exactly one writer domain: \"async\" (the default) or \"cycle\""
)]
pub trait Writes<W>: View {}

impl Writes<CycleWriter> for Cycle {}
impl Writes<AsyncWriter> for Task {}

/// The names of the operations a view does not have, as they appear in a
/// `J-E0701` message.
#[allow(non_camel_case_types)]
pub mod api {
    /// `history()` and `history_agg()` of a port.
    #[derive(Debug)]
    pub enum history {}
    /// `write()` of an output, which waits for the commit.
    #[derive(Debug)]
    pub enum write {}
    /// `changed()` of an input.
    #[derive(Debug)]
    pub enum changed {}
    /// `pending()` of an output.
    #[derive(Debug)]
    pub enum pending {}
}
