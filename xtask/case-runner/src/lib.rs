//! Runs a core conformance case against `joker-core` with a virtual clock,
//! fixed-capacity storage and no allocator (docs/13 §3, §5; ref/03 §2.6).
//!
//! This is the part of the conformance runner that every target shares: the
//! host runner (`cargo xtask conformance`) reads the suites and reports with
//! `std`, and the QEMU firmware of `thumbv8m.main-none-eabihf` and
//! `riscv32imac-unknown-none-elf` embeds the same cases as static data. Both
//! drive the core through [`exec::run`], check the expectations with
//! [`check`] and hash the per-tick results with [`digest::SequenceDigest`],
//! so the digests they report are comparable byte for byte (docs/11 Phase 0
//! gate 2, docs/16 §7).
//!
//! The crate is `no_std` and never allocates (AGENTS.md §2 rule 2).

#![no_std]

pub mod check;
pub mod digest;
pub mod exec;
pub mod model;

pub use check::{Verdict, holds};
pub use digest::SequenceDigest;
pub use exec::{Observer, OutputTick, SetupError, TickGuard, TickView, Unguarded, run};
pub use model::{
    Action, ActionKind, Expected, Fixture, Node, OutcomeLit, OutputSpec, Predicates, QualityLit,
    SafeDef, Script, SignalSpec, Subject, When, WriterSpec,
};
