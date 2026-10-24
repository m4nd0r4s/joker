# ADR 0062 — Wide signal values: the wide slot, decoding, the envelope, the API, the decision log and the trace

**Status:** accepted (v1) · **Date:** 2026-10

## Context

The specification has carried `string`, `bytes` and `record` signal values since the start: document 02 §2 caps device `string` and `bytes` at 64 B, ref/00 §4.1 gives their JSON form, ref/01 §3.6 their manifest literals, ref/04 §3.2 the wide section of the cycle image and the `INLINE` flag of a write, and document 04 §3.4 the `Internal<T>` record of an app. What it does not carry is everything between those surfaces:

- Document 01 §3.2 and ref/00 §4.1 put these values in "a separate arena with double buffering", which nobody designed, and ref/08 §2.9.1 budgets two 64 B buffers per signal for it. `joker_core::value::ValueType` points at the same arena, which does not exist.
- How a driver's raw value becomes a wide value (length, encoding, padding) and what a failed or malformed one writes.
- Which envelope steps apply to an output that has no numeric meaning, and what the arbiter compares to decide that the applied value changed (ADR 0033, decision records).
- What the decision log, the manual level record of A14 and the trace (document 16 §7, ADR 0037) hold for a value that does not fit their `u64` fields.
- The exact base64 the API speaks.

The implementation stopped at three deliberate gates: `jokerd` does not write a wide value into any slot (since the fixes of PR #65 it writes `bad(config_error)` into the signal's scalar slot, so the gap is visible but no value ever arrives), the arbiter refuses every wide proposal, and the cycle host refuses every wide port. Deploying the real I²C adapter on the bench (AUR-401) made the input gate the first to hurt. The design note `docs/superpowers/specs/2026-10-23-wide-signal-values-design.md` (AUR-419, AUR-420, AUR-421) proposes a `WideSlot` primitive; a review found that it changes public semantics without an ADR. This ADR takes its decisions, fixes what it left open, and corrects it where it conflicted with the references.

## Decision

### Storage

1. **Wide slot.** Every `string`, `bytes` and `record` signal keeps its last value in a **wide slot**: the seqlock of document 01 §3.2 (a single writer, odd version = write in progress, readers never wait, bounded retries, every field a 32-bit atomic, no `unsafe`), with a payload of up to 64 B and its length. Layout: `version`, `quality`, `ts_src` (two words), `ts_acq` (two words), `len` (0–64), `payload` (16 words): 96 B, aligned to 32 B. The scalar `Slot` does not change. The "arena with double buffering" of document 01 §3.2 and ref/00 §4.1 is replaced by it, and ref/08 §2.9.1 budgets 96 B per wide signal instead of 128 B.
2. **Dense wide index.** Wide signals are numbered densely among themselves, in `SignalId` order (0, 1, 2, … skipping every scalar signal), once per generation, from the registry. The wide slots are one table indexed by that number; a manifest without wide signals has an empty table. `SignalMeta` does not change, and the registry hash (ADR 0031) does not change, because the index is derived from the types it already hashes. A wide signal still has its 32 B `Slot`, which nobody writes or reads.
3. **Length.** A wide slot never holds more than 64 B and never holds a truncated value. A write of more than 64 B stores zero bytes with `bad(config_error)`. Every producer checks the length before writing (decisions 5, 8, 11), so this is a last line of defence, never a path a valid configuration reaches. (The design note truncated silently; that would turn a configuration error into a plausible value, against invariant 4.)
4. **`Internal<T>`.** An `Internal<T>` of an app uses a wide slot of the same type. There is **one slot per internal of an app**, shared by its cycle and async instances (they are one writer, A1); the declared side (`writer`) writes and the other reads, as document 04 §3.4 says. Whether that slot lives in the node's wide table or in a per-app allocation is not observable and is left to the implementation (AUR-421).

### Input

5. **Decoding** (ref/03 §3 step 1, ref/06 §2.7). A wide input point becomes bytes as follows, and only then reaches its slot:
   - `bytes` and `text` raw values are taken as they are;
   - `regs` (a Modbus `string` or `bytes` point of `len` registers) gives 2 B per register in protocol order, each register big-endian, swapped by `byte_order = "little"`; `word_order` does not apply;
   - the result must be at most 64 B (ref/00 §10); a `string` must be valid UTF-8, and with `encoding = "ascii"` (ref/03 §2.2.4, the default) every byte must be below `0x80`;
   - padding is **not** trimmed: the value is the bytes the device sent.
   A raw kind other than the ones above, a value over 64 B, or a `string` that fails its encoding is `bad(config_error)` with zero bytes, as for a scalar that does not convert (ref/06 §2.7, `J0869`).
6. **Failures write.** A failed read, a bad sample, a link that goes down and a device that cannot start write `bad(<reason>)` with zero bytes into the wide slot, exactly as the scalar path writes `bad` into the scalar slot (document 16 §3, invariant 4). The last good wide value never stays in place behind a failure.
7. **Expressions** still cannot read a wide signal (`J0304`, ADR 0034). `requires_good` may name one, because it reads only the quality.

### Output and envelope

8. **Proposals.** A wide proposal is validated at step 1 (E10): more than 64 B, or a `string` that is not valid UTF-8, is `Rejected(WrongType)`. Its content travels with the proposal in fixed-capacity storage of the group (64 B per entry), so the tick does not allocate (invariant 6).
9. **Envelope steps.** `range`, `on_range`, `rate`, `safe_ramp`, `deadband` and `verify.tolerance` have no meaning on a `string` or `bytes` output and are `J0233` at check time (the code that already refuses them on `bool` and `enum`); `safer` other than `"equal"` is also `J0233`. No new code is needed: the design note's "new diagnostic" is `J0233`. The clock step, the permissive (`interlock`, `on_interlock`), `requires_good`, `min_interval`, `max_actuations`, trips and `reset_when` apply unchanged. An actuation (E8) is an applied value whose bytes differ from the previous applied value's.
10. **Applied value.** The applied value of a wide output is the content of its own wide slot, written by the arbiter **only when the applied bytes change** (a byte comparison of at most 64 B), so the slot's sequence number counts changes of applied value. `safe` for a wide output is the literal of ref/01 §3.6. Delivering a wide value to a real driver stays out of scope: `CommitBox` has no byte payload and the output stage of ADR 0047 is still open, so a wide output belongs, for now, to a device whose driver does not take writes for it, or to none.

### Manual levels

11. **API input** (ref/09 §3.3). A `string` output takes a JSON string, which is its text; a `bytes` output takes a JSON string in base64 (decision 13). More than 64 B after decoding, base64 that does not decode, or any other JSON type is `J-E1036`. The API checks before staging, and step 1 checks again (decision 8).
12. **A14 record.** The record of a persisted manual level stores a wide value in its JSON form (decision 13), never an index or a slot sequence, which a restart does not preserve. Restoring stages it as an ordinary proposal, like a scalar level.

### Representation

13. **JSON** (ref/00 §4.1): a `string` value is a JSON string; a `bytes` value is base64 with the standard alphabet and padding (RFC 4648 §4), and input accepts exactly that form. A wide sample that is `bad`, or was never written, has `"value": null`, as a scalar one does. `max_age` and its staleness grading apply to wide signals unchanged.
14. **Decision log** (ADR 0033). The core's fixed-size record carries, in place of the applied value bits of a wide output, the **sequence number of its wide slot** at the commit, and change detection uses it (decision 10). The `Decision` of ref/09 §5.7 shows `proposed` and `applied` as JSON values: the host fills them outside the tick from the content it staged (the proposal) and from `safe`, the only two sources an applied wide value can have, because no envelope step changes a wide value.
15. **Trace v1** (document 16 §7, ADR 0037). The trace keeps its fixed-size entries and does not carry wide contents. For a wide signal, the `u64` of an input entry, of a proposal and of a result is the **length in bytes** of the value; a wide proposal sets bit 1 (`INLINE`, ref/04 §3.3) of its flags. The quality and the input `seq` are recorded as for any signal. Replay compares outcomes and lengths of wide outputs; it cannot replay a wide content (see the open questions).

### Apps

16. Ref/04 and the WIT do not change: the wide section of the image and the `INLINE` write already describe how an app sees a wide value. The latch copies the wide section from the wide slots. The cycle host keeps refusing wide ports other than `Internal<T>` until it serves the wide section; that is an implementation gap of the cycle subset (ADR 0052), not a reservation.

### Open questions, decided conservatively here

- **Wide contents in the trace.** Decision 15 records lengths only, so a trace that contains wide inputs read by an app cannot be replayed byte for byte. A later ADR may add a flagged per-tick wide section to trace v1 (a header flag bit, so a reader of this version rejects it) before the trace format is frozen.
- **Padding.** Decision 5 keeps NUL and space padding of Modbus strings. Trimming would need a profile key; none is added.
- **Historian.** Recording wide samples in the historian is not decided here; the historian keeps its scalar scope.
- **Driver delivery.** Decision 10 leaves wide writes to drivers to the output stage of ADR 0047.
- **Where `Internal<T>` slots live** (decision 4), and whether `GET /v1/values` shows an internal (ref/09 lists the `internal` direction in `SignalMeta`, but does not say it is readable), stay open; this ADR does not expose internals through the API.

## Consequences

- Document 01 §3.2, document 02 §2, document 16 (E10, E11, A14, §7), ref/00 §4.1, ref/01 §5.10, ref/04 §3.2, ref/08 §2.9.1, ref/09 §3.3 and §5.7 and the `Value`/`ValueInput` schemas of `spec/openapi/joker-api.yaml` change in the same set of commits. `crates/joker-manifest/codes.toml` does not change.
- The implementation plan `docs/superpowers/plans/2026-10-23-wide-slot-input.md` follows this ADR; where the design note differs (silent truncation, a new diagnostic code, `lower`/`higher`/`equal` "clamps", one slot per app instance), this ADR wins.
- A wide signal costs 128 B of slots (its unused `Slot` and its wide slot), against 160 B with the arena it replaces. A manifest with no wide signal pays nothing.
- `jokerd` gains a base64 dependency, subject to the policies of ADR 0055.
