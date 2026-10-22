//! The local HTTP API of the daemon (ref/09).

pub mod backend;
pub mod id;
pub mod model;
pub mod problem;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
mod ws;

pub use backend::{Backend, Headers, Principal, Query, Reply};
pub use problem::Problem;
