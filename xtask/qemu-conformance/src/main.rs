//! Conformance firmware for the QEMU targets of docs/13 §5: Cortex-M33 on
//! `mps2-an505` (`thumbv8m.main-none-eabihf`) and RISC-V on `virt`
//! (`riscv32imac-unknown-none-elf`).
//!
//! `cargo xtask qemu` reads the core suites, writes the cases that run as
//! static data, builds this firmware with `JOKER_CONFORMANCE_CASES` set to
//! that file, runs it in QEMU and compares what it prints with the host. The
//! firmware runs every case with the executor of `joker-case-runner`, the
//! same as the host, and prints over semihosting one line per case: its
//! index, its status and the digest of its result sequence (docs/16 §7).
//!
//! No allocator is linked (docs/11 Phase 0 gate 3): neither this crate nor
//! its dependencies use `alloc`. On a host target the crate is an empty
//! program, so that the workspace builds and lints it everywhere.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
mod firmware;

/// The host stub: the firmware only exists for `target_os = "none"`.
#[cfg(not(target_os = "none"))]
fn main() {}
