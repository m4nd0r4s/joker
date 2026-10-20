# ADR 0020 — A `no_std` core from day one, and Joker Micro in v1

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Running on microcontrollers was Phase 5, as a separate product ("Joker Lite"). But it is of great value to the project's goal: hard real time, hardware sampling, low cost, I/O coprocessors and integration into existing firmware. And the decision that makes it cheap (a `no_std` core, without allocation, with fixed capacities) is only cheap if it is taken before the first line of code.

## Decision

- The core crates are `#![no_std]`, without allocation in the tick and with fixed capacities; `alloc` is an optional feature for Linux. The list of `no_std` crates is the one in document 13, §1 (`no_std` rule).
- Synchronization in the core is done **only** through `critical-section` and `portable-atomic` (`AtomicU64` for the `seq` values where the hardware has no 64-bit atomics), never through executor primitives. In library mode, the integrator provides the `critical-section` implementation, or enables `cs-single-core` (ref/08 §2.4, rule 8).
- **Joker Micro** (document 19) is part of v1, in Phase 2, with three delivery modes (firmware, C library, Rust crate), Joker Link and the coprocessor pattern.
- Conformance runs on both hosts and must give the same bytes.

## Reasons

- The same core, the same semantics and the same apps on Linux and on MCUs.
- Three limits now have an answer: hard real time, high-rate sampling and microcontrollers (document 12).
- The device fallback (M1) becomes natural on a coprocessor: `safe` is guaranteed on the MCU itself.

## Consequences

- Phase 0 gains `no_std` CI targets (`thumbv8m.main-none-eabihf` and `riscv32imac-unknown-none-elf`, with conformance in QEMU, blocking; Xtensa non-blocking) and conformance on the bench ESP32, and on an RP2350 when there is a board (document 13, §5).
- The core structures cannot grow at runtime; capacity is a build decision.
- No Wasm on the MCU in v1: apps compile natively (the same source), and `wasmi` is left for v1.x.
