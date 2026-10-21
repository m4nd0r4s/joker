# ADR 0037 — Encoding details of the `.jktrace` v1 format

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 16, §7, defines the trace layout, and the format must be right in 1.0 because it cannot be changed by software update in the field (document 26, §5). Writing the reader and writer (document 13, task 13) left a few facts that two implementations could read differently:

- what the `seq` of a tick is;
- what the `u16` flags of a proposal contain;
- what the header flag bits mean exactly, and what `compressed_bytes` holds when a segment is not compressed;
- how a reader knows how many inputs a tick has, since the count is not in the file;
- which applied value is recorded for `safe = "relinquish"`, and which value for an input that has never had a sample.

## Decision

1. **Header flags.** Bit 0 set means the segments are **not** compressed (ref/08 §2.6.o). Bit 1 set means a state segment follows the header. All other bits are 0 in v1, and a reader rejects a file with an unknown bit set.
2. **`compressed_bytes`** is the length of the segment payload as stored: the zstd frame when compressed, the raw tick bytes when not. The same holds for the state segment.
3. **Tick `seq`** is the tick index `k` of ADR 0032 (`t_k = t_0 + k·T`). It jumps after a skipped tick.
4. **Inputs per tick.** A tick carries one 16-byte entry per latched port of the group, in `SignalId` order, and no count. The reader takes the port count of each group from the generation whose registry hash is in the header (on an MCU, the `latch` list of `GROUPS`, ref/08). A reader without the generation cannot parse ticks, and says so. An input that never had a sample is written as `seq` 0, `uncertain(initial)`, value 0 (ref/04 §3.2).
5. **Proposal flags** (`u16`): bits 0–7 are the `WRITE` flags of the cycle ABI (ref/04 §3.3: bit 0 `RELINQUISH`). Bit 8 is set when the proposal was drained from the async inbox in step 2, and clear for a cycle task in step 4. Bits 9–15 are 0 in v1.
6. **Results** are one entry per output owned by the group, in `SignalId` order, in every tick. The `u8` is the outcome code of ref/00 §4.4 for the output's outcome in that tick (ADR 0033). The applied value is the value bits, or 0 when the applied state is `relinquish`.
7. **Integers** are little-endian everywhere, as document 16, §7, says. Instants are ns, including on an MCU.

## Consequences

- Traces from Linux and from an MCU can be parsed by the same reader, given the generation.
- The trace cannot tell `relinquish` apart from a value of 0, cannot carry the reason of a `rejected`, and cannot carry the TTL of a manual write. The decision log has the first two. The third makes a manual write's expiry depend on the TTL declared for the writer when a trace is replayed. These gaps should be closed before 1.0 by a format change with its own ADR, while the format can still change (document 26, §5).
