# ADR 0069 — How `jokerd` derives and re-evaluates the clock quality

**Status:** accepted (v1) · **Date:** 2026-10

## Context

M14 (docs/01 §18, ref/01 §5.5) says that `jokerd` does not synchronise the clock: it reads the state of the system's time daemon and derives the quality, `synced`, `holdover` or `unsynced`. ref/07 §8.4 fixes the derivation for a Joker Micro (how long since the last `TIME`, `max_holdover`), but nothing fixes it for a Linux node, nor how often it is evaluated. The first implementation read it once, at startup, from the presence of `/run/systemd/timesync/synchronized`: a node that lost its time source kept `synced` until it restarted, `holdover` never happened, and the device hosts stamped every instant `synced` whatever the clock said.

## Decision

1. **Source.** In 1.0 the source is `systemd-timesyncd`: the modification time of `/run/systemd/timesync/synchronized`, which the daemon touches on every accepted sample and which lives on a tmpfs (absent until the first synchronisation of each boot). That time is the **last synchronisation**. With another time daemon (chrony, linuxptp) there is no last synchronisation and the quality is `unsynced`, the conservative reading; reading those daemons is a later addition that only changes the source.
2. **Derivation**, at instant `now` (wall clock), with `age = now − last synchronisation`:

   | Condition | Quality |
   |---|---|
   | `[clock] sync = "none"`, or no synchronisation since boot | `unsynced` |
   | `age ≤ 4096 s` (twice `systemd-timesyncd`'s longest poll interval, 2048 s) | `synced` |
   | `age ≤ max_holdover` (ref/01 §5.5, default 24 h) | `holdover`, since the last synchronisation |
   | otherwise | `unsynced` |

   A last synchronisation in the future (the wall clock stepped back) counts as `age = 0`.
3. **Re-evaluation.** The quality is evaluated when a generation starts (with that generation's `[clock]`) and then **once a second** by the daemon's service loop, never by the cycle thread. The result is held in one atomic cell; every clock of the node reads it when it stamps an instant: the cycle pacer (so envelope step 0, docs/16 E1, sees each change on the next tick), the device hosts, the API (`Clock`, the `Joker-Clock` header) and the traces.
4. **Record.** Each change of the quality is written to the decision log, immediately (`fsync`), as a record `clock` with `gen` (null with no generation running), `from`, `quality`, `last_sync` (null when there is none) and `ts`. Like the records of ADR 0061, it is read from the log file; `GET /v1/decisions` keeps serving `output` and `write`.
5. **API.** `Clock.last_sync` is the last synchronisation and, with `holdover`, `Clock.holdover_since` is the same instant (ref/07 §8.4: holdover counts from the last `synced`).

## Consequences

- ref/01 §5.5 states the derivation and refers here.
- An app with `requires_clock = "synced"` has its writes rejected (`Rejected(ClockUnsynced)`) from the first tick after the time source is lost for more than 4096 s, and accepted again from the first tick after a re-evaluation finds it `synced`, without a generation switch.
- The manual levels of A14 keep using the quality evaluated at startup, before the first generation, with the default `[clock]`.
