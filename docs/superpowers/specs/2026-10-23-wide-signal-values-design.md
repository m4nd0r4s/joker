# Wide signal values (`bytes`/`string`/`record`): the `WideSlot` design

Status: approved by the user in brainstorming (2026-10-23), not yet implemented.

**Normative decisions: ADR 0062** (`docs/adr/0062-wide-signal-values.md`, 2026-10-24). This note is background; where it differs from the ADR, the ADR wins. The differences: a write over 64 B stores zero bytes and `bad(config_error)` instead of being truncated (decision 3); the "clamps" of an output are `range`, `on_range`, `rate`, `safe_ramp`, `deadband` and `verify.tolerance` (`lower`/`higher`/`equal` are the values of `safer`), and they are refused with the existing `J0233`, not a new code (decision 9); an `Internal<T>` has one slot per app, shared by its cycle and async instances, not one per instance (decision 4); Modbus `string`/`bytes` registers are decoded too (decision 5); and the trace records lengths, not contents (decision 15). Since PR #65 a wide-typed signal on `main` reads `bad(config_error)` in its scalar slot rather than staying at `uncertain(initial)`; the Problem section below describes the code before that fix.

## Problem

`bytes`/`string`/`record` signal values are deliberately refused everywhere in the runtime today — not an oversight, three independent, intentional gates:

- `jokerd::drivers::raw_to_bits(value: &OwnedRaw, ty: ValueType) -> Option<u64>` only handles `Bool`/`I64`/`U64`/`F64`/`F32`/`Enum`/`Bits`; it returns `None` for `Bytes`/`String`/`Record`, and the caller silently skips the slot write on `None`.
- `joker_core::output::OutputState::accepts(bits: u64) -> bool` returns `false` unconditionally for `String`/`Bytes`/`Record`.
- `joker_host::cycle` refuses any WASM app port typed `String`/`Bytes`/`Record` with `"wide values are not served by the cycle subset"`.

This was found while deploying the real `i2c` Linux adapter (AUR-401) to the bench Pi: a real i2c read — success or failure — never updates its signal's slot. It stays at `seq=0`/`uncertain(initial)` forever, because `raw_to_bits` returns `None` for `RawValue::Bytes` and the write is skipped. The driver and the real ioctl path are correct and verified; the gap is entirely upstream, in how a wide value would reach a signal slot at all.

`joker_core::value::ValueType`'s own doc comment already says `Bytes`/`String`/`Record` "live in the wide arena, not in the slot value" — but no such arena exists anywhere in the codebase. `docs/02-signal-model.md` §2 confirms the intended shape: device `string`/`bytes` signals are capped at 64 bytes; app `Internal<T>` records are "up to 64 bytes with scalars only, stored in a wide slot with the same seqlock protocol" (`docs/04-sdk.md` line 168: "last value (wide slot ≤ 64 B), seqlock; the cycle reads from the latch").

## Scope

Three independent subsystems, sharing one new primitive (§1). Each of §2/§3/§4 becomes its own Linear issue; §1 is shared foundation work, folded into whichever issue lands first (recommend §2, the one with a live downstream consumer today — AUR-401's i2c driver).

1. The `WideSlot` primitive and its registry wiring (shared).
2. Device **input**: a driver's wide read reaches its signal's slot.
3. Device **output**: a wide value can be proposed, arbitrated, and applied (not delivered to a real driver yet — see Non-Goals).
4. App `Internal<T>`: cross-instance (async ↔ cycle) wide state for apps.

## §1 — The `WideSlot` primitive and registry wiring

### Storage: a separate, sparse table — not a bigger `Slot`

Three storage approaches were considered:

- **(A) Enlarge `Slot` itself** so every signal's slot is big enough for 64 bytes. Rejected: `joker_core::slot::Slot` is the single most safety-critical, most-used primitive in the codebase (32 bytes, `no_std`, audited with `loom`, used by every signal regardless of type). Blowing it up to ~96+ bytes taxes every scalar signal — the overwhelming majority — for a feature most manifests never use, and directly fights the project's own embedded-size discipline (MCU targets without 64-bit atomics, `AUR-267`'s binary-size CI).
- **(B) A separate, sparse `WideSlot` table, sized only to signals actually declared `bytes`/`string`/`record`.** Chosen. `Slot` is untouched — zero risk to the existing, audited scalar path. Memory cost is proportional to actual use (zero for a manifest with no wide signals).
- **(C) A tagged union inside `Slot`.** Rejected: combines the downsides of both — still reserves the largest variant's size for every signal, and complicates the existing seqlock protocol with a variable-length payload inside the same atomic-word scheme.

### `WideSlot` (new, in `joker_core`)

Same seqlock algorithm as `Slot` (odd version = write in progress), same word-splitting discipline (every field `AtomicU32`, no `unsafe`, portable to MCUs without 64-bit atomics), just larger:

```
WideSlot {
    version: AtomicU32,
    quality: AtomicU32,
    ts_src_lo: AtomicU32, ts_src_hi: AtomicU32,
    ts_acq_lo: AtomicU32, ts_acq_hi: AtomicU32,
    len: AtomicU32,            // 0..=64
    payload: [AtomicU32; 16],  // 64 bytes
}
```

96 bytes, `#[repr(C, align(32))]` (matches `Slot`'s own alignment convention). `write(bytes: &[u8], quality, ts_src, ts_acq) -> u32` truncates silently to 64 bytes rather than panicking (`joker-core` denies `clippy::panic`); truncation is a last-resort safety net — the owning point's `len` (1–64, already validated at the manifest/plan layer, e.g. `joker-driver-i2c`'s own `Address::parse`) is the real guarantee. `try_read()`/`read(retries)` mirror `Slot`'s exactly, returning a `WideSample { bytes: [u8; 64], len: u8, quality, ts_src, ts_acq, seq }` with an `as_slice()` accessor.

### Registry wiring: a derived table, not a new `SignalMeta` field

Every signal already gets a dense `SignalId` (byte order of canonical paths, deterministic across targets) via `SignalMeta` (which already carries `ty: ValueType`). Do **not** add a field to `SignalMeta` itself — that's a public, `const fn`-constructed struct used as array literals in `no_std`/MCU builds; adding a field breaks every existing call site and fixture for no benefit.

Instead: a **derived**, separate table, `SignalId → Option<WideSignalId>`, built once when the registry/plan is finalized (not per tick):

- Linux (`alloc`): computed once in `OwnedRegistry`'s constructor, as a `Vec<Option<u32>>`.
- MCU (`no_std`): a fixed-size array filled by one pass over `signals()` in dense order — the same shape of computation that already assigns the dense `SignalId` itself (counting-in-order, deterministic, no runtime dependency on manifest text).

`WideSignalId` is dense **only** among wide-typed signals (0, 1, 2, … in canonical order, skipping every scalar signal) — so `shared.wide_slots: Box<[WideSlot]>` is sized exactly to the number of wide signals actually declared, zero for a manifest that declares none.

This same table and the same `WideSlot` type serve **both** directions (input and output) and **both** owners (device signals and, separately allocated, app `Internal<T>` fields — see §4): a signal is a signal, whether an app or a device produces its value.

## §2 — Device input (unblocks AUR-401's i2c read; the first issue to land)

`jokerd::drivers::run_driver`'s settle loop, today:

```rust
let Some(bits) = raw_to_bits(&s.value, target.ty) else { continue }; // wide: always skipped, success or failure
if let Some(slot) = shared.slots.get(target.signal.index()) {
    slot.write(bits, s.status.into(), NO_TS_SRC, s.ts_hw.unwrap_or(now));
}
```

Changes:

- `PointTarget` (already resolved once per device, not per tick) gains `wide: Option<WideSignalId>`.
- In the settle loop: if `target.ty` is `Bytes` or `String` (not `Record` — no device-input signal is ever a `Record` in the spec; that's `Internal<T>`-only), write to `shared.wide_slots[target.wide.unwrap().index()]` instead of `shared.slots[...]`. A **failed** read still writes — `len = 0`, the error `Quality` — exactly matching what the scalar path already does on failure. This is the precise bug found on the bench Pi: today neither success nor failure ever writes, because both fall through the same `None` from `raw_to_bits`.
- `GET /v1/values`/`jk read` (`joker-api`): when a signal's `ty` is wide, read from `wide_slots` instead of `slots`, and serialize per `ref/00` §4.1's own table (already normative, not a new decision): `bytes` as base64, `string` as a plain JSON string (UTF-8 validated at the point where it was written, not at every read).

## §3 — Device output (arbiter-side only; driver delivery stays blocked on ADR 0047)

An output is a signal too, so it gets the same `WideSignalId` as any wide input (§1). The key simplification: **the applied wide value for an output is just the current content of its own `WideSlot`** — the arbiter writes there on `commit()`, exactly the way a driver writes there on a settled read. `Applied` (today `Value(u64) | Relinquish`) gains a third variant, `Wide` — a bare marker, no inline payload, so `OutputState` (which embeds `last: Applied` directly, by value, on the arbiter's hot path) does not grow by 64 bytes for every output, including the scalar majority.

Concretely:

- **`accepts()`**: unchanged signature (still scalar-only, `bits: u64`). A new, parallel `accepts_wide(&self, bytes: &[u8]) -> bool` — `len() <= 64`, plus valid-UTF-8 if the output's type is `String`.
- **Envelope (clamps, interlock, `reset_when`)**: clamps (`lower`/`higher`/`equal`) are a numeric concept with no meaning for `bytes`/`string` — evaluation is skipped for a wide output (trivially "in bounds"). Interlock and `reset_when` are conditions over *other* signals, not the output's own value, so they are unaffected and still apply. Manifest validation gains a new diagnostic rejecting a declared `lower`/`higher`/`equal` clamp on a wide-typed output (a config error, not a silent no-op) — its exact code number is assigned at implementation time from the registry (`docs/16` §9), the same way every other diagnostic code already is; not a design decision to make now.
- **Manual write (`jk write`, AUR-266's persisted `ManualRecord`/`manual.json`)**: a wide manual write persists its actual content, encoded the same way `GET /v1/values` already does (§2: base64 for `bytes`, a plain string for `string`) — not an index, since indices are not stable across a restart. Restoring re-injects the stored value as an ordinary `Proposal`, exactly like a scalar manual write restores today; the proposal's win still settles into the output's own `WideSlot` through the normal commit path.
- **Decision log (`DecisionRecord`)**: stays the fixed-size, `Copy`, no-allocation-on-the-tick-path struct it is today. A decision about a wide output's commit does **not** inline the bytes — it records the `WideSlot`'s own sequence number at that commit (one `u64`, fits the struct's existing fields). A reader correlates "this decision ↔ `wide_slots[idx]` at sequence N" out of band — the same correlation-by-sequence idea AUR-268's (not-yet-built) causal tracing already proposed for a different problem; this reuses that shape rather than inventing a second one.

### Non-goal, explicit

Delivering a wide value to a **real driver** (`joker_hal::CommitBox`/`RawEncoding`, which has no `Bytes` variant) stays exactly where it already was: blocked on ADR 0047's still-open "output stage" (the same boundary that keeps `modbus`'s own outputs to FC 5 coils, and that `joker-driver-i2c`'s own docs already call out for multi-byte writes). This design gets a wide value as far as the arbiter's applied state and the decision log — inspectable via `GET /v1/outputs/{path}` — not into hardware. That is separate, pre-existing, already-scoped future work, not something this design silently expands into.

## §4 — App `Internal<T>` (smallest of the three; reuses §1's primitive wholesale)

`docs/04-sdk.md` already specifies the shape: "`Internal<T>` ... last value (wide slot ≤ 64 B), seqlock; the cycle reads from the latch" (`writer = "async"` default, or `"cycle"`). `joker_host::meta::PortKind::Internal` exists in the enum today but is matched nowhere — fully unimplemented.

No new storage mechanism: each `#[internal]` field an app declares gets **one `WideSlot`** (§1's exact type), owned by that app instance (not `shared.wide_slots` — that table is for device/output signals only). Exactly one side writes (`async` or `cycle`, per the macro's `writer` argument; never both), the other only reads — already the single-writer/wait-free-readers shape `WideSlot` (and `Slot`) are built for, with zero changes to the primitive itself.

- `PortEntry` (today: `param: u64` for scalar ports) gains a parallel `internal: Option<InternalHandle>` pointing at the instance's own `WideSlot`, resolved once when the app instance is assembled — the same timing as `signal`/`slot` are resolved today for other port kinds.
- `Published<T>`/`Counter` (other `PortKind` variants, telemetry) are unrelated and out of scope here.

## Testing strategy (every §)

- `WideSlot` itself: the same style of test `Slot` already has (write-then-read, concurrent-readers-never-see-torn-samples, a `loom` model) — reusing `Slot`'s own proof structure, not inventing a new one.
- §2: a conformance-style test with a double driver that returns `RawValue::Bytes` (success and failure), asserting the signal's slot — now `wide_slots` — actually advances past `seq=0`. This is the exact gap the bench-Pi i2c deployment found; it should have a regression test from day one.
- §3: a manual wide write through the real HTTP API, asserting it settles into the output's `WideSlot` and that a persisted-manual-write restart test (mirroring AUR-266's existing `a_manual_write_survives_a_jokerd_restart_within_its_ttl`) restores it.
- §4: an app with one `#[internal(writer = "async")]` field, asserting the cycle side reads what the async side wrote on the next tick.

## Linear issue breakdown

- **Issue A (§1 + §2):** `WideSlot` primitive, registry wiring, device input settle path, `jk read`/`GET /v1/values` serialization. Unblocks AUR-401's i2c read end to end on the bench Pi.
- **Issue B (§3):** device output wiring (arbiter, manual write, decision log). Explicitly stops at the arbiter's applied state; does not touch `joker-hal`/ADR 0047.
- **Issue C (§4):** app `Internal<T>`, reusing Issue A's `WideSlot` type with no changes to it.
