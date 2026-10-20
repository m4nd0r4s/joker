# ADR 0002 — Apps are WebAssembly components

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Apps need to be portable across architectures, hot-swappable, isolated from one another and from the runtime, and written in more than one language. Containers solve isolation, but they cost memory, startup and IPC on every read.

## Decision

Apps are Component Model **components** in the `joker:runtime` world, on WASI 0.3 (native async, `stream<T>`, `future<T>`), executed in wasmtime with AOT required in production. Rust is tier 1; TypeScript and Python are tier 2. Native apps exist only as an exit gate (document 04, §5).

## Reasons

- One `.wasm` artifact runs on aarch64, x86_64 and armv7 (on the latter with Pulley).
- Sandbox without ambient authority: the app only sees the bound ports.
- Hot swap at a tick boundary, with state migration.
- Small boundary cost with the packed image (document 09, attack 2).
- WASI 0.3 was published in June 2026 and wasmtime 46 implements the final version.

## Consequences

- The Component Model is still evolving, and that is why the M21 versioning policy exists.
- The tier 2 SDKs embed a JS or Python engine (MB and GC pauses) and are limited to slow cycles.
- Rejected alternatives: **containers** (memory and latency); **embedded Lua/scripting** (no types, no ecosystem); **native plugins** (no isolation, no stable ABI).
