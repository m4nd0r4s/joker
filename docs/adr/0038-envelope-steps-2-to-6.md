# ADR 0038 — Details of envelope steps 2–6

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01, §7, and document 16, E1–E10, fix the order of the envelope steps and their outcomes. Implementing the permissive, quality, expression range, deadband, `min_interval` and actuation budget steps (document 13, task 10) showed facts that two implementations could read differently. These facts change outcomes, so they change conformance cases and the bytes of a trace:

- when the expressions of an output are evaluated, given that stateful functions (`held`, windows) depend on every evaluation;
- when the `grace` of `requires_good` starts and ends;
- whether a change equal to the deadband is suppressed;
- what `min_interval` is measured from;
- which writes count against `max_actuations`, and over which window;
- which outcome a value gets when it is clamped and then suppressed;
- what a budget rejection applies;
- in which tick `reset_when` ends a trip;
- where the keepalive comes from, given E8.

## Decision

1. **Evaluation.** In every tick of the owner group, the output's expressions are evaluated in this order: `interlock`, then the low and high `range` limits, then `reset_when`. This happens whether or not a writer is active and whatever the output's state, so stateful functions see every tick of their group. The quality of the `requires_good` signals is checked in every tick as well.
2. **Permissive (step 2).** The step fails when the interlock is not `true`, or when its quality is `Bad`, or `Uncertain` without `permit_uncertain` (document 16, §3.2). An expression range limit whose quality fails the same test, or two limits with the low one above the high one, also fail step 2 (E6). The outcome is `rejected(interlock)`, and the output applies `on_interlock` (E3).
3. **Quality (step 3).** A grace timer starts at the first tick in which some `requires_good` signal is not `Good`. It stops when all of them are `Good` again. The step fails in the ticks where the timer has run for at least `grace`, so with `grace = 0` it fails in the first tick. The outcome is `rejected(quality)`, and the output applies `on_interlock` (E4).
4. **Deadband (step 6).** A value whose distance to the last applied value is **strictly less** than `deadband` is suppressed. A change equal to the deadband passes.
5. **`min_interval` (step 6).** A value is suppressed when less than `min_interval` has passed since the last **actuation** of the output, measured between latch instants. On the first tick of a generation there is no previous actuation.
6. **Budget (step 6).** An actuation is a tick in which the applied value differs from the previous one (ref/01 §5.10). Suppressed values and keepalives are not actuations. The budget counts the actuations whose latch instant is within the declared window that ends at the current latch (a sliding window). A value whose application would exceed the budget gets `rejected(budget)`, and the output keeps its last applied value. Going to `safe` is never refused by the budget (document 01, §7: the safe value is always admissible), but it counts as an actuation.
7. **Order inside step 6:** deadband, then `min_interval`, then budget. A suppressed value applies nothing new, so its outcome is `suppressed` even if an earlier step clamped it. This refines ADR 0033 §6, which still holds for the clamps that reach the commit.
8. **Steps 2, 3 and 6 never block `safe`.** When the output goes to `safe` (no active level, clock, trip, `on_interlock = "safe"`), only `safe_ramp` applies.
9. **`reset_when` (E7).** It ends the trip in the tick in which it evaluates to `true` with `Good` quality, before that tick's envelope runs. The decision record has cause `reset`, as for `jk reset`.
10. **Keepalive (E8).** The driver's output stage generates keepalives from the latest-wins box (ref/06 §2.9 and §3.6). Step 6 only decides whether a new value goes into the box, and never touches a keepalive.

## Consequences

- Conformance cases with `held`, windows or `grace` can be written tick by tick, without knowing which path the envelope took.
- An output with a budget keeps the latch instants of its last actuations, as many as the budget allows, in the core's fixed state arena.
- The keepalive stays out of the core and out of the trace, which records only applied values.
