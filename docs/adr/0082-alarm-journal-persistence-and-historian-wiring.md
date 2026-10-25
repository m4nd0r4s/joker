# ADR 0082 — The alarm journal, alarms across restarts, and the historian in jokerd

**Status:** accepted (v1) · **Date:** 2026-10

## Context

The core runs the alarm lifecycle of ADR 0040, but in `jokerd` its events reached the commit sink's empty default: nothing recorded them, the API had no alarm endpoint, and every restart or generation switch returned an unacknowledged or shelved alarm to `Normal` (AUR-288). ADR 0081 built the historian; `jokerd` did not record anything into it. Document 02, §11, says alarm transitions are "recorded and sent with the `alarms` priority", ref/09 §5.5 and §5.8 define the endpoints, and docs/01 §7 asks that what an operator must still act on survives a restart. What they leave open:

1. what of an alarm survives a restart, and how a restored alarm re-enters the core;
2. how an operator command reaches the state machine the cycle thread owns, and what the API answers;
3. where the journal behind `GET /v1/alarm-events` lives, and what its `seq` is;
4. how `[telemetry.record]` becomes samples, in particular `on_change` ("one sample per new `seq`");
5. which parts of the alarm and historian surface are not built in 1.0.

## Decision

### Alarms across restarts

1. What survives is the **underlying** state (ADR 0040 §3) with its pending acknowledgement, the end of a shelve (UTC), who acknowledged and when, and when the state last changed. The `Unknown` overlay and the delay timers do not survive: the first evaluation derives them again.
2. The alarm book is persisted to `state/alarms` (atomic write with `fsync`, like `state/trips`) after every batch of events. A failed write is recorded once in the decision log (`state_write_failed`, ADR 0070) and retried.
3. At the start of every generation, after a restart and on every switch, each alarm the generation declares takes back its state with `Alarms::restore` before the first tick, without an event; a shelve whose end has passed is dropped; an alarm the generation no longer declares leaves the book. An active restored alarm trips its outputs on the first tick, as ADR 0040 §7 says for every tick it stays active.

### Operator commands

4. `ack`, `shelve` and `unshelve` go to the cycle thread through a bounded queue and run at the start of the next tick of any group; the reply comes back through another. Who asked and why travel as an annotation the alarm pump attaches to the event the command causes. The API answers after the book has applied that event, so the `Alarm` it returns is the state after the command.
5. They need the role `operator` (`J-E1002`). `ack` in `active_acked` is idempotent; in `normal` and `unknown` it is `J-E1204`, also while an underlying acknowledgement is pending under `Unknown` (ref/09 §5.8 wins over the core's leniency). A shelve without a TTL, or with a zero TTL, is `J-E1201`; above `max_shelve`, `J-E1202`; without a reason, `J-E1013`. An unshelve of an alarm that is not shelved is `J-E1203`. A full command queue or a tick that does not come within 5 s is `J-E1014`.

### The journal

6. The cycle thread reports each alarm event into a bounded queue (drops counted). The alarm pump of the generation (`svc/alarms`) applies it to the book and writes an `AlarmEvent` (ref/09 §5.8) to the historian's event series `journal:alarms`, class `alarm`, kept 400 days, appending it to the store itself, so it is synced at once like every alarm record (ADR 0081 rule 4); only then do the book's waiters and the API see the event, so an answer never precedes its journal entry. `message` is interpolated when the event happens.
7. The `seq` of an `AlarmEvent` is the journal's store sequence number. `GET /v1/alarm-events` pages by it; the WebSocket topic `alarms` orders by it, starts with a `resync` holding the active alarms, and follows a `gap` with another.

### The historian in `jokerd`

8. One store per daemon, under `<state>/store`; a generation's `[telemetry.store]` sets its `fsync`, `fsync_interval` and `max_disk` (a quarter of the partition when absent, measured with `statvfs` on Linux, 8 GiB where it cannot be measured).
9. Each generation's recorder (`svc/record`) resolves `[telemetry.record]` and the `record` shorthand of `[signals]` and `[devices.<d>.signals]` with the specificity rule of ref/01 §5.17.1, and reads the slots every 10 ms: `every` takes the last value at its period; `on_change` takes each new `seq` it sees, so changes faster than 10 ms are coalesced to the last one. A sample's `ts` is its acquisition instant in UTC (ADR 0081 rule 18).
10. `GET /v1/history` with `tier=auto` takes the coarsest tier with a resolution at most `step` whose coverage starts at or before `from`; when none does, the finest tier with data, a `warning` and the `coverage` of each series, which carries its `path`. `fill` fills every missing interval between `from` and `to`. The cursor is opaque. `text/csv` and `application/x-ndjson` are rendered by the server from the JSON response.

### Not built in 1.0 (reserved)

11. Recording `string`, `bytes` and `record` signals, signals with shape `event` (so `GET /v1/events` answers an empty list to a valid query), and blocks (1.1).
12. Reporting suppression: the core does not publish whether `suppress_when` holds, so `Alarm.suppressed` is omitted, `state=suppressed` lists nothing, and the journal has no `suppress` or `unsuppress` events.
13. `jk export --parquet` and `jk trace-from-history`.

## Consequences

- An operator sees after a restart exactly the acknowledgements and shelves that were pending; a sensor that came back during the outage still needs the acknowledgement of the activation it caused.
- The alarm journal shares the historian's durability and retention; an alarm record is never dropped for telemetry pressure, and its own drops are counted.
- `on_change` is exact for signals that change at most every 10 ms, which covers the acquisition periods of 1.0 except the fastest cycle groups.
