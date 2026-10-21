# ADR 0055 — Supply chain and dependency policies

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 26 §4 (E2, E8, E9) and §9 leave four policies for Phase 0: the dependency policy, the toolchain policy, the wasmtime version policy and the licence allowlist. Document 13 §1 asks for a test that reads the dependency graph, and §5.1 fixes the toolchain rules without saying how they are checked. Three facts found while writing the checks change what the documents say:

- The cycle subset of `joker-host` pins wasmtime `=49.0.4`. Wasmtime publishes a major every month and supports a major with security fixes only while it is one of the two most recent, except every twelfth major (24, 36, 48, 60…), which is a long-term support (LTS) line with fixes for 24 months. Wasmtime 49 stops getting fixes when 51 is out, in November 2026; wasmtime 48 (2026-08-20) is supported until August 2028.
- Ref/08 §2.11 and document 25 §5 list the crates the core may depend on (`heapless`, `critical-section`, `portable-atomic`, `minicbor`), but `joker-expr` already uses `libm` (0.2, published by the Rust project for `compiler-builtins`), which neither list names.
- `cargo vet` writes a header comment in each file of its store and refuses a store without it, like the header of `Cargo.lock`.

## Decision

1. **The policies live in `docs/policies/dependencies.md`** and rank with the ADRs (`docs/README.md`). They hold the rules that CI enforces, the critical-dependency table with its fork plans, the toolchain and 0.x policy and the wasmtime policy. A rule changes there and in its check in the same commit.
2. **Checks.** `cargo deny` (licences, bans, advisories, sources), `cargo vet --locked` and the tests of the `joker-supply-chain` crate (the graph of `cargo metadata` and the toolchain files) are blocking CI jobs on every commit.
3. **Core allowlist.** The core crates (`joker-units`, `joker-expr`, `joker-core`, `joker-link-proto`, `joker-agent-proto`) depend only on `heapless`, `critical-section`, `portable-atomic`, `minicbor` and `libm`. `libm` is added as a trivial crate: pure functions, no `unsafe` that matters to the core, maintained by the Rust project. Being in the list allows a dependency; it does not choose one (the link keeps its own CBOR codec, E8).
4. **MCU licences.** Every external crate reachable from the `no_std` crates of document 13 §1, without their `std` feature, is available under MIT or Apache-2.0. The workspace allowlist of `deny.toml` stays wider.
5. **`cargo vet`.** Criteria `safe-to-deploy` for what ships and `safe-to-run` for development dependencies. Audits are imported from the sets of the cargo-vet registry (Bytecode Alliance, Google, Mozilla, ISRG, Zcash, Ariel OS), with `imports.lock` committed. A publisher is trusted for a crate only when at least two imported sets already trust it, for one year. The exemptions at adoption are a frozen baseline; a new crate or version is covered by an import, a trust entry under that rule or an audit of ours. The headers that `cargo vet` writes are the tool's, as in `Cargo.lock`; AGENTS.md rule 12 is about the files we write.
6. **Toolchains** (document 13 §5.1, made checkable): only the root `rust-toolchain.toml` and `crates/joker-micro/esp/rust-toolchain.toml` exist; the root one pins an exact version and the workspace `rust-version` is its minor; the core MSRV is at most the Rust version of the most recent `esp` toolchain (1.99 on 2026-10-21), a version the check holds and the per-phase update changes. Toolchains and 0.x dependencies are updated once per phase, with conformance as the gate.
7. **wasmtime** follows the most recent LTS line, pinned exactly: `=48.0.N` in Phase 0. A patch of the line that fixes a security advisory is taken within 7 days (critical or high) or 30 days (the others), outside the per-phase cadence. The move to the next LTS line is the per-phase task of the phase in which it is published, and never later than six months before the current line loses support. A non-LTS major needs an ADR that names the feature and the date of the return to LTS.

## Consequences

- `joker-host` moves from wasmtime 49 to 48.0.7, with `wasmtime-wasi` on the same line; the engine identity of ADR 0052 changes with it, so every AOT cache is invalidated by construction.
- Ref/08 §2.11, document 01 §9, document 13 §1, §5 and §5.1, document 25 §5 and `docs/README.md` change in the same commit.
- The core dependencies still exempt in `cargo vet` (`critical-section`, `portable-atomic`) get an audit of ours before the end of Phase 0.
- Building the `.cwasm` of each board triple in CI waits for `jk build --target` (document 07); the cache is already keyed by wasmtime version and triple.
