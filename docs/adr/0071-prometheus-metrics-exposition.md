# ADR 0071 — The Prometheus `/metrics` exposition

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 26 §7 puts F3 in 1.0: "`/metrics` in Prometheus format", because it is what operations teams already scrape. ref/09 §5.2 only defines `GET /v1/metrics`, a JSON snapshot (`Metrics`), and nothing says on which listener the Prometheus text is served, which role it needs, or how the fields of `Metrics` become Prometheus metric names and labels. AUR-271 left that open on purpose until an ADR fixed it.

Three constraints shape the answer:

1. ref/09 §2.1: there is no plaintext TCP port and no TLS without a client certificate. Prometheus scrapes HTTP and HTTPS, and it can present a client certificate (`tls_config.cert_file`).
2. AGENTS.md invariants 6 and 7: nothing may reach the cycle domain, and every loss must be visible.
3. `Metrics` is already the normative model of the runtime's own metrics (document 01 §19). A second model would drift from it.

## Decision

### Endpoint

1. `GET /metrics`, outside `/v1`: it is the default `metrics_path` of Prometheus, and the exposition format carries its own version. It is served by the API router, so on every API listener: the Unix socket and, when the node has one, the mTLS TCP listener of ref/09 §2.1, which Prometheus scrapes with a client certificate from the client CA. There is no other listener and no plaintext port.
2. Minimum role `viewer`, the role of `GET /v1/metrics`.
3. The body is the Prometheus text exposition format, version 0.0.4, with `Content-Type: text/plain; version=0.0.4; charset=utf-8`. `Accept` is not negotiated in 1.0 (no OpenMetrics, no protobuf).

### Source

4. A scrape reads the same snapshot as `GET /v1/metrics` (one `Metrics`, with its `api` object) and renders it in the API layer, into one buffer. A scrape is a read of the counters the daemon already keeps; it adds nothing to the tick path and never waits on it.
5. A field that `Metrics` does not carry is not exposed. No metric is fabricated as zero; a family without samples is left out, with its `# HELP` and `# TYPE`.

### Names and labels

6. A metric name is `joker_<area>_<name>`, with the unit suffix of Prometheus last and `_total` on counters. The entity of a section becomes a label: `group`, `device`, `output`, `app`. Durations are in seconds, sizes in bytes, temperatures in degrees Celsius. Label values are escaped as the format requires (`\\`, `\"`, `\n`).
7. The families, in this order:

| `Metrics` field | Family | Type | Labels |
|---|---|---|---|
| `groups[].ticks` | `joker_cycle_ticks_total` | counter | `group` |
| `groups[].overruns` | `joker_cycle_overruns_total` | counter | `group` |
| `groups[].consecutive_overruns` | `joker_cycle_consecutive_overruns` | gauge | `group` |
| `groups[].jitter` | `joker_cycle_jitter_seconds`, `joker_cycle_jitter_max_seconds` | summary, gauge | `group` |
| `groups[].duration` | `joker_cycle_duration_seconds`, `joker_cycle_duration_max_seconds` | summary, gauge | `group` |
| `drivers[].link` | `joker_driver_link_state` | gauge | `device`, `state` |
| `drivers[].latency` | `joker_driver_latency_seconds`, `joker_driver_latency_max_seconds` | summary, gauge | `device` |
| `drivers[].timeouts` | `joker_driver_timeouts_total` | counter | `device` |
| `drivers[].reconnects` | `joker_driver_reconnects_total` | counter | `device` |
| `drivers[].dropped_samples` | `joker_driver_dropped_samples_total` | counter | `device` |
| `drivers[].invalid_samples` | `joker_driver_invalid_samples_total` | counter | `device` |
| `drivers[].bytes_rx`, `bytes_tx` | `joker_driver_received_bytes_total`, `joker_driver_sent_bytes_total` | counter | `device` |
| `drivers[].coalescing` | `joker_driver_coalescing_ratio` | gauge | `device` |
| `envelope[].accepted` … `tripped` | `joker_envelope_results_total` | counter | `output`, `result` |
| `arbiter.manual_active` | `joker_arbiter_manual_active` | gauge | — |
| `arbiter.leases_expiring` | `joker_arbiter_leases_expiring` | gauge | — |
| `apps[].traps`, `restarts` | `joker_app_traps_total`, `joker_app_restarts_total` | counter | `app` |
| `apps[].memory` | `joker_app_memory_bytes` | gauge | `app` |
| `apps[].cpu_async` | `joker_app_cpu_async_seconds_total` | counter | `app` |
| `store.queue`, `dropped`, `disk_used` | `joker_store_queue`, `joker_store_dropped_total`, `joker_store_disk_used_bytes` | gauge, counter, gauge | — |
| `api.connections`, `subscriptions` | `joker_api_connections`, `joker_api_subscriptions` | gauge | — |
| `api.sampled_per_s` | `joker_api_sampled_per_second` | gauge | — |
| `api.dropped`, `rate_limited` | `joker_api_dropped_total`, `joker_api_rate_limited_total` | counter | — |
| `system.temp` | `joker_system_temperature_celsius` | gauge | — |
| `system.throttled` | `joker_system_throttled` | gauge (0 or 1) | — |
| `system.memory_used`, `disk_free` | `joker_system_memory_used_bytes`, `joker_system_disk_free_bytes` | gauge | — |

8. **Percentiles are summaries.** ref/09 defines the histograms of `Metrics` as `Percentiles` (`p50`, `p99`, `p999`, `max`, `window`). `p50`, `p99` and `p999` are the samples of a `summary` family with `quantile="0.5"`, `"0.99"` and `"0.999"`; `max` is a separate gauge (`…_max_seconds`), since Prometheus summaries have no maximum. The node keeps no running sum, so the summaries carry no `_sum` and no `_count`.
9. **The link state is a state set:** one sample per value of `LinkState` (`connecting`, `up`, `degraded`, `down`, `drift`, `stopped`), 1 for the current one and 0 for the rest, so that a series does not vanish when the state changes.
10. Series appear in the order of the families in point 7 and, inside a family, in the order of the arrays of `Metrics`; the same snapshot always gives the same bytes.

## Consequences

- ref/09 §3.12 and §5.2 and `spec/openapi/joker-api.yaml` (`GET /metrics`) say so; document 01 §19 names the endpoint.
- The `joker.*` signals of document 01 §19 and this exposition read the same counters; a name here is not a signal path, and the signals keep their own.
- What the daemon does not count yet (HDR histograms of the cycle, measured WCET, the envelope, apps, store and system sections) is absent from both `GET /v1/metrics` and `GET /metrics` until it is counted; it appears in both at once.
- The Grafana dashboards of F3 are built on these names.
