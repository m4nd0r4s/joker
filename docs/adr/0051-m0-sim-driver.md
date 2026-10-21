# ADR 0051 — The M0 `sim` driver: how the model reaches it, the format subset and its time

**Status:** accepted (M0) · **Date:** 2026-10

## Context

The `sim` driver (ref/01 appendix A, ref/03 §2.4) runs the learning track's parts 1–5 on a laptop. Writing it showed three gaps:

- ref/01 appendix A gives `sim` the device key `model` (a `File`), and ref/06 §2.4 says the driver never reads configuration files and `open` does no I/O. Nothing says how the model's content reaches the driver, nor what the address of a `sim` point is (the column "per-signal keys" is empty: the model declares the signals).
- ref/03 §2.4 is the whole model format, including the full expression language and units of document 16, which `joker-expr` and `joker-units` do not provide to the driver yet.
- ref/03 §2.4.2 fixes the integration step in virtual time, but not where step 0 is, how noise streams are drawn, nor what a momentary key does once its pulse ends.

## Decision

### Model and points

1. The core reads the model file when it compiles the generation (it needs the model's signals for the registry anyway) and passes its **source text** to the driver in `device-spec.config`, under the key `model`, in place of the path. `params` is passed as the CBOR of the device's `params` table. Equal files give equal bytes, so `plan` stays deterministic.
2. The address of a `sim` point is `{ signal = "<name>" }`, the signal's name in the model. `plan` parses the model and refuses an unknown signal (`J0853`) and a point whose direction or raw kind differs from the model (`J0809`). Raw kinds: `bool` → `boolean`, `i64` → `int`, `u64` → `uint`, `f64` and `f32` → `float`.
3. The descriptor's `protocols` is `["sim"]` and its only bus kind is `none`.

### The M0 subset of the format

4. Supported: `[model]` (`description`, `step`, `latency`, `seed`), `[params]` with `$name` substitution and `param.<name>`, `[signals]` of types `bool`, `i64`, `u64`, `f64` and `f32` with `init`, every generator except `replay`, `noise`, `resolution` and `panel`, `[dynamics]` and `[faults]` (scheduled faults and panel fault keys).
5. Accepted without effect: `bounce`, a signal's `latency`, `limits`, `label`, `description`. Refused with `J0807`: `[model] profile`, `replay`, `enum`, `[plant]`, `[actions]`, `[enums]`, `[implements]`.
6. `[dynamics]` takes numbers with units, the model's signals, `param.<name>`, `+ - * /`, unary minus, parentheses, `abs`, `min`, `max`, `clamp`, `integrate`, `lag` and `delay`; anything else is `J0833`. Stateful calls cannot be nested.
7. Units: a literal in the signal's own unit is taken as is; durations convert between themselves and to seconds; `K` converts to `°C` as a difference (`45 °C + 30 K`, document 16 §4). Any other conversion is an error in M0. This is replaced by `joker-units` when the driver moves to it.

### Time and determinism

8. Step 0 is the instant of the first `connect`; step `k` is at `t0 + k · step`. A `read` or `write` first runs every step up to the host's `now` (ADR 0007), so the plant follows a virtual clock in tests and replay and the real one in `jk dev`.
9. Within a step: generators and constants, then the stateless dynamics in dependency order, and the stateful calls are updated after the step, in path order (ref/03 §2.4.6).
10. Each signal draws from its own SplitMix64 stream, seeded with `seed ^ FNV-1a(signal name)`; Gaussian values use Box–Muller. Transcendental functions come from `libm`, so the same model, clock and writes give the same bits on x86_64 and aarch64. `resolution` rounds ties to even.
11. A momentary key holds `true` for `pulse` and then hands the signal back to its generator or `init`. A slider replaces the generator until released; noise still applies.

### Faults

12. A disconnected device (`"*"` or a panel `disconnect`) fails every `read` with `comm-failure` and every write with `not-connected`, so the core shows `bad(comm_failure)`. A `bad(…)` quality fault delivers no value; an `uncertain(…)` one keeps delivering the value the signal had when the fault started (the last good one, ref/03 §2.4.8).

## Consequences

- `jk dev` and scenarios drive the panel through `SimHandle` (keys, `set`, `release`), on the same plant the driver reads.
- When ref/01 and ref/03 are next revised, appendix A must say that the core passes the model's content under `model`, and the per-signal key `signal`.
