# ADR 0081 — Historian storage: WAL, segments, checkpoints and retention

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01, §12, fixes the shape of the local historian (`joker-store`): a shared WAL with batched `fsync`, per-series segments of 4 KiB blocks with Gorilla for scalars and zstd for events, aggregates `1s`, `1m` and `15m` with their own `keep`, retention by size before retention by time, and a persistent cursor per uplink. §20 adds the disk-full behaviour and ADR 0013 the bounded queues. AUR-287 implements the crate and found what the documents leave open:

1. how blocks reach stable storage without the WAL growing for ever when a series is slow;
2. what "4 KiB blocks" means for events, whose size is only known after compression;
3. what a `Bad` sample contributes to an aggregate and what "dominant quality" is on a tie;
4. which data goes first when the disk passes 90% of `max_disk`, and what "< 1%" is a percentage of;
5. how the store numbers records for the cursors of store-and-forward;
6. document 01, §18, has every sample stored in monotonic time and translated to UTC through a correspondence table that a clock correction rewrites retroactively; nothing in 1.0 builds that table.

## Decision

### Layout

1. A store is a directory (its path is node configuration, ref/02): `series.cat` (the catalog), `wal/<n>.wal`, `seg/<tier>-<class>-<keep>/<n>.seg` with an `<n>.idx` beside each closed segment file, and `cursors/<consumer>.cur`. Every file is a sequence of checksummed records (CRC-32, IEEE); a record that fails its checksum ends the valid prefix of its file.
2. A **series** is one recorded signal, one alarm journal or one event stream, with a kind (scalar or event), a **class** (`telemetry`, `alarm` or `decision`) and a `keep` per tier. A tier without a `keep` is not built; event series have only `raw`. The catalog is synced before any record of a new series enters the WAL.
3. Every record of a series gets a **store sequence number** `n`, 1, 2, 3… without gaps except for records the WAL refused. It orders a series, pages queries, and is what a cursor stores: a cursor is, per consumer, the `n` of the last record it delivered for each series. This is the "sequence number per series" with which the uplinks deliver `at_least_once` (document 01, §13).

### WAL and `fsync`

4. The writer appends one WAL frame per batch. `fsync = "batch"` syncs the WAL at most every `fsync_interval` (1 s) and, at once, after a batch that holds an alarm or a decision record; `"always"` syncs after every batch; `"never"` syncs it only at checkpoints (rule 9). A power cut therefore loses at most `fsync_interval` of telemetry with `batch`, and no alarm or decision record that was acknowledged.
5. A failed write is rolled back to the previous end of its file, so a frame never follows a torn one.

### Blocks and segments

6. A scalar block is a 64-byte header and a Gorilla stream: delta-of-delta of the acquisition instant in ns (0 in one bit; otherwise 10, 17, 25, 33 or 64 bits), XOR of the value's transport bits, the quality word in one bit while it repeats, delta-of-delta of the signal's `seq`, and the source instant relative to the acquisition instant. A block is sealed before the next row could pass **4096 bytes**, header included. An aggregate block encodes `count`, `valid`, `min`, `max`, `mean`, `first`, `last` and the quality the same way.
7. An event block holds rows until their **uncompressed** size reaches 4032 bytes, and is then compressed with zstd. A single larger event takes a block of its own.
8. Sealed blocks go to the current segment file of their tier, class and `keep`, shared by every series with the same three. A file is closed at 16 MiB or when it would span more than 1 h (`raw`), 6 h (`1s`), 1 d (`1m`) or 7 d (`15m`); closing syncs it and writes its index.

### Checkpoints and recovery

9. A **checkpoint** syncs the segment files, starts a new WAL file, writes into it a snapshot of every open block and of every aggregate interval in progress, syncs it, and deletes the older WAL files. It runs when the WAL file passes 64 MiB, every 5 min, on a clean shutdown and at the end of every open. A slow series therefore never pins the WAL, and recovery replays at most one WAL file.
10. Opening a store cuts torn tails (catalog, WAL, segment files without a valid index), reads the sealed blocks, and replays the WAL: a record or a snapshot row whose `n` a tier already holds is skipped, so replaying twice gives the same store.

### Aggregates

11. An interval is `[k × resolution, (k + 1) × resolution)` in UTC from the epoch. A sample that arrives for an earlier interval than the one in progress counts in the one in progress. Queries see the closed intervals only.
12. `count` counts every sample; a `Bad` sample enters nothing else (AGENTS.md invariant 4). `min`, `max` and `mean` are over the valid samples of a numeric series, and absent when there are none or the series is not numeric (`bool`, `enum`, `bits`, wide values); `first` and `last` are the first and last valid values.
13. The **dominant quality** is the most frequent quality code among the interval's samples; on a tie, the worse one.

### Retention and the disk

14. Retention deletes whole segment files. **By time:** a file whose newest row is older than its `keep`. **By size:** while the segment and WAL files together pass 90% of `max_disk`, the file with the oldest newest row among, in order, `raw` telemetry, `1s`, `1m`, `15m` telemetry, and only then alarms and decisions.
15. The disk is **critical** when the partition's free space is below 1% of its size, when the store is at `max_disk` after retention, or after a failed write until the next check. While it is critical only alarm and decision records are accepted; the others are refused and counted. The store never blocks a producer and never stops the core; the runtime raises an alarm while the disk is critical (AUR-427).
16. Retention and the disk check run every 10 s on the writer thread.

### Ingestion

17. Producers push into two bounded lock-free queues, one for alarms and decisions and one for telemetry, so a telemetry flood never drops an alarm. A full queue drops the new record and counts it (ADR 0013). One thread (`svc/store`) drains both, priority first, in batches of at most 4096 records.

### Time

18. In 1.0 the historian stores the UTC instant computed at acquisition. The monotonic storage and retroactive correspondence table of document 01, §18, are **reserved**; until they exist, a clock correction does not change the `ts` of samples already recorded. ref/09 §5.5 and document 01, §18, say so.

## Consequences

- At most one WAL file and one snapshot per open block are replayed at startup.
- Each sealed block costs about 72 bytes of memory in the store's index (under 2% of the disk it describes); an index on disk is left for later.
- The write load of the historian is bounded by the size of the data plus one snapshot per open block per checkpoint, which is what `jk check` projects (J0381).
- On this project's development machine (Apple silicon, NVMe) the store ingests about 4 million samples per second on one thread, at 2.3 bytes per sample with steady timestamps and 4.3 bytes with 50 µs of jitter (`cargo run --release -p joker-store --example ingest`). The CM4 figure of document 01, §21, is still to be measured.
