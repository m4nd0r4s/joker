# ADR 0028 — Stable = implemented

**Status:** accepted (v1, 2026-10-21; document 26, §5 and §9) · **Date:** 2026-09

## Context

The v1.0 specification is much wider than the planned team can implement in the schedule of document 11 (document 26, §5: ~350–435 person-weeks against ~155). And the golden rule of ADR 0022 freezes, for the whole of series 1, everything that 1.0 declares stable. Declaring stable hundreds of surfaces that were never implemented is guaranteeing permanent design errors.

## Decision

1. In 1.0, only what was implemented and passed its exit gate is **stable**.
2. The rest is **reserved**, with the mechanisms that ML already uses: `@unstable` in the WIT, a reserved key in the manifest (a "reserved for a future version" code, such as `J0290`), `x-joker-since` > 1.0 in the OpenAPI (the operation responds `J-E1017`), a reserved message identifier in Joker Link and in the agent, `status = "reserved"` in the code registry.
3. A reserved surface enters a minor when it is implemented; nothing that is stable changes. The golden rule stays intact.
4. These must be right from 1.0, because they stay in the field without a software update: the trace format, layers L1–L3 of Joker Link with negotiation in `HELLO`, the OTA receiver in the MCU firmware, the minimal `agent/1.0` core, the stable subset of the WIT, the lock format, the code namespace and the media types.
5. The initial cut is that of the table in document 26, §5, to be reassessed at the end of M0 with real figures.

## Consequences

- Document 11 is redone: a new schedule baseline, pilot gates at bench scale, Deck and Hub off the critical path of the runtime.
- The references mark the reserved surfaces (mechanical work, to be done when the ADR is accepted).
- Specifying remains cheap; freezing now requires implementation.
