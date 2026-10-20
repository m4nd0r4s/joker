# ADR 0005 — No write path bypasses the envelope

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Edge systems tend to have "back doors": a diagnostic command, a maintenance API, an integrator's script. It is through these doors that accidents happen.

## Decision

All writes (apps, CLI, API, bridges, uplinks, approved intents) go through the arbiter and the envelope. Drivers only receive applied writes. The only exception is the supervisor's `force_safe` bit, which can only lead to the safe state.

## Reasons

- The guarantee does not depend on the discipline of whoever integrates.
- The decision log is complete, because everything goes through the same place.

## Consequences

- A technician who needs to "force" a value outside the envelope must change the envelope through config (with a justified and audited `widen`), and not through a command.
- Invariant 3 of `AGENTS.md`.
