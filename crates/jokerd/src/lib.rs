//! The Joker daemon (docs/01 §2, ADR 0016).

pub mod actions;
pub mod api;
pub mod catalog;
pub mod compress;
pub mod control;
pub mod cycle;
pub mod daemon;
pub mod decisions;
pub mod devices;
pub mod generations;
pub mod io;
pub mod load;
pub mod manual;
pub mod native;
pub mod pacer;
pub mod plan;
pub mod replay;
pub mod resources;
pub mod runner;
pub mod shared;
pub mod task;
pub mod trace_ring;
pub mod traces;
pub mod wasm;
pub mod wire;
