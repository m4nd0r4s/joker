//! Runs a core conformance case against `joker-core` with a virtual clock,
//! fixed-capacity storage and no allocator (docs/13 §3, §5; ref/03 §2.6).
//!
//! This is the part of the conformance runner that every target shares: the
//! host runner (`cargo xtask conformance`) reads the suites and reports with
//! `std`, and the QEMU firmware of `thumbv8m.main-none-eabihf` and
//! `riscv32imac-unknown-none-elf` embeds the same cases as static data. Both
//! size the expression and state storage with [`exec::needs`], drive the
//! core through [`exec::run`], check the expectations with
//! [`check`] and hash the per-tick results with [`digest::SequenceDigest`],
//! so the digests they report are comparable byte for byte (docs/11 Phase 0
//! gate 2, docs/16 §7).
//!
//! The crate is `no_std` and never allocates (AGENTS.md §2 rule 2).

#![no_std]
#![allow(clippy::result_large_err)]

pub mod check;
pub mod digest;
pub mod env;
pub mod exec;
pub mod model;

pub use check::{Verdict, holds};
pub use digest::SequenceDigest;
pub use exec::{
    AlarmTick, Needs, Observer, OutputTick, STAGING, SetupError, TickGuard, TickView, Unguarded,
    needs, run,
};
pub use joker_expr::Program;
pub use model::{
    Action, ActionKind, AlarmSpec, AtomicSpec, CalcSpec, CommitSpec, Expected, Fixture,
    InstanceSpec, Item, LimitSpec, Link, MethodSpec, Node, OutcomeLit, OutputSpec, ParamSpec,
    Predicates, QualityLit, SafeDef, Script, SignalSpec, StmtSpec, Subject, When, WriterSpec,
};
