# ADR 0109 — The `compiler` feature of `jokerd` and an engine identity without a compiler

**Status:** accepted (v1) · **Date:** 2026-11

## Context

Document 07 §1 (principle 8) says nothing is compiled on the node: `jk` precompiles every app for the node's engine and sends the `.cwasm` with the `.wasm` (ADR 0052, ADR 0067). `jokerd` nevertheless always carried the Cranelift compiler, for one case only: an app of a `dev` node that arrives without a usable `.cwasm`, which the node compiles once (ADR 0067, point 3).

On the bench Pi 3 B+ (32-bit, AUR-436) that compiler is most of the binary: `jokerd` measured 22.5 MB, and `mlockall` makes the whole binary resident, so it weighs on the RSS target of document 01 §21 (≤ 60 MB). Without Cranelift the binary is 14.9 MB.

The engine identity of ADR 0052 is a digest of wasmtime's `precompile_compatibility_hash`, which only exists in a wasmtime built with a compiler, and which hashes the compiler's flags. A node without a compiler could not compute it, and would not have the identity of the laptop that compiles for it.

## Decision

1. **The feature.** `joker-host` has a `compiler` feature (wasmtime's `cranelift` and `parallel-compilation`); `all-arch` implies it. `jokerd` has a `compiler` feature that turns on `joker-host/compiler`, **off by default** and outside `minimal`. `jk`, which precompiles for nodes, always has it (`all-arch`), and so does `jk dev`, which runs the daemon in-process.
2. **A node without a compiler.** It loads apps only from `.cwasm` artifacts accepted by its engine. An app without one is refused in every mode with `J-E0010`, whose detail says the runtime has no compiler; ADR 0067 point 3 ("in `dev` the node compiles it once") applies only to a `jokerd` built with `compiler`.
3. **The engine identity.** `<engine>` stays `wasmtime-<version>-<target triple>-<16 hex digits>`, but the digits are now a 64-bit FNV-1a of the text of every setting `joker-host` gives wasmtime that changes the compiled code: the compilation target (`pulley32` on 32-bit Arm, the triple otherwise), the memory reservation and guard, the component model and its async, relaxed SIMD off, NaN canonicalization, the `speed` optimization level and epoch interruption. It does not depend on whether the build has a compiler, nor on the backends compiled in. wasmtime still checks every `.cwasm` against its engine when it loads one, so a setting missed by the text gives a refused artifact, never a wrong one.

## Consequences

- The `jokerd` of the bench Pi 3 B+ goes from 22.5 MB to 14.9 MB, and its RSS with the bench project from 61 MB to 55 MB.
- Every engine identity changes, so every AOT cache, on nodes and in `~/.cache/joker/aot`, is rebuilt by construction.
- A board whose `arch` does not match the node's system makes `jk apply` precompile for the wrong triple. A node with a compiler hid it by compiling in `dev`; a node without one reports `J-E0010`. The bench Pi 3 B+ runs a 32-bit system while the catalogue's `rpi3b-plus` says `aarch64`.
- Document 01 §9, ref/04 (`J-E0010`), ref/06 §3.8 and ref/09 §5.13 follow.
