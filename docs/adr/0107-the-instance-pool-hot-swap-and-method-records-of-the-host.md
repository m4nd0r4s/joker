# ADR 0107 — The instance pool of the host, the hot swap of an app's instances, and method records in the cycle subset

**Status:** accepted (v1) · **Date:** 2026-11

## Context

Document 11 (Phase 1) asks `joker-host` for pooling, epochs, two instances per app and a proposal buffer with method records (AUR-397). The epochs are ADR 0102. Three things were missing, and the documents leave open how to do them:

- **Pooling.** Document 01 §9 says linear memory and tables are pre-reserved per instance, but not how many instances, nor what a slot reserves. wasmtime's pooling allocator reserves one slot per instance up front; with its defaults a 64-bit slot is 4 GiB of address space plus a 32 MiB guard, and a 32-bit slot is 10 MiB, below the 16 MiB default `memory.max` of a Rust cycle instance. The bench Pi 3 runs a 32-bit `jokerd` (Pulley), whose user address space is 3 GiB. The node had no pool: every instance was allocated on demand.
- **Two instances per app.** Document 01 §9 swaps a new version in at a tick boundary, with the old one serving until then. ADR 0091 made an app restart build fresh instances outside the cycle, but each group *prepared* them (created the wasmtime instance and ran `init`) on its own thread, between two ticks. On the bench laptop that takes about 1 ms in a debug build, against 0.2 ms for the tick: a swap made the group late, and with a slow `init` it would miss ticks.
- **Method records.** Ref/04 §3.3 defines the `METHOD` record and ref/04 §2.3 `cycle-io.call`, and the core expands method calls at step 5a (AUR-230). The cycle subset of ADR 0052 refused every app with a device, and a `METHOD` record was a malformed buffer.

## Decision

1. **The pool.** The engine of a node (`HostEngine::pooled`, used by `jokerd`) takes every instance from a wasmtime pooling allocator, created with the engine when `jokerd` starts. Its size is a number of instance slots (`PoolConfig::instances`): 32 on 64-bit nodes and 16 on 32-bit nodes by default. Each slot has one linear memory, two tables of up to 10,000 elements and up to 8 core instances; a component may use up to 2 memories and 4 tables of the pool. There are no pooled fiber stacks (the cycle calls are synchronous), and freed slots are reused before cold ones (`max_unused_warm_slots = 0`), so the resident set follows the instances alive at once. An instance past the pool is refused with `J-E0909`. Cross engines (`jk build --target`) and `HostEngine::native` have no pool.
2. **The memory of a slot.** On 64-bit targets the engine keeps wasmtime's reservation (4 GiB and a 32 MiB guard), so that Cranelift needs no bounds checks. On 32-bit targets (Pulley) every engine, native or cross, reserves 16 MiB per linear memory with a 64 KiB guard. That setting is part of the engine identity of ADR 0052, so a 32-bit node and the laptop that compiles for it keep the same identity. On a 32-bit node a cycle instance therefore cannot grow past 16 MiB, whatever `memory.max` it declares.
3. **The budget.** The pool reserves address space, not resident memory: with the defaults, 32 × (4 GiB + 32 MiB) + 32 MiB ≈ 129 GiB on a 64-bit node and 16 × (16 MiB + 64 KiB) ≈ 257 MiB on a 32-bit one, plus under 5 MiB for tables, all `PROT_NONE`. Creating the pool added nothing to the resident set on the bench laptop. A live instance makes the pages it touches resident (with `mlockall`, locked): the first echo instance added 1.7 MiB, the second 80 KiB, because its data pages are copy-on-write from the same image. `HostEngine::pool_usage` reports the slots in use and the reservation.
4. **Two instances per app, and the hot swap.** A swap has two halves, both outside the cycle thread:
   - `warm_app` creates the new instances of the app's cycle tasks and runs their `init` against an image with the layout of each task's group (its group id, period and latched ports), while the groups keep running the old ones. An `init` that fails, or a full pool, stops the swap there, and the old instances keep running.
   - `swap_in` hands the warm instances to their groups. A group takes them between two ticks: tick *k* runs the old instance, tick *k* + 1 the new one. Taking a warm instance is a pointer swap: the group sees that the instance was prepared for its layout and prepares nothing. The old instance goes to a bounded queue, and the daemon drops it outside the cycle (on its next poll, or before the next swap), which frees its slot.
   - Each group swaps at its own tick boundary (document 01 §15). The state migration of document 01 §9 (`export-state`/`migrate`) is not part of this ADR.
   - An app restart (ADR 0091, decision 6) uses the same two halves. A restart whose `init` fails answers `J-E0909` (409), and the old instances keep running; the group no longer prepares a restarted instance on its own thread. This amends ADR 0091 decision 6.
5. **Method records.** The cycle subset serves the devices of `joker:meta` (key 8) for their methods:
   - `CycleSetup::devices` binds each device, in declaration order, to its instance in the core and maps each of the device's methods to the instance's method number, with the type of each parameter. `init-context.instances` lists the bound devices.
   - A `METHOD` record, or a `cycle-io.call`, whose device or method is out of range is a malformed buffer (`J-E0907`), as ref/04 §3.4 says. Its arguments, a CBOR map with the keys `0..n` (ref/04 §4.10), are decoded without allocating into the value bits of each parameter (`bool`; integers; `f64` and `f32` as `f64` bits, from any CBOR number, finite only). Arguments that do not fit are `J-E0504`: the call is not staged and nothing traps.
   - A decoded call is staged as a core method call of the app's writer and task, in the order of ref/04 §3.4: the `cycle-io.call` calls first, then the records of the buffer. The core expands it at step 5a.
   - Outside the subset: a `Many` device (`J-E0601`); the members of a device, which get no ports in the port table yet; implementations.
   - `jokerd` binds no devices yet. The manifest checks see no devices in an app's interface, and the Rust SDK has no device handle, so an app with a device does not load on a node (`J-E0008`) until those pieces exist.

## Consequences

- The cycle thread never creates or drops a wasmtime instance for a swap or a restart. On the bench laptop the longest tick of a 10 ms group across a hot swap stayed at about 0.2 ms, against about 1 ms when the group prepared the instance itself.
- A node runs at most `PoolConfig::instances` cycle instances at once, counting the warm half of a swap. An app restart or a hot swap of an app with *n* cycle tasks needs *n* free slots.
- The 32-bit engine identity changes, so `.cwasm` files built for armv6/armv7 nodes before this ADR are rebuilt (the AOT cache keys on the identity, ADR 0052).
- The `J-E0504` rejections of a method call stay in the host's report of the call; reaching the decision log follows the wiring of devices in `jokerd`.
