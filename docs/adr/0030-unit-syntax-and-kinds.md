# ADR 0030 — Unit syntax, quantity kinds in compounds, and literal errors

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 16 §5 lists the v1 catalogue (atoms, prefixes, quantity kinds) and says compounds are "products and quotients" of catalogue units, with examples (`kW/s`, `%/s`, `m3/h/%`, `°C/min`). Implementing `joker-units` (document 13 §4, tasks 1–2) needs answers the specification does not give:

- the exact syntax of a compound (operators, exponents such as `m2` and `s2`, grouping);
- which quantity kind a compound carries, so that `J0108` can be checked on rates (`25 kvar/s` against an output in `kW`);
- how `%` stays apart from `1` while still being dimensionless (document 02 §3);
- which code a malformed quantity literal (`"1.kW"`) gets, since `J0151` only names a bare number and `J0109` only an unknown unit.

## Decision

1. **Unit syntax.** `unit := term (op term)*`, left-associative, with `op` one of `/`, `*` and `·` (U+00B7). `term := atom-with-prefix exponent? | "(" unit ")" exponent?`, where `exponent` is a positive decimal integer written right after the term (`m3`, `m/s2`, `(m/s)2`). No spaces inside a unit. `1` is an atom (`1/s`). The micro prefix is accepted as U+00B5 and U+03BC; the ohm as U+03A9 and U+2126. An exact atom wins over a prefixed reading (`min` is the minute, `mol` the mole, `cd` the candela).
2. **`%` is its own dimension axis.** The dimension vector has eight exponents: the seven SI base dimensions and `%`. So `%` and `1` never convert into each other implicitly (`J0107`), and `%/s` or `m3/h/%` keep track of it. The explicit `as = "fraction"` binding divides by 100 and removes the axis.
3. **Quantity kinds in compounds.** A catalogue atom carries its kind (`W` active power, `var` reactive power, `VA` apparent power, `Wh` and `J` energy, `varh` reactive energy, `°C` temperature). `K`, `Nm` and the other atoms carry none; a signal may declare one. Dividing by a pure time unit keeps the kind, except that energy per time is power (`kWh/h` and `kJ/s` are active power, `kvarh/h` reactive power) and an absolute temperature per time is a temperature difference. Any other product, quotient or power has no kind. Two units are incompatible (`J0108`) only when both carry a kind and the kinds differ.
4. **`°C` inside a compound** is a temperature difference (`°C/min` is `K/min`, as ref/01 §3.3 already reads it). A `°C` raised to a power or with a prefix is `J0109`.
5. **Literal errors.** A unit outside the catalogue, a prefix on an atom that takes none, malformed unit syntax or a symbol longer than 32 bytes is `J0109`. A malformed or non-finite number in a quantity literal is `J0151`, like a missing unit. A malformed duration is `J0152`.
6. **Durations.** Decimal digits are rounded to the nearest nanosecond, halves away from zero. The canonical form uses `us` for microseconds.

## Consequences

- `joker-units` implements this grammar; `jk check` diagnostics for units follow these codes.
- Adding an atom or a kind stays a patch change in `joker-units` (document 16 §5).
- A future unit that needs a kind on a non-time compound (for example specific energy) needs an ADR that extends rule 3.
