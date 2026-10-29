# ADR 0091 — `jk why`, and the app lifecycle on the node

**Status:** accepted (v1) · **Date:** 2026-10

## Context

AUR-259 builds the node operation commands of ref/09 §9.1. Two of them meet gaps in the documents:

- **`jk why <output>`.** Document 26 §7 puts F2 in 1.0: why the output has this value, on one screen (winning level, lease, clamps, interlocks, latest decisions and the inputs read). Neither document 07 nor ref/09 §9.1 has a row for it, so it maps to no endpoint.
- **`jk app start|stop|restart <app>`.** These map to `POST /v1/apps/{app}/start|stop|restart` (ref/09 §5.14). `jokerd` 1.0 keeps no app state of its own: no crashloop, no backoff, no shadow. Its apps can also be native tasks compiled into the daemon (docs/01 §4), which have no version and no digest, yet the OpenAPI `App` requires both.

## Decision

### `jk why <output>`

1. `jk why <output>` composes three existing reads and adds no node API:
   - `GET /v1/outputs/{path}`: owning group, active level, occupied levels with their writer, value and lease, applied value, envelope and last result.
   - `GET /v1/decisions?output=<path>&limit=5`: the latest decisions, newest first.
   - `GET /v1/values` of the signals the output's envelope reads: its `requires_good` list, its permissive and the inputs of its expressions. `GET /v1/outputs/{path}` serves the declared `envelope` of `OutputState` with one more field, `reads`, which lists them.
2. It prints them on one screen, in that order. A clamp or a rejection shows as the last result, with its reason. An interlock shows as the permissive and `requires_good` signals with their quality. `--json` prints one object, `{output, decisions, inputs}`.
3. Document 07 gains the row, and ref/09 §9.1 the mapping.

### The app lifecycle in `jokerd` 1.0

4. **Stop.** `POST /v1/apps/{app}/stop` (a `reason` is required) stops the app's cycle tasks:
   - they skip their ticks from the next one on;
   - the app's outputs follow their leases, which run out because nothing renews them;
   - the stop is volatile: it ends with the next generation or the next start of `jokerd`.
5. **Start.** `POST /v1/apps/{app}/start` lets the tasks of a stopped app run again from their next tick, with the state their instances had.
6. **Restart.** `POST /v1/apps/{app}/restart` builds fresh instances of the app's cycle tasks outside the cycle. Each owner group swaps them in between two of its ticks and prepares them like the instances of a first tick.
   - The old instances are handed back and dropped outside the cycle.
   - An instance that does not prepare is dropped, and the old one keeps running (`task_errors` counts it).
   - A restart does not start a stopped app.
7. **What `GET /v1/apps` reports.** The state is `running` or `stopped`: `jokerd` 1.0 reaches no other state of `app_state`. `version` and `digest` are given when the node knows them (`digest` from the app's artifact in the generation) and left out otherwise. The OpenAPI `App` requires `name` and `state` only.
8. **Decision log.** Each lifecycle command is an `app` record, immediate (`fsync`), with:
   - `gen`, `ts`;
   - `app`;
   - `op` (`start`, `stop`, `restart`);
   - `principal`;
   - `reason` when one was given.

### `jk trace` and the `signal_trace` topic

9. `signal_trace` (ref/09 §6) is built from the continuous trace ring: one record per tick of a group that read, proposed or committed the signal and changed something about it. Each record holds:
   - the sample the group latched;
   - the proposals and their writers;
   - the envelope's result and the applied value.

   It is delivered as an ordered stream of records, like the event topics: a client that falls behind loses the oldest, and the loss is a counted `gap` (§6.6 point 3). Sampling would skip the very ticks a trace is read for. `jk trace <signal>` prints the signal's lineage (`GET /v1/signals/{path}/lineage`), then follows the topic.

## Consequences

- `jk app start|stop|restart <app>` print the `App` the endpoint returns, and `jk app logs <app>` is `jk logs --app <app>`.
- A crashloop and its backoff, when they come, add their states to the same object. Nothing in this ADR has to change for that.
- ref/09 §5.14 states the stop and restart semantics, and document 07 and ref/09 §9.1 gain `jk why`.
