# ADR 0040 — Alarm lifecycle: delays, `Unknown`, suppression and shelving

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 02, §11, gives the alarm states (`Normal`, `Active·Unacked`, `Active·Acked`, `RTN·Unacked`) and their transitions; document 16, §3.2, adds `Unknown` when the condition is `Bad`; ref/01 §5.15 adds `delay_on`, `delay_off`, `suppress_when`, `max_shelve` and `actions.trip`. Document 16, §10, left the transitions **from** `Unknown` to an ADR in task 12. Implementing the state machine (document 13, task 12) also needs:

- how the delays are measured;
- what an acknowledgement does while the alarm is `Unknown`;
- how suppression and shelving interact with activation and with trip actions;
- what `alarm.<a>.active` holds while the alarm is `Unknown`.

## Decision

1. **Evaluation.** The condition (`when`) and `suppress_when` are evaluated in step 3 of every tick of the alarm's group (document 01, §4), before the arbiter and the envelope. A trip action therefore takes effect in the same tick.
2. **Delays.** `delay_on` counts from the first tick in which the condition is `true` (with `Good` or `Uncertain` quality) and has stayed so. The alarm activates in the first tick in which that time is at least `delay_on`, so `delay_on = 0` activates in the same tick. `delay_off` is measured the same way, for `false`. A tick in the other value restarts the count.
3. **`Unknown` is an overlay.** When the condition is `Bad`, the published state is `Unknown`, and the alarm keeps its **underlying** state (the one it had before) and its pending acknowledgement. The delay timers stop. While `Unknown`:
   - an acknowledgement applies to the underlying state;
   - the condition makes no transitions.

   When the condition is no longer `Bad`, the published state is the underlying one again. The normal transitions resume from there, with both delay timers restarted at that tick. This is the decision that document 16, §10, assigned to this task.
4. **`Uncertain`** evaluates normally, and the events carry the `uncertain` flag (document 16, §3.2).
5. **Suppression** (`suppress_when` `true`, with `Good` or `Uncertain` quality) blocks the transitions **into** `Active·Unacked`, from `Normal` and from `RTN·Unacked`, and the trip actions that go with them. It is a design decision of the manifest. An alarm that is already active follows its lifecycle. A `Bad` `suppress_when` does not suppress, so an unusable suppression condition fails toward annunciating.
6. **Shelving** (`jk alarm shelve`, TTL at most `max_shelve`) is an operator decision about annunciation only. The state machine runs as usual, and **trip actions still happen**: shelving never disables a protection. The events carry the `shelved` flag. The shelve ends at its TTL or with an unshelve.
7. **Trip actions** trip their outputs when the alarm enters `Active·Unacked` (after `delay_on`), and again in every tick in which the alarm stays active (also when it is `Unknown` with an active underlying state). An output already in trip is left as it is, so a `jk reset` while the alarm is still active ends the trip for that tick only, and the next tick trips it again (ref/01 §5.15). The decision log records each new trip with cause `trip`.
8. **Signals.** `alarm.<a>.state` carries the published state code (ref/00 §6) with `Good` quality. `alarm.<a>.active` is `true` in both `Active·*` states. While `Unknown`, it carries the underlying value with `Bad` quality and the condition's reason. A permissive that reads it, such as `!alarm.x.active`, therefore fails toward safe. Both are written by the alarm's group, only when they change.

## Consequences

- A sensor that drops out does not acknowledge, clear or re-raise an alarm by itself. The operator sees `Unknown`, and the history keeps the state it came back to.
- Protections cannot be shelved away; only the manifest can suppress them, and that is reviewed with the configuration.
- Document 16, §10, now points to this ADR for the transitions from `Unknown`.
