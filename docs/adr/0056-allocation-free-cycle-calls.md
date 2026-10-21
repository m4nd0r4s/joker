# ADR 0056 — Allocation-free cycle calls: the reserved buffers of `on-cycle`, the trapping allocator and `J-E0903`

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Ref/04 §4.13 carries invariant 6 into the cycle instance: during `on-cycle` the guest's global allocator traps (`J-E0903`), and the canonical ABI of `on-cycle` uses buffers reserved at `init`, which `cabi_realloc` returns, plus a 4 KiB arena, reset at each tick, for the values of `cycle-io.read`. ADR 0053 put the guest glue in `joker-sdk-sys`. Building it on wasmtime 49 and `wit-bindgen` 0.61 showed what the reference leaves open:

- The `on-cycle` glue that `wit-bindgen` generates lifts the image into a `Vec<u8>` and lowers `cycle-result` from an owned `Vec<u8>` and two `String`s, which the post-return frees. Every tick allocates in the guest, and the image and proposal buffers cannot be the reserved ones.
- Wasmtime 49 runs the post-return inside `TypedFunc::call`. A host that borrows the result (`WasmList`) reads memory the post-return has already freed; the host of AUR-194 therefore lifted the result into an owned `Vec<u8>`, one host allocation per call.
- A guest trap is a plain Wasm trap (`unreachable`). The host cannot tell `J-E0903` from a panic (`J-E0905`).
- `cabi_realloc` is one function for every lowering into the guest: the image of `on-cycle`, the arguments of `init`, `on-params` and `migrate`, and the results of `cycle-io`. Ref/04 does not say how it tells them apart.
- `#[joker::cycle]` creates the state of a stateful task with `Default` on the first tick, which allocates in `on-cycle` (`Box`).
- With the component model async enabled, which the world needs because `run-task` and `on-action` are `async func`, wasmtime 49 (and 50) creates and frees, for **every** synchronous call into a component, a task, a thread, a waitable set and a hash set in the store's concurrency state (`enter_guest_sync_call`): 4 allocations and 4 frees per call, inside wasmtime, that no embedder setting avoids while the engine accepts async-typed exports.

## Decision

1. **Reserved buffers.** `init` reserves, through `joker_sdk_sys::cycle::reserve(image-len, proposal-capacity)`, an image buffer of exactly `image-len` bytes and a proposal buffer of `proposal-capacity` bytes. Both live until the instance ends; the global allocator never frees them.
2. **`cabi_realloc`.** Outside `on-cycle`, a request for `image-len` bytes with alignment 1, while the image buffer is not lent, returns the image buffer; every other request goes to the heap, as `wit-bindgen`'s does. Inside `on-cycle`, every request is served from the 4 KiB arena, reset at the start of each call; a request that does not fit, or a reallocation, traps with `J-E0903`. An argument of another export that happens to get the image buffer stays valid until the next `on-cycle`, and freeing it is a no-op.
3. **The `on-cycle` export is written by `joker-sdk-sys`, not by `wit-bindgen`** (`skip: ["on-cycle"]`). The task writes the buffer of §3.3 in place into the reserved proposal buffer; `cycle-result` points at it and at two static buffers for `error.code` (16 B) and `error.message` (256 B), the lengths the host keeps. The export has **no post-return**: the result stays intact until the next call into the instance. An image that did not land in the image buffer (an `on-cycle` before `init`) is freed after the task.
4. **The host reads the result after the call**, as borrowed views (`WasmList<u8>`, `WasmStr`), and copies them into buffers reserved when the instance is created. A buffer larger than `proposal-capacity`, or an error that is not UTF-8, is `J-E0907`. This is part of the cycle ABI v1 for every SDK, Rust and tier 2: **the memory of `cycle-result` must not change between the return of `on-cycle` and the next call into the instance**, so an `on-cycle` post-return, if any, must not free or reuse it.
5. **The trapping allocator.** `joker-sdk-sys` installs the global allocator of a component: the system allocator outside `on-cycle`; inside it, any allocation or growth writes the log record `J-E0903: heap allocation during a cycle task` (level `error`, no fields) and traps. Freeing is allowed. When a call ends in a plain Wasm trap and the last `error` record of that call starts with `J-E0903:`, the host reports `J-E0903` instead of `J-E0905`. Only codes of the guest-raised traps of ref/04 §7 (`J-E0903`) are honoured this way; the record is written before the log's level filter and rate limit apply, so the code is reported even when the record is dropped.
6. **Task state at `init`.** `#[joker::cycle]` registers a constructor for the state of a stateful task; `init` creates every state, so the first tick does not allocate.
7. **Wasmtime's own allocations.** The 4 allocations and 4 frees per call of wasmtime's concurrency bookkeeping are accepted for M0 and measured: the no-alloc test of the cycle path (1,000 ticks of the track light app under `assert_no_alloc`) asserts that every allocator call on the tick path is one of those 8 per call, and that joker-host, joker-core and the guest add none. Removing them needs one of: a wasmtime that reuses its sync-call task records, a cycle engine without the component model async (a world whose cycle instance has no async-typed exports), or a preallocated, lock-free pool behind the global allocator of `jokerd` for the cycle thread. The choice is for F1-HOST-COMPLETO.

## Consequences

- No guest or joker-host allocation per tick; the light app's call costs less (no copy into an owned `Vec`, no guest `malloc`/`free`).
- A `Vec` grown in a cycle task traps on the first tick it grows, not on a later tick with silent corruption (ref/04 §10, S3). Capacity reserved in `init` (or in the state's `Default`) can be used.
- Values of `cycle-io.read` (strings and bytes) live only until the next call: keeping one in the task state keeps a view into the reset arena.
- A tier 2 SDK whose generated `on-cycle` frees the result in a post-return breaks rule 4 and reads as garbage, refused by the structural validation (`J-E0907`) at best. The tier 2 SDKs (ref/05) need the same reserved-buffer glue.
- A component that installs its own `#[global_allocator]` does not link with `joker-sdk-sys`.
- Ref/04 §2.6.1, §4.13 and §7 change in the same commit.
