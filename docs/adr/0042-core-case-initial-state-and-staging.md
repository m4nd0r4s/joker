# ADR 0042 — Initial state and staging of a core conformance case

**Status:** accepted (v1) · **Date:** 2026-10

## Context

The core conformance runner (document 13, §3, task 14) runs each case of ref/03 §2.6 against `joker-core` with a virtual clock, and the same cases must give the same bytes on Linux and on an MCU (ADR 0020, document 16, §7). Writing the runner showed three facts that the schema does not state:

- The clean state of a case (ref/03 §2.6.1) lists the generation, the outputs at `safe`, no active writer and the alarms in `normal`, but not the **clock quality** before the first `clock` action. Step 0 of the envelope (16 E1) depends on it.
- ref/03 §2.6.2 and document 16, §8, say that an `async` proposal enters at step 2 and a `cycle` one at step 4, but not where the proposals of `manual`, `bridge` and `impl` writers, nor a `release`, enter. The order decides which proposal wins within a tick (16 A4).
- `generation = "same"` swaps to an identical generation; 16 A8 fixes that the last applied value is inherited, but not what happens to the levels held in the arbiter, nor to the latched samples.

## Decision

1. **Clock.** A case starts with the clock `synced`. A `clock` action changes it from the latch of its tick on, until the next one.
2. **Staging.** The proposals and releases of `async`, `manual` and `bridge` writers enter at step 2 (they arrive through the inbox: an app's async instance, the API or CLI, a bridge); those of `cycle` and `impl` writers enter at step 4. Within each step, the actions keep the order in which the case lists them.
3. **Order inside a tick.** At the boundary of tick `t`, in this order: the generation swap of `t`, if any; the `clock` actions of `t`; the `input` samples of `t` (acquired at `t`); the `reset` actions of `t`; then the latch, and the proposals as in point 2.
4. **Generation swap.** The new generation starts with **no active level** (as at any startup) and with the last applied value of each output inherited (16 A8). The samples of the inputs are not touched: they belong to the signals, which the identical generation keeps. A case that needs a writer to keep control proposes again in the tick of the swap.

## Consequences

- The runner (`cargo xtask conformance`) and the conformance firmware of an MCU (ref/08 §3.2) take the same decisions, so the full result sequence of a case is the same on every host.
- A case that wants another initial clock quality says so with a `clock` action at `0ms`, as the cases of `conformance/envelope/clock.conformance.toml` already do.
- ref/03 §2.6.1 and §2.6.2 state the four points.
