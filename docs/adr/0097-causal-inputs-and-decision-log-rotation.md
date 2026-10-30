# ADR 0097 — Causal inputs in the decision log, and its rotation

**Status:** accepted (v1) · **Date:** 2026-10

## Context

M3 (document 08, round 1c) asks every applied write to keep what its task read. ADR 0043 gives the core the causal set of each write (`CommitSink::causal`, right after the write's decision record), and ref/09 §5.7 exposes it as `Decision.inputs`, filtered by `cause` (`jk decisions --cause`). The Linux node never stored it: its sink ignored the call, and the `cause` filter was accepted and not applied (AUR-268).

The cycle domain does not allocate (AGENTS.md invariant 6), so the causal set must travel in a fixed-size structure, and a task can read any number of ports. No document bounds what a record keeps.

The decision log is a single append-only file with no bound. On the bench Pi 3 it reached 650,000 lines (260 MB) in a week. Opening it read the whole file to count its lines, and no document says how long it is kept. Document 08, attack (c), asks for the causality of 14 days of grid orders; ref/09 §8 says the decision log is the last to lose writes when the disk is full.

## Decision

1. **The causal set travels with its record.** The sink of a group holds each decision record until the next call. When `causal` follows, it copies the set into a fixed-size causal record, at most **16 inputs** (`SignalId`, sample), pushes it to a bounded queue of **512** entries, and then pushes the decision record with the number of its causal record. The decision writer joins them by that number. Because the causal record is queued first, the writer never sees a write before its set.
2. **What counts as read.** A native task's reads are its `read` calls. A Wasm app reads its packed image in its own memory, without calling the host, so the host cannot tell which entries it touched: the causal set of a Wasm task is every bound port the host packed from the group's image for that call. This is a superset of what ADR 0043 decision 1 describes, never a subset, so a `cause` query never misses a write that depended on a signal. The value a `[param]` port carries is not a signal and is never in the set.
3. **What is stored.** A `write` record of the log carries `inputs`: one object per signal read, in `SignalId` order, with `path`, `seq`, `value` (the JSON form of ref/00 §4.1) and `quality` (and `reason` when it has one). When the task read more than 16 signals, the first 16 in `SignalId` order are kept and `inputs_omitted` is the count left out. `Decision.inputs` and `Decision.inputs_omitted` expose them. A causal record that did not fit in its queue is counted (`Metrics.runtime.dropped.causes`, `joker_runtime_dropped_total{queue="causes"}`), and its write is logged without `inputs`. A missing `inputs` never reads as "read nothing": a task that read nothing has `inputs: []`.
4. **`cause`.** `GET /v1/decisions?cause=<path>` returns only the decisions whose `inputs` contains that path. Aliases resolve as for `output`. `jk decisions --cause <signal>` sets it.
5. **Rotation.** The current file stays `decisions.jsonl`. Once it holds **64 MiB** or more, the next append first closes it, syncs it and renames it to `decisions.<seq>.jsonl`, where `<seq>` is the `seq` of its first entry written as 20 digits, so the names sort in log order. The `seq` numbers continue across segments. Opening the log reads only the last entry, of the current file or else of the newest segment, to continue them.
6. **Retention.** After a rotation, the oldest segments are deleted while the current file and the segments together take more than **1 GiB**. The current file is never deleted. Each deletion appends a `retention` entry with the `seq` range removed and its size, synced at once, so a gap in `seq` is always explained in the log itself. At the bench rate, 1 GiB holds about four weeks. Making the two bounds configurable is reserved.
7. **Readers.** `GET /v1/decisions` and every other reader read the current file and then the segments, newest first, or the reverse for `order=asc`. Cursors are `seq` numbers and stay valid across a rotation until their segment is deleted.

## Consequences

- `CommitSink::causal` has an implementation on Linux; the core does not change.
- ref/09 §5.7 and the OpenAPI `Decision` gain `inputs_omitted`; `Metrics.runtime.dropped` gains `causes`. ADR 0090 point 5's list of queues grows by one.
- The disk the decision log takes is bounded. A site that must keep the log longer than the bound copies the rotated segments out before they are deleted (the agent upload of ref/10 is the path for that).
- A causal set costs about 800 B in the queue and one more JSON array per write record. Writes are logged only on change (document 01 §19), so the log grows in proportion to changes, not to ticks.
