# ADR 0064 — The profile acquisition stage of 1.0, and profile keys this runtime refuses

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ref/03 §2.2 defines the profile format and §3 the acquisition order (decoding, `invert`, `invalid`, `scale`, unit, NaN, `limits`, `enum`, `deadband`, `stuck_detect`); ref/06 §2.7 and §3.6 place that stage in the core, after each `read`. The generic profile loader of AUR-414 read only the subset the manifest checks needed, and a review of it found that everything else was **read and dropped without a word**:

1. `scale` and `offset` were never applied, and an integer raw type with `scale` or a unit was typed as an integer signal, against ref/03 §2.2.7.
2. `[defaults]` was read for `poll` and `timeout` only: a `word_order`, `byte_order` or `fc` declared there never reached the points (§2.2.3).
3. `[enums]` kept the variant names and dropped their integers, so a raw value could not be mapped to its variant.
4. `limits`, `stuck_detect`, `[coherent]`, `[connect]`, `[relinquish]`, the register forms of `[fallback]`, `native` actions and others were accepted and ignored. A profile that says "this value is bad outside these limits" or "write this at every connection" was loaded as if it did not say it.
5. `[fallback] applies_to` was ignored, so every output of a device with a fallback counted as covered (`J0205` was never raised for the others), and `[fallback] value` was never compared with `safe`.

ADR 0028 (stable = implemented) already answers what a surface that is specified but not implemented is: **reserved**, and refused with a "reserved for a future version" code. ref/03 §2.9 has that code, `J0807`.

## Decision

### What a profile may declare in this runtime

1. A profile is read strictly (ref/03 §2.1.1): an unknown key is `J0140`, a `_` key is `J0141`, an `x_` key is ignored. A profile with any error is **not loaded**; the manifest that refers to it then fails as for a missing profile.
2. These keys and forms of the 1.0 surface are **refused with `J0807`** until the runtime implements them, and each becomes accepted, without any other change, in the minor that implements it:
   - sections `[coherent]`, `[config]`, `[connect]`, `[relinquish]`, `[events]`, `[discovery]`;
   - `[profile]` keys `firmware` and `identity`;
   - point keys `quantity`, `scale_factor`, `bits`, `encoding`, `invalid`, `max_age`, `limits`, `stuck_detect`; the two-point `scale`; an `offset` written as a quantity; `signal_type = "f32"`; a `kind` other than `scalar`;
   - `[fallback]` keys `timeout_register`, `value_register`, `values`, `behavior`, `enable_register`, `enable_value`;
   - an **output** whose value would have to be converted to raw: one with `scale`, `offset`, `invert` or `enum`, or a float signal on an integer raw type. The output stage of ref/06 §2.7 (the inverse conversion, `round_ties_even`, `Rejected(Range)`) is not implemented; such an output would reach the driver unconverted.
3. These keys are accepted because they do not change any value the runtime produces: `label`, `description`, `readback`, `safe_hint` and `range_hint` (hints that ref/03 §2.2.4 says are never applied), and `monotonic` (its effects are the historian's aggregation and a counter metric; the quality of the signal is the same with or without it).
4. `[defaults]` admits the point keys marked "yes" in the default column of ref/03 §2.2.4 and the address keys of the family; every key of `[defaults]` is merged into every point, and the point's own key wins. `poll` and `timeout` in `[defaults]` are the **device defaults** (the poll of a point that does not declare one, and the deadline of a call), as the bench profile of document 23 uses them.
5. `[actions.<a>]` (`args`, `result`, `long`, `native`) and the action map of `[implements]` are read as ADR 0066 says; a `"$name"` in a step that is not in `args` is `J0821`, and so is a step that cannot be compiled (an unknown kind, a missing `value`, `into`, `equals`, `timeout` or `message`): a step is never skipped.

### The acquisition stage

6. After decoding (`word_order`, `byte_order`, `bit`, raw type), the stage applies, in this order: `invert` (a `bool`); the `enum` mapping, where the raw integer becomes the position of its variant in the declared list (the same position ADR 0060 gives an enum constant), and an integer without a variant is `bad(out_of_range)`; `scale` and `offset` (`raw × scale + offset`); a NaN or an infinity is `bad(nan)`; an integer that does not fit the signal's type is `bad(out_of_range)`; a value of the wrong form is `bad(config_error)`.
7. An integer raw type with `scale`, `offset` or a unit other than `1` is an `f64` signal (ref/03 §2.2.7); `signal_type = "i64"` or `"u64"` keeps it an integer when there is no `scale` or `offset`.
8. **Deadband.** A `good` value closer than `deadband` (in the unit of the signal) to the last value published is published as that last value, with its new acquisition time: the signal stays fresh for `max_age` without moving. Any other sample, and any sample that is not `good`, is published as it is and becomes the reference.
9. A point's own `poll` (a duration, or `"on_demand"` on a `param`, `J0817` elsewhere) wins over the device default.

### The device fallback

10. `[fallback] applies_to` lists the outputs the fallback covers, each an output of the profile (`J0820`); without it every output is covered. An output that is not covered has no device fallback for `J0205`, and its io task does not renew it with `keepalive`.
11. `[fallback] value` is compared with the `safe` of each covered output by its `safer` (docs/01 §8): equal with `"equal"`, not above it with `"lower"`, not below it with `"higher"`, after converting a quantity to the output's unit. A value that is not as safe, or that cannot be compared, is `J0205`.

## Consequences

- ref/03 §2.2.1 states rules 1–3 and points here; ref/03 §2.9 is unchanged (`J0807` already says "key reserved for a future version").
- `joker-manifest` (`profile::parse`, `ProfileInfo`, `SignalInfo::acquisition`) carries the acquisition steps; `jokerd::bridge` passes them to each point and `jokerd::drivers` applies them.
- The manifest-side adjustments of ref/01 §5.8.1 (`scale`, `offset`, `limits`, … in `[devices.<d>.signals]`) are not part of this decision and are still not applied by the runtime.
