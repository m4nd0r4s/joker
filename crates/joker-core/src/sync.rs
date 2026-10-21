//! The only synchronisation primitives the core uses: `portable-atomic` 32-bit
//! atomics and fences (docs/13 §1, ref/08 §2.4 rule 8), swapped for `loom`
//! models when the crate is built with `--cfg loom`.
//!
//! The core uses no 64-bit atomics: on `thumbv8m` and `riscv32imac`,
//! `portable-atomic` only provides them through its lock-based fallback, which
//! a reader in an interrupt could deadlock on.

#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicU32, fence};
#[cfg(not(loom))]
pub(crate) use portable_atomic::{AtomicU32, fence};

pub(crate) use core::sync::atomic::Ordering;
