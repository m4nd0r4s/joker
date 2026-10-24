# ADR 0061 — Runtime state files, supervisor and restore records, and per-output errors of a bulk reset

**Status:** accepted (v1) · **Date:** 2026-10

## Context

AUR-274 built `rt/supervisor`, persistent trips and the A14 persistence of manual levels, and a review of it found gaps that the specification does not settle on its own:

1. **Where the state files live.** ref/02 §2.2.4 lists `state/trips` and says that everything `jokerd` writes goes through `tmp/` and `rename(2)`. It does not list a file for the manual levels of A14, nor the startup counter of docs/01 §20, and the implementation wrote all three at the root of the state directory, next to a `.tmp` sibling.
2. **What a broken state file means.** A `state/trips` that cannot be parsed was loaded as "no trips": an output in trip silently came back at its normal value after a restart, which is exactly what docs/01 §7 forbids.
3. **What the decision log records outside the arbiter.** ADR 0033 fixes the `output` and `write` records and ADR 0058 the `action` record. The supervisor forcing `safe`, a manual level restored without a `synced` clock (A14, `manual_restored`) and a trip reset (ref/09 §5.7, "the reset is also recorded with `fsync`") have no record, and the M9 alarm path that would carry `manual_restored` does not exist yet.
4. **What a bulk reset reports when one output fails.** `ResetResult.result` has no value for "this output could not be reset" (a full reset queue, a missing group), so the implementation reported such an output as `not_tripped`, hiding the failure.

## Decision

### State files

1. The runtime state of `jokerd` lives under `state/` in the state directory (ref/02 §2.2.4): `state/trips` (docs/01 §7), `state/manual` (docs/16 A14) and `state/boot_count` (docs/01 §20, a decimal integer). Each is a JSON document except `boot_count`.
2. Every write goes to a file in `tmp/` (same file system), is `fsync`ed, is renamed over the target, and the directory of the target is `fsync`ed.
3. A state file that exists but cannot be read or parsed is a startup error of `jokerd`: the daemon does not start, and the message names the file. A missing file is the first boot and loads as empty. A file left at the root of the state directory by an earlier build (`trips`, `manual`) is read when the `state/` one does not exist yet, and the next write moves it under `state/`.
4. `state/trips` is written on each **transition** of an output into or out of trip, observed by the services of the running generation, and never on a read of the API. A persisted trip is restored **before the first tick** of every generation (a trip ends only by a reset, docs/16 E7, not by a generation switch); a persisted manual level is restored before the first tick of the first generation that the daemon starts (a later generation still starts without manual levels).
5. Restoring has no fixed capacity: trips are applied by the cycle thread before its loop starts, and the manual levels of a group are staged in its inbox before the cycle thread starts. If a group would receive more manual levels than its inbox holds, the generation does not start, and the decision log says why (`restore_failed`, below).

### Decision-log records

6. Four record kinds join `output`, `write` (ADR 0033) and `action` (ADR 0058), all **immediate** (`fsync`), all with `gen` and `ts`:
   - `supervisor_force_safe`: `rt/supervisor` found groups that had not ticked for more than three periods and forced every output to `safe` (docs/01 §8). Fields: `groups` (names), `silent_ns` (the longest silence), `at_mono_ns`. It stands for the critical alarm until M9 exists.
   - `manual_restored`: a manual level was restored at startup without a `synced` clock (docs/16 A14). Fields: `output`, `level` (`manual:<n>`), `principal`, `remaining_ns`. It stands for the alarm `joker.output.<s>.manual_restored` until M9 exists. A level restored with a `synced` clock has `synced: true` and the same fields; a level whose deadline had passed is recorded as `manual_expired` with `output`, `level` and `principal` (A14's `kind = expiry`).
   - `reset`: a trip reset requested through the API (ref/09 §5.7). Fields: `output`, `principal`, `reason`, `result` (the `ResetResult.result`), and `error` (`code`, `detail`) when it failed.
   - `restore_failed`: a generation could not restore the persisted state before its first tick. Fields: `what` (`manual`), `group`, `count`, `capacity`.
7. `GET /v1/decisions` keeps serving the `output` and `write` records only; these records are read from the log file (`jk decisions` will show them once ref/09's `Decision` gains a kind for them).

### Supervisor and watchdog

8. `rt/supervisor` reads the monotonic clock of the pacer of its generation, the one the cycle thread stamps its ticks with (one origin for every generation of a daemon). A group's silence counts from its last tick or, if it has not ticked since the cycle loop started, from that start: a group that never ticks is stale once its deadline passes. A generation run on a virtual clock (tests, replay) has no supervisor.
9. A supervisor that cannot be started is an error of the activation, not a warning.
10. The hardware and systemd watchdog feed (docs/01 §8) feeds only while the supervisor of the running generation has beaten within the last 500 ms; it stops for good once a supervisor has forced `safe`. With no generation running there is nothing to supervise and it feeds. `READY=1` is sent once the API socket is bound.

### Bulk reset

11. `ResetResult.result` gains `failed`, and `ResetResult` gains `error` (`code`, `detail`, the same pair as a problem, ref/09 §3) present only with `failed`. In a bulk reset (`POST /v1/trips/reset`), an output that could not be reset is reported as `failed` with its error, and the others are still reset; the response is 200. A single reset (`POST /v1/outputs/{path}/reset`) keeps answering a failure with the problem itself.

## Consequences

- ref/02 §2.2.4 lists `state/manual` and `state/boot_count`.
- ref/09 §5.7 and `spec/openapi/joker-api.yaml` (`ResetResult`) describe `failed` and `error`.
- A corrupt `state/trips` or `state/manual` keeps the node from starting until an operator removes or repairs it: a node that cannot know whether an output is in trip does not guess.
- The alarm path of M9 (AUR-288) replaces the stand-in records of decision 6 with real alarms; the records stay, since the decision log is the audit.
