# ADR 0074 — Subscription resumption, re-resolution on a generation swap, and the sampling budget

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ref/09 §6.6, §6.7 and §6.9 describe resumption after a reconnection, the `resolved` notice of a generation swap, the node sampling budget and the per-group minimum of `outputs`, and the subscription engine built for AUR-243 left all four out. Writing them showed points the text leaves open:

1. What the server keeps of a dropped session, how many such sessions it keeps, and what happens to the subscriptions of the new connection and to those the `resume` does not name.
2. The shape of `resumed`: `WsServerMessage.subs` is a list of objects without fields.
3. What "replayed" and "resync" mean for an event topic whose items arrived while the client was away, and what `lost` counts in a `gap` with `resume_unavailable`.
4. What the "implicit resume" of SSE (`Last-Event-ID`) does when the session is gone: an `EventSource` cannot recover from an error answer.
5. How the rate of a subscription is counted for the budget of 1,000,000 samples/s, and whether a resumed or re-resolved subscription is checked again.
6. Which period is "the period of the owning group" for an `outputs` subscription whose selectors resolve to outputs of several groups, and what the default `min_interval` is when it is shorter than that period.

## Decision

### Resumption (§6.7)

1. When a connection ends, whatever the cause, its session is **parked** for `resume_window` (60 s, the value `welcome` reports): its `session` id, the principal, and per subscription its topic, filters, `min_interval`, `seq`, the baseline of `mode = change`, the cursor of an event topic and a **replay buffer**. At most 16 sessions are parked per node; parking one more drops the oldest. A parked session is not sampled and its subscriptions do not count in `api.subscriptions` nor in the budget.
2. The replay buffer of a subscription holds the last 256 `data` and `resync` messages it queued; the buffers of a session hold at most 1 MiB together, and the oldest message of the session goes first.
3. `resume` is accepted on a connection without subscriptions; otherwise it is `J-E1907`. It names a parked session of the same principal; any other `session` (never seen, expired, dropped, from another boot, or of another principal) is `J-E1907`, and the connection continues so that the client subscribes again.
4. On success the connection takes over the session: its `session` becomes the resumed one (a later reconnection resumes it again), and the server answers `resumed` with `id`, `session` and `subs`, one `{sub, result}` per requested subscription, in the order of the request, where `result` is:
   - `replayed`: every message of that subscription with `seq` above `last_seq` was still in its buffer, and they follow, in order, unchanged;
   - `resync`: they were not (or `last_seq` is above the subscription's `seq`). A state topic gets a `resync` with the next `seq`; an event topic gets a `gap` with `reason = resume_unavailable` and `lost` = the number of messages that could not be resent;
   - `unknown`: the session has no such subscription.
5. Subscriptions of the parked session that the `resume` does not name end, as with `unsubscribe`.
6. After `resumed`, sampling continues from the kept state: a state subscription sends what changed against its baseline, and an event subscription delivers the items after its cursor, including those that arrived while the session was parked, under the same ring and `gap` rules (§6.6 point 3).
7. **SSE.** `Last-Event-ID: <session>.<seq>` resumes the one subscription of that parked session with `last_seq = <seq>`, under points 3 to 6 (the request's own `topic` and filters are ignored). When it cannot be resumed, the request starts a new session as if the header were absent and, on an event topic, the first message after `subscribed` is a `gap` with `resume_unavailable`. The end of an SSE response parks its session like a WebSocket.

### Re-resolution on a generation swap (§6.9)

8. A `signals` or `outputs` subscription keeps the list of paths its selectors resolved to. When a read resolves to another list, the server sends `resolved` with `sub`, `generation` (the `GenerationRef` `{gen, hash}` of the running generation), `added` and `removed`, **before** any `data` with an added path. The baseline of a removed path is forgotten; an added path arrives in the next `data` as a change. The surviving paths continue without interruption.
9. The role of a connection is recomputed when the running generation changes (checked at most once a second). A subscription whose topic needs more than the new role ends with a `gap` (`role_changed`) and an `error` (`J-E1002`); the connection continues.

### Sampling budget (§6.6 point 5)

10. The rate of a subscription is its resolved items (at least 1) times the number of samples per second of its `min_interval`, rounded up; an event subscription counts one read per interval. Subscribing reserves the rate against the node budget (1,000,000 samples/s); a subscription that does not fit is refused with `J-E1903`, and the `detail` says how many samples/s still fit. The reservation is released by `unsubscribe`, by the end of the subscription and by parking; a resumed subscription takes its reservation back, and a re-resolution updates it, without the check. `Metrics.api.sampled_per_s` is the sum of the reservations.

### Minimum interval of `outputs` (§6.5)

11. In `outputs`, the minimum `min_interval` is the **shortest** period among the groups that own the outputs the selectors resolve to (`OutputState.group`, the period from `Status.groups`), and never less than 10 ms; nothing in the selection changes faster. Below it is `J-E1905`, with the minimum in the `detail`. Without `min_interval`, the default `100ms` is raised to that minimum.

## Consequences

- ref/09 §3.11, §6.3, §6.5, §6.6, §6.7, §6.9 and §6.11 and `spec/openapi/joker-api.yaml` (`WsServerMessage.subs`) say so.
- A client that keeps its `session` and its `last_seq`s recovers a short network drop without a full resync; a longer one, or a restart of `jokerd`, is a `J-E1907` and a new subscription, as ref/09 §6.7 already says.
- The parked sessions bound the memory of resumption at 16 MiB of replay buffers in the worst case; in practice a buffer holds the few messages queued since the last read of the client.
