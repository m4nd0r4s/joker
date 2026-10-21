# ADR 0007 — The clock is an input of the core

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Deterministic replay, tests with a virtual clock and incident analysis require that "the same trace gives the same result". A call to the system clock inside the logic makes this impossible.

## Decision

`joker-core` and `joker-expr` receive time through a `Clock` trait. Apps receive `now()` from the tick's latch. The wall clock has quality (M14).

## Reasons

- Exact replay, and tests with simulated days in seconds.
- Correct timestamps even after NTP jumps.
- It is the same decision as Cravo's (Cravo's ADR 0007).

## Consequences

- The internal discipline is strict: `std::time` is forbidden in those crates, and this is checked with a lint.
- Apps that need the correct time declare `requires_clock = "synced"`.
