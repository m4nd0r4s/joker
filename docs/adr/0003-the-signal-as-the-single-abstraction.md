# ADR 0003 — The signal is the single abstraction

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Each protocol has its own model (registers, nodes, objects, PDOs, pins). If apps speak protocols, they stop being portable, and each one reimplements scale, units and quality.

## Decision

Everything apps see is a **signal**: typed value, unit, quality (code + reason) and time (`ts_src`, `ts_acq`, `seq`), with the block, stream and event variants, and actions for commands. Drivers deliver raw values, and the semantics (scale, unit, quality) are applied by the core.

## Reasons

- Portability of apps across sites through bindings.
- Quality as a first-class citizen eliminates the family of bugs "stale value treated as good".
- Units checked at compile time eliminate the "W instead of kW" family.

## Consequences

- Protocol detail is lost in two places (per-user sessions and history on the equipment), which are declared (document 12, §14).
- Coherent commands and writes required M19 and M20 (document 09, attack 1).
