# ADR 0063 — Socket roles, subscription limits and device counters in status and metrics

**Status:** accepted (v1) · **Date:** 2026-10

## Context

A review of the WebSocket and SSE engine (AUR-243) and of the observability endpoints (AUR-271) found points that the specification leaves open:

1. **Who is `admin` on the socket.** ref/09 §2.3 makes uid 0 and "the `joker` user" `admin`. `jokerd` runs as the `joker` service account in production, but `jk dev` embeds the daemon (ADR 0057) and runs it as the developer, who then talks to a node of their own as `viewer`. The implementation avoided the question by answering `admin` to everyone, which ref/09 §2.3 forbids.
2. **What "full" means for the output queue of a connection.** ref/09 §6.6 bounds the queue at 1 MiB and closes with 4008 when it "stays full for more than 30 s", but a state subscription never queues a second message while its first is still there (point 2), so the queue rarely reaches exactly 1 MiB, and a client that stops reading would never be closed.
3. **The shape of the `status` and `metrics` topics.** §6.4 gives `Status` and `Metrics` as the items, but `data` and `resync` carry an `items` array.
4. **What `/v1/status` and `/v1/metrics` say about devices.** The io task of each device already counts reconnections, samples dropped by a full sample ring and samples that broke ref/06 §2.8 (`J0869`), and AGENTS.md invariant 7 requires every loss to be counted and visible. `Status.devices` has no counters and `Metrics.drivers` has no link state and no sample losses.

## Decision

### Roles on the socket

1. On the Unix socket, the principal whose uid is the effective uid of `jokerd` itself is `admin`, as uid 0 and the `joker` user are: it is the account the node runs as (the `joker` service account in production, the developer under `jk dev`), and whoever has it already has the node.
2. The role is computed from the transport on every HTTP request and when a WebSocket or SSE connection opens; it is the `role` of `welcome` and of `caller` in `GET /v1/info`. No answer carries a fixed role.

### Subscription limits

3. The output queue of a connection counts the bytes of the messages it holds, up to 1 MiB. It is **full** when a message did not fit in it; it stays full until it drains below 1 MiB. Full for more than 30 s closes the connection with `closing` and a close frame 4008 (`J-E1906`); the server waits at most 5 s for those to leave and then drops the connection.
4. A message that does not fit is handled by kind: a state subscription sends it at a later interval (nothing is lost, §6.5); an item of an event subscription stays in the ring of the subscription, and what the ring evicts is a `gap` with `lost` and counts in `joker.api.dropped` (`Metrics.api.dropped`).
5. The `status` and `metrics` topics send `items` with exactly one element, the `Status` or the `Metrics` of `GET /v1/status` and `GET /v1/metrics`.

### Device counters

6. Each element of `Status.devices` carries `reconnects`, `dropped_samples` and `invalid_samples`, the counters since the generation started; `link` is the effective `link_state` of the device (ref/00 §6).
7. Each element of `Metrics.drivers` carries `link` and the same three counters.
8. A device whose driver runs inside the core (`sim`) has no io task and does not appear in either list.

## Consequences

- ref/09 §2.3, §5.1, §5.2, §6.4 and §6.6 and `spec/openapi/joker-api.yaml` (`Status`, `Metrics`) say so.
- The roles that a generation's `[security.roles]` grants on the socket are not yet applied: until the daemon carries them, the defaults of ref/01 §5.20 hold (`joker-ops` is `operator`, `joker-admin` is `admin`) and every other principal is `viewer`. That can only lower a role, never raise one.
- The checks of the minimum role of each HTTP operation (`x-joker-role`) are separate work; this ADR only fixes how the role is computed and reported.
