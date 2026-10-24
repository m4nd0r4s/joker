# ADR 0060 — Action permits fail closed; step conditions honour quality

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ref/01 §5.8.2 makes `permit` an `Expr<bool>` evaluated before each invocation, with `Uncertain` counting as false, and `joker-manifest` type-checks it (`J0304`). ADR 0058 places the step interpreter in `joker-hal` and adds the `action` decision record. Three gaps remained between those texts and the runtime:

1. **A permit that does not compile against the running plan.** The manifest checks a `permit` against its own path registry; the runtime compiles it again against the plan's `SignalId` space. Nothing said what happens when the second compilation fails, and the runtime treated such a permit as not declared, so the action was always permitted. A permissive that silently becomes `true` is the opposite of a permissive.
2. **Enum operands.** The canonical example of ref/01 §5.8.2 (`pcs.state == PcsState.fault`) compares an enum signal with an enum constant. ADR 0034 types that comparison; the runtime could not resolve enum names, so the example did not compile there.
3. **Quality in step conditions.** ref/03 §2.2.12 says `until` "reads until the value matches" and `fail_if` "ends with `Err(message)` if the value matches", without saying what a `Bad` or `Uncertain` read means. document 16 §3.2 already fixes the rule for expressions (the result has the worst quality of the operands; only `Good` permits), and invariant 4 forbids turning a `Bad` sample into a number.

## Decision

### Permits

1. A declared `permit` is compiled against the generation's plan when the plan is built. If it does not compile, the generation **does not load**: it is a validation error carrying the expression's own registry code (`J0301`–`J0304`, ref/01 §10.1), rendered with the device and action it belongs to. No new code is introduced.
2. At invocation, a declared `permit` with no compiled program (it cannot happen after rule 1, but the runtime does not assume it) **denies** the invocation: the dispatcher sees "not permitted", and the `action` record of ADR 0058 is written with `outcome = "rejected_not_permitted"` and `failure` naming the cause. The same holds when the evaluation itself fails. An action without a declared `permit` is always permitted, as ref/01 §5.8.2's default `"true"` says.
3. Enum signals and enum constants (`Enum.variant`) resolve in the runtime exactly as in the manifest: the enum names are those of the profile or class that declares the signal, and the numeric value of a variant is its position in the declared list. `AlarmState` is always known.
4. A permit's program keeps its state arena (windows, `held`, `rose`, `fell`, `hyst`) for the whole generation: each evaluation continues from the previous one instead of starting from the initial state. The state is discarded with the generation.

### Step conditions

5. The value a step compares (`until`, `fail_if`) or stores (`read`) is a sample with a quality, and the condition takes that quality (document 16 §3.2):
   - **`Bad`**: the condition is `Bad` and the action ends with `device(error)` (ref/06 §2.10), the error naming the quality reason. The raw value is never compared.
   - **`Uncertain`**: `fail_if` ends the action the same way (a condition that cannot be trusted does not let the program go on); `until` does not count it as a match and keeps polling until a `Good` match or its `timeout`.
   - **`Good`**: the comparison decides, as before.
6. A `read` step's value enters the result record under `into` together with its quality; an `Uncertain` value is kept with its flag, a `Bad` one ends the action as in rule 5.
7. A failure the interpreter cannot resolve (a non-scalar raw kind, an unresolved `$arg`) stays `unsupported` (ref/06 §2.10); it is never reported as a communication failure.

## Consequences

- ref/01 §5.8.2 and ref/03 §2.2.12 state rules 1–2 and 5–7 in their tables.
- `jokerd` validates permits in `load` and denies on a missing program in `Controller::invoke_action`; `joker-hal`'s interpreter gains a quality failure and a sink for `read` results.
- The `action` record keeps ADR 0058's shape; a denied invocation may now carry `failure` with `outcome = "rejected_not_permitted"`.
