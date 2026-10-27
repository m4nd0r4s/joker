# ADR 0086 — The hung-group alarm (`joker.group.<g>.stalled`)

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01's failure table answers a hung group with the supervisor's `force_safe` and a **critical alarm**. ref/01 §5.15, the normative list of runtime alarms, and ADR 0085, which raises them, name no such alarm. On the bench Pi (M0 friction report, F-8), `jokerd` was frozen for 20 s: every output went to its `safe` through the supervisor, and the only alarm was the device's `offline`. An operator had nothing in `GET /v1/alarms` or `jk status` to say that control had stopped.

Three facts constrain the answer:

- The supervisor (ADR 0061, AUR-274) decides that a group is stale when it has gone more than three periods without a tick, and then sets the `force_safe` bit of the outputs. Today one `rt/cycle` thread runs every group, so a real hang makes every group stale together, and the response forces every output of every group.
- The `force_safe` bit is sticky: nothing clears it within a generation. If the thread resumes (a `SIGSTOP` and `SIGCONT`, or a long page fault), the group ticks again and its outputs **stay** at `safe` until the next generation or a restart. With a hardware watchdog, the supervisor also stops feeding it and the board restarts.
- The runtime alarms are evaluated by `svc/watch`, a thread of their own (ADR 0085), so they still run while the cycle thread is hung.

## Decision

1. **The alarm.** `joker.group.<g>.stalled`, severity `critical`, one per cycle group, raised by `svc/watch` with the lifecycle of ADR 0085 (no delay, ack, shelve up to 24 h, journal and decision log).
2. **Condition.** It holds while **either**:
   - the group has gone more than three of its periods without a tick, counted from the start of the cycle loop if it has not ticked since then (the supervisor's own test); **or**
   - the supervisor's `force_safe` holds at least one of the group's outputs.

   The second clause keeps the alarm active while outputs stay forced after the thread resumes, so the alarm is never `normal` while the plant is held at `safe` behind the operator's back. It ends with the generation that clears the bits (a new generation, a rollback, or a restart).
3. **Message.** It says which clause holds: how long the group has been silent, or how many of its outputs the supervisor holds at `safe`.
4. **Across a restart.** The alarm book persists (ADR 0082). After a restart by the watchdog, the next generation evaluates the condition as false and the alarm reads `rtn_unacked` until someone acknowledges it: the hang stays visible after the board comes back.
5. **No new key, no code.** The threshold is the supervisor's (three periods) and is not configurable in 1.0. No `J` code is added.

## Consequences

- ref/01 §5.15 gains the row, and document 01's failure table names the alarm.
- With one cycle thread, a hang raises the alarm of **every** group. Once groups have their own threads (AUR-275), the first clause narrows to the stale group. The second clause follows what the supervisor forces, so it narrows with it.
- A group with no outputs can only raise the alarm through the first clause.
