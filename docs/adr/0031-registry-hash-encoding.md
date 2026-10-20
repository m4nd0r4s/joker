# ADR 0031 — Signal ordering and the registry hash encoding

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01, §3.1, says that every signal gets a dense `SignalId` "ordered by path", and document 16, §7, puts a 32-byte "registry hash (ordered list of SignalId, types, groups)" in the header of every `.jktrace`. Neither fixes the exact order (paths are compared how?), how groups are numbered, or the bytes that are hashed. The trace format is frozen in 1.0 (document 26, §5), and a trace recorded on a Raspberry Pi has to be replayed on a laptop and on an MCU (ADR 0020). So two implementations, or two builds of the same one, must produce the same ids and the same hash from the same configuration.

## Decision

1. **Signal order.** Signals are sorted by their canonical path, comparing the UTF-8 bytes. Paths only contain `[a-z0-9_.]` (document 02, §9), so this is plain ASCII order, where `.` sorts before digits, digits before `_`, and `_` before letters (`a.b` < `a_b` < `ab`). `SignalId(n)` is the n-th signal in that order, starting at 0. Aliases (document 16, P1) get no id of their own and do not take part in the order.
2. **Group order.** Cycle groups are sorted by name in the same byte order. `GroupId(n)` is the n-th group, starting at 0. This is the `u32` group number in the trace segments.
3. **Registry hash.** SHA-256 over the following bytes, with every integer little-endian:

   ```
   "JKREG"  u8:encoding version (= 1)  u32:n_signals
   for each signal, in SignalId order:
       u16:path length  path (UTF-8)  u8:signal type (ref/00 §6)  u32:owner GroupId (0xFFFF_FFFF when none)
   ```

   The owner group is the group that owns an output or a derived signal (document 16, A11); inputs have none. Units, directions, `max_age` and aliases are not hashed. A change in any of them that matters for replay already changes the generation hash, which sits next to the registry hash in the trace header.
4. The core computes SHA-256 itself (`joker_core::hash`), so the hash has no external dependency and gives the same bytes on every target.

## Consequences

- Reordering signals or groups in a manifest, or the order of overlay layers, never changes ids or hashes.
- Adding, removing or renaming a signal, changing its type or moving an output to another group changes the registry hash, so `jk replay` can tell that a trace and a generation do not match.
- The encoding has its own version byte. A future change writes a new ADR and bumps it; readers compare hashes and never parse them.
- Document 16, §7, and ref/00 §5 should cite this ADR when they are translated (ADR 0029).
