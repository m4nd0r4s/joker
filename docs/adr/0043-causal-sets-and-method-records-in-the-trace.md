# ADR 0043 — Causal sets, and method records in the trace

**Status:** accepted (v1) · **Date:** 2026-10

## Context

M3 (document 08, round 1c; document 10) asks every applied write to keep the tick, the `(SignalId, seq)` the task read, and the proposal and commit instants. The decision log exposes these as the write's causal set (`inputs`, ref/09 §5.7). Document 09, attack 12, prices it as "a bitset of reads per tick". Document 16, §7, reserves the `n_métodos` section of a tick for method records, `(u32:writer u32:instance u16:method u16:bytes args)`. Ref/00 §4.1 says that the arguments of a method are deterministic CBOR in binary, with integer keys equal to the position of each field. ADR 0041 keeps expanded writes out of the trace's proposals, so that the replay expands the recorded calls again.

Open questions:

- what exactly a "read" is and who marks it;
- how a write finds its causal set;
- how the arguments are encoded, given that the core holds only the slot words of the values and not their declared types.

## Decision

1. **Reads.** The host marks, for each cycle task of a tick, every port of the group's image the task read. This covers the image entries the SDK accessed and the `cycle-io.read` calls. The core keeps one bitset per task over the image's ports, cleared at each latch. Async tasks have no causal set in the core: they do not read the latched image (document 02, §6.4).
2. **Attaching.** A proposal, and a method record, carries the number of the task that staged it. Writes expanded from a method record inherit its task. After each decision record of a write, the core passes the causal set of that write's task to the sink: the ports that were read, with the latched samples (value, quality, `seq`). The host stores it with the record. Output records refer to the write records by their cause (`Proposal(writer)`) and carry no set of their own.
3. **Method records in the trace.** A tick records its method records in staging order, in the `n_métodos` section. The arguments are a deterministic CBOR map (RFC 8949 §4.2.1) whose keys are the parameter positions (0, 1, …) and whose values are the **slot words** of the arguments (ref/00 §4.1), encoded as unsigned integers. This keeps the representation of ref/00 §4.1 (integer keys by position) without the core knowing the declared types: the reader reads each value as a slot word, as it reads the inputs and the proposals. The tick's proposal section holds only the direct proposals.
4. Nothing else in the format changes. Proposals carry no task number in the trace, because replay does not need it: the causal sets are for the decision log, and the trace already has every input of every tick.

## Consequences

- `jk decisions --cause <signal>` can filter on the stored sets, and an audit can show which samples a decision read.
- A traced method call replays exactly: the replay stages the recorded call and expands it again (ADR 0041).
- The method section is readable by any CBOR decoder. Its values are slot words, as everywhere else in the trace.
