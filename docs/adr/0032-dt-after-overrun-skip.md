# ADR 0032 — `dt()` counts whole periods, also after `overrun = "skip"`

**Status:** accepted (v1) · **Date:** 2026-10

## Context

A cycle task reads `dt()` (`dt-ns` in the cycle ABI, ref/04 §3.2: "effective period of the group") to integrate, filter and ramp. The envelope's rate step (document 01, §7, step 5) limits a change to `rate × dt`. Document 16, §10, left one question open for task 8 of document 13: after an overrun with `overrun = "skip"` (document 01, §4), the next tick runs two periods after the previous one. Should `dt()` be the nominal period or the real elapsed time?

There are three candidates:

1. **The nominal period, always.** Integrators lose a whole period on every skip, so a PI controller under load drifts, and the rate limiter is twice as strict as declared.
2. **The measured time between latches.** This is physically right, but it carries scheduling jitter into every task and into the envelope, so two runs of the same trace only agree if the latch instants are recorded with full precision and the jitter is replayed too.
3. **Whole periods since the previous executed tick.** Ticks are numbered `k`, with `t_k = t_0 + k·T` (document 01, §4). `dt = (k − k_prev) × T`, which is `T` normally and `2T` after one skipped tick.

## Decision

1. `dt()` is **whole periods since the previous executed tick of the group**: `dt = (k − k_prev) × T`, where `k` is the tick index and `T` the nominal period of the group. On the first executed tick of a group in a generation, `dt = T`.
2. The same `dt` drives every time-dependent step of the tick that needs a step length, starting with the rate limit of the envelope: the largest change in one tick is `rate × dt`.
3. The tick index `k` is the one of the scheduler slot that runs. A skipped slot is never executed and never numbered again, so the index jumps. `catch_up` runs the late slot with its own index, so `dt = T`.
4. The cycle ABI's `AFTER_SKIP` flag (ref/04 §3.2) is set exactly when `k − k_prev > 1`, and `FIRST_TICK` on the first executed tick.
5. Jitter (the difference between the measured latch instant and `t_k`) goes to the statistics of step 9. It never reaches `dt()` or the envelope.

## Consequences

- Integrators and ramps stay correct across skips, and their results do not depend on scheduling jitter. Replay only needs the tick indices, which the trace already records as the tick `seq` (document 16, §7).
- A task that wants to treat a skip specially (for example, by resetting a derivative term) uses `AFTER_SKIP`; it does not have to compare `dt()` with the period.
- Document 16, §10, now points to this ADR. The `joker-core` process image implements it (`ProcessImage::latch`, `TickInfo`).
