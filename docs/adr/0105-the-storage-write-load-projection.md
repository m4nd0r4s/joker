# ADR 0105 — The storage write-load projection of `jk check` (`J0381`)

**Status:** accepted (v1) · **Date:** 2026-11

## Context

Document 01 §12 and ref/03 §2.3.1 ask `jk check` to project the write load of a manifest (historian, decision log, `kv`, continuous trace, and the number of fsyncs: bytes per day and fsyncs per hour) and to compare it with what the board's `storage` (`sd`, `emmc`, `ssd`) withstands, with the warning `J0381` naming the sources in order. ADR 0048 left `J0381` out because no document gives an endurance model per storage class, and `J0381` was never emitted (AUR-268).

A manifest does not say how often a signal changes, nor how often an output's result changes, so part of the projection must assume a rate. The node measures the real load later (`jk doctor`). On the bench Pi 3, with the default `fsync = "batch"`, the node made 59 `fdatasync` calls in 60 s (AUR-268, strace).

## Decision

1. **When.** `jk check` projects the load when the manifest names a board that is not a microcontroller. A board without `storage` counts as `sd`, the default of ref/03 §2.3.1. Without a board, there is no projection.
2. **Bytes per day**, by source:
   - **historian samples:** for each recorded signal (the rule of `[telemetry.record]` that applies to it, ref/01 §5.17.1; `string` and `bytes` signals are not recorded), its rate times **64 B** (the WAL record of a sample and its compressed row). The rate is `1 / every`; with `on_change`, the poll rate of its device (`poll`, else 1 s), and 1 sample per second for a signal without a device; at most 100 per second (the 10 ms read of `on_change`).
   - **historian checkpoints:** for each recorded signal and each tier it keeps (`raw` and each aggregate layer of `keep`), one snapshot of half a block (**2 KiB**) per checkpoint, every 5 min (ADR 0081, rule 9): 288 per day.
   - **decision log:** for each output of `[outputs]`, one entry every 10 s, of **512 B**. A stable output writes much less (document 01 §19); this is the assumed rate of change.
   - **continuous trace:** for each group with `trace.continuous`, one tick per period, of 64 B plus 16 B per signal of the site (ADR 0048, point 6, which counts the same ring in memory).
   - **`kv`:** nothing. No key of the manifest gives its rate; `jk doctor` measures it.
3. **Fsyncs per hour**, by source:
   - **historian WAL**, when a signal is recorded or an alarm is declared: with `fsync = "batch"`, `3600 / fsync_interval`; with `"always"`, one per batch, counted as the total sample rate (at most 100 per second) times 3600; with `"never"`, none.
   - **historian checkpoints**, when a signal is recorded: 2 per checkpoint (the segments and the new WAL), 24 per hour.
   - **decision log**, when the manifest has outputs: its batch sync every second (ADR 0101), 3600 per hour. The immediate fsyncs of trips, manual writes, rejections and level changes depend on operation, not on the manifest, and are not projected.
4. **What each class withstands:**

   | `storage` | bytes per day | fsyncs per hour |
   |---|---|---|
   | `sd` | 2 GiB | 10,800 (3 per second) |
   | `emmc` | 20 GiB | 36,000 (10 per second) |
   | `ssd` | 200 GiB | 360,000 (100 per second) |

   The `sd` figures assume a 32 GB card of about 1,000 cycles with a write amplification of 8 for small writes: 2 GiB per day lasts about five years. `emmc` assumes 3,000 cycles and an amplification of 3; `ssd`, about 0.3 drive writes per day of a 256 GB disk. The fsync limits keep a card's flash translation layer away from the per-write overhead that dominates small synced writes.
5. **`J0381`** (warning) is emitted once when the bytes per day or the fsyncs per hour are above the limit of the class. Its label says both totals against their limits; its help lists the sources from the largest down, and names the keys that lower them (`every`, `on_change`, `keep`, `fsync`, `fsync_interval`, `trace.continuous`).

## Consequences

- `J0381` leaves the deferred list of the manifest checks; `jk check` emits it. `jk doctor`'s measured `J0381` is still to come.
- The bench Pi 3 with the default `batch` projects about 7,200 fsyncs per hour, under the `sd` limit and above the measured 3,540: the projection is an upper bound for a quiet site.
- The assumed rates (decision entries every 10 s, 1/s for `on_change` without a device) and the per-class limits are estimates. Measurements from `jk doctor` on real nodes come back as a new ADR that replaces them; the shape of the projection does not change.
