# ADR 0072 — Every output envelope key takes effect in `jokerd`: step 6, `verify`, `keepalive` and `pulse`

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ref/01 §5.10 lists the keys of an output's envelope, and AGENTS.md invariant 4 (and ADR 0028) does not let a declared key be read and dropped. A review of `jokerd::plan` (AUR-424) found that the plan built each `OutputConfig` without six of them:

1. `deadband`, `min_interval` and `max_actuations` (step 6, docs/16 E8): the core implements them, and the conformance area `envelope.suppress` covers it, but the daemon never passed them, so a relay declared with `max_actuations = "20/h"` actuated without a budget.
2. `verify` (docs/01 §7): nothing compared an output with its readback.
3. `keepalive` on an output: only the device's `fallback.keepalive` (AUR-404) renewed the equipment fallback; the output's own period was ignored.
4. `pulse`: ref/06 §2.9 says the core generates two `set` writes, the value and then `safe`, and the driver does not know what a pulse is; no part of the runtime did.

Three of them also left their exact semantics open: what "does not confirm" means over time for `verify`, whether an output `keepalive` may appear on an output its device fallback does not cover, and what a `resync` or a `keepalive` sends on a pulse output after the pulse has ended.

## Decision

### Step 6

1. `deadband` (a difference, converted to the output's unit), `min_interval` and `max_actuations` reach the core's `OutputConfig` unchanged. Their semantics are those of docs/16 E8 and ADR 0038.

### `verify`

2. **Resolution, at plan time.** `readback` must be an input. Numeric readbacks are converted from their unit to the output's (same dimension); a `bool` output needs a `bool` readback, an `enum` output a readback with the same variants, in the same order. `tolerance` is a difference in the output's unit, required on an `f64`/`f32` output; without it the comparison is exact. `within` defaults to twice the `poll` of the readback's device (1 s without a declared one) and may not be shorter than one `poll`. A violation of any of these is `J0235`, and the manifest is not loaded.
3. **Evaluation.** After each commit of its owner group, the output's applied value is compared with the latest sample of the readback. The readback **agrees** when it is `good` and within `tolerance` of the applied value (after the unit conversion). When the readback has disagreed for `within` or longer without interruption, the output's slot becomes `uncertain(not_confirmed)`, with its applied value unchanged. The first tick on which it agrees makes the slot `good` again; this also ends the `uncertain(not_confirmed)` of a `device-rejected` ack (ref/06 §2.9, ADR 0065). A new applied value starts a new window. An output whose applied value is `relinquish` is not verified.
4. The evaluation runs in the cycle thread, on memory allocated before the first tick (AGENTS.md invariant 6). It never changes the applied value and never writes to the driver.

### `keepalive`

5. The output's `keepalive` replaces the device's `fallback.keepalive` (default 1/5 of the profile's fallback timeout) as the period at which the io task renews that output. It is only valid on an output its device fallback covers (`[fallback] applies_to`, ADR 0064 §10) and must be below the fallback's timeout; otherwise it is `J0276` (`error-in-prod`, like the device key). An output that is not covered is never renewed.

### `pulse`

6. `pulse` is done by the output stage of the device's io task (ref/06 §2.9, §3.6): when a `set` write of a value other than `safe` goes to the driver, the stage writes the declared `safe`, as a second `set`, once `pulse` has elapsed. A `set` of `safe` (or a `force-safe`) before then ends the pulse without a second write.
7. Outside a pulse, a `resync` or a `keepalive` of a pulse output writes `safe`, the state the equipment rests in, never the pulse value: a reconnection or a fallback renewal does not pulse the equipment again (docs/18 P3).
8. The applied value in the core is the writer's, unchanged by the pulse: the next pulse needs the applied value to change (back to `safe` and away from it again). `CommittedWrite`s keep their two sources only (the applied value and the declared `safe`): `joker_hal::CommitRx::safe_write` builds the second write of a pulse from the box's `safe`.

### What is not covered

9. The runtime alarms of ref/01 §5.15 (`joker.output.<s>.budget`, `joker.output.<s>.unconfirmed`, and the rest of that table) are not raised by this runtime yet: none of them is. The outcome `Rejected(Budget)` is in the decision log and the quality `uncertain(not_confirmed)` is on the output's signal, which is what an app, the API and the historian see; the alarms come with the runtime alarm stage, as a separate change.

## Consequences

- `jokerd::plan::OutputPlan` carries every 1.0 key of ref/01 §5.10, or the key is consumed by the manifest checks (`safer`, `accept_missing_fallback`, `widen`) or refused (`remote_levels` on Linux, `J0145`; the 1.1 keys, `J0290`). A test of `jokerd` reads the key table of ref/01 §5.10 and the manifest schema, and fails when a key appears there without an effect.
- ref/01 §5.10 states the `keepalive`, `pulse` and `verify` rules above and points here; ref/06 §2.9 adds rule 7 to its pulse bullet.
- `joker-hal` gains `CommitRx::safe_write` and `CommitRx::is_safe`; `jokerd::shared::Shared::keepalive` becomes `Shared::stages`, the output stage of each output (`covered`, `keepalive_ns`, `pulse_ns`).
