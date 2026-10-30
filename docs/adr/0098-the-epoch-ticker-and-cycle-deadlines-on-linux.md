# ADR 0098 — The epoch ticker and the deadline of a Wasm cycle call on Linux

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01 §4 gives each Wasm call an epoch deadline: "the supervisor increments the engine epoch every 100 µs, and the instance is interrupted if it exceeds `max(wcet × 2, 200 µs)`". `joker-host` enables epoch interruption and sets the deadline before each `on-cycle` call, but nothing in `jokerd` advanced the epoch (AUR-397). An app that loops forever held its group's thread for good. The supervisor of ADR 0089 only notices that a group stopped ticking and stops the generation.

Three things are not written down:

- **Which thread advances the epoch, and at what priority.** Under ADR 0089 the group threads run `SCHED_FIFO` at 90 and below. A ticker at the default policy never runs while a spinning app holds a CPU, so the spin is never cut.
- **Which WCET the deadline uses.** The node does not receive the lock's `[wcet]` (ref/02 §2.1.13). Admission (`joker-manifest`, docs/12) prices a task at its measured WCET, else its declared one, else 20% of its group's period.
- **Rounding to epochs.** A deadline of `n` epochs set at a random phase of the ticker gives the call between `n − 1` and `n` periods.

## Decision

1. **The ticker.** While an engine that runs cycle instances lives, one thread `rt/epoch` advances its epoch every 100 µs. It asks for `SCHED_FIFO` 99, the supervisor's priority, so that it preempts every group. Without `CAP_SYS_NICE` it keeps the default policy, as the group threads do. It holds the engine weakly and ends when the engine is dropped.
2. **The deadline.** The `on-cycle` call of a task gets `ceil(max(2 × wcet, 200 µs) / 100 µs) + 1` epochs. The extra epoch makes the call always get at least `max(2 × wcet, 200 µs)`, and at most one period more. `wcet` is the WCET the task declares (`#[joker::cycle(wcet = …)]`, its `joker:meta`). A task that declares none gets 20% of its group's period, the same price admission uses. The lock's measured WCET reaches the node in a later version; until then admission and the deadline may differ when only the lock has one.
3. **What happens.** A call past its deadline traps with `J-E0904` (ref/04 §7). The host discards the app's proposals of that tick, and the trap goes through the trap report and the crashloop rule of ADR 0067. The rest of the tick runs.
4. **Replay** (`RunMode::Replay`) has no ticker. A replay reproduces the recorded results, and a trap recorded in the trace comes from the trace, not from the replay's clock.

## Consequences

- A runaway app costs its group at most its deadline per tick, not the group.
- The ticker wakes 10,000 times a second while Wasm apps run. On the bench Pi 3 it takes about 13% of one core (16 s of CPU in 120 s, measured on generation 6 with `light`). A ticker that only runs while a call is in flight is the way to lower it, if that cost matters on a board.
- An app whose declared WCET is far below what it takes on the board (`light` declares 5 µs) is interrupted if a call takes longer than its deadline. The deadline counts wall time, so a call that is preempted for longer than the deadline is also interrupted. `jk doctor --rt` and the WCET measured by group (`GET /v1/groups`) show both cases.
