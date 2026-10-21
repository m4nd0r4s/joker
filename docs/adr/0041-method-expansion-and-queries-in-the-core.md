# ADR 0041 — Method expansion, `own()` and queries in the core

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 17, §4.1, says that method records are expanded in step 5a of the tick, in the order of the calls, into ordinary proposals of the caller. `own(x)` sees the caller's earlier proposals of the same tick, and nested calls on fixed class members expand in the same step. Ref/04 §3.4 repeats this, and document 16, P6, makes nested paths aliases of the same `SignalId`. The core needs facts these texts do not fix:

- where expanded writes sit relative to the tick's direct writes, which matters for "the last one wins" (A4);
- exactly what `own(x)` returns;
- what a call does when an assignment or an argument has no usable value;
- which group evaluates a class query, given that a slot has a single writer (document 01, §3.2) but the query may be "evaluated in the latch of each group that consumes it" (document 17, §4.3);
- how an alias that names another alias resolves.

## Decision

1. **Position.** Step 5a comes after step 4. The writes expanded from method calls are appended after all the direct proposals of the tick, async and cycle. Calls expand in staging order: calls drained from the async inbox in step 2 come first, then cycle calls in task order. Inside a call, writes follow statement order, and a nested call expands in place. A direct write and a method write of the same writer to the same output in one tick resolve by A4, so the method write wins.
2. **`own(x)`** is the value of the caller's last proposal for `x` in this tick, among the direct proposals and the writes already expanded. Without one, it is the value the caller's level holds for `x` if that level is active. Without that either, it is the applied value of `x` (document 16, §4).
3. **Values.** Arguments are the values of the record, in the declared unit of each parameter. Assignments and the arguments of nested calls are evaluated on the tick's snapshot, and `Uncertain` results are used. If any assignment or nested argument is `Bad` or does not fit the target's type, or a call names an unknown method, or the argument count is wrong, the **whole call** expands to nothing. The core then reports `J-E0504` (invalid arguments or values) or `J-E0505` (unknown method) for that call in the decision log. A call never expands partially.
4. **Nested calls** are resolved when the generation is built, to (instance, method) pairs. Their depth is at most 3 (document 17, §8). The expanded writes are the top-level caller's, at its level (A1, P6).
5. **Queries** are derived signals. Each query has **one** owner group, which evaluates it in step 3 and writes its slot: by default the fastest group that consumes it, as for alarms (ref/01 §5.15). Other groups read the slot as they read any other group's signal (document 02, §6.3). The value carries the expression's quality, so a `Bad` query is never a number.
6. **Alias chains.** An alias may name another alias. The registry follows the chain to the canonical path when it is built, rejects cycles, and allows at most 8 links.

## Consequences

- Recorded method calls replay exactly: the trace keeps the method records (document 16, §7, `n_métodos`) and the direct proposals apart, and the replay expands the calls again.
- A method never leaves an instance half-commanded because one input was `Bad`.
- A query used by a 10 ms group and a 1 s group is evaluated at 10 ms, and the 1 s group reads it from its slot.
