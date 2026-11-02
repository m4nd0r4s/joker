# ADR 0108 — Cycle deadlines on the first call and without real-time scheduling

**Status:** accepted (v1) · **Date:** 2026-11 · **Amends:** ADR 0102 decision 2

## Context

ADR 0102 gives every `on-cycle` call `ceil(max(2 × wcet, 200 µs) / 100 µs) + 1` epochs, counted in wall time. Two cases make that deadline measure something other than the app:

- **The first call of an instance.** It touches the instance's memory and the `.cwasm` code for the first time: page faults and lazy initialization that later calls do not pay. The bench `light` app declares a WCET of 5 µs, so its first call gets about 300 µs.
- **A node without real-time scheduling.** Without `CAP_SYS_NICE` or an `RLIMIT_RTPRIO` (a developer laptop, a CI runner, a container), the group threads and `rt/epoch` keep the default policy (ADR 0089, ADR 0102 decision 1). The kernel may preempt the group thread in the middle of a call for longer than the deadline.

In both cases a healthy app traps with `J-E0904`. A trapped instance is poisoned and the app stops until it is restarted, so a single spurious trap at tick 0 takes the app down for the whole run. This was seen on the CI runners: `jokerd: app light trapped in task blink at tick 0 (J-E0904)`, after `rt/main: SCHED_FIFO 90: OS(1); running at default priority`.

## Decision

1. **The first `on-cycle` call of each instance** gets at least one group period: `ceil(max(period, 200 µs) / 100 µs) + 1` epochs, or the ADR 0102 deadline when that is larger. Later calls get the ADR 0102 deadline.
2. **A node that cannot schedule `SCHED_FIFO`** gives every `on-cycle` call one group period, with the same formula. On Linux the node can when its effective capabilities include `CAP_SYS_NICE` or its `RLIMIT_RTPRIO` is not zero, read when the instance is created. Other systems have no `SCHED_FIFO` for the node and always use one group period.
3. Nothing else changes: the ticker, `J-E0904`, the trap report and the crashloop rule of ADR 0067 stay as ADR 0102 states them. A replay still has no ticker.

## Consequences

- A runaway app still costs its group at most one period per tick on a node without real-time scheduling, and at most the ADR 0102 deadline on a real-time node after its first call.
- On a node without real-time scheduling a slow app is no longer cut at `2 × wcet`. The group's overrun policy and the WCET measured by group (`GET /v1/groups`) still show it; `jk doctor` already warns that the node has no `CAP_SYS_NICE`.
- A production node runs with real-time scheduling (docs/12), so its deadlines are the ones of ADR 0102 from the second call on.
