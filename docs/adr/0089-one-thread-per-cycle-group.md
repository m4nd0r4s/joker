# ADR 0089 — One thread per cycle group

**Status:** accepted (v1) · **Date:** 2026-10 · **Amends:** ADR 0061 decision 6 (`supervisor_force_safe`)

## Context

Document 01 §2 gives each cycle group its own thread, `rt/<group>`, `SCHED_FIFO` with a rate-monotonic priority and pinned to an `rt` CPU. `jokerd` has run every group on one `rt/cycle` thread since M0. Two consequences show on the bench:

- A hang in one group stops every group. The supervisor then forces every output of the node to `safe` (ADR 0061, ADR 0086), not only the outputs of the group that hung.
- A long tick of a slow group delays the ticks of a fast one.

Moving to one thread per group raises questions that the documents leave to the implementation, because with one thread they never came up:

- An alarm's `actions.trip` may name an output of another group. With one thread, the alarm's group set the trip directly on the shared output state. With one thread per group, the output's state belongs to another thread.
- An acknowledgement, a shelve and a write confirmation used to be drained by whichever group ticked first. With per-group state, each must reach its owner.
- Replay and the tests must stay deterministic (AGENTS.md invariant 9). Threads on a real clock are not.
- A group thread can panic, and the others go on.

## Decision

1. **The clock chooses the model.** On the real clock, each cycle group runs on its own thread `rt/<group>`. On a virtual clock (replay, tests), every group runs on one thread, earliest deadline first and ties by group index, as before. Both models tick the same per-group state with the same code. Only the scheduling differs, so a trace replays the same in both.
2. **Per-group state.** Each group holds:
   - the arbiter and envelope state of the outputs it owns, and the atomic groups it owns (J0272 already gives them one owner);
   - the state of its derived signals and its alarms;
   - its readback verification;
   - its commit channels, one per device, bound only to its own outputs.

   Every output slot and every latest-wins box keeps one writer, its owner group.
3. **An alarm that trips an output of another group** reaches the owner group in two ways:
   - **On entering `active`**, a trip request goes into the owner group's trip queue. The owner trips the output at its next tick. A full queue is counted (`trip_requests_dropped`).
   - **On every tick**, the owner group reads the alarm's `alarm.<a>.active` signal and trips the output while it reads `true`. This keeps ref/01 §5.15's rule that a reset of an output tripped by an active alarm trips it again on the next tick. It also covers a dropped request.

   The trip takes effect at the next commit of the owner group, what it already did with one thread. An activation shorter than the owner's period is delivered by the request. If that request is also dropped, the loss is counted, never silent.
4. **Commands reach their owner.**
   - Alarm commands (ack, shelve, unshelve) go into the queue of the alarm's group.
   - Write confirmations from a readback go into the queue of the output's owner group, which writes the output slot.
5. **Priorities.** Group threads run `SCHED_FIFO` and are pinned to `[resources] rt.cpus`.
   - The shortest period gets priority 90, the next distinct period 89, and so on, never below 2. Equal periods share a priority.
   - `rt/supervisor` stays at the maximum, so it runs even when a group never yields.
   - Without the permission to set them (no `CAP_SYS_NICE`), each thread warns once and runs at the default policy, as the supervisor already does.
6. **A group thread that panics** stops ticking. The supervisor sees it stale, forces its outputs to `safe` and raises its `stalled` alarm, as for a hang. The other groups keep running.
   - When the generation stops, its exit names the group and the daemon logs it. The switch to the next generation goes on: refusing it would leave the node with no generation running.
   - The next generation inherits nothing from it, so that group's outputs start at `safe` (docs/16 A8).
   - A dead group is not restarted in place. Its state is lost, and acting on the plant from a half-rebuilt group is worse than holding it at `safe`. Recovery is a new generation, a rollback or a restart.
7. **The supervisor's response narrows.**
   - `rt/supervisor` sets the `force_safe` bit of the outputs of the stale groups only.
   - The `supervisor_force_safe` record (ADR 0061 decision 6) says "forced the outputs of the stale groups to `safe`"; its fields do not change.
   - ADR 0086's alarm narrows with it, as that ADR anticipated.
   - ADR 0061 decision 10 does not change: once any group was forced, the watchdog feed stops, and a hardware watchdog restarts the board. Until then, or without a hardware watchdog, the other groups keep running.

## Consequences

- Document 01 §3.4 says how an alarm trips an output of another group. The thread table of §2 already describes this model.
- A hang or a panic in one group no longer stops the others or forces their outputs.
- Each group keeps its own copy of the derived-signal and alarm state arenas. That is a few kilobytes per group; it uses the room ADR 0087's RSS figures leave.
- The decision log interleaves the records of different groups in push order; each record carries its own tick and instant. No order across groups was ever promised (docs/02 §6.3).
