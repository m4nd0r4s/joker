# ADR 0090 — The runtime's own signals on Linux (`joker.*`)

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01 §19 says the runtime's metrics "are `joker.*` signals (therefore recorded, alarmable and sendable like any other signal)", and ref/09 §5.2 says they are read with `/v1/values?sel=joker.**`. ref/07 §8 lists the minimum set an MCU publishes; nothing lists the paths, types or writer of these signals on Linux. `jokerd` registers none, so `jk read 'joker.**'` returns nothing, an alarm cannot watch the board's temperature, and `[telemetry.record]` cannot keep the overrun count.

Three other gaps of AUR-271 sit next to it:

- **`arbiter.leases_expiring`** in `Metrics` (ref/09 §5.2, ADR 0071) has no definition of "about to expire".
- **Internal drops.** AGENTS.md invariant 7 wants every drop counted in a metric, but the drops of the daemon's internal queues (decisions, write results, trace chunks, alarm events, trip requests, logs) are counters that no endpoint shows.
- **`GET /v1/groups`.** The endpoint is normative in ref/09 §5.2, with the measured WCET per task, but it is not served, and nothing measures a task.

## Decision

### The signals

1. **Registry.** The runtime signals are registered with the manifest's paths, in the same registry, before `[calc]` and `[alarms]` are checked. They are `input`s with no device: an expression, a telemetry selector or an app port reads them like any input. ref/01 §4 already reserves the first segment `joker`, so no manifest name can collide with them (`J0141`).
2. **Writer.** One service thread, `svc/metrics`, writes every runtime signal once per second, with the write instant as `ts_acq` and quality `good`. A value the node cannot read (no thermal zone, a group the plan does not run) is `bad(not_connected)`. The cadence is fixed, and `max_age` is 3 s.
3. **The list.** Counters are `int` and never decrease within one run of `jokerd`.

   | Path | Type | Unit | Value |
   |---|---|---|---|
   | `joker.group.<g>.ticks`, `.overruns` | int | — | per group of `[cycles]` and the implicit `default` and `calc` |
   | `joker.group.<g>.jitter_p99`, `.jitter_max`, `.duration_p99`, `.duration_max` | float | s | from the group's histograms (ref/09 `Percentiles`) |
   | `joker.device.<d>.link` | enum `LinkState` (ref/00 §6 `link_state`) | — | per device |
   | `joker.device.<d>.reconnects`, `.dropped_samples`, `.invalid_samples` | int | — | per device (ADR 0063); the drivers do not count timeouts yet, so there is no `.timeouts` until they do |
   | `joker.output.<s>.accepted`, `.clamped`, `.rejected`, `.suppressed`, `.tripped` | int | — | per output: the envelope results of `Metrics` |
   | `joker.arbiter.manual_active`, `joker.arbiter.leases_expiring` | int | — | node-wide |
   | `joker.system.temp` | float | °C | thermal zone 0 |
   | `joker.system.throttled` | bool | — | the firmware's throttling flag, where the board exposes it |
   | `joker.system.memory_used_bytes`, `joker.system.disk_free_bytes` | int | — | `/proc/meminfo`; free space of the state directory |
   | `joker.node.boot_count` | int | — | `state/boot_count` (ADR 0070) |
   | `joker.node.uptime` | float | s | since `jokerd` started |
   | `joker.runtime.dropped` | int | — | the sum of `Metrics.runtime.dropped` (point 5) |

   Byte counts are dimensionless integers named `_bytes`, because the unit catalogue of docs/16 §5 has no byte.
4. **Not recorded by default.** A runtime signal is recorded, sent upstream or alarmed only when the manifest says so, like any other signal.

### `leases_expiring`, internal drops and `GET /v1/groups`

5. **`Metrics.runtime.dropped`.** An object with one counter per internal queue of the daemon: `decisions`, `write_results`, `trace`, `alarm_events`, `method_results`, `reset_results`, `trip_requests`, `confirmations`, `write_acks`, `logs`. The Prometheus exposition (ADR 0071) gains `joker_runtime_dropped_total` with a label `queue`.
6. **`leases_expiring`** is the number of outputs whose active level is not manual and whose last commit received no proposal: the level holds on its lease alone, and nothing renews it.
   - A cycle writer renews its lease on every tick, so a healthy one is never counted.
   - A writer that stopped is counted from its first missed tick until its lease ends.
   - The count reads the output status the cycle already publishes, so it adds nothing to the tick.
7. **WCET per task.** The cycle reads the clock before and after each task of a tick and records the difference in a per-task allocation-free histogram, the kind the group jitter uses. `GET /v1/groups` and `GET /v1/groups/{group}` serve `Group` as ref/09 defines it:
   - `tasks[].wcet_measured_p999` comes from that histogram;
   - `wcet_declared` comes from the manifest;
   - `stats` comes from the group's counters.

   A field the node does not know is left out (ADR 0071 point 5).

## Consequences

- `jk read 'joker.**'` and `/v1/values?sel=joker.**` read the runtime's signals. An alarm such as `when = "joker.system.temp > 80 °C"` and a `[telemetry.record]` of `joker.group.*.overruns` work with no new key.
- ref/09 §5.2 and the OpenAPI `Metrics` gain `runtime.dropped`. ADR 0071 gains its family, and the Grafana dashboards are built on those names.
- The registry hash of a manifest now covers the runtime signals, so the same manifest gives a different hash on a runtime with a different list. That hash was always computed by the runtime from the plan; generations are stored by their manifest hash, which does not change.
- Each task costs two clock reads per tick, and the allocation guard still holds.
