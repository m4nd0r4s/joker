# ADR 0034 — Expression typing, two new codes, and the semantics of stateful functions

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 02 §10 gives the grammar and the functions of the expression language, document 16 §3.2 and §4 its quality and unit rules, and ref/01 §3.7 its error codes. Implementing `joker-expr` (document 13 §4, tasks 3–5) found questions they leave open:

- there is no code for a type error (`pump.run + 1`, `if meter.p then …`) nor for an unknown function or a bad argument (`frobnicate(x)`, `avg(x, y)`): `J0301` names dimensions only and `J0158` syntax only;
- a selector (`inv*.ac.p`) and a product without spaces (`a*b`) are spelled alike;
- `count(sel)`, `quality(x)` and `age(x)` have no defined result, and the affine rules of document 16 §4 do not cover `K + °C`, `-°C`, `abs`, `sum` or `rate` of a temperature;
- the windows of document 16 §3.2 fix the quality of the result, but not how samples are taken, what value a window has when some samples are `Bad`, what happens before the window is full, or what `rate` means;
- `held`, `rose`, `fell` and `hyst` have an initial state, but not what a `Bad` input does to the state.

## Decision

### Checking

1. **New codes.** `J0304` (error): an operand of the wrong type, two enum operands of different enum types, a signal of a type expressions cannot read (`string`, `bytes`, `record`), or an expression whose result type does not fit its key (an `interlock` that is not `bool`). `J0307` (error): an unknown function, a wrong number of arguments, or an argument that must be a signal path (`good`, `quality`, `age`, `own`), a duration literal (windows, `held`) or a constant (`bit` index, `pow` exponent of a dimensioned base).
2. **Other codes in expressions.** Same dimension and different quantity kinds (`meter.p > 5 kvar`) is `J0108`, as in bindings. A selector that matches nothing is `J0155`. A window or hold time that is not a valid duration, or is zero, is `J0152`. A unit outside the catalogue is `J0109`.
3. **Selectors are lexed only at the start of an aggregation argument** (`any`, `all`, `count`, `sum`, `maxs`, `mins`): a run of path characters and `*` with no spaces. Everywhere else `*` is multiplication, so `max(a*b, c)` is a product and `sum(a*b)` is a selector. A selector anywhere else is `J0158`.
4. **Literals.** A unit belongs to a number when it follows it directly or after one space (`5 %`, `20 kW/s`); the keywords `then` and `else` are never units. A number without a decimal point, exponent or unit is an `i64`; any other literal is an `f64`. In a window or `held` position the literal is a duration (ref/00 §3.1, `m` is the minute); elsewhere it is a quantity (`m` is the metre).
5. **Unit alignment.** Operands of `+`, `-`, comparisons, `if` branches, `min`, `max`, `clamp`, `hyst` and aggregations are converted to one unit at compile time: the unit of the first operand, except that a constant is converted to the unit of the signal it is compared with. `K` compared with `°C` is an absolute temperature.
6. **Affine units.** Besides document 16 §4: `K + °C → °C`; `K − °C`, `-°C`, `abs`, `sqrt`, `pow` and `sum` of `°C` are `J0301`; `min`, `max`, `clamp`, `avg`, `minw`, `maxw`, `maxs`, `mins` and `hyst` keep `°C`; `rate` of `°C` is in `K/s`.
7. **Functions.** `min` and `max` take two or more arguments. `quality(x)` is the quality code of ref/00 §4.3 (0 good, 1 uncertain, 2 bad) as a dimensionless integer; `age(x)` is in seconds; `count(…)` is the number of true elements. Aggregations take selectors and expressions, mixed. `bit(x, n)` takes an integer signal and a constant `n` in 0–63. `pow(x, n)` with a dimensioned `x` needs a constant integer `n`; `sqrt(x)` needs even dimension exponents. `rate(x, w)` is in the unit of `x` per second. `held` takes any positive duration (it keeps a timer, not a window).
8. **Integers.** The arithmetic is in `f64`; a comparison is exact only between two integers of the same signedness that need no unit conversion (document 16 §4).

### Evaluation

9. **Straight-line evaluation.** Every sub-expression is evaluated on every tick, both branches of an `if` and both sides of `&&` and `||` included; only the result is selected. So every stateful function sees every tick, whatever branch is taken, and an evaluation has a fixed cost.
10. **Values.** A non-finite input or result is `Bad(nan)`. An `if` whose condition is `Bad` is `Bad` with the condition's reason.
11. **Windows** (`avg`, `minw`, `maxw`, `rate`). A window takes one sample per evaluation of its expression (one per tick of the group that runs it), and holds `ceil(window / period)` samples. Each slot counts as one period of the window. Before the window is full, the missing slots count as `Uncertain(initial)`. The quality rule of document 16 §3.2 is applied to the slots. The value is computed over the samples that are not `Bad` (a `Bad` sample is never a number); with none, the result is `Bad`. `rate` is `(last − first) / (t_last − t_first)` over the samples that are not `Bad`, and `Bad(nan)` with fewer than two.
12. **`held(c, d)`** starts its timer on the first tick on which `c` is true (`Good` or `Uncertain`) and is true when `now − start ≥ d`; a false or `Bad` input restarts it. **`rose`/`fell`** compare with the last value that was not `Bad` (initially `false`); a `Bad` input gives `false` with `Bad` quality and leaves the state unchanged. **`hyst(x, lo, hi)`** starts `false`, becomes true above `hi` and false below `lo`; a `Bad` input leaves the state unchanged.
13. `good(x)`, `quality(x)` and `age(x)` are always `Good`; `age` is `now − ts_acq`, never negative.

## Consequences

- `spec/codes.toml` and the code table of ref/01 §10.1 gain `J0304` and `J0307`; ref/01 §3.7 and document 16 §4 point to this ADR.
- The window memory is `ceil(window / period)` slots per window: a 15-minute window in a 1 ms group is 900 000 slots, which the memory estimate (`J0380`) has to count.
- A window that should not see some ticks needs its own `calc`; an `if` does not stop it from sampling.
