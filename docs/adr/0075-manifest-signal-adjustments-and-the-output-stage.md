# ADR 0075 — Manifest signal adjustments take effect, and the output stage converts values to raw

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0064 left two gaps (AUR-425):

1. The manifest's adjustments of a device signal (ref/01 §5.8.1, `[devices.<d>.signals.<s>]`, and §5.9, `[signals."<path>"]`) were checked and then dropped by the runtime: `scale`, `offset`, `unit`, `limits`, `deadband`, `stuck_detect`, `debounce` and `max_age` never reached the acquisition stage, and only `alias`, `poll` and `record` had an effect. ref/01 says the manifest's `scale` applies "after the profile's", but not how it composes with `offset`, `unit` and a profile `deadband`.
2. An output whose value must be converted to raw (`scale`, `offset`, `invert`, `enum`, or a float on an integer raw type) was refused with `J0807`, because the output stage of ref/06 §2.7 did not exist. ref/06 §2.7 asks for a non-representable write to be `Rejected(Range)` at runtime, but the core's envelope has one range step and no notion of a raw type.

## Decision

### Composition of the adjustments

1. The adjustments of one signal are the keys of its `[devices.<d>.signals.<s>]` table and of every `[signals."<path>"]` entry whose path, alias or selector reaches it (two different values for one key stay `J0175`). One function of `joker-manifest` (`adjust::adjust`) composes them; the check and the runtime both use it.
2. With the profile's `raw × s₁ + o₁` in the profile's unit:
   - the manifest's `scale = k` and `offset = b` give `(raw × s₁ + o₁) × k + b`, still in the profile's unit (`b` a number, or a quantity of that dimension);
   - `scale = { from, to }` maps `from` (in the profile's unit) linearly onto `to`, whose unit becomes the signal's; with `offset` it is `J0145`;
   - `unit` then converts the result to another unit of the same dimension (`J0107` otherwise).
   The whole chain folds into one `raw × scale + offset` in the signal's final unit, which is what the acquisition stage applies and what the output stage inverts.
3. An integer signal that gains a manifest `scale` or `offset` becomes a float, as an integer raw type with a profile `scale` does (ref/03 §2.2.7).
4. `limits`, `deadband`, `stuck_detect` and `debounce` are in the signal's final unit and replace the profile's. A profile `deadband` that the manifest does not replace follows the manifest's `scale` and `unit`.
5. `max_age` replaces the default of 3 × `poll`.
6. Steps added to the acquisition stage, in the order of ref/03 §3: `limits` after the NaN check (`bad(out_of_range)`); `stuck_detect` compares the acquired value, before the deadband, so a value that moves inside its deadband is not frozen, and marks a `good` value unchanged for the window `uncertain(frozen)`; `debounce` follows docs/16 §3.4 (D1–D5) for every `bool` the io task reads by poll.
7. A manifest adjustment that does not fit (any of the codes of ref/01 §5.8.1) stops the plan with its code; a `[signals."<path>"]` key the check could not see on the device is caught there.

### The output stage

8. The output stage of each device's io task converts every write of a point with a numeric or `bool` raw type: `invert`; the raw integer of an `enum` variant; `(v − offset) / scale` with the folded `scale` and `offset` of rule 2; rounding to the nearest integer with ties to even, computed the same way on every target; the range check of the raw type; and, on a register point, the encoding into its registers with `word_order` and `byte_order`. Other points keep the typed value (`int`, `uint`, `float`). The conversion lives in `joker_hal::encode`, so a converted write is still built only from an applied value or the declared `safe`.
9. **Where `Rejected(Range)` comes from.** The plan computes, for each converted output whose raw type bounds its values, the engineering interval the raw type can carry:
   - an output without a `range` gets that interval as its `range`, with `on_range = "reject"`, so the envelope rejects what does not fit with `Rejected(Range)`, in the decision log, and nothing reaches the driver;
   - a `safe`, a constant `range` or an `enum` integer outside the raw type is `J0858`, an error at plan time;
   - an expression `range` on such an output is `J0858` too, because its bounds cannot be checked before they are evaluated.
   This is stricter than ref/06 §2.7 (a warning for a non-representable `range`, then a runtime rejection), and it is what lets the output stage never meet a value it cannot write. If it does (a defect), it drops the write, reports it and leaves the output `uncertain(not_confirmed)`; it never saturates.
10. A `bool` output in a register bit, which would need a read-modify-write, is still refused (`J0807`).
11. The `J0807` refusal of converted outputs (ADR 0064 §2, last item) is withdrawn.

### What is not covered

12. The jokerd `sim` device publishes only the readbacks of its own outputs, a copy of the applied value: it has no acquisition stage, and the adjustments of its signals are validated (rule 7) but have no value to act on. The profile keys `limits` and `stuck_detect` stay refused (`J0807`, ADR 0064): only the manifest's are read.

## Consequences

- ref/01 §5.8.1 states rules 2–6; ref/03 §2.2.1 drops converted outputs from the refused list; ref/06 §2.7 states rule 9.
- `joker_manifest::context::Acquisition` gains `limits`, `stuck_detect` and `debounce`; `joker_manifest::adjust` composes the adjustments; `jokerd::bridge::device_plan` and `device_plans` take the path registry, and `jokerd::shared::OutputStage` carries each output's `OutputConversion`.
