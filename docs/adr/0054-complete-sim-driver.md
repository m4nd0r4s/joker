# ADR 0054 — The complete `sim` driver: what reaches it, the dynamics language, actions and the plant surface

**Status:** accepted (v1) · **Date:** 2026-10 · **Extends:** ADR 0051

## Context

ADR 0051 gave the `sim` driver an M0 subset of ref/03 §2.4. Completing it (document 13 §4, task 18) found questions that ref/01 appendix A, ref/03 §2.4 and ref/06 §2.10 leave open:

- The driver does no I/O (ref/06 §2.4), but a model can name more files than itself: the profile of `[model] profile`, the CSV of a `replay` generator and the component of `[plant]`.
- ref/03 §2.4.6 says the dynamics language "is that of 02 §10". `joker-expr` is the core's evaluator: it carries quality, samples once per tick of a group and has neither the three dynamics extensions nor a state that advances per step of virtual time. And `good`, `quality` and `age` read the quality of an acquired sample, which a plant value does not have.
- Nothing fixes how stateful calls update when one feeds another, how `delay` rounds to the step, what a window holds before it is full inside a plant, nor how units are checked in dynamics.
- `bounce` delivers "edges with an exact timestamp", without saying how many or when.
- A `set` step "fixes the value until the next `set` with `release = true`", without the form of that `set`.
- ref/06 §2.10 says the core runs the step language, but the simulated steps change model signals that only the driver has.
- `"*"` in `[faults]` is named only for `disconnect`.
- `replay` accepts Parquet, and `[plant]` needs a Wasm runtime, which no driver crate carries.
- A model's `limits` do not pass through a profile, so nothing says who marks `bad(out_of_range)`.
- More than 1,024 signals (ref/03 §2.1.5) has no code.

## Decision

### What reaches the driver

1. The core passes, in the device's `config`, besides `model` and `params` (ADR 0051): `profile`, the source text of the profile named by `[model] profile` (the one pinned in the lock); `files`, a map from each path as written in the model (`replay` `file`, `[plant] component`) to the file's bytes; and `seed`, the seed override of a scenario. A missing profile is `J0803`, a missing file is `J0831` (`replay`) or `J0837` (`[plant]`).
2. The seed is `config.seed`, else `[model] seed`, else the FNV-1a of the device name.

### Units and the dynamics language

3. Literals, units and conversions come from `joker-units` (the catalogue of document 16 §5). `amp`, `std`, `noise`, `resolution`, panel `step` and fault `offset` are differences, converted to the signal's difference unit (`"3 K"` on a `°C` signal is 3). Parameters with `unit` are checked against it (`J0836`).
4. The sim keeps its own evaluator of the whole grammar of document 02 §10: `|| && !`, comparisons, `+ - * /`, unary minus, `if then else`, booleans, `Enum.variant`, the model's signals and `param.<name>`; the functions `abs`, `min`, `max`, `clamp`, `sqrt`, `pow`, `bit`, `avg`, `minw`, `maxw`, `rate`, `held`, `rose`, `fell`, `hyst` and the aggregations over selectors of the model's own signals. `good`, `quality` and `age` are `J0833`; `own` is `J0306`. It moves to `joker-expr` when that crate has a dynamics mode; the semantics below are those of ADR 0034 where it gives one.
5. Typing follows document 16 §4 and ADR 0034 rules 5 and 6: a bare number takes the unit around it; `+`, `-`, comparisons, `if` branches, `min`, `max`, `clamp`, `hyst` and aggregations convert to the first unit; `K + °C` is `°C`, `°C − °C` is `K`, `°C + °C`, `K − °C`, `-°C`, `abs` and `sum` of `°C` are `J0301`. The entry converts to the signal's unit, and a mismatch is `J0301`. An unknown signal is `J0302`, an unknown variant `J0303`, a window, `delay` or `lag` above 15 min `J0305`, a selector without matches `J0155`; other type and structure errors are `J0833`.
6. Stateful calls hold their state in the signal's unit, starting at `init`: `integrate(d)` takes `d` in the signal's difference unit per second, `lag` and `delay` take `u` in the signal's unit. They may appear anywhere in the entry and may nest. After each step, every argument is evaluated with the old states and then all states change at once (explicit Euler, independent of order). `delay(u, t)` is `max(1, round(t / step))` steps.
7. Every sub-expression is evaluated on every step, both branches of `if` included (ADR 0034 rule 9), so a function with memory samples once per step. A window holds `ceil(w / step)` samples and, before it is full, is computed over the samples it has; `rate` is `(last − first) / (t_last − t_first)` over them, and 0 with fewer than two.

### Generators, signals and faults

8. `replay` reads CSV with a header row: `time_column` (default `t`) in seconds or as a duration literal, strictly increasing; values bare (in the signal's unit) or quantities. The value is that of the last row at or before `t`, the first row before it. With `loop`, the series repeats with a period from the first row to the end of the last, which lasts as long as the interval before it. Parquet is reserved (`J0807`).
9. Signals take the types of document 02 §2: `bool`, `i64`, `u64`, `f64`, `f32`, `enum = "<Name>"` with the model's or the profile's `[enums]`, `bits16`/`bits32`/`bits64`, and `string` and `bytes` (constant at `init`, changed by writes and `set` steps). Enum and bits values travel as `uint`, `string` as `text`, `bytes` as `bytes`. With `[model] profile`, a profile's named bits are `bool` signals `<path>.<name>` that follow their word.
10. A changed `bool` with `bounce` is delivered as 3, 5 or 7 edges (drawn from a stream of its own, seeded like the noise): the first at the step of the change, the others uniformly inside the bounce time. A read delivers the value after the edges that have happened and `ts_hw` = the last of them.
11. `latency` of a signal replaces `[model] latency` for it; a batch waits for its slowest point.
12. The driver applies the model's `limits` itself: a value outside them is delivered as `bad(out_of_range)`, and a non-finite one as `bad(nan)`.
13. `"*"` takes every effect: `disconnect` downs the device, the others apply to every acquired signal. Faults overlap when they share a signal, counting a non-`disconnect` `"*"` fault as on every signal (`J0834`). Faults on an `output`, and on `panel` as a signal, are `J0834`.
14. More than 1,024 signals is `J0811`, the code of a path collision in a profile.

### Actions and classes

15. The model's `[actions]` are **native actions** of the `sim` driver (capability `native-actions`): the core sends `invoke` with the `ActionSpec` name, and the driver runs `wait`, `fail_if` and `set` against the plant. Other steps are `J0821`; a `wait` beyond the timeout is `J0821`; the timeout of the manifest, when given, wins (document 17 §4.2). A `fail_if` that matches ends with `device` and its `message`.
16. Arguments arrive as the record of ref/06 §2.10, with integer keys by position or text keys by name; a number is in the unit declared in `args` (`"f64 K"`) and is converted to the signal's unit.
17. A `set` table with `release = true` releases the signals it names (their values are ignored): they go back to their generator, dynamics or `init`. A signal named `release` cannot be set.
18. `[implements]` (the profile's and then the model's) is checked against the model only: class reference grammar (`J0823`), targets that are signals (`J0101`) or actions (`J0824`), at most two levels of `members` and 16 classes; `instances` is `J0807`. The core checks it against the classes.

### The Wasm plant

19. The driver exposes the `joker:sim/plant` world as two traits, `PlantEngine` (`init`: instantiate a component with the `[plant] config` in deterministic CBOR and the seed) and `PlantInstance` (`step`). The node's Wasm host implements the engine and opens `SimDriver<Engine>`; the default build has none and refuses a model with `[plant]` at `open` with `J0837`.
20. `step(dt, inputs)` runs at every step after the first, with the applied outputs of `[plant] inputs`; its values go to `[plant] outputs` and through noise and resolution. An `Err` or a wrong number of values puts every signal of the device in `bad(device_fault)` until the device is opened again. An output of `[plant]` with a generator is `J0832`.

### Determinism

21. Arithmetic is IEEE 754 `f64` without fused operations, transcendental functions come from `libm` and random draws from per-signal SplitMix64 streams (ADR 0051). A golden hash of a model that uses every generator, function and fault is checked by the tests, and it is the same on x86_64 and aarch64.

## Consequences

- When ref/01 and ref/03 are next revised: appendix A gains the `config` keys of rule 1 (ADR 0051 asks the same for `model`); ref/03 §2.4.5 marks Parquet as reserved; §2.4.6 names the functions that are not in dynamics (rule 4) and the update of rule 6; §2.4.8 says what `"*"` takes; §2.4.9 gives the `release` form and says the actions are native; §2.4.10 points to rule 19; ref/06 §2.10 says that the `sim` steps are the exception to "the core runs the steps".
- `joker-host` owes a `PlantEngine` on wasmtime (Phase 1). Parquet replay is a follow-up.
- Two evaluators of the expression grammar exist until `joker-expr` grows a dynamics mode; the sim one follows ADR 0034 wherever it applies.
