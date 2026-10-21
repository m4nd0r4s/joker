# ADR 0048 — Real-time admission: the formula, what the WCET contains, and the memory estimate

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01 §4 asks `jk check` to sum the task budgets per `rt` CPU and to apply the rate-monotonic schedulability test "Liu & Layland, with a 30% margin" (M23, `J0350`), and document 16 §10 leaves the exact formula to this task (document 13 §4, task 17). Five things are open:

- what the 30% margin multiplies: the WCET, the utilisation, or the bound;
- what goes into a group's cost besides the app tasks, which the core also runs on the same thread (latch, expressions, arbiter, envelope, trace);
- which WCET counts when the lock has a measurement and when it does not (ref/02 §2.1.13, ref/05 §19 D15);
- how groups without an explicit `cpu` are placed on the `rt` CPUs, and what an explicit `priority` may do (`J0283`, ref/01 §5.11);
- how `J0380` ("estimated memory above 70% of the board's RAM") is estimated.

The Liu & Layland bound is sufficient, not necessary: it rejects harmonic task sets that any fixed-priority scheduler meets (two groups of 10 ms and 20 ms at 80% each half), and the bench groups of document 23 are harmonic.

## Decision

1. **The cost of a group** `C_g` per tick is the sum of:
   - each cycle task that runs in the group: the **p99.9** of `[wcet]` in the lock for a tier 1 app (Rust), the **maximum** for a tier 2 app (TypeScript, Python; ref/05 §19, D15), when the entry is valid for the app digest and the board (ref/02 §2.1.13); otherwise the `wcet` the app declares in `joker:meta`; otherwise, for tier 2, 20% of the task period (ref/05 §19); a tier 1 task with neither is counted at 20% of the period and reported by `J0430` in `prod`;
   - the call overhead of each task: 5 µs for tier 1, 200 µs for TypeScript and 1 ms for Python (the targets of ref/05 §19);
   - the core work of the tick: 20 µs fixed, plus 10 ns per latched signal and 1 µs per owned output (arbiter and envelope), the estimates of document 01 §4 until the M0 measurements (document 26 §6, gate 8) replace them in a later ADR.
2. **The margin multiplies the cost:** each `C_g` counts as `1.3 × C_g`.
3. **The test, per `rt` CPU**, with `U = Σ 1.3 × C_g / T_g` over the groups placed on it, priorities rate-monotonic (shorter period, higher priority; ties broken by group name):
   - if `U ≤ n(2^{1/n} − 1)` the CPU is schedulable (Liu & Layland);
   - otherwise, the exact **response-time analysis** decides: for each group in priority order, `R = 1.3 C_g + Σ_{h higher} ⌈R / T_h⌉ · 1.3 C_h` iterated to a fixed point, and the group is schedulable if `R ≤ T_g` (deadline = period);
   - a CPU that fails is `J0350`, naming the CPU, `U`, and the first group whose response exceeds its period.
4. **Placement.** A group with `cpu` stays on it (`J0284` outside `[resources.rt]`). The others are placed in order of period, then name: each on the `rt` CPU with the lowest utilisation so far, the lowest index on a tie. Without `[resources.rt]`, all groups share one virtual CPU, which is the case of a board with `cpus = 1` (document 01 §4: the margin cushions, it does not prove).
5. **Explicit priorities.** `priority` is the `SCHED_FIFO` priority (1–98). Two groups on the same CPU whose explicit priorities order them against their periods (a longer period with a higher priority) are `J0283`; equal periods may take any order.
6. **The memory estimate** (`J0380`) sums: 40 MiB for `jokerd`; per app, the `cycle_memory` and `async_memory` of the manifest, else the suggestion in `joker:meta`, else the tier defaults of ref/05 §19 (Rust 16 + 64 MiB, TypeScript 64 + 64 MiB, Python 128 + 128 MiB); per expression window, `ceil(window / period)` slots of 16 B (ADR 0034); per group with `trace.continuous`, `ceil(continuous / period)` ticks of 64 B plus 16 B per latched signal. The estimate above 70% of the board's `memory` is `J0380`; without a board, the check is skipped and `jk check` says so.

## Consequences

- The overhead constants of point 1 are estimates. The first real numbers of the M0 (jitter, call cost) come back as a new ADR that replaces them; the formula does not change.
- RTA admits the harmonic bench groups that the bound alone would reject, and still rejects a 1 ms group with 900 µs of tasks.
- The storage write load of `J0381` needs an endurance model per storage class that no document gives yet; it stays out of this ADR and of the admission.
