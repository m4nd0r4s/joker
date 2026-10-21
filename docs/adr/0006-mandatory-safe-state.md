# ADR 0006 — Mandatory safe state, with no default

**Status:** accepted (v1) · **Date:** 2026-09

## Context

What is "safe" depends on the process: 0 kW on a battery, closed on a gas valve, open on a relief valve, "hand back to the BMS" in a building, "eject" on an inspection line. Any default is wrong for someone.

## Decision

Each output declares `safe` (a value or `relinquish`). Missing it is `J0201`. The same principle applies to `on_late` (`J0206`). The equipment's own fallback is declared in the profile and checked against `safe` (M1).

## Reasons

- It forces whoever configures to think about the failure mode of each output, at the moment they declare it.
- It makes the three layers of safe state (lease, supervisor, equipment) coherent.

## Consequences

- One more line per output in the manifest. Profiles may suggest a value, but the manifest must accept it explicitly.
