# ADR 0004 — Process image, two execution domains, two instances per app

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Control needs periodicity and consistency. ML, networking and slow I/O need async. Mixing the two in the same thread or in the same Wasm instance brings priority inversion and jitter.

## Decision

- **Cycle domain:** `SCHED_FIFO` threads per group, with latch → tasks → arbiter → envelope → commit, without allocation, without blocking and without `await`.
- **Async domain:** tokio, with last-value reads and writes as proposals to the owning group.
- **Two instances of the same component** per app (cycle and async), which share only internal signals, mailboxes and events.

## Reasons

- Snapshot consistency within the tick, and writes applied all at once.
- Async cannot delay the cycle, by construction.
- Isolation between instances gives robustness (an async trap does not kill the cycle).

## Consequences

- DX cost: state is not shared implicitly, and the SDK must make that obvious (document 04, §3.4).
- Rejected alternatives: one instance with a mutex; async running on the RT thread between ticks; shared Wasm threads (not yet stable). Detail in document 09, attack 6.
