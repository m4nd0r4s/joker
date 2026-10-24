# ADR 0070 — State write failures, trips of outputs a generation removed, the startup counter in the API, and `jokerd`'s bench flags

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0061 settled where the runtime state files live and what a corrupt one means at startup. A review of the result found four questions it leaves open:

1. **A state file that cannot be written while running.** docs/16 A14 says each accepted manual write is recorded with `fsync`, and docs/01 §7 says the same of trips. When that write fails (a full or read-only disk), the write already took effect in the arbiter; the failure only reached standard error, so nothing an operator audits showed that the level or the trip would not survive a restart.
2. **A persisted trip of an output the new generation does not declare.** ADR 0061 restores trips before the first tick of every generation, but a generation that removed the output cannot restore it, and the API could neither show nor reset it: the trip stayed in `state/trips` forever and came back, unannounced, in any later generation that declared the output again.
3. **The startup counter.** docs/01 §20 lists the startup counter as where a dead `jokerd` shows, and ref/02 §2.2.4 keeps it in `state/boot_count`, but no API exposes it; the implementation added `runtime.boot_count` to `GET /v1/info` outside the contract.
4. **Flags outside ref/02 §2.2.3.** `jokerd` accepts `--manifest`, `--socket` and `--board`, which the flag table does not list. `--board` exists because the board a node runs on is not yet resolved from the generation's lock, and the hardware-watchdog feed (docs/01 §8, ref/03 §2.3.2) needs its `[watchdog]`.

## Decision

### State write failures

1. A failure to write `state/manual` or `state/trips` while running is recorded in the decision log, immediately (`fsync`), as a record `state_write_failed` with `gen`, `file` (`state/manual` or `state/trips`), `output` (null when the whole table was being rewritten), `op` (`set`, `remove` or `clear` for manual levels; `trip` or `reset` for trips), `error` (the system's message) and `ts`. It is also reported on standard error.
2. The write that caused it keeps its effect: the level or the trip holds in the running generation, and the request that set it is answered as usual. A trip transition that failed to persist is retried on each later observation, and recorded once per transition, not once per retry.

### Trips of outputs a generation does not declare

3. A persisted trip of an output that the starting generation does not declare stays in `state/trips` (a trip ends only by a reset, docs/16 E7) and is not restored into that generation. Each such trip is recorded when the generation starts, immediately, as a record `trip_kept` with `gen`, `output`, `since` and `ts`. If a later generation declares the output again, the trip is restored into it.
4. `GET /v1/trips` lists those trips too, with `declared: false` (`Trip.declared`, absent or `true` for the outputs of the running generation).
5. `POST /v1/outputs/{path}/reset` and `POST /v1/trips/reset` reach those trips: the reset removes the trip from `state/trips` without going through a group (there is none), answers `cleared`, and is recorded like any reset (`reset`, ADR 0061). A path that is neither an output of the running generation nor a persisted trip is still `J-E1100`.

### The startup counter

6. `Info.runtime.boot_count` is the value of `state/boot_count` after this start's increment: 1 on the first start of a state directory. It is not a signal: 1.0 has no runtime signal namespace for node counters.

### `jokerd`'s bench flags

7. ref/02 §2.2.3 lists three more flags, without variables:
   - `--manifest <path>`: with no confirmed generation to boot, proposes and activates that manifest (and its lock, the same name with `.lock`).
   - `--socket <path>`: the path of the API's Unix socket, in place of `<paths.run>/api.sock`.
   - `--board <path>`: the board profile (ref/03 §2.3.2) whose `[watchdog]` the daemon feeds. A file that cannot be read or has no `[watchdog]` is a warning, and only systemd's watchdog is fed. It is replaced by the board of the generation's lock once `jokerd` resolves it, and then removed.

## Consequences

- ref/09 §5.1 (`Info`) and §5.7, ref/02 §2.2.3 and `spec/openapi/joker-api.yaml` (`Info.runtime.boot_count`, `Trip.declared`) change with this ADR.
- An operator can find and clear a trip left by an output that a new generation removed, instead of meeting it again when the output returns.
- `GET /v1/decisions` keeps serving `output` and `write` records only (ADR 0061); `state_write_failed` and `trip_kept` are read from the log file.
