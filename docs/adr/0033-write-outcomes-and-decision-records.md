# ADR 0033 — Write outcomes per proposal and per tick, and decision records

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 16, §1–§2, fixes the arbiter and envelope rules, and ref/00 §4.4 fixes the outcome codes. Writing the core (document 13, tasks 9–10) showed that several facts the conformance cases, the trace and the decision log depend on are not stated anywhere:

- A tick of an output can carry zero, one or several proposals. The trace stores **one** outcome per output per tick (document 16, §7), and so do the conformance expectations (`outcome`, document 16, §8). The SDK, the tickets and the decision log, however, need an outcome **per proposal** (ref/04 §3.2, ref/09 §5.6). The link between the two is not defined.
- What a write to a manual level held by another principal gets inside the tick (document 16, A13 only defines the API answer, `J-E1105`).
- What a `relinquish` gets, and what earlier proposals of the same writer in the same tick get (A4 only says that the last one wins).
- What is applied when the range step refuses a value (`on_range = "reject"`); document 01, §7, only states `on_interlock` for the permissive and quality steps.
- Which outcome a value gets when it is clamped by the range **and** by the rate, given that "the first step that fails decides the result" (E1).
- What proposals get while the output is tripped.
- When the decision log writes a record (document 01, §19, says "per change of outcome, with repeat counts"), and how every write, including refused ones on a lower level, enters it (AGENTS.md invariant 8).

## Decision

### Per proposal

1. **Intake** (steps 5 and 6 of the tick, after the leases that ended at the latch are expired, A2):
   - a writer that may not write the output, or a proposal to a **manual level held by another writer**, is `rejected(not_owner)`;
   - a value that does not serve the output's type (E10) is `rejected(wrong_type)`;
   - neither renews a lease (A3).

   A takeover or a forced release (A13, ref/09 §5.6) is staged by the API as a `relinquish` **of the occupant**, followed by the new proposal.
2. A **relinquish** frees the writer's level at intake (document 01, §6, rule 3). Its outcome is `accepted`, with the value applied in that tick.
3. Every other proposal occupies its writer's level and renews the lease (A3). Its outcome is decided after the envelope:
   - **the last proposal of the active writer** in this tick gets the envelope's result (`accepted`, `clamped_range`, `clamped_rate`, `rejected(…)`, `tripped`);
   - a proposal on a lower level gets `overridden`, with the active level as detail;
   - an **earlier** proposal of a writer that proposed again in the same tick (A4) gets `overridden`, with its own level as detail.

### Per output and tick

4. The outcome of an output in a tick, which the trace records, the conformance cases check and the decision log tracks, is:
   - the outcome of the active writer's last proposal in this tick;
   - otherwise, the outcome of the last proposal to the output in this tick;
   - otherwise `none`.

   The applied value is always the value after the envelope, including on ticks with outcome `none`, where the envelope still runs (A7).

### Envelope facts

5. `on_range = "reject"` applies nothing new: the output keeps its last applied value, and the outcome is `rejected(range)`.
6. Clamps do not stop the envelope. A value clamped by the range continues to the rate step. The outcome is that of the **first** step that changed or refused the value (E1). A value clamped by both is therefore `clamped_range`, and the applied value is the rate-limited one.
7. While an output is tripped, it applies `safe` (document 01, §7), and every proposal that the arbiter places gets `tripped`. Leases are still renewed (A3). The trip ends with a reset (E7).
8. Going to `safe` (no active level, `rejected(clock_unsynced)`, a trip, or `on_interlock = "safe"`) applies `safe` at once, unless `safe_ramp` is declared, in which case the rate step runs toward `safe` at `safe_ramp`, with the `dt` of ADR 0032. The rate step of an active writer limits each tick's change to `rate × dt`.
9. The clock step (E1) compares the node's clock with the winning writer's `requires_clock`: `synced` needs `synced`; `holdover` accepts `holdover` or `synced`.

### Decision records

10. The core reports, at the end of each tick:
    - one **output record** each time the triple (applied value, output outcome, active level) of an output changes, carrying the previous triple's repeat count and its first and last tick;
    - one **write record** each time the (outcome, applied value) of a writer's proposals on an output changes, with the same counts.

    Refused proposals are tracked per writer as well, so every write enters the log (invariant 8) without one record per tick.
11. Each record carries its durability class. Trips, manual writes, rejections and changes of active level are **immediate** (fsync); everything else is **batch** (document 01, §19). The core only classifies; persistence is the host's.

## Consequences

- Conformance cases and the trace share one definition of "the outcome of an output in a tick". Per-proposal outcomes reach apps (`last_outcome()`), tickets and the decision log.
- A lower-priority writer that keeps proposing under a manual override produces one write record when it becomes `overridden`, and one when that changes; a stable output produces no records at all.
- The trace result keeps only the outcome code (`u8`, document 16, §7). The reason of a `rejected` and the detail of an `overridden` are only in the decision records. If replay needs them byte for byte, the trace format needs a change, which must happen before 1.0 (document 26, §5).
