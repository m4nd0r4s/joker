# ADR 0052 — AOT cache identity, load errors, WASI terminals and the seed expansion

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Writing the cycle subset of `joker-host` (document 26 §6, M0) against wasmtime 49 showed four facts the specification depends on but does not state:

- Document 01 §9 and ref/02 §2 key the AOT cache as `aot/<engine>/<sha256>.cwasm`, "by the hash of the engine and of the CPU flags", and document 07 §1 (principle 8) says the node never compiles. They do not say what `<engine>` is, nor how a `.cwasm` cross-compiled on the laptop can carry the same key as the node's engine, whose CPU flags wasmtime detects at run time by default.
- Ref/04 §7 has no code for a component that does not validate or compile, nor for a production node that finds no usable AOT artifact.
- Every Rust `std` component for `wasm32-wasip2` imports `wasi:cli/terminal-input`, `terminal-output`, `terminal-stdin`, `terminal-stdout` and `terminal-stderr`. The table of ref/04 §2.7 does not list them, and "any other import is refused (`J-E0006`)" would refuse every Rust app.
- Ref/04 §2.7 seeds a xoshiro256\*\* (256 bits of state) with a 64-bit `seed`, and hashes `kind` and `implements` without saying how they are written. Replay reproduces `random-u64` from the generation, so both sides must expand and hash the same way.

## Decision

1. **Engine identity.** `<engine>` is `wasmtime-<version>-<target triple>-<16 hex digits>`, the digits being a 64-bit FNV-1a of wasmtime's `precompile_compatibility_hash` (every setting that changes the compiled code). Every engine, native or cross, is created for an **explicit target triple with the baseline CPU features of that triple**: no host feature detection. The laptop's cross engine for `aarch64-unknown-linux-gnu` and the Pi's native engine are then the same engine, with the same identity, and the node loads what `jk build --target` produced without compiling.
2. **Codes.** `J-E0009` (error) "component is not a valid Wasm component or failed to compile"; `J-E0010` (error) "no usable AOT artifact for this engine and component": missing in `prod`, unreadable, or rejected by the engine. In `dev`, a miss compiles on the node and fills the cache.
3. **WASI terminals.** `wasi:cli/terminal-*` are allowed in both instances and always report "not a terminal" (`none`). The whole `wasi:cli`, `wasi:io`, `wasi:clocks`, `wasi:random`, `wasi:filesystem`, `wasi:sockets` and `wasi:http` packages are allowed at load; what each function does is the table of ref/04 §2.7.
4. **Seed.** `kind` is the WIT case name (`cycle` or `task`) and `implements` is the instance name or the empty string, both UTF-8, concatenated without separators after `generation-hash` and `app`. The 256-bit state of xoshiro256\*\* is the first four outputs of SplitMix64 started at `seed`. `wasi:random` draws from the same generator, in call order.

## Consequences

- A node with a newer CPU does not use its extra features (AVX2, newer NEON extensions) in apps: the price of one artifact per triple and of never compiling on the node. Cycle apps are small; `jk bench` measures the real cost.
- Changing a code-generation setting of the engine changes the identity and empties the cache by construction.
- Ref/04 §2.7 and §7 and `spec/codes.toml` change in the same commit.
- Found on the way, for the full host (the async instance): with wasmtime 49 a store whose linker holds an `async func` import must be driven with `call_async`. The cycle instance of an app that also has async tasks therefore needs either `call_async` polled to completion on the cycle thread or a split linker; the M0 cycle subset refuses such components with `J-E0601`.
