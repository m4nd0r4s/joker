# ref/01 — The manifest (`joker.toml`) and overlays

Complete reference of a site's manifest: all the sections and keys, the exact grammar of the values, the merge rules of the overlays, the effective manifest and the registry of the validation codes `J01xx`, `J02xx` and `J03xx`.

It is **normative**. It sits above document 03, of which it is the closed form (ADR 0023). The semantics (arbiter, envelope, quality, expressions, units, paths) stay in document 16, and the common representation (versions, names, time, values, errors) in `ref/00`. Where this document and 03 diverge, this one prevails, and the divergence is listed in the findings (appendix C) so that 03 is corrected.

The Rust types of the `joker-manifest` crate are written from this reference (§12), and the published JSON Schema is generated from them (document 16, §10).

---

## 1. Scope and sources

### 1.1 What it covers

| Covers | Does not cover (and where it is) |
|---|---|
| `joker.toml` and all the overlays (`joker.sim.toml`, `joker.rpi.toml`, `templates/joker.*.toml`, `sites/joker.*.toml`) | `joker.lock`, node startup configuration, `jk` client, secrets vault → `ref/02` |
| the grammar of the values written in TOML (durations, quantities, rates, expressions, selectors, references) | equipment profiles, board profiles, simulation models, scenarios, conformance cases → `ref/03` |
| the resolution order of the overlays and the effective manifest | the `.jkc` IDL → document 17 |
| the device keys of each driver of the v1 catalog (appendix A) | how the driver receives that configuration (`Config`, `plan`) → `ref/06` |
| the validation codes of the manifest (`J01xx`, `J02xx`, `J03xx`), existing and new | codes `J04xx` (lock), `J05xx` (ML), `J06xx` (classes), `J07xx` (Joker Micro): only referred to |
| the keys reserved for 1.1 (ML, streams, pipelines, blocks, scheduled writes) | the semantics of those features → document 06 |

### 1.2 Sources

| Document | What it gives this reference |
|---|---|
| 03 | the original list of sections and keys, the examples, the codes of §15 |
| 16 | arbiter (A1–A11), envelope (E1–E9), quality, expressions, units, paths (P1–P5), code registry |
| `ref/00` | versions, names, duration and quantity literals, values, levels, errors, security, limits |
| 01, 02 | tick, step-by-step envelope, device fallback, generations, time; types, quality, paths, expressions, alarms, actions, atomics |
| 05 | drivers, profiles, buses, discovery, bus budget |
| 14, 17 | instances (implicit, composite, virtual), `map` and adapters, instance actions |
| 04, 06, 19, 20 | ports and bindings, ML reservations, Joker Micro, family security |
| 08, 09, 18, examples, track | all the TOML blocks used as a coverage test (appendix B) |

### 1.3 Links to other references

| Reference | Contract with this one |
|---|---|
| `ref/02` | the lock stores `widen`, `accept_missing_fallback` and `accept_unsafe_reset` with the keys from here, and pins `[profiles]`, `[boards]`, `[drivers]`, `[discovered]` and `[fallback]` by the keys of the devices, profiles, boards and drivers declared here; the node's trust bundle is what `[security.trust]` can only restrict; the vault resolves `secret:` |
| `ref/03` | the profiles define the signals that `[devices.*.signals]` tunes; the boards give `recommended`, CPU cores, RAM, architectures and, on MCUs, the default capabilities; the class simulator interprets `[instances.*] sim` |
| `ref/04` | the `joker:meta` section gives the ports, parameters, tasks, secrets and `optional` against which `bind`, `params`, `secrets` and `cycles` are validated |
| `ref/06` | each driver publishes the schema of its configuration in the descriptor; for the catalog drivers, that schema is appendix A; the keys of `[drivers]` (§5.7) configure the W and X hosts |
| `ref/07`, `ref/08` | the keys of the `joker-link` devices (appendix A), the `[link]` section and the manifest restrictions on a Joker Micro (§5.22) |
| `ref/09` | the API exposes the effective manifest and the provenance of each key (§8.4) |
| `ref/10` | the desired state carries the hash of the **generation** (ref/10 §2.6.2), which covers the hash of the **effective manifest** defined in §8.3 |

---

## 2. How to read this reference

### 2.1 Table columns

| Column | Meaning |
|---|---|
| **Type** | one of the types of §2.2 |
| **Req.** | `yes`: required in the **effective manifest** (after the overlays), and not in each file. `prod`: required only in `mode = "prod"`. `cond.`: required under a condition described in the validation |
| **Default** | the value used when the key is missing. `—` means "no value": the feature stays off |
| **Validation** | rules beyond the type; the code in parentheses is the error |
| **Origin** | document the key comes from; `new` when this reference introduces it |
| **Phase** | `1.0`, or a later version (`1.0.x`, `1.1`, `1.2`): **reserved**. A reserved key exists in the 1.0 schema, `jk` validates its form, and using it on a runtime that does not implement it is `J0290`. The 1.0 cut that decides what is reserved is that of document 26, §5 (ADR 0028, ADR 0035); `1.0.x` is a patch release that adds drivers and extensions (ADR 0022, rule 3) |

### 2.2 Types

| Type | TOML | Grammar | Example |
|---|---|---|---|
| `Text` | string | UTF-8, ≤ 1 KiB unless stated otherwise | `"Over-frequency"` |
| `Bool` | boolean | | `true` |
| `Integer` | integer | | `3` |
| `Number` | integer or float | | `0.45` |
| `Name` | string or key | `[a-z][a-z0-9_]*`, ≤ 32 characters (16, P5) | `peak_shaver` |
| `DnsName` | string | DNS label (`ref/00` §5) | `site-lisboa-01` |
| `Duration` | string | §3.1 | `"200ms"` |
| `Quantity<D>` | string | §3.2; `D` is the expected dimension | `"125 kW"` |
| `Rate<U>` | string | §3.3; unit of `U` per time | `"25 kW/s"` |
| `Budget` | string | §3.4 | `"120/h"` |
| `Size` | string | §3.5 | `"512 MiB"` |
| `DataVolume` | string | §3.5 | `"300 MB/month"` |
| `Percentage` | string | §3.2 with `%` | `"20 %"` |
| `Value<S>` | depends on the signal `S` | §3.6 | `false`, `"0 kW"`, `"StatusMode.off"` |
| `Expr<T>` | string | expression of document 02, §10, with result `T` (§3.7) | `"meter.f > 49.8 Hz"` |
| `Path` | string | §3.8 | `"pcs.p_set"` |
| `Selector` | string | §3.9 | `"inv*.w_max_lim_pct"` |
| `Ref` | string | §3.10 | `"model:load_forecast"` |
| `Level` | string | §3.11 | `"optimizer"`, `"optimizer:13"` |
| `File` | string | path relative to the file that declares it (§7.5) or absolute | `"sim/tank.sim.toml"` |
| `List<T>` | array | | `["a", "b"]` |
| `Table<K, V>` | table | free keys `K` (names, paths, selectors) with values `V` | `[calc]` |

A **list** is ordered (the order matters) or a **set** (the order does not matter and duplicates merge). The table of each key says which one it is, because that changes the effect of `append` (§7.3).

### 2.3 General rules of the file

1. **TOML 1.0, UTF-8** (ADR 0018). A file that is not valid TOML or is not UTF-8 is `J0146`.
2. **Unknown keys are an error** (`J0140`), with a suggestion by edit distance (`max_leas` → "did you mean `max_lease`?"). It is rule 3 of `ref/00` §1.
3. **Extension keys** start with `x_` (`x_owner = "energy-team"`), in any table, and are ignored by the runtime. They do not enter the hash of the effective manifest (§8.3), and therefore not that of the generation either, but they pass into the effective manifest, for the tools.
4. **Keys starting with `_`** are reserved (`J0141`).
5. The keys of free tables (names of devices, apps, groups, alarms, `calc`) follow `Name`; those of `[outputs]`, `[signals]` and `[telemetry.record]` are paths or selectors, and so they go in quotes (`[outputs."pcs.p_set"]`).
6. **A key is required in the effective manifest**, not in each file: a template without `[project] name` is valid as a layer, and the `name` comes from the site (§7).
7. The key `[project] joker` declares the minimum runtime version and, with it, the schema. A key introduced in a version later than the declared one is `J0148` (§10.2).

---

## 3. Grammar of the values

Quantities, durations, expressions and references are strings, because TOML has no units. Each position of the schema has a known type, and the string is interpreted **according to that type**. So there is no ambiguity between `"1m"` (one minute, in a duration) and `"1 m"` (one meter, in a length quantity).

### 3.1 Durations

The grammar is that of `ref/00` §3.1:

```
duration := number space? unit-t
unit-t   := "ns" | "us" | "µs" | "ms" | "s" | "m" | "min" | "h" | "d"
number   := digits ("." digits)?
```

- No sign: a duration is never negative (`J0152`).
- `"0s"` is only accepted where the table says so (`grace`, `min_interval`, `delay_on`, `delay_off`). In the other keys, zero is `J0152`.
- No compounds (`"1h30m"` is `J0152`); you write `"90m"`. Sums (`"2m+1.2s"`) only exist in the instants of scenarios and cases (`ref/00` §3.1, `ref/03` §2.1.3), and not in the manifest.
- Rounding to ns. The absolute maximum is `"3650d"`; each key may have a smaller maximum.
- A bare number (`200` instead of `"200ms"`) is not accepted: it is `J0151`.

### 3.2 Quantities and percentages

```
quantity := "-"? number exponent? space? unit
exponent := ("e" | "E") ("+" | "-")? digits
unit     := a unit of the catalog of document 16, §5, with prefixes and compounds
```

- The **dimension** must be the one expected by the key (`J0107`), and so must the **quantity**, when the key knows it (`J0108`: `"5 kvar"` in the range of an output in `kW`).
- A value in a unit different from that of the signal is **converted** (`"125000 W"` in an output in `kW` is `125 kW`). The overlay comparisons (§7.4) are made already converted.
- `%` is a unit like the others: `"5 %"` does not bind to a signal in `1` (document 16, §4).
- **Number without a unit:** only accepted when the expected dimension is dimensionless (`1`), or when the signal has no unit (integers without a unit, `bits`). In other cases it is `J0151`, with the suggestion of the signal's unit. The reason: if the profile changed from `W` to `kW`, a `safe = 0` would stay correct by chance, and a `range = [0, 125]` would change meaning silently.
- `°C` is affine: an absolute value is written in `°C` and a difference in `K` (document 16, §4). `deadband`, `tolerance` and `max_divergence` are differences: on an output in `°C` they are written in `K`, and `"0.5 °C"` is `J0107` with the suggestion `"0.5 K"`.
- A `Percentage` is a quantity with unit `%`.

### 3.3 Rates

```
rate := quantity          with unit  U "/" ("s" | "min" | "h")   or the equivalent derived unit
```

- `U` has the dimension of the output: `"25 kW/s"` for an output in `kW`, `"20 %/s"` for `%`, `"1.5 m/s2"` for a speed in `m/s` (`m/s2` = (m/s)/s), `"0.5 °C/min"` for a temperature (read as `K/min`).
- A dimension inconsistent with the output is `J0202`. A negative or zero rate is `J0202`.
- Internally, everything is converted to output-unit per second.

### 3.4 Actuation budgets

```
budget := integer "/" (unit-t | duration)
```

`"120/h"`, `"6/h"`, `"7200/h"`, `"3/10min"`. The period is a sliding window. Integer ≥ 1; period between `1s` and `7d` (`J0159`).

### 3.5 Sizes and data volumes

```
size        := number space? ("B" | "KiB" | "MiB" | "GiB" | "TiB")
data-volume := number space? ("kB" | "MB" | "GB" | "KiB" | "MiB" | "GiB") "/" ("day" | "month")
```

- Memory and disk sizes use IEC prefixes (`ref/00` §3.2); `"512 MB"` in a `Size` is `J0159` with the suggestion `"512 MiB"` or `"488 MiB"`.
- Uplink volumes accept SI, because data plans are expressed that way (`"300 MB/month"`). `month` is the calendar month in UTC; the estimator projects up to the end of the current month (document 01, §13).

### 3.6 Signal values

A `Value<S>` (used in `safe`, `on_late.apply`, `atomic.commit.value`, `params`, `const:` adapters) is written according to the type of `S`:

| Signal type | TOML | Example |
|---|---|---|
| `bool` | boolean | `safe = false` |
| `i64`, `u64` without a unit, `bits<N>` | integer | `value = 1` |
| `f64`, `f32` with a unit | quantity (string) | `safe = "0 kW"` |
| `f64`, `f32` in `1` | number or `"0.5"` | `safe = 0.0` |
| `enum<E>` | string `"E.variant"` (canonical) or `"variant"` | `safe = "StatusMode.off"` |
| `string` | string, ≤ 64 B | `safe = "idle"` |
| `bytes` | string `"base64:…"`, ≤ 64 B | `safe = "base64:AAA="` |

A value of the wrong type is `J0231` (in `safe`) or `J0143` (in other keys); a nonexistent variant is `J0303`.

### 3.7 Expressions in strings

The language is that of document 02, §10, with the edge cases of document 16, §4. The compilation errors are the `J03xx` (§10). Where each key accepts expressions:

| Key | Result | Windows | `own()` | Selectors | State (edges, `held`) |
|---|---|---|---|---|---|
| `outputs.*.interlock` | `bool` | yes, ≤ 15 min | no (`J0306`) | only in aggregations | yes |
| `outputs.*.range[i]` | number with the dimension of the output | yes | no | only in aggregations | yes |
| `outputs.*.reset_when` | `bool` | yes | no | only in aggregations | yes |
| `alarms.*.when`, `alarms.*.suppress_when` | `bool` | yes | no | only in aggregations | yes |
| `calc.*` | any scalar | yes | no | only in aggregations | yes |
| `devices.*.actions.*.permit`, `instances.*.actions.*.permit` | `bool` | yes | no | only in aggregations | yes |
| `apps.*.fallback.when` (1.1) | `bool` | KPI grammar (document 06, §10.1) | no | no | — |
| `models.*.shadow.promote_if`, `.kpi`, `.until` (1.1) | KPI grammar | up to 30 d | no | no | — |
| `pipelines.*.steps[].expr`, `training.*.data.where`, `telemetry.record.*.trigger` (1.1) | `bool` | yes | no | only in aggregations | yes |

- **Constant or expression:** in a position that accepts both (`range`), a string that is entirely a `quantity` is a constant; anything else is an expression. `"-125 kW"` is a constant; `"-bms.p_max_discharge"` is an expression.
- **References** (`model:`, `pipe:`, `class:`) are not paths (16, P4) and do not appear in expressions, except in the model functions of the 1.1 positions (`good(model:m)`, `ood_ratio(model:m, 15m)`, `model_threshold(m)`).
- **Evaluation group:** the expressions of an output run in its owning group (A11); `calc` and alarms run in the fastest group that consumes them, or in the implicit `calc` group (100 ms), or in the group indicated by `group` (§5.13, §5.14). What consumes a signal, the order of the `calc` and alarms inside a tick and which samples an expression reads are in ADR 0068. A `permit` runs at each invocation, not in a tick; its windows are sized with the period of the fastest group that owns an output of the action's device, or 100 ms (ADR 0068).
- **Dependencies between `calc`** form an acyclic graph; a cycle is `J0190`.
- An expression with a syntax error is `J0158`; unknown signal `J0302`; unknown variant `J0303`; dimensions `J0301`; types `J0304`; window above the maximum `J0305`; function or arguments `J0307`. The typing rules and the semantics of windows and stateful functions are in ADR 0034.

### 3.8 Paths and names

```
path      := segment ("." segment)*
segment   := [a-z][a-z0-9_]*            (≤ 32 characters; ≤ 8 segments)
```

- Violating the grammar is `J0153`.
- **Owned prefixes** (document 02, §9): `<device>.`, `<instance>.`, `app.<app>.`, `calc.`, `alarm.`, `bridge.<bridge>.`, `to` paths of pipelines (1.1), `board.<name>.` (fixed IO of the board, `ref/03` §2.3.2), `joker.` (reserved).
- An alias or a `to` that starts with `calc.`, `alarm.`, `app.`, `board.`, `joker.` or `bridge.` (outside its own bridge) is `J0141`. The first segment `@` is reserved for v2 (`ref/00` §2). A device, instance or app named `joker` or `board` is also `J0141`.
- **Names** of devices, instances, apps, groups, buses, alarms, `calc`, bridges, uplinks and atomic groups are `Name` (16, P5). The name of an app must not start with `joker` (`J0141`). The reserved names of ports and of class members are those of `ref/00` §2 (validated against `joker:meta` and the `.jkc`, and not here). The `[project] name` is a `DnsName` (it is the site identifier, `ref/00` §5).
- Driver names (`modbus-tcp`, `spi-adc`) follow `[a-z][a-z0-9-]{0,31}` (`ref/06` §2.2), because they come from the catalog or from the descriptor, and not from whoever writes the manifest.

### 3.9 Selectors

```
selector  := sseg ("." sseg)*
sseg      := "**" | pattern
pattern   := ( [a-z0-9_] | "*" )+          (starts with a letter or "*")
```

| Form | Matches | Example |
|---|---|---|
| `*` inside a segment | zero or more characters `[a-z0-9_]` **of that segment** | `"inv*.ac.p"` matches `inv1.ac.p`, `inv12.ac.p`; `"vav_3*"` matches `vav_30` |
| segment `*` | exactly one segment | `"p1.acc.*"` |
| segment `**` | **one or more** segments | `"meter.**"` matches `meter.f` and `meter.l1.v`, not `meter` |
| `"**"` alone | all paths | `low = "**"` |

- `**` can only be a whole segment: `"**alarm.**"` is `J0154` (and it is an error of the example of document 03, §11, appendix C).
- Resolution is done in `jk check`, over **all paths** (canonical, aliases, instance members), and the result is deduplicated by `SignalId`: a selector that catches the canonical path and an alias of the same signal counts once.
- The order of the results is the **lexicographic order of the canonical path**. It is the order of `OutputMany`, `InputMany` and `Many<T>` bound by selector.
- A selector without matches is `J0155` (error) in `outputs`, `bind`, `requires_good`, `alarms.*.actions.trip`, `bridges.*.writer.writable` and `atomic`; and `J0156` (warning) in `telemetry`, `bridges.*.expose` and `security`. A template applied to a site without inverters does not fail because of the telemetry.
- More than 4096 matches in a selector is `J0149`.
- In expressions, selectors are only accepted as an argument of aggregations (`any`, `all`, `count`, `sum`, `maxs`, `mins`; document 02, §10).

### 3.10 References

| Form | Where | Meaning | Phase |
|---|---|---|---|
| `"oci://<registry>/<repo>:<tag>"`, `"oci://<registry>/<repo>@sha256:<hex>"` | `apps.*.source`, `models.*.source`, `drivers.*.source` | OCI artifact (document 20, §3.2) | 1.0 (models 1.1) |
| `"sha256:<64 hex>"` | `digest` | content digest | 1.0 |
| `"app:<app>"` | `instances.*.implemented_by`, `apps.*.fallback.controller` | an app declared in `[apps]` | 1.0 (fallback 1.1) |
| `"class:<class>"` | `bind` of class ports `Many<T>` | all the instances of the class (document 14, §8, A3) | 1.0 |
| `"const:<value>"` | `instances.*.map` (only `in` and `param` members; on an `out` it is `J0603`) | constant with quality `Good` (17, §5.2) | 1.0 |
| `"secret:<name>"` | only the keys marked **(secret)** in the tables of this reference: `password` and `token` of the uplinks (§5.17.3), `password` of the `mqtt` and `opcua` drivers and `key` of `joker-link` (appendix A), the keys of type `secret` of the schema of a W or X driver (`ref/06` §2.3.1), and the values of `[apps.<app>.secrets]` (§5.12) | secret from the node's vault, by name (`ref/00` §9, `ref/02` §2.4.6) | 1.0 |
| `"model:<model>"` | `bind` of model ports | a `[models.<model>]` | 1.1 |
| `"pipe:<pipeline>"` | `bind`, `training.*.data.signals` | the output of a pipeline as an event stream, with frame metadata | 1.1 |
| `"training:<job>"` | `models.*.source` | the promoted version of the result of `[training.<job>]` | 1.1 |
| `"unix:<path>"`, `"tcp:<host>:<port>"` | `security.api.listen` | listening endpoints | 1.0 |
| `"cert:<ATTR>=<pattern>"`, `"local:user=<u>"`, `"local:group=<g>"`, `"deck:<deck_id>/user:<id>"`, `"deck:<deck_id>/group:<g>"` | `security.roles.*` | identities (§5.20) | 1.0 (`deck:` used from 1.3 onward) |

- An unknown scheme, or a reference in a key that does not accept it, is `J0157`.
- `<class>` follows document 17, §3.2: `<package>/<name>@<major>`, with optional `std/` (`"pump@1"` ≡ `"std/pump@1"`).
- A plaintext secret in a key of the secret type is `J0294`. A `secret:` in a key that is not of the secret type is `J0157` (it is the code that `ref/02` §2.4.6 also uses).

### 3.11 Arbiter levels

```
level    := band | band ":" priority
band     := "safety" | "manual" | "supervisor" | "control" | "optimizer" | "schedule"
```

| Band | Priorities | Band default | Who may occupy it |
|---|---|---|---|
| `safety` | 1–2 | 2 | apps |
| `manual` | 3–5 | — | only users (CLI, API, HMI) and approved intents: an app or bridge here is `J0252` |
| `supervisor` | 6–8 | 8 | apps, bridges |
| `control` | 9–12 | 12 | apps, bridges, implementations |
| `optimizer` | 13–15 | 15 | apps, bridges |
| `schedule` | 16 | 16 | apps, bridges |

- In the writers (`[apps.*.writer]`, `[bridges.*.writer]`), the level is written with `level` + `priority` (document 03) or in the compact form `level = "optimizer:13"` (`ref/00` §5). The two together must agree (`J0250`).
- `priority` outside the band is `J0251`. A bridge in the `safety` band is `J0253`.
- Without `priority`, the **band default** applies, the lowest one (the largest number). It is the rule that gives `control:12` to the single writer (A11 and document 03, §8).

---

## 4. Structure of the file

| Section | Free keys | Req. | Phase | Origin | Reference |
|---|---|---|---|---|---|
| `extends` (root) | — | no | 1.0 | 03 §2 | §5.1, §7 |
| `[project]` | — | yes | 1.0 | 03 §3 | §5.2 |
| `[target]` | — | prod | 1.0 | 03 §3 | §5.3 |
| `[target.capacity]`, `[target.extern]`, `[target.micro]` | — | no | 1.0 (Joker Micro only) | `ref/08` §2.17, new | §5.22 |
| `[resources.<class>]` | `rt`, `io`, `ml`, `bg` | no | 1.0 (`accel` 1.1) | 03 §3, 01 §2 | §5.4 |
| `[clock]` | — | no | 1.0 | 03 §3, 01 §18 | §5.5 |
| `[buses.<bus>]` | `Name` | no | 1.0 | 03 §4 | §5.6 |
| `[drivers.<driver>]` | driver name | no | 1.0 | new | §5.7 |
| `[devices.<device>]` | `Name` | no | 1.0 | 03 §5 | §5.8 |
| `[signals."<path>"]` | `Path` or `Selector` | no | 1.0 | 03 §9.1, track 04 | §5.9 |
| `[outputs."<path>"]` | `Path` or `Selector` | cond. | 1.0 (scheduled writes 1.1) | 03 §6 | §5.10 |
| `[cycles.<group>]` | `Name` | no | 1.0 | 03 §7 | §5.11 |
| `[apps.<app>]` | `Name` | no | 1.0 | 03 §8 | §5.12 |
| `[instances.<instance>]` | `Name` | no | 1.0 | 03 §9.2, 14, 17 | §5.13 |
| `[calc]` | `Name` | no | 1.0 | 03 §10 | §5.14 |
| `[alarms.<alarm>]` | `Name` | no | 1.0 | 03 §10, 02 §11 | §5.15 |
| `[atomic.<group>]` | `Name` | no | 1.0 | 03 §10, 02 §7.3 | §5.16 |
| `[telemetry]` | — | no | 1.0 (blocks 1.1) | 03 §11 | §5.17 |
| `[bridges.<bridge>]` | `Name` | no | 1.0 | 03 §12 | §5.18 |
| `[security]` | — | no | 1.0 | 03 §13 | §5.19–§5.20 |
| `[link]` | — | no | 1.0 (Joker Micro only) | 19 §6, new | §5.22 |
| `[models.<model>]` | `Name` | no | **1.1** | 03 §9, 06 | §6.1 |
| `[streams."<path>"]` | `Path` | no | **1.1** | 03 §9 | §6.2 |
| `[pipelines.<pipeline>]` | `Name` | no | **1.1** | 03 §9, 06 §5 | §6.3 |
| `[training.<job>]` | `Name` | no | **1.1** | 06 §9 | §6.4 |

There is no `[api]` at the root: the local API is configured in `[security.api]` (§5.19), because every decision about it is a security decision. There is also no interpolation of environment variables (§14, D6).

---

## 5. Sections (v1.0)

### 5.1 Root

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `extends` | `List<File>` (ordered) | no | `[]` | local files only, relative to the file that declares them (`J0160`); no cycles (`J0161`); ≤ 8 levels and ≤ 32 files (`J0162`); it is the first key of the file (it belongs to the root) | 03 §2 | 1.0 |
| `x_*` | any | no | — | ignored | `ref/00` §1 | 1.0 |

`extends` is not merged: each file has its own list, and the effective manifest does not have it (§7).

### 5.2 `[project]`

```toml
[project]
name  = "site-lisboa-01"
joker = "1.0"
mode  = "prod"
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `name` | `DnsName` | yes | — | lowercase DNS label, ≤ 63 (`J0150`); it is the identifier of the site | 03 §3 | 1.0 |
| `joker` | string `"MAJOR.MINOR"` | prod | the version of the `jk` that validates | the runtime must be ≥ (`J0147`); format (`J0147`); limits the accepted keys (`J0148`) | 03 §3, 09 A8 | 1.0 |
| `mode` | `"dev"` \| `"prod"` | no | `"prod"` | `J0144` | 03 §3 | 1.0 |

**`mode`.** In `prod`: unsigned artifacts are rejected; the codes of severity `error-in-prod` are errors; discovery does not use maps that are not in the lock; drift blocks the outputs of the device (`J0401`); `jk apply` requires `--confirm` when the generation touches network, security or uplinks (ADR 0015); `[project] joker` and `[target] board` are required. In `dev`, the `error-in-prod` codes appear as `W` warnings with the same number (document 03, §15).

The default is `"prod"` because forgetting the key on a real site must not turn signatures off. `jk init` writes `mode = "dev"` explicitly (track, part 1).

### 5.3 `[target]`

```toml
[target]
board  = "revpi-connect-5"
kernel = "preempt_rt"
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `board` | string `"<board>"` or `"<board>@<major>"` (board profile, `ref/03` §2.3); without a major, the highest known major | prod | — (in `dev`, without a board, the checks that depend on it, such as `J0380` and `recommended`, are not done, and `jk check` says so in a note) | known board (`J0172`); missing in `prod`, `J0142`; the lock pins it in `[boards."<value>"]` (`ref/02` §2.1.4) | 03 §3 | 1.0 |
| `arch` | `"aarch64"` \| `"x86_64"` \| `"armv7"` \| `"riscv64"`, or an MCU target from the board catalog (`ref/03`) | no | that of the board; without a board, that of the host | must match that of the board (`J0145`) | 03 §3 | 1.0 |
| `kernel` | `"preempt_rt"` \| `"standard"` | no | `"standard"` | `J0351`, `J0352`; on an MCU it does not apply (`J0145`) | 03 §3 | 1.0 |

The default of `kernel` is the conservative one: a group < 10 ms without `kernel = "preempt_rt"` declared is `J0351`, even if the node has it. `jk doctor` checks whether the node meets what the manifest declares.

### 5.4 `[resources.<class>]`

```toml
[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
ml = { cpus = [1, 2], memory = "1 GiB" }
bg = { cpus = [0], memory = "512 MiB" }
```

The classes are fixed: `rt`, `io`, `ml` and `bg` (document 01, §2). Any other key is `J0140`.

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `cpus` | `List<Integer>` (set) | no | the board's recommendation (`recommended`); without a board, all CPUs and no cpuset | indices < CPU cores of the board (`J0144`); `rt` does not overlap the others and is not empty when there are groups (`J0360`); the others may overlap each other. **Exception:** on a board with `cpus = 1`, the overlap of `rt` with the other classes coming from the board's `recommended` (or declared equal to it) is a `jk check` note (real-time admission counts the shared CPU core, document 01 §4), not `J0360` | 03 §3 | 1.0 |
| `memory` | `Size` | no | no limit (`rt` never has a limit) | `memory.max` of the cgroup; in `rt` it is `J0145` | 03 §3 | 1.0 |
| `accel` | `List<string>` `"<type>:<id>"` (`"npu:hailo0"`, `"gpu:0"`) | no | `[]` | only in `ml` (`J0145`) | 03 §3 | **1.1** |

The `ml` class exists in 1.0 with nothing that uses it: it is one of the reservations of document 11 ("Prepared for ML").

### 5.5 `[clock]`

```toml
[clock]
sync         = "ntp"
servers      = ["ntp1.example.com", "ntp2.example.com"]
max_holdover = "24h"
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `sync` | `"ntp"` \| `"ptp"` \| `"gnss"` \| `"link"` \| `"extern"` \| `"none"` | no | `"ntp"` (Linux), `"link"` (Joker Micro) | `link` and `extern` only on a Joker Micro (`J0145`): `link` is the `TIME` of Joker Link, `extern` is the clock given by the integrator's firmware (`jk_time_set_utc`, `ref/08` §2.6.m); `none` with apps `requires_clock = "synced"` is `J0145` | 03 §3, 19 §6, `ref/08` §2.17 | 1.0 |
| `servers` | `List<string>` (hosts) | no | those of the operating system | only with `ntp`; `jk doctor` checks that the OS daemon uses them | 03 §3 | 1.0 |
| `iface` | string (interface) | cond. | — | required with `ptp` (`J0142`) | new | 1.0 |
| `max_holdover` | `Duration` | no | `"24h"` | ≤ `"30d"` | 03 §3 | 1.0 |

`jokerd` does not implement NTP or PTP: it reads the state of the system daemon (chrony, systemd-timesyncd, linuxptp) and derives the quality of the clock (document 01, §18): `synced` up to 4096 s after the last synchronisation, `holdover` up to `max_holdover` after it, `unsynced` after that or before any synchronisation since boot, re-evaluated once a second and at each generation start, with each change recorded in the decision log (ADR 0069). With `sync = "none"`, the quality is always `unsynced`. The time of the Deck only serves as a coarse source, never beyond `holdover` (`ref/02` §3.2.4). On a Joker Micro, synchronization comes from the `TIME` message of Joker Link, or from the firmware with `extern`.

### 5.6 `[buses.<bus>]`

```toml
[buses.rs485]
kind       = "serial"
port       = "/dev/serial/by-id/usb-1a86_USB_Serial-if00-port0"
baud       = 9600
parity     = "none"
stop       = 1
turnaround = "5ms"

[buses.can0]
kind    = "can"
iface   = "can0"
bitrate = 500000
```

A `serial`, `i2c`, `spi` or `ethernet` bus **serializes** access: all the devices that use it share a request scheduler, with a single call in flight at a time (document 05, §5; `ref/06` §2.14), and `jk check` estimates the occupancy (`J0370`). A `can` bus does **not** serialize between devices (frames are asynchronous; `jk check` estimates the bus load all the same), and `ethercat` is served by a `bus` instance of the driver, which owns the cycle. Different protocols on the same serial bus (Modbus RTU and Joker Link, for example) are `J0171`.

| `kind` | Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|---|
| all | `kind` | `"ethernet"` \| `"serial"` \| `"can"` \| `"ethercat"` \| `"i2c"` \| `"spi"` | yes | — | `J0144`; `spi` is reserved for 1.0.x, `can` for 1.1 and `ethercat` for 1.2 (`J0290`) | 03 §4 | 1.0 |
| `ethernet` | `iface` | string | yes | — | | 03 §4 | 1.0 |
| `serial` | `port` | `File` (device) on Linux; `Name` on an MCU | yes | — | on Linux, an absolute path; **on an MCU** (§5.22), the name of a `[uart.<name>]` of the board (`ref/03` §2.3.3), such as `port = "uart0"`, and never a path (nonexistent `J0828`; a path `J0145`) | 03 §4 | 1.0 |
| `serial` | `baud` | `Integer` | yes | — | 300–4 000 000 | 03 §4 | 1.0 |
| `serial` | `parity` | `"none"` \| `"even"` \| `"odd"` | no | `"none"` | | 03 §4 | 1.0 |
| `serial` | `stop` | `1` \| `2` | no | `1` | | 03 §4 | 1.0 |
| `serial` | `data_bits` | `7` \| `8` | no | `8` | | new | 1.0 |
| `serial` | `turnaround` | `Duration` | no | 3.5 characters at the declared `baud` (Modbus RTU frame silence) | | 03 §4 | 1.0 |
| `serial` | `dtr`, `rts` | `"keep"` \| `"low"` \| `"high"` | no | `"low"` | state of the control lines **when the port is opened** and while it is open. `"low"` (deasserted) on both is what lets the port be opened without triggering the *auto-reset* of ESP32 boards (the DTR/RTS circuit of the USB bridge pulls `EN` and `GPIO0` to zero, and the ESP32 restarts or enters flashing mode); `"keep"` does not touch the lines (whatever state the bridge controller has); `"high"` asserts it (some RS-485 adapters use RTS for the direction). Only on Linux; on an MCU `J0145`. With `duplex = "half"` and an adapter that does the direction through RTS, the driver controls RTS and `rts` is `J0145` | new (track, part 4) | 1.0 |
| `serial` | `duplex` | `"full"` \| `"half"` | no | `"full"` | `half` on two-wire RS-485: Joker Link uses turn-taking (`ref/07` §15); Modbus RTU ignores it. `half` on a bus with a `joker-link` device is reserved for 1.1 (`J0290`) | `ref/07` §15 | 1.0 (`half` with Joker Link: 1.1) |
| `can` | `iface` | string | yes | — | | 03 §4 | 1.1 |
| `can` | `bitrate` | `Integer` (bit/s) | yes | — | | 03 §4 | 1.1 |
| `can` | `fd` | `Bool` | no | `false` | | new | 1.1 |
| `can` | `data_bitrate` | `Integer` | cond. | — | only with `fd = true` (`J0145`) | new | 1.1 |
| `ethercat` | `iface` | string | yes | — | | 03 §4 | 1.2 |
| `ethercat` | `cycle` | `Duration` | yes | — | ≥ 100 µs | 03 §4 | 1.2 |
| `ethercat` | `dc` | `Bool` (distributed clocks) | no | `false` | | 08 R4 | 1.2 |
| `i2c` | `dev` | `File` | yes | — | | example 00, old track (part 4) | 1.0 |
| `spi` | `dev` | `File` (`/dev/spidevB.C`) | yes | — | | new (fixes 08 R3) | 1.0.x |
| `spi` | `mode` | `0`–`3` | no | `0` | | new | 1.0.x |
| `spi` | `speed` | `Quantity<Hz>` | no | `"1 MHz"` | | new | 1.0.x |

A TCP device without `bus` uses the system routing and does **not** serialize with the others (each one has its own connection). With a `bus` of `kind = "ethernet"`, it connects through that interface and shares the scheduler (useful when a PLC only accepts one TCP connection, or behind a Modbus TCP/RTU gateway).

### 5.7 `[drivers.<driver>]` (reserved 1.2)

Tier N drivers are compiled into `jokerd`. Tier W and X drivers are referred to with a prefix in `driver` (`"w:dlms-cosem"`, `"x:bacnet-ip"`) and are declared here. **The tier W and tier X hosts are 1.2** (document 26, §5; ADR 0035): in 1.0 the whole section is reserved, and a `[drivers]` entry or a `w:`/`x:` driver is `J0290`. When the hosts ship, tier W is still **unstable** until it is opened to third parties: only in-tree drivers pass in `prod`, and a W driver from outside the tree is `J0860` (`ref/06` §4).

```toml
[drivers.bacnet-ip]
tier      = "x"
exec      = "/usr/lib/joker/drivers/joker-drv-bacnet-ip"
launch    = "systemd"
memory    = "256 MiB"
cpu       = "50 %"
net       = ["10.20.0.0/16"]
env       = { BACNET_LOG = "warn" }

[drivers.dlms-cosem]
tier   = "w"
source = "oci://registry.example.com/joker/drivers/dlms-cosem:0.3.1"
memory = "8 MiB"
```

| Key | Tier | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|---|
| `tier` | w, x | `"w"` \| `"x"` | yes | — | must match the prefix used in `driver` (`J0170`) | new | 1.2 |
| `source` | w, x | `Ref` `oci://…` or `File` | cond. | — | required in `w`; in `x`, an alternative to `exec` (`J0145`) | new | 1.2 |
| `digest` | w, x | `"sha256:…"` | no | pinned in the lock (`ref/02` §2.1.8) | different from the lock `J0420` | new | 1.2 |
| `exec` | x | `File` (absolute) | cond. | — | | new | 1.2 |
| `args` | x | `List<string>` (ordered) | no | `[]` | | new | 1.2 |
| `launch` | x | `"systemd"` \| `"spawn"` \| `"external"` | no | `"systemd"` where there is systemd, otherwise `"spawn"` | `ref/06` §5.2.1; `spawn` with the `jokerd` user in `prod` `J0865` | `ref/06` | 1.2 |
| `user` | x | `"dynamic"` or a user name | no | `"dynamic"` with `launch = "systemd"` (`DynamicUser`); `"joker-drv-<driver>"` with `launch = "spawn"` | nonexistent user `J0864` | 01 §17, `ref/06` §5.2.4 | 1.2 |
| `groups` | x | `List<string>` (set) | no | `[]` | only device-access groups (`dialout`, `video`, `gpio`, `i2c`, `spi`); `joker`, `root`, `wheel`, `sudo` `J0865` | `ref/06` | 1.2 |
| `memory` | w, x | `Size` | no | W: that of the descriptor (16 MiB); X: `"256 MiB"` | W ≤ `"64 MiB"` (`J0874`) | `ref/06` | 1.2 |
| `cpu` | x | `Percentage` of one CPU | no | `"100 %"` | `"1 %"`–`"400 %"` (`J0144`) | `ref/06` | 1.2 |
| `cpu_per_call` | w | `Duration` | no | `"50ms"` | ≤ `"1s"` (`J0152`) | `ref/06` | 1.2 |
| `plan_fuel` | w | `Integer` | no | 10⁹ | ≤ 10¹⁰ (`J0144`) | `ref/06` | 1.2 |
| `isolation` | x | `"process"` \| `"per_device"` | no | `"process"` | `per_device`: one process per device | `ref/06` | 1.2 |
| `net` | x | `List<string>` (IP or CIDR; set) | no | `[]` | destinations beyond the addresses of the devices (`J0184` if malformed) | `ref/06` | 1.2 |
| `devices` | x | `List<File>` (`/dev` nodes; set) | no | `[]` | extra nodes for `DeviceAllow` | `ref/06` | 1.2 |
| `raw_net` | x | `Bool` | no | `false` | `AF_PACKET` (GigE Vision, external EtherCAT) | `ref/06` | 1.2 |
| `env` | x | `Table<string, string>` | no | `{}` | `JDP_*` keys reserved (`J0141`) | `ref/06` | 1.2 |
| `restart` | w, x | `{ min = Duration, max = Duration }` | no | `{ min = "1s", max = "5m" }` | `min ≤ max` (`J0145`) | `ref/06` §4.5, §5.7 | 1.2 |

A key marked for only one tier in an entry of the other tier is `J0145`. A `driver = "x:<name>"` without `[drivers.<name>]` is `J0170`. The driver name follows the grammar of §3.8 (hyphen allowed). How `jokerd` starts, isolates and supervises the driver, and how each key translates into properties of the systemd unit, are in `ref/06` §4.5 and §5.2; this table is the normative list of the keys.

### 5.8 `[devices.<device>]`

```toml
[devices.meter]
driver  = "modbus-tcp"
bus     = "plant"
address = "192.168.10.20:502"
unit_id = 1
profile = "janitza/umg604@2"
poll    = "200ms"
timeout = "300ms"
retries = 1

[devices.pcs]
driver   = "modbus-tcp"
address  = "192.168.10.30:502"
profile  = "acme/pcs-125@3"
poll     = "200ms"
align    = "meter"
fallback = { configure = true, keepalive = "5s" }
```

**Keys common to all drivers:**

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `driver` | string: `<name>` (tier N), `"w:<name>"`, `"x:<name>"`, or `"extern"` (only on a Joker Micro, §5.22) | yes | — | a driver from the catalog of this build or declared in `[drivers]` (`J0170`); the profile's family must accept it (`J0808`, `ref/03` §2.2.2) | 03 §5 | 1.0 |
| `bus` | `Name` from `[buses]` | cond. | — | required in the drivers that need a transport (appendix A); `kind` compatible with the driver's descriptor (`J0171`) | 03 §5 | 1.0 |
| `profile` | string `"<vendor>/<model>@<major>"`, or the short form `"sunspec"` (≡ `"joker/sunspec@1"`, the built-in profile, `ref/03` §2.2.15) | cond. | — | required in the drivers with a profile (appendix A); major required; resolved in `profiles/`, then in the registry, then in the built-in ones (`ref/03` §3); not found or without a major `J0172`; the lock pins it in `[profiles."<name>@<major>"]`, always in the full form (`ref/02` §2.1.5) | 03 §5 | 1.0 |
| `poll` | `Duration` | no | that of the profile (`defaults.poll`); without it, `"1s"` | ≥ the `min_interval` of the profile's `quirks` | 03 §5 | 1.0 |
| `timeout` | `Duration` | no | that of the profile; without it, `"1s"` | | 03 §5 | 1.0 |
| `retries` | `Integer` | no | `1` | 0–10 | 03 §5 | 1.0 |
| `align` | `Name` of another device | no | — | the poll of this device is scheduled right after that of the one indicated (document 08, R1e); nonexistent device `J0157` | 03 §9.1 | 1.0 |
| `discover` | `Bool` | no | `false` | only in drivers with the `discover` capability (`J0177`); result in the lock, in `[discovered.<device>]` (M2, `ref/02` §2.1.11) | 03 §5 | 1.0 |
| `fallback` | table (below) | no | — | | 03 §5 | 1.0 |
| `signals` | `Table<signal name, adjustment>`; the name is a `Name`, or, in a `joker-link` device, the path of the signal on the MCU (`io.led`, which becomes `<device>.io.led`; ref/07 §6.3) | no | — | §5.8.1 | 03 §5.1 | 1.0 |
| `actions` | `Table<action name, configuration>` | no | — | §5.8.2 | 03 §5.2 | 1.0 |

The keys specific to each driver (`address`, `unit_id`, `chip`, `model`, `params`, `node_id`, `eds`, `config`, `position`…) are in **appendix A**, which is the `config` schema of the descriptor of each catalog driver (`ref/06` §2.3). For a W or X driver, those of the schema it publishes apply. A key that exists in another driver but not in this one is `J0173`; one that exists in none is `J0140`; a value outside the driver's schema is `J0852` (`ref/06` §2.3.1). Two devices with the same address on the same bus are `J0178`; so is a device that uses the same kernel resource (LED, GPIO line, thermal zone) as a fixed IO `board.<name>` referred to by the manifest. The fixed IO of the board only exists when the manifest refers to `board.<name>`, and its `fallback` only counts through that path (`ref/03` §2.3.2).

**Two keys with the same name and different roles.** `config` is a **table** validated by the `[config]` of the **profile** (chip keys, `ref/03` §2.2.10); `revpi` has no `config`: it reads the configuration piControl has loaded (appendix A, ADR 0078). `model` is the **file** of the simulation model in the `sim` driver; the commercial designation of a piece of equipment is `[profile] model`, in the profile, and never appears in the manifest.

**`fallback` (M1):**

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `fallback.configure` | `Bool` | no | `false` | with `true`, the core writes at startup the fallback declared in the profile (`ref/06` §2.1, step 5); a profile without `[fallback]` is `J0275`; a profile with `configurable = false` is `J0275` | 03 §5 | 1.0 |
| `fallback.keepalive` | `Duration` | no | 1/5 of the equipment's fallback timeout (read from the lock) | `equipment timeout > keepalive + jokerd restart time` (`J0276`, `error-in-prod`) | 03 §9.1, 01 §8 | 1.0 |

With or without `configure`, the core **reads and checks** the equipment fallback at startup, and `jk lock` pins it in `[fallback.<device>]` (`ref/02` §2.1.12), with `configured` = the value of `fallback.configure`. `configure = false` means "I do not write; I accept what is there, if it is consistent with `safe`" (`J0205`). On boards, the fixed IO has its `fallback` in the board itself (`ref/03` §2.3.2), and it counts as the device fallback for that signal.

#### 5.8.1 `[devices.<d>.signals.<s>]`

The profile defines the signals; the manifest only **adjusts** them. In the drivers without a profile (`gpio`, `mqtt`, `joker-link` generated by `HELLO`, appendix A), the manifest **declares** them. A signal that does not exist in the profile is `J0101`, with a suggestion.

```toml
[devices.meter.signals]
p_total = { max_age = "1s", record = { every = "1s" } }
f       = { limits = ["45 Hz", "55 Hz"], stuck_detect = "1m" }

[devices.adc.signals]
a0 = { alias = "tank.level", scale = { from = ["0.6 V", "3.0 V"], to = ["0 m", "3 m"] } }
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `alias` | `Path` or `List<Path>` (set) | no | — | a second path for the same `SignalId` (16, P1); collision with another path `J0174`; reserved prefix `J0141` | 03 §5.1 | 1.0 |
| `max_age` | `Duration` | no | 3 × `poll` | **greater** than the `poll` (`J0816`, the rule of `ref/03` §2.2.4) | 03 §5.1 | 1.0 |
| `limits` | `[Quantity, Quantity]` | no | those of the profile | physical limits: outside them, `Bad(OutOfRange)`; minimum < maximum (`J0176`); numeric only (`J0176`) | 03 §5.1 | 1.0 |
| `stuck_detect` | `Duration` | no | that of the profile; without it, off | ≥ 2 × `poll`; does not apply to `bool` (`J0176`) | 03 §9.1 | 1.0 |
| `deadband` | `Quantity` (difference, in the dimension of the signal) | no | `0` | acquisition deadband (document 01, §3.3): a numeric value whose change from the last published value is at most the band is suppressed; `0` suppresses nothing, and the first value or a change of quality is always published (ADR 0047); does not apply to `bool` or `enum` (`J0176`) | track 05 | 1.0 |
| `debounce` | `Duration` | no | `0` | only `bool` (`J0176`); in a driver that supports it (GPIO chardev), it is done by the kernel, and the acquisition stage does not repeat it. On a `bool` **read by poll** (Modbus, `sysfs`, a GPIO without `edges`), the published value only changes after consecutive reads equal to the new value that cover at least the `debounce` (document 16, §3.4) | track 04 | 1.0 |
| `scale` | `Number` (factor) or `{ from = [Quantity, Quantity], to = [Quantity, Quantity] }` | no | `1` | linear, applied **after** the profile's scale (`ref/03` §3, "Acquisition"); with `from`/`to`, `from` is in the profile's unit, and the unit of the signal becomes that of `to` (the canonical one and the aliases); it is the same grammar as the two-point `scale` of profiles (`ref/03` §2.2.4) | example 00 | 1.0 |
| `offset` | `Number` or `Quantity` | no | `0` | applied after `scale`; incompatible with the `from`/`to` form (`J0145`) | 17 §5.2 | 1.0 |
| `unit` | unit | cond. | that of the profile | on a profile signal, only for another unit of the **same** dimension (`J0107`); required on the numeric signals declared in drivers without a profile | 08 R3 | 1.0 |
| `type` | signal type (`ref/00` §6) | cond. | `bool` in `gpio` | only in drivers without a profile (`J0176`) | new | 1.0 |
| `direction` | `"input"` \| `"output"` \| `"param"` | cond. | `"input"` | only in drivers without a profile; on a profile signal the direction is not changed (`J0176`) | track 02 | 1.0 |
| `poll` | `Duration` or `"on_demand"` | no | that of the device | `on_demand` only in `param` (document 05, §3) | 05 §3 | 1.0 |
| `record` | recording table (§5.17.1) | no | — | shorthand for `[telemetry.record."<canonical path>"]`; the two together for the same signal are `J0195` | 03 §5.1 | 1.0 |
| `data_class` | `"operational"` \| `"personal"` \| `"sensitive"` | no | `"operational"` | §6.7; reserved until implemented (`J0290`) | ADR 0079 | 1.0 (reserved) |
| `kind` | `"scalar"` \| `"block"` | no | `"scalar"` | `block` is M7 | 08 R3 | **1.1** |
| `block` | `Integer` (samples per block) | cond. | — | required with `kind = "block"` | 08 R3 | **1.1** |
| driver address keys (`line`, `bias`, `active_low`, `edges`, `timestamp`, `drive`, `topic`, `node`…) | appendix A | | | only in drivers without a profile; on a profile signal, the address belongs to the profile (`J0176`) | track 02, 09, 08 R2 | 1.0 |

**Adjusting a profile signal or declaring a signal.** On a signal that comes from a profile, the manifest only accepts `alias`, `max_age`, `limits`, `stuck_detect`, `deadband`, `debounce`, `scale`, `offset`, `unit` (same dimension), `poll` and `record`; it is the list of `ref/03` §2.2.4. The other keys (`type`, `direction`, address keys) are only used in the drivers without a profile, and the address keys are the `address` schema of the driver's descriptor (`ref/06` §2.3), with the same names as in `ref/03` §2.2.5.

**How the adjustments compose** (ADR 0075). The adjustments of a signal are those of its `[devices.<d>.signals.<s>]` table and of the `[signals."<path>"]` entries that reach it (§5.9). On top of the profile's `raw × scale + offset`, in the profile's unit: the manifest's `scale` (a factor) and `offset` apply in that order, still in the profile's unit; `scale = { from, to }` maps `from` onto `to` instead, and the signal takes the unit of `to`; `unit` then converts to another unit of the same dimension. The chain is one `raw × scale + offset` in the signal's final unit, which the acquisition stage applies and the output stage inverts (`ref/06` §2.7). An integer signal that gains a `scale` or `offset` becomes `f64` (`ref/03` §2.2.7). `limits`, `deadband`, `stuck_detect` and `debounce` are in the final unit and replace the profile's; a profile `deadband` the manifest does not replace follows its `scale` and `unit`. `max_age` replaces 3 × `poll`. `stuck_detect` compares the value before the deadband. An adjustment that does not fit stops the generation from loading with its code.

#### 5.8.2 `[devices.<d>.actions.<a>]`

```toml
[devices.pcs.actions.reset_fault]
permit  = "pcs.state == PcsState.fault"
role    = "operator"
timeout = "5s"
```

The action must exist in the profile (`J0101`). The implementation (steps, registers) belongs to the profile, not to the manifest.

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `permit` | `Expr<bool>` | no | `"true"` | permissive evaluated before each invocation (`Uncertain` counts as false); one that does not compile against the plan stops the generation from loading with the expression's code, and a declared permit the runtime cannot evaluate denies the invocation (ADR 0060); its windows are sized with the period of the fastest group that owns an output of the device, or 100 ms (ADR 0068) | 03 §5.2 | 1.0 |
| `role` | `"viewer"` \| `"operator"` \| `"admin"` | no | `"operator"` | minimum role of whoever invokes through the CLI, API, HMI or bridge; `"viewer"` is accepted with the warning `J0298` and counts as `"operator"`, because the API requires at least `operator` for any action (`ref/09`) | 03 §5.2 | 1.0 |
| `timeout` | `Duration` | cond. | that of the profile | without either of the two, `J0142` | 03 §5.2 | 1.0 |
| `dedup_window` | `Duration` | no | `"60s"` | window of the idempotency key (16, E9) | 16 E9 | 1.0 |

An **app** only invokes an action through an `Action` port bound in its `bind`: the binding is the authorization, and `role` does not apply to it.

### 5.9 `[signals."<path>"]`

Adjustments independent of the device that provides the signal. They serve the common manifest of a sim/hardware pair of overlays (track, parts 4–5):

```toml
[signals."panel.button"]
debounce = "20ms"

[signals."panel.pot"]
deadband = "0.01 V"
```

- The key is a path (canonical, alias or instance member) or a selector (`J0155` if it matches nothing).
- Accepted keys: `debounce`, `deadband`, `max_age`, `limits`, `stuck_detect`, `record`, with the rules of §5.8.1, and `data_class` (§6.7, reserved). Any other is `J0140`.
- **One adjustment per key and per signal.** If `[signals]` and `[devices.*.signals]`, or two entries of `[signals]` (through selectors), define the same key for the same signal with **different** values, it is `J0175`. With the same value, it is accepted: that is the case of a common manifest and an overlay that declare the same `debounce`.
- On derived signals (`calc.*`, `app.*`, `alarm.*`), only `record` and `data_class` apply (`J0176`).

### 5.10 `[outputs."<path>"]`: the envelope

Every output needs an entry here (invariant 5 of `AGENTS.md`). The key is the path of the output (canonical, alias or `out` member of an instance) or a selector; the envelope of a selector applies to each **output** it catches (the inputs it catches are ignored, and a selector that catches no output is `J0155`). **An output has a single envelope** (16, P2): declaring it through two paths, or through a path and a selector that also catches it, is `J0215`.

```toml
[outputs."pcs.p_set"]
range          = ["-125 kW", "125 kW"]
on_range       = "clamp"
rate           = "25 kW/s"
deadband       = "0.5 kW"
min_interval   = "200ms"
max_actuations = "120/h"
safe           = "0 kW"
safer          = "equal"
safe_ramp      = "50 kW/s"
interlock      = "meter.f > 49.8 Hz && meter.f < 50.2 Hz && bms.soc >= 5 % && !alarm.bms_fault.active"
on_interlock   = "safe"
requires_good  = ["meter.p_total", "bms.soc"]
grace          = "2s"
max_lease      = "15m"
max_manual     = "8h"
verify         = { readback = "pcs.p_set_rb", tolerance = "1 kW", within = "2s" }
reset_when     = "bms.state != BmsState.fault"
group          = "fast"
```

**Keys of the envelope**, in the order of the steps of document 01, §7:

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `safe` | `Value<S>` or `"relinquish"` | **yes** | — (none; ADR 0006) | missing `J0201`; type `J0231`; must be constant `J0232`; outside the constant `range` `J0203`; `relinquish` only in drivers that support it `J0207` | 03 §6 | 1.0 |
| `safer` | `"lower"` \| `"higher"` \| `"equal"` | no | `"equal"` | how to compare the equipment fallback with `safe` (M1, `J0205`); with `relinquish` it does not apply; without a device fallback it is ignored (`J0238`, warning); on a `string` or `bytes` output only `"equal"` (`J0233`) | 03 §6 | 1.0 |
| `safe_ramp` | `Rate<S>` | no | — (going to `safe` is immediate) | `J0202`; on a `string` or `bytes` `J0233` | 03 §6 | 1.0 |
| `interlock` | `Expr<bool>` | no | — (always permitted) | step 2; `Bad` counts as false; `Uncertain` too, unless `permit_uncertain` | 03 §6 | 1.0 |
| `permit_uncertain` | `Bool` | no | `false` | applies to `interlock` and to the `range` limits with expressions (E6) | 03 §9.1 | 1.0 |
| `on_interlock` | `"safe"` \| `"hold"` | no | `"safe"` | 16 E3; also applies to `requires_good` (16 E4) | 03 §6 | 1.0 |
| `requires_good` | `List<Path \| Selector>` (set) | no | `[]` | step 3; unknown path `J0101` | 03 §6 | 1.0 |
| `grace` | `Duration` | no | `"0s"` | 16 E4; accepts zero | 03 §6 | 1.0 |
| `range` | `[Limit, Limit]`, with `Limit` = `Quantity<S>` or `Expr<number>` | no | — (no range step) | two elements, minimum < maximum when both are constant (`J0230`); dimension `J0107`; expressions E6; on a `bool`, `enum`, `string` or `bytes` `J0233` | 03 §6 | 1.0 |
| `on_range` | `"clamp"` \| `"reject"` \| `"trip"` | no | `"clamp"` | step 4; on a `string` or `bytes` `J0233` | 03 §6 | 1.0 |
| `rate` | `Rate<S>` | no | — | step 5; relative to the last applied value (16 E2); `J0202`; on a `bool`, `enum`, `string` or `bytes` `J0233` | 03 §6 | 1.0 |
| `deadband` | `Quantity` (difference) | no | — | step 6; on a `bool`, `enum`, `string` or `bytes` `J0233` | 03 §6 | 1.0 |
| `min_interval` | `Duration` | no | `"0s"` | step 6 | 03 §6 | 1.0 |
| `max_actuations` | `Budget` | no | — | step 6: an actuation is an applied write with a value different from the previous one; `Suppressed` and keepalive do not count (16 E8); exhausted → `Rejected(Budget)` and alarm `joker.output.<s>.budget`; syntax `J0159` | 03 §6 | 1.0 |
| `max_lease` | `Duration` | no | `"15m"` | ≤ `"24h"` (`J0255`); lease of each writer ≤ `max_lease` (`J0213`) | 03 §6 | 1.0 |
| `max_manual` | `Duration` | no | `"8h"` | ≤ `"7d"` (`J0255`); limit of the TTL of manual writes (16 A10); 15 min before the end of a TTL, alarm `joker.output.<s>.manual_expiring`; a manual level restored at startup without a `synced` clock, alarm `joker.output.<s>.manual_restored` (16 A14; both `medium`, §5.15) | 03 §6 | 1.0 |
| `verify` | table | no | — | below | 03 §6 | 1.0 |
| `reset_when` | `Expr<bool>` | no | — (only `jk reset`) | 16 E7; on an output without `on_range = "trip"` and without an alarm with `trip` on it, it is `J0236` (warning) | 03 §6 | 1.0 |
| `group` | `Name` from `[cycles]` | no | the A11 rule | nonexistent group `J0237` | 03 §6 | 1.0 |
| `keepalive` | `Duration` | no | the device's `fallback.keepalive` | the period at which the output stage renews this output's device fallback (ADR 0072); only on an output its device fallback covers (`[fallback] applies_to`), and below that fallback's timeout (`J0276`) | 01 §8, new | 1.0 |
| `pulse` | `Duration` | no | — | only `bool` (`J0233`); after applying a value ≠ `safe`, the output returns to `safe` once `pulse` has elapsed: the output stage writes the value and then `safe` (`ref/06` §2.9); a `resync` or `keepalive` outside a pulse writes `safe`; the applied value stays the writer's (ADR 0072) | 03 §6 | 1.0 |
| `remote_levels` | `List<Level>` (set) or string `"<n>-<m>"` (`"3-16"`) | no | the levels that no local writer of the MCU uses | **only on a Joker Micro**: arbiter levels delegated to the link (16 A12, `ref/07` §15); on a Linux node `J0145`; a level also used by a local writer `J0210` | `ref/07` §15 | 1.0 |
| `accept_missing_fallback` | `Text` (justification ≥ 10 characters) | no | — | turns the `J0205` for a missing fallback into a warning, and `jk lock` copies it to `[accepted]`; on an output that has a fallback it is `J0277` (warning) | 01 §8, track 02 | 1.0 |
| `widen` | `Text` or `Table<key, Text>` | no | — | overlays (§7.4) | 03 §2 | 1.0 |
| `schedule_depth` | `Integer` | no | `0` (scheduled writes off) | 0–256 (`J0239`) | 02 §7.2 | **1.1** |
| `timed` | `Bool` | no | `false` | driver without timed outputs `J0239` | 02 §7.2 | **1.1** |
| `on_late` | `"apply_now"` \| `"drop"` \| `{ apply = Value<S> }` | cond. | — (none) | required with `schedule_depth > 0` (`J0206`) | 02 §7.2 | **1.1** |
| `late_margin` | `Duration` | no | one period of the owning group (`timed = false`) or the declared latency of the driver (`timed = true`) | the `margin` of document 02, §7.2 | new | **1.1** |

**`verify`:**

| Key | Type | Req. | Default | Validation |
|---|---|---|---|---|
| `verify.readback` | `Path` of an input | yes | — | must be an input, with the same dimension (`J0235`); on a `bool` output a `bool`, on an `enum` output one with the same variants (`J0235`) |
| `verify.tolerance` | `Quantity` (difference) | cond. | — (exact equality) | required on `f64`/`f32` outputs (`J0235`); on a `string` or `bytes` output `J0233` |
| `verify.within` | `Duration` | no | 2 × `poll` of the `readback` device (1 s without a declared `poll`) | ≥ one `poll` of that device (`J0235`) |

An output that does not confirm becomes `Uncertain(NotConfirmed)` and fires `joker.output.<s>.unconfirmed` (document 01, §7). The readback **confirms** when it is `good` and within `tolerance` of the applied value; the output does not confirm when it has disagreed for `within` without interruption, and the first readback that agrees makes it `good` again. A new applied value starts a new window, and `relinquish` is not verified (ADR 0072).

**Rules that involve several keys:**

- A key that is not an output path (direction ≠ `output`) is `J0234`. The outputs of a `joker-link` device are governed by the envelope of the Joker Micro: declaring them here too is `J0234` (document 19, §6).
- The virtual commands of an instance implemented by an app (`p1.run_cmd`, `light.mode`) are outputs like the others and need an envelope (document 14, §7.2).
- **Owning group** (A11): `group`, if declared; otherwise, the group of the highest-priority app that writes the output in a cycle; otherwise, the implicit group `default` (100 ms).
- **Arbiter defaults** (A9, A10): `lease` = 3 × period of the owning group; `max_lease` = 15 min; `max_manual` = 8 h.

### 5.11 `[cycles.<group>]`

```toml
[cycles.fast]
period     = "200ms"
resources  = "rt"
overrun    = "skip"
trip_after = 10
trace      = { continuous = "6h" }
order      = ["hello_world", "status_light_ctl"]

[cycles.drive]
period     = "1ms"
overrun    = "trip"
trip_after = 3
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `period` | `Duration` | yes | — | Linux: 1 ms–1 h; Joker Micro: 100 µs–1 h (`J0280`); < 10 ms without `kernel = "preempt_rt"` `J0351`; ≥ 10 ms without it `J0352` (warning); admission `J0350` | 03 §7 | 1.0 |
| `resources` | `"rt"` | no | `"rt"` when `[resources.rt]` exists; otherwise, no class (no guaranteed RT) | another class `J0282` | 03 §7 | 1.0 |
| `overrun` | `"skip"` \| `"catch_up"` \| `"degrade"` \| `"trip"` | no | `"skip"` | document 01, §4; `degrade` without `optional` tasks `J0286` (warning); `catch_up` with outputs that have `rate`, `min_interval` or `max_actuations` `J0287` (warning) | 03 §7 | 1.0 |
| `trip_after` | `Integer` ≥ 1 | no | `1` with `overrun = "trip"`; otherwise, no trip | N **consecutive** overruns put the group's outputs in trip | 03 §7 | 1.0 |
| `alarm_after` | `Integer` ≥ 1 | no | `10` | N consecutive overruns fire `joker.group.<g>.overrun` | 01 §4, new | 1.0 |
| `recover_after` | `Integer` ≥ 1 | no | `10` | clean ticks for `degrade` to turn the `optional` tasks back on | 01 §4, new | 1.0 |
| `order` | `List<"<app>" \| "<app>.<task>">` (ordered) | no | `[]` | entries that do not run in this group `J0281`; duplicates `J0164` | 03 §7 | 1.0 |
| `trace.continuous` | `Duration` | no | — (no continuous ring) | ≤ `"7d"`; the estimated disk usage counts toward `J0380` (`J0285` above the maximum) | 03 §7 | 1.0 |
| `trace.async` | `Bool` | no | `false` | also records the reads of the async tasks (document 09, attack 10) | 03 §9.1 | 1.0 |
| `cpu` | `Integer` | no | assigned by `jk check` (the least busy `rt` CPU, in the order of the periods) | must belong to `[resources.rt].cpus` (`J0284`) | new | 1.0 |
| `priority` | `Integer` 1–98 (`SCHED_FIFO`) | no | automatic rate-monotonic | a priority that inverts the rate-monotonic order between groups on the same CPU is `J0283` | new | 1.0 |

**Order of the tasks in the tick.** First those of `order`, in the order of the list (an entry `"<app>"` runs the tasks of that app in this group in the order in which the app declares them); then the remaining ones, in lexicographic order of `<app>.<task>`. The lexicographic order, and not "the order of the manifest", makes the result independent of the order of the overlay layers (appendix C).

**Implicit groups** (reserved names, `J0288`):

| Name | Period | Purpose | Can it be declared? |
|---|---|---|---|
| `default` | 100 ms | outputs written only by async tasks, bridges, CLI or async implementations (A11) | yes, `[cycles.default]`, to change `period`, `trace` or `cpu` |
| `default_<period>` (e.g. `default_10ms`) | the one requested by the task | cycle tasks of apps without `[apps.<a>.cycles]`: one group per distinct period, with no guaranteed RT | no |
| `calc` | 100 ms | `calc` and alarms that no group consumes | yes, `[cycles.calc]` |

### 5.12 `[apps.<app>]`

```toml
[apps.peak_shaver]
source    = "oci://registry.example.com/joker/peak-shaver:1.4.2"
digest    = "sha256:9f2c…"
writer    = { level = "optimizer", priority = 13, lease = "2s" }
resources = { async = "bg", memory = "32 MiB" }
cycles    = { control = "fast" }
net       = { allow = ["api.tariffs.example.com:443"] }
requires_clock = "synced"

[apps.peak_shaver.bind]
grid_power = "meter.p_total"
soc        = { signal = "bms.soc", as = "fraction" }
battery    = "pcs.p_set"

[apps.peak_shaver.params]
limit   = "400 kW"
reserve = "10 %"

[apps.peak_shaver.secrets]
api_token = "secret:tariff_api_token"
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `source` | `Ref` `oci://…` | cond. | — | exactly one of `source` and `path`, unless `kind = "native"` (`J0180`) | 03 §8 | 1.0 |
| `path` | `File` (app directory) | cond. | — | the directory exists (`J0160`); `jk build` produces the component | 03 §8 | 1.0 |
| `digest` | `"sha256:…"` | no | pinned in the lock | different from the lock or from the artifact `J0180` | 03 §8 | 1.0 |
| `kind` | `"wasm"` \| `"native"` \| `"extern"` | no | `"wasm"` | `native`: the app must be compiled into this build of the runtime (feature `native-apps`; on an MCU, the native backend of the SDK, `ref/08` §2.11.5), without `source` or `path` (`J0187`); `extern`: **only on a Joker Micro**, an external writer fed by the integrator's C or Rust code, with `writes`/`reads` and without `source`, `path`, `bind` or `cycles` (§5.22, `ref/08` §2.17; on a Linux node `J0145`) | 04 §5, 08 R4, `ref/08` | 1.0 |
| `writer` | table (below) | cond. | a single writer per output: `control:12`, lease of 3 periods | required if any output the app writes has more than one writer (`J0211`); on an app without outputs `J0257` (warning) | 03 §8 | 1.0 |
| `bind` | `Table<port, Binding>` | cond. | — | §5.12.1; required port without a binding `J0110` | 03 §8 | 1.0 |
| `params` | `Table<parameter, Value \| { value, runtime, persist }>` | no | the app's defaults | unknown parameter `J0121`; out of range `J0120`; unit `J0107`; parameter without a default and without a value `J0142`; the table form (`runtime = true`: changeable without a new generation; `persist = true`: the changed value survives restarts) only on a Joker Micro (§5.22, `ref/08` §2.6.l; on a Linux node `J0145`) | 03 §8, `ref/08` | 1.0 |
| `cycles` | `Table<task, group Name>` | no | implicit groups `default_<period>` (§5.11) | unknown task or group `J0182`; period outside the tolerance `J0130` (warning) | 03 §8 | 1.0 |
| `period_tolerance` | `Percentage` | no | that of the app; without it, `"50 %"` | `J0130` | 04 §3.2 | 1.0 |
| `resources.cycle` | `"rt"` | no | that of the group | must match that of each group where the tasks run (`J0186`) | 03 §8 | 1.0 |
| `resources.async` | resource class | no | `"bg"` | must not be `rt` (`J0186`) | 03 §8 | 1.0 |
| `resources.memory` | `Size` | no | — | sets `cycle_memory` and `async_memory` at the same time | 03 §8 | 1.0 |
| `resources.cycle_memory` | `Size` | no | the app's suggestion in `joker:meta`; without it, that of the tier: Rust `"16 MiB"`, TypeScript `"64 MiB"`, Python `"128 MiB"` (`ref/05` §19) | `memory.max` of the cycle instance (document 01, §9); the manifest wins over the suggestion | new | 1.0 |
| `resources.async_memory` | `Size` | no | likewise: Rust `"64 MiB"`, TypeScript `"64 MiB"`, Python `"128 MiB"` | `memory.max` of the async instance | new | 1.0 |
| `net.allow` | `List<"<host>:<port>">` (set) | no | `[]` (no network) | DNS host or IP, port required, no wildcards (`J0184`); async tasks only | 03 §8 | 1.0 |
| `secrets` | `Table<local name, "secret:<name>">` **(secret)** | no | `{}` | binds each secret the app declares in `joker:meta` (`ref/04`) to a secret of the site; a required secret of the app without a binding, or a local name the app does not declare, `J0927`; a value that is not `secret:` `J0294`; a name missing from the node's vault `J0185` (`error-in-prod`) (`ref/02` §2.4.7, D6) | 01 §17, `ref/02` | 1.0 |
| `requires_clock` | `"any"` \| `"holdover"` \| `"synced"` | no | `"any"` | the **strictest** between this key and the one declared by the app in `joker:meta` (`ref/04`) applies; without the requested quality, the app's outputs go to `safe` (document 01, §18) | 03 §9.1, `ref/04` | 1.0 |
| `keep_previous` | `Duration` | no | `"10m"` | time during which the previous version stays warm after a hot swap (document 01, §16) | 01 §16 | 1.0 |
| `crashloop` | `{ traps, window, max_backoff }` | no | `{ traps = 3, window = "10s", max_backoff = "5m" }` | document 01, §9 | 01 §9, new | 1.0 |
| `shadow` | table (§5.12.3) | no | — | M16 for apps | 03 §8 | 1.0 |
| `writes` | `List<Path \| Selector>` (set) of outputs | cond. | — | only with `kind = "extern"` (required there; with another `kind` `J0145`): the outputs on which the firmware proposes (`jk_propose`), with the app's `writer` | `ref/08` §2.17 | 1.0 |
| `reads` | `List<Selector>` (set) | no | `[]` | only with `kind = "extern"`: the signals the firmware reads through `jk_read`, besides its own outputs | `ref/08` §2.17 | 1.0 |
| `fallback` | table (§6.5) | no | — | M17 | 06 §7 | **1.1** |

**`writer`:**

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `writer.level` | `Level` | cond. | `"control"` | `J0250`; `manual` band `J0252` | 03 §8 | 1.0 |
| `writer.priority` | `Integer` 1–16 | no | the default of the band (§3.11) | outside the band `J0251`; two writers at the same level of the same output `J0210` | 03 §8 | 1.0 |
| `writer.lease` | `Duration` | no | 3 × period of the owning group of each output | ≤ `max_lease` of each output and, on an output of an MCU, ≤ `max_lease` of the link (`J0213`, 16 A12); < period of the owning group `J0256` | 03 §8 | 1.0 |
| `writer.kind` | `"direct"` \| `"proposal"` | no | `"direct"` | `proposal` (M18): without `level`, `priority` or `lease` (`J0254`); direct writes `J0212` | 03 §8 | **1.1** (`proposal`) |

An app has **one** writer (A1): the cycle and async instances count as the same one, at the same level, on all the outputs it writes.

#### 5.12.1 Forms of `bind`

| Form | Port | Example | Validation |
|---|---|---|---|
| `"<path>"` | scalar (`Input`, `Output`, `Action`, `EventInput`, `EventOutput`) | `grid_power = "meter.p_total"` | type and unit (`J0107`, `J0108`); unknown path `J0101` |
| `"<instance>"` | class (`#[device] led: Led`) | `led = "panel_led"` | class and major (`J0630`); requested capabilities (`J0602`) |
| `"<selector>"` | `InputMany`, `OutputMany`, `Many<T>` | `inverters = "inv*.w_max_lim_pct"`, `vavs = "vav_3*"` | no matches `J0155`; limits of `Many` `J0610` |
| `["a", "b", …]` | `InputMany`, `OutputMany`, `Many<T>` | `pumps = ["p1", "p2", "p3"]` | the order of the list is the order in the app |
| `"class:<class>"` | `Many<T>` | `valves = "class:valve.motor@1"` | all the instances of the class, in name order |
| `{ signal, as, optional }` | scalar | `soc = { signal = "bms.soc", as = "fraction" }` | below |
| `{ each, map }` | `Many<LocalClass>` (document 14, §6.5) | `zones = { each = "bms.zone_*", map = { temp = "{}.temp" } }` | below |
| `"model:<m>"`, `"pipe:<p>"` | model port, pipeline event port | `forecast = "model:load_forecast"` | **1.1** (`J0290`) |

Keys of the scalar table form:

| Key | Type | Req. | Default | Validation | Origin |
|---|---|---|---|---|---|
| `signal` | `Path` | yes | — | | 03 §8 |
| `as` | `"fraction"` \| `"percent"` | no | — | `fraction` binds a signal in `%` to a port in `1` (÷100); `percent` the opposite (×100); other units `J0183` | 02 §3, new (`percent`) |
| `optional` | `Bool` | no | `false` | with `true`, a `signal` that does not exist on this site leaves the port unbound, which is only accepted if the port is `optional` in the code (otherwise `J0110`) | 08 R5 |

Keys of the `{ each, map }` form:

| Key | Type | Req. | Validation |
|---|---|---|---|
| `each` | `Selector` with `*` in a segment | yes | matches **prefixes** of paths (nodes of the tree), and not signals; each prefix is an element, in lexicographic order |
| `map` | `Table<member, template>` | yes | each template contains `{}`, replaced by the prefix (`"{}.temp"` → `bms.zone_01.temp`); missing member of the local class `J0601` |

A form that does not fit the type of the port (a list on a scalar port, `each` on a port that is not a `Many` of a local class, `as` on a `bool` port) is `J0181`. A port bound in two places (in the app's `bind` and in `implemented_by.bind`) is `J0189`.

#### 5.12.2 Parameters

The values of `params` are `Value` (§3.6) validated against the app's declaration in `joker:meta`: a quantity with a unit of the same dimension, a number in `1`, `bool`, string or enum variant. A parameter changed in a new generation reaches the app through `on-params`, without restarting (track, part 1).

#### 5.12.3 `shadow` (M16, apps; reserved 1.1)

Shadow mode for apps is **1.1** (document 26, §5; ADR 0035). The keys below exist in the 1.0 schema, and using them on a 1.0 runtime is `J0290`.

```toml
[apps.hvac_policy.shadow]
of      = "none"
until   = { duration = "14d" }
promote = { outputs = "ahu3.**" }
```

| Key | Type | Req. | Default | Validation | Origin |
|---|---|---|---|---|---|
| `of` | `"<app>@<version>"` or `"none"` | yes | — | the active version, installed on the node or pinned in the lock (`J0188`); `none` when there is no active version (the shadow is compared with what the existing system does) | 03 §8, 08 R6 |
| `until.duration` | `Duration` | cond. | — | at least one criterion in `until` (`J0142`) | 03 §8 |
| `until.max_divergence` | string `"<quantity> <p50\|p95\|p99\|max>"` | no | — | difference between the shadow's proposals and the applied ones; dimension of the output | 03 §8 |
| `promote.outputs` | `Selector` or `List<Path>` | no | all the outputs of the app | partial promotion (document 08, R6b) | 08 R6 |
| `promote.auto` | `Bool` | no | `false` | with `true`, promotes on meeting `until`; otherwise, waits for `jk shadow promote` | 06 §10, new |

The version declared in `source` is the candidate, and it runs with `Shadowed` writes; the one in `of` keeps writing.

### 5.13 `[instances.<instance>]`

An instance comes into being in **one** of three ways: composite (`map`), virtual (`implemented_by`) or simulated (`sim`). More than one is `J0192`, with one exception: a virtual instance of a class with class members binds those members in a `map` that only has class members (below). The implicit ones come from the profiles and are not declared here (document 14, §5.1).

```toml
[instances.p1]
class        = "std/pump@1"
capabilities = ["pressure"]
map          = { run_cmd = "io.do_1", running = "io.di_1", fault = "io.di_2", pressure_out = "ai.ch1" }

[instances.p2]
class          = "pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_3", feedback = "io.di_3", thermal_trip = "io.di_4" } }

[instances.p2.actions.reset]
role = "operator"

[instances.termo2]
class = "temp_sensor@1"
sim   = { mean = "22 °C", amp = "3 K", period = "10m" }
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `class` | `Ref` of a class (`"<package>/<name>@<major>"`, `std/` optional) | yes | — | unknown class `J0193`; major different from the one requested by the app `J0630` | 03 §9.2 | 1.0 |
| `capabilities` | `List<Name>` (set) | no | `[]` | a capability the class does not have `J0193`; capability members without a mapping `J0602` | 03 §9.2 | 1.0 |
| `map` | `Table<member, Mapping>` | cond. | — | missing required member `J0601`; `const:` on an `out` `J0603`; wrong adapter `J0194`; class members `J0650`–`J0654` (below), `J0656` on a virtual one; on a virtual instance, only class members (`J0192`) | 03 §9.2, 17 §8 | 1.0 |
| `implemented_by` | `"app:<app>"` or `{ app, bind, params }` | cond. | — | `J0191`; chain > 3 layers `J0620` (the depth of the class is `JC0501`, separately) | 03 §9.2, 17 §5.3 | 1.0 |
| `sim` | `Bool` or `Table` (parameters of the class simulator, `ref/03`) | no | `false` | only with the `sim` driver available; in `prod` `J0145`; class simulators are reserved for 1.0.x (`J0290`) | 03 §9.2 | 1.0.x |
| `actions` | `Table<action, configuration>` | no | the suggestions of the class (`@timeout`, `@permit_hint`, `@role_hint`) | the keys of §5.8.2; the manifest wins (17, §4.2) | 17 §4.2 | 1.0 |

**`implemented_by`:**

| Form | Effect |
|---|---|
| `"app:<app>"` | the instance is served by the app `[apps.<app>]`, with its `bind`, `params` and `cycles`. An app like this serves **one** instance |
| `{ app = "<app>", bind = { … }, params = { … } }` | creates **one instance of the app per virtual instance** (document 17, §5.3). `bind` and `params` are merged over those of `[apps.<app>]`, key by key; `cycles`, `writer` and `resources` are those of the app |

The app must implement the class (`#[implements]`, with the same major and the contract hash in the lock); otherwise, `J0191`.

**`Mapping`** (values of `map`):

| Form | Member | Example |
|---|---|---|
| `"<path>"` | any | `run_cmd = "io.do_1"` |
| `"<device>.<action>"` | `action` | `reset = "vfd1.reset_fault"` |
| `"const:<value>"` | `in`, `param` | `rated_flow = "const:30 m3/h"` |
| `{ signal, scale, offset, invert }` | `in`, `out`, `param` | `speed_set = { signal = "speed_ref", scale = 0.01 }` |
| `{ signal, enum_map = { variant = n } }` | enum | `mode = { signal = "op_mode", enum_map = { off = 0, standby = 3 } }` |
| `{ edge = { rise = "<action>", fall = "<action>" } }` | `out` `bool` | document 17, §5.2 |
| `{ on_rise = "<path>" }`, `{ on_change = "<path>" }`, with `fields = { field = "<path>" }` | `event` | `tripped = { on_rise = "io.di_2" }` |
| `"<instance>"` | fixed class member (`member boot: std/button@1`) | `boot = "button"` |
| `["<instance>", …]` | collection class member (`member pumps: std/pump@1[1..8]`) | `pumps = ["p1", "p2", "p3"]` |

The members of an instance are aliases `<instance>.<member>` (16, P3); the collision rules are those of P3 (`J0604`).

**Class members** (document 17, §8). The value is the **name of an instance** of the site (implicit, composite, virtual or simulated), and not a path; in a collection, a **list** of names, and the order of the list is the order of the collection (that of `iter()` and of `index()`). `jk check` checks:

| Check | Code |
|---|---|
| the name is that of an instance (a signal path here gives the suggestion of the instance that has it) | `J0650` |
| the class and the major of the instance are those of the member (`std/button@1`); the minor may differ (17 §9) | `J0651` |
| the instance has the capabilities that the member requires with `@requires` | `J0652` |
| the number of elements fits in `[min..max]`; a fixed member does not accept a list, and a collection accepts a list with a single name | `J0653` |
| there are no cycles: an instance is not a member of itself, directly or indirectly | `J0654` |
| required member left unbound; member of a declared capability left unbound | `J0601`; `J0602` |

- The nested paths (`esp_io.boot.clicked`) are aliases of the members of the bound instance (16, P6).
- An instance can be a member of several compositions. Its `out` members have a single envelope and a single arbiter, and the writes that arrive through each composition belong to the writer that called the method (16, P6). The warning `J0655` flags the sharing between different apps.
- **On a virtual instance**, the scalar members belong to the implementation, and the class members are bound in a `map` with only class members, next to `implemented_by`. A scalar member in that `map` is `J0192`. The class members of a virtual instance belong to the implementation: whoever binds the instance reads them, but does not write to them (16, P6); a class whose methods call methods of the members cannot have virtual instances (`J0656`).
- **Depth:** that of the class is counted in `jkc` (`JC0501`, along the chain of class members, 17 §8); the implementation layers are counted here (`J0620`, along the chained virtual instances). They are two limits of 3 that do not add up.

### 5.14 `[calc]`

```toml
[calc]
site_load  = "meter.p_total - pcs.p_ac"
rse_limit  = "if rse.k4 then 0 % else if rse.k3 then 30 % else if rse.k2 then 60 % else 100 %"
grid_limit = "400 kW"
speed = { expr = "clamp(avg(panel.pot, 200ms) / 3.3 V, 0, 1)", group = "main" }
```

| Form | Type | Validation | Origin | Phase |
|---|---|---|---|---|
| `<name> = "<expr>"` | `Expr<scalar>` | creates `calc.<name>` with the inferred type and unit; `J03xx`; cycle between `calc` `J0190`; name with dots `J0150` | 03 §10 | 1.0 |
| `<name> = { expr, group }` | `expr`: `Expr`; `group`: `Name` from `[cycles]` | `group` fixes where the `calc` is evaluated (nonexistent group `J0237`) | new | 1.0 |

A constant (`"400 kW"`) is a valid expression and gives a `Good` signal with the fixed value. Without `group`, a `calc` runs in the fastest group that consumes it, or in the implicit group `calc`.

### 5.15 `[alarms.<alarm>]`

```toml
[alarms.grid_overfreq]
when          = "meter.f > 50.2 Hz"
delay_on      = "2s"
delay_off     = "5s"
severity      = "high"
suppress_when = "site.mode == SiteMode.maintenance"
message       = "Grid overfrequency: {meter.f}"
actions       = { trip = ["pcs.p_set"] }
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `when` | `Expr<bool>` | yes | — | `J03xx`; `Bad` → state `Unknown` (16 §3.2) | 02 §11 | 1.0 |
| `severity` | `"low"` \| `"medium"` \| `"high"` \| `"critical"` | yes | — | `J0144` | 02 §11 | 1.0 |
| `delay_on` | `Duration` | no | `"0s"` | | 02 §11 | 1.0 |
| `delay_off` | `Duration` | no | `"0s"` | | 02 §11 | 1.0 |
| `suppress_when` | `Expr<bool>` | no | — | with `true`, the alarm does not become active (it keeps evaluating) | 02 §11 | 1.0 |
| `message` | `Text` with `{<path>}` placeholders (`{{` and `}}` escape) | no | the name of the alarm | unknown path `J0302` | 02 §11 | 1.0 |
| `actions.trip` | `List<Path \| Selector>` of outputs (set) | no | `[]` | trip of the outputs on **entering** active (after `delay_on`); not an output `J0234`; without matches `J0155` | 02 §11 | 1.0 |
| `max_shelve` | `Duration` | no | `"24h"` | maximum TTL of `jk alarm shelve` | new | 1.0 |
| `group` | `Name` from `[cycles]` | no | the fastest one that consumes it, or `calc` | `J0237` | new | 1.0 |

Each alarm creates `alarm.<alarm>.state` (`enum`, `ref/00` §6) and `alarm.<alarm>.active` (`bool`), and an event series. The alarms declared by an app have the id `app.<app>.<name>` and the paths `alarm.app.<app>.<name>.state` and `.active` (`ref/04`); that is why the name `app` is reserved for the alarms of the manifest, and an alarm named `app`, or a name that collides with another created path, is `J0196`. A `jk reset` of an output tripped by an alarm that is still active trips again on the next tick: the reset only "takes" once the condition has passed.

**Runtime alarms (`joker.*`).** They are not declared in the manifest, but it is the keys of this reference that enable or tune them. This is the list that `ref/09` §5.8 refers to; document 01, §19, gives the `joker.*` metrics signals.

| Alarm | When | Key that controls it | Where it is defined |
|---|---|---|---|
| `joker.output.<s>.budget` | actuation budget exhausted (`Rejected(Budget)`; `high`) | `outputs.*.max_actuations` | §5.10, 16 E8 |
| `joker.output.<s>.unconfirmed` | the output did not confirm through the `readback` (`high`) | `outputs.*.verify` | §5.10, 01 §7 |
| `joker.output.<s>.manual_expiring` | 15 min before the end of the TTL of a manual write (`medium`) | `outputs.*.max_manual` | 01 §6, `ref/09` §5.6 |
| `joker.output.<s>.manual_restored` | manual level restored at startup without a `synced` clock (`medium`) | `outputs.*.max_manual` | 16 A14 |
| `joker.group.<g>.overrun` | `alarm_after` consecutive overruns (`high`) | `cycles.*.alarm_after` | §5.11, 01 §4 |
| `joker.group.<g>.stalled` | the group has not ticked for more than three periods, or the supervisor's `force_safe` holds one of its outputs (`critical`) | — (the supervisor's fixed threshold) | 01 §20, ADR 0086 |
| `joker.device.<d>.degraded`, `.offline`, `.drift` | link state of the device (`medium`, `high`, `high`) | `devices.*` | 05 §12, `ref/06` §2.12 |
| `joker.store.disk_full` | the historian's disk is critical: only alarms and decisions are recorded (`high`) | `telemetry.store.max_disk` | 01 §20, ADR 0081 |
| `joker.driver.<n>.crashloop` | W or X driver in crashloop (critical) | — (fixed threshold; the backoff is `drivers.*.restart`) | `ref/06` §4.5, §5.7 |
| `joker.node.config_invalid`, `joker.node.crl_expired` | invalid `node.toml`; expired CRL | — (node configuration) | `ref/02` §2.2.2, §3.2.1 |
| `joker.agent.*` | agent state | — (node configuration) | `ref/10` §2.18 |

The runtime alarms follow the lifecycle of docs/02 §11 without delays, can be acknowledged and shelved (`max_shelve` 24 h), and every transition is in the alarm journal and the decision log (ADR 0085). In 1.0 `joker.driver.<n>.crashloop`, `joker.node.*` and `joker.agent.*` are not raised: the runtime has no W or X drivers, does not read `node.toml` and has no agent yet.

### 5.16 `[atomic.<group>]` (M20)

```toml
[atomic.pcs_cmd]
outputs = ["pcs.mode", "pcs.p_set", "pcs.q_set"]
order   = ["pcs.mode", "pcs.p_set", "pcs.q_set"]
on_rate = "independent"
commit  = { signal = "pcs.apply", value = 1 }
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `outputs` | `List<Path>` (set, ≥ 2) | yes | — | each one with an envelope; an output in two groups `J0270`; outputs of devices without a common transport (same device, or same bus with a common frame, like EtherCAT) `J0271`; different owning groups `J0272` | 02 §7.3 | 1.0 |
| `order` | `List<Path>` (ordered) | no | the order of `outputs` | permutation of `outputs` (`J0273`) | 02 §7.3 | 1.0 |
| `on_rate` | `"independent"` \| `"proportional"` | no | `"independent"` | 02 §7.3 | 03 §10 | 1.0 |
| `commit.signal` | `Path` | cond. | — | output of the same device, outside `outputs`, without its own `[outputs]` and without bindings (`J0274`) | 02 §7.3 | 1.0 |
| `commit.value` | `Value` | cond. | — | required with `commit.signal` | 02 §7.3 | 1.0 |

The `commit` signal is a **group command register**: only the core writes it, right after the writes of the group, and it has no arbiter and no envelope. The `atomic` methods of classes generate such groups automatically over the members of the instance (document 14, §8, A5).

### 5.17 `[telemetry]`

#### 5.17.1 `[telemetry.record."<selector>"]`: the local historian

```toml
[telemetry.record]
"meter.**"          = { every = "1s", keep = { raw = "7d", "1m" = "400d" } }
"pcs.p_set"         = { on_change = true, keep = { raw = "90d" } }
"vision.detections" = { keep = { raw = "7d" } }
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `every` | `Duration` | cond. | — | periodic sampling of the last value; ≥ `"100ms"` | 03 §11 | 1.0 |
| `on_change` | `Bool` | cond. | `false` | one sample per new `seq`, with the acquisition deadband of the signal | 03 §11 | 1.0 |
| `keep` | `Table<layer, Duration>` | no | `{ raw = "7d" }` | layers `raw`, `1s`, `1m`, `15m` (document 01, §12); another layer `J0197` | 03 §11 | 1.0 |
| `blocks` | `true` \| `"on_trigger"` | cond. | — | block signals (M7) | 03 §11, 08 R3 | **1.1** |
| `trigger` | `Expr<bool>` | cond. | — | required with `blocks = "on_trigger"` | 08 R3 | **1.1** |
| `pre` | `Duration` | no | `"0s"` | RAM ring before the trigger; counts toward `J0380` | 08 R3 | **1.1** |
| `post` | `Duration` | no | `"0s"` | | 08 R3 | **1.1** |

- On a scalar signal, exactly one of `every` and `on_change` (`J0195`). On an event, neither: all occurrences are recorded. On a block signal, `blocks`.
- **Overlapping selectors:** for each signal the **most specific** entry applies: an exact path beats a selector; among selectors, the one with more literal segments wins, and then the longest literal prefix. A tie is `J0195`. This is the only section in which specificity resolves overlaps, because recording is not a safety decision; in `[outputs]` and `[signals]`, an overlap is an error.
- `on_change` reads the signal every 10 ms: changes faster than that are coalesced to the last one. `string`, `bytes` and `record` signals and signals with shape `event` are not recorded in 1.0 (ADR 0082).
- The decision log, the trips and the alarm transitions are always recorded, regardless of this section. The log stores each change of result, and `fsync = "always"` applies to trips, manual writes, rejections and level changes; the rest goes in batches (document 01, §12 and §19).
- A signal that no entry catches is not recorded.

#### 5.17.2 `[telemetry.store]`

```toml
[telemetry.store]
max_disk = "8 GiB"
fsync    = "batch"
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `max_disk` | `Size` | no | 25% of the node's data partition, measured at `jk apply` | retention by size beats retention by time (document 01, §12) | 03 §11 | 1.0 |
| `fsync` | `"batch"` \| `"always"` \| `"never"` | no | `"batch"` | `batch` also syncs at once after alarm and decision records; `never` syncs only at checkpoints (ADR 0081) | 03 §11 | 1.0 |
| `fsync_interval` | `Duration` | no | `"1s"` | only with `batch` | 01 §12, new | 1.0 |

The on-disk path of the historian is node configuration (`ref/02`), not site configuration.

#### 5.17.3 `[telemetry.uplink.<uplink>]`

```toml
[telemetry.uplink.cloud]
kind     = "mqtt-sparkplug"
broker   = "mqtts://broker.example.com:8883"
group_id = "bess"
budget   = "300 MB/month"
classes  = { alarms = "alarm.**", high = ["meter.p_total", "pcs.**"], low = "**" }
live     = { every = "5s" }
history  = { delivery = "at_least_once" }
password = "secret:mqtt_password"
```

**Common keys:**

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `kind` | `"mqtt"` \| `"mqtt-sparkplug"` \| `"zenoh"` \| `"otlp"` \| `"http"` | yes | — | `J0144`; `otlp` and `http` are reserved for 1.1 and `zenoh` for 1.2 (`J0290`) | 03 §11, 01 §13 | 1.0 (`mqtt`, `mqtt-sparkplug`); 1.1 (`otlp`, `http`); 1.2 (`zenoh`) |
| `classes` | `Table<class, Selector \| List<Selector>>` | no | `{ alarms = "alarm.**" }` | classes `alarms`, `events`, `high`, `low`, `bulk`, in this order of priority (document 01, §13); another one `J0198`; a signal goes in the highest-priority class that catches it | 03 §11 | 1.0 |
| `budget` | `DataVolume` | no | — (no budget) | M12; alarms are never degraded | 03 §11 | 1.1 |
| `live.every` | `Duration` | no | `"5s"` | live values, `latest` delivery | 03 §11 | 1.0 |
| `history.delivery` | `"at_least_once"` | no | `"at_least_once"` | store-and-forward from the historian; a sent signal that is not recorded only goes live (`jk check` note) | 03 §11 | 1.0 |
| `history.enabled` | `Bool` | no | `true` | | new | 1.0 |
| `username` | string | no | — | | new | 1.0 |
| `password`, `token` **(secret)** | `Ref` `secret:` | no | — | plaintext `J0294` | new | 1.0 |
| `accepts` | `Set<"operational" \| "personal" \| "sensitive">` | no | `["operational"]` | data classes the uplink may send (§6.7); a selected signal of another class `J0261`; reserved until implemented (`J0290`) | ADR 0079 | 1.0 (reserved) |

The default of `classes` only sends alarms: nothing leaves the node without being asked for (document 12, §10). Uplink budgets (M12) are **1.1** (document 26, §5; ADR 0035): in 1.0, `budget` is `J0290`, and an uplink has no budget (the classes keep their meaning).

**Keys by `kind`:**

| `kind` | Key | Type | Req. | Default | Validation |
|---|---|---|---|---|---|
| `mqtt`, `mqtt-sparkplug` | `broker` | `mqtt://` or `mqtts://` URL with port | yes | — | `mqtt://` in `prod` `J0296` |
| `mqtt-sparkplug` | `group_id` | string | yes | — | |
| `mqtt-sparkplug` | `node_id` | string | no | the node name (enrollment) | |
| `mqtt` | `topic` | template with `{node}`, `{site}`, `{path}` | no | `"joker/{node}/{path}"` | |
| `mqtt` | `encoding` | `"json"` \| `"cbor"` | no | `"json"` | |
| `mqtt` | `qos` | `0` \| `1` | no | `1` | |
| `zenoh` | `endpoints` | `List<string>` (`"tls/host:7447"`) | yes | — | |
| `zenoh` | `key_prefix` | template | no | `"joker/{node}"` | |
| `zenoh` | `mode` | `"client"` \| `"peer"` | no | `"client"` | |
| `otlp` | `endpoint` | `https://` URL | yes | — | `http://` in `prod` `J0296` |
| `otlp` | `signals` | `List<"metrics" \| "logs" \| "traces">` | no | `["metrics"]` | metrics of the runtime itself (document 01, §19) |
| `otlp` | `every` | `Duration` | no | `"10s"` | |
| `http` | `url` | `https://` URL | yes | — | `J0296` |
| `http` | `events` | `List<"alarms" \| "events">` | no | `["alarms"]` | webhook |

`classes`, `live` and `history` do not apply to `otlp` (`J0198`). The TLS client credentials are the node's identity (`ref/02`), not manifest keys.

### 5.18 `[bridges.<bridge>]` (M11; reserved 1.2)

The OPC UA and Modbus server bridges and the Zenoh/ROS 2 bridge are **1.2** (document 26, §5; ADR 0035). The whole section exists in the 1.0 schema, and a `[bridges]` entry on a 1.0 runtime is `J0290`.

```toml
[bridges.scada]
kind     = "opcua-server"
listen   = "0.0.0.0:4840"
expose   = ["meter.**", "pcs.**", "alarm.**"]
writer   = { level = "supervisor", priority = 7, lease = "30s", writable = ["pcs.p_set"] }
security = { policy = "Basic256Sha256", mode = "SignAndEncrypt", trust = ["keys/scada-client.der"] }

[bridges.ros]
kind      = "zenoh-ros2"
expose    = { "base.odom" = "/odom", "lidar.scan" = "/scan" }
subscribe = { "/cmd_vel" = { type = "geometry_msgs/msg/Twist", fields = ["linear.x", "angular.z"], to = ["bridge.ros.cmd_v", "bridge.ros.cmd_w"], max_age = "200ms" } }
```

**Common keys:**

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `kind` | `"opcua-server"` \| `"modbus-server"` \| `"zenoh-ros2"` | yes | — | | 03 §12 | 1.2 |
| `expose` | `List<Selector>` or, in `zenoh-ros2`, `Table<Path, topic \| { topic, type, fields }>` | no | `[]` | without matches `J0156` (warning); instance paths expose the whole instance (methods included, in OPC UA) | 03 §12 | 1.2 |
| `writer.level`, `writer.priority`, `writer.lease` | as in `[apps.*.writer]` | cond. | — | required if `writable` or some `to` is an output; `safety` band `J0253`; `manual` `J0252` | 03 §12 | 1.2 |
| `writer.writable` | `List<Path \| Selector>` (set) of outputs | no | `[]` (read-only) | only outputs with an envelope (`J0199`); without matches `J0155` | 03 §12 | 1.2 |
| `accepts` | `Set<"operational" \| "personal" \| "sensitive">` | no | `["operational"]` | data classes the bridge may expose (§6.7); an exposed signal of another class `J0261` | ADR 0079 | 1.2 |

**Keys by `kind`:**

| `kind` | Key | Type | Req. | Default | Validation |
|---|---|---|---|---|---|
| `opcua-server` | `listen` | `"<host>:<port>"` | no | `"127.0.0.1:4840"` | all interfaces with a non-empty `writable` in `prod` `J0297` (warning) |
| `opcua-server` | `security.policy` | `"None"` \| `"Basic256Sha256"` \| `"Aes128_Sha256_RsaOaep"` \| `"Aes256_Sha256_RsaPss"` | no | `"Basic256Sha256"` | `None` in `prod` `J0296` |
| `opcua-server` | `security.mode` | `"None"` \| `"Sign"` \| `"SignAndEncrypt"` | no | `"SignAndEncrypt"` | `None` in `prod` `J0296` |
| `opcua-server` | `security.trust` | `List<File>` (client certificates) | cond. | — | required with a non-empty `writable` |
| `modbus-server` | `listen` | `"<host>:<port>"` | cond. | — | exactly one of `listen` (Modbus TCP) and `bus` (RTU server) (`J0199`) |
| `modbus-server` | `bus` | `Name` of a `serial` bus | cond. | — | |
| `modbus-server` | `unit_id` | `Integer` 1–247 | no | `1` | |
| `modbus-server` | `profile` | profile (`ref/03`) | yes | — | the register map served, in the profile format; each signal of the profile is an exposed or written path |
| `zenoh-ros2` | `subscribe` | `Table<topic, { type, fields, to, max_age }>` | no | `{}` | `fields` and `to` with the same length (`J0199`) |
| `zenoh-ros2` | `subscribe.*.to` | `List<Path>` | yes | — | in `bridge.<bridge>.…` (creates the signal) or an output (needs `writer`) (`J0199`) |
| `zenoh-ros2` | `subscribe.*.type` | string (ROS 2 type) | yes | — | |
| `zenoh-ros2` | `subscribe.*.fields` | `List<string>` (message fields) | no | the `data` field (`std_msgs` messages) | |
| `zenoh-ros2` | `subscribe.*.max_age` | `Duration` | no | 3 × the observed period | `max_age` of the created signals |
| `zenoh-ros2` | `endpoints` | `List<string>` | no | those of the local `rmw_zenoh` | |

**A write that arrives through a bridge is a write by a declared writer** (document 01, §14): it has a level, a lease and an envelope, and it goes into the decision log as `bridge:<bridge>`. Authentication is that of the bridge's protocol (OPC UA session with a certificate; Modbus has none, and so a Modbus bridge with `writable` should only listen on a field interface). Bridges do not carry a nonce: they authenticate through their own protocol and write with a level and a lease (`ref/00` §9; D16 of document 22).

### 5.19 `[security]`: trust and local API

```toml
[security]
trust = { apps = ["keys/apps.pub"], profiles = ["keys/profiles.pub"], revoked = ["ed25519:3f2a…"] }
api   = { listen = ["tcp:0.0.0.0:7443"] }
```

| Key | Type | Req. | Default | Validation | Origin | Phase |
|---|---|---|---|---|---|---|
| `trust.<role>` | `List<File>` of public keys (`.pub`, `ref/02` §2.5.2; set) | no | no restriction: the keys of the node's trust bundle with that role | roles: `config`, `runtime`, `firmware`, `apps`, `drivers`, `profiles`, `boards`, `classes`, `secrets`, `deck`, `models` (1.1) (`ref/02` §2.5.3); another one `J0144`; it can only **restrict**: each key must be in the bundle with that role (`J0292`) | 03 §13 | 1.0 (`models` 1.1) |
| `trust.revoked` | `List<string>` of key ids (`"ed25519:<32 hex>"`; set) | no | `[]` | adds revocations to those of the node; never removes any (`ref/02` §2.5.4) | `ref/02` | 1.0 |
| `api.listen` | `List<"unix:<path>" \| "tcp:<host>:<port>">` (set) | no | `[]` | **adds** listeners to those of `node.toml` (`ref/02` §3.2.2); never removes them; `J0157`. TCP is always TLS 1.3 with mTLS (`ref/09` §2.1): there is no key to turn it off | 03 §13 | 1.0 |

There is no `api.mtls`: the key was removed, because TCP without mTLS does not exist. A manifest that declares it gives `J0140` ("unknown key"), with the help "TCP is always mTLS".

**Why trust only restricts.** A configuration signed with a key that it declares itself proves nothing. The keys by role come from the node's **trust bundle**, signed by an anchor of the organization (`ref/02` §2.5.4; D7 of document 22, ADR 0025), and the Deck only carries it (document 20, principle 5); the manifest can say "on this site, I only accept apps signed by this key", but never add a key that the bundle does not have. `jk check` gives `J0292` when it knows the bundle (`--node`, or a local bundle); the node always checks, on apply, with the same code (`J0292`, `ref/02` §3.5.1, step 3). `J0916` is a different check: an artifact or a generation signed by a key without that role. It is a fix to document 03, §13 (appendix C).

### 5.20 `[security.roles]`

```toml
[security.roles]
operator = ["cert:CN=ops-*", "local:group=joker-ops"]
admin    = ["cert:CN=admin-*"]
```

| Key | Type | Req. | Default in `prod` | Default in `dev` | Validation |
|---|---|---|---|---|---|
| `viewer` | `List<Principal>` | no | any authenticated principal (certificate from the node's client CA, or whoever opens the Unix socket) | same | |
| `operator` | `List<Principal>` | no | `["local:group=joker-ops"]` | any authenticated principal | |
| `admin` | `List<Principal>` | no | `["local:group=joker-admin"]` | any authenticated principal | |

```
principal := "cert:" attribute "=" pattern
           | "local:user=" name | "local:group=" name
           | "deck:" deck_id "/user:" id | "deck:" deck_id "/group:" name
attribute := "CN" | "OU" | "O" | "SAN"
pattern   := text with "*" as wildcard
```

- `cert:SAN=<pattern>` matches a URI, DNS name or email in the certificate's SAN; it is the way to put a role in the certificate (`cert:SAN=urn:joker:role:operator`, `ref/09` §2.3).
- `deck:<deck_id>/user:<id>` and `deck:<deck_id>/group:<g>` are the principals of remote sessions through the Deck tunnel (`ref/10`, agent/1.3); on a runtime without a tunnel they are accepted and match nothing.
- Roles are cumulative: `admin` ⊇ `operator` ⊇ `viewer` (document 01, §14).
- **Adds to the node.** The effective roles are those of `[roles]` of `node.toml` **united** with the ones here (`ref/02` §3.2.2): a generation never removes the node's minimum administrators. uid 0 and the `joker` user on the socket are always `admin` (`ref/09` §2.3).
- An invalid principal is `J0295`. A role granted to any certificate (`"cert:CN=*"`) is `J0293` (warning).
- In `dev`, without `[security.roles]`, any authenticated principal is `admin`: this is what allows `jk write` from the laptop in the track. In `prod`, the defaults are the restrictive ones.

### 5.21 Extension keys

Any table accepts `x_*` keys, with any TOML value. The runtime ignores them, `jk check` does not validate them, and the effective manifest keeps them (with provenance) for third-party tools, such as the Deck (`x_owner`, `x_ticket`). They do not enter the hash of the effective manifest (§8.3).

### 5.22 On a Joker Micro

The manifest of a Joker Micro node is a `joker.toml` like the others, validated by the same `jk check` and compiled to the generation blob (document 19, §4; `ref/08` §2.13). The sections that apply are `[project]`, `[target]` (with `[target.capacity]`, `[target.extern]` and `[target.micro]`), `[clock]` (`sync = "link" | "extern" | "none"`), `[buses]` (`serial`, `can`, `i2c`, `spi`), `[devices]` (MCU drivers and `driver = "extern"`), `[signals]`, `[outputs]` (with `remote_levels`), `[cycles]`, `[apps]` (only `path`, native apps, or `kind = "extern"`), `[instances]`, `[calc]`, `[alarms]`, `[atomic]`, `[telemetry.uplink]` (`mqtt`, only on M2 MCUs), `[security.trust]`, `[link]` and `[net.wifi]` (only on M2 MCUs with Wi-Fi). The others (`[resources]`, `[telemetry.record]`, `[telemetry.store]`, `[bridges]`, `[security.api]`, `[security.roles]`, `[drivers]`) are `J0720` on an MCU target (a Joker Micro code, `ref/08`). The list of keys that `jk build` expects is that of `ref/08` §2.17; the syntax is this one.

**`[target]` on an MCU:** `board` is a board with `os = "none"` (`ref/03` §2.3.3); `arch` and `kernel` do not apply (`J0145`).

**The core version rules what the manifest may use.** `[project] joker` states the minimum version of `jk`; on an MCU, what counts is the core compiled into the firmware, pinned in the node's lock in `[mcu.<d>] core` (`ref/02` §2.1.10). A key, section or rule that this core does not know is `J0796` in `jk check`, even if `jk` accepts it (`ref/08` §3.11).

**The mode of the MCU and that of the node.** The `[project] mode` of the MCU's manifest is the mode of its generation blob (`DEV` bit of the header, `ref/08` §2.13.2). **A node in `prod` only sends `prod` generation blobs to an MCU:** a `joker-link` device whose `micro` points to a manifest in `dev`, in a node manifest in `prod`, is `J0794` in `jk check`, and the node rejects the generation with the same code at `jk apply` (and so does the Deck, at validation). A node in `dev` accepts both. In `prod`, the `error-in-prod` checks of the MCU are errors like those of the node, in particular `J0786` (board with `reset_safe = false`).

**`accept_unsafe_reset`** (key of `[target]`, MCU only): `Text`, a justification with ≥ 10 characters (`J0442`). It accepts, with a justification, a board without safe outputs at reset (`reset_safe = false`, `ref/03` §2.3.1): it turns `J0786` into a warning, as `accept_missing_fallback` does to `J0205` (§5.10). `jk lock` copies it to `[accepted] accept_unsafe_reset."<board>"` of the lock of the MCU's manifest (`ref/02` §2.1.14), with the resolved board (`"esp32-devkit@1"`): changing board invalidates the acceptance (`J0440`). On a board with `reset_safe = true`, or on a Linux target, it is `J0145`. The justification states what the hardware does between reset and step 6 of startup (`ref/08` §2.12.3) and why it is acceptable on that site (no load, an external *pull-down* resistor, a relay that only closes with the pin active…); it does not change the behavior of the firmware.

```toml
# micro/esp/joker.toml (excerpt, in prod)
[project]
mode = "prod"

[target]
board               = "esp32-devkit@1"
accept_unsafe_reset = "bench: no load on the ESP32 GPIOs; the floating LED at reset has no consequences"
```

| Table | Key | Type | Default | Validation and meaning |
|---|---|---|---|---|
| `[target.capacity]` | the names of the capability table of `ref/08` §2.13.3 (`signals`, `outputs`, `groups`, `instances`, `apps`, `alarms`, `programs`, `trace_bytes`, `kv_bytes`, …) | `Integer`, or `Size` for the `_bytes` names | the board's `[capacities]` (`ref/03` §2.3.3) | capabilities of the build; unknown name `J0140`; the generation above them `J0710` |
| `[target.extern]` | `expose` | `List<Selector>` (set) | `[]` | signals that the integrator's firmware may read beyond those of the `extern` apps (`ref/08` §2.8.2) |
| `[target.micro]` | `service_deadline` | `Duration` | `"100ms"` | service deadline (`ref/08` §2.6.q) |
| `[target.micro]` | `freeze_on_trip` | `Bool` | `false` | a trip freezes the trace rings (`jk_trace_freeze`, `ref/08` §2.6.o) |
| `[target.micro]` | `samples_ring` | `Integer` | `256` | entries of the link's `SAMPLES` ring |
| `[target.micro]` | `event_queue` | `Integer` | `16` | depth of the firmware event queue that this generation uses (`NODE` section); must not exceed the `event_queue` capability of the build (`[target.capacity]`, or the board's; `J0710`) |
| `[target.micro]` | `confirm` | `Duration` | `"5m"` | default confirmation window of a generation received by OTA (`ref/08` §2.12.4) |

**`[devices.<d>]` with `driver = "extern"`:** a device without a driver, fed by the integrator's code (`jk_set_input`, `ref/08`). It has no `bus`, `profile` or driver keys; the signals are declared in `[devices.<d>.signals]` with `type`, `unit` and `direction` (a signal without `type`, `direction` or, if numeric, `unit`: `J0785`, `ref/08`).

**`[apps.<a>]` with `kind = "extern"`:** an external writer (§5.12: `writer`, `writes`, `reads`). The `params` of any app accept, on an MCU, the form `{ value, runtime, persist }` (§5.12).

**`[link]`** (MCU only; on the Linux side, the MCU is a `joker-link` device, appendix A):

| Key | Type | Req. | Default | Validation | Origin |
|---|---|---|---|---|---|
| `bus` | `Name` of a `serial` or `can` bus | cond. | — | exactly one of `bus` and `address`; a `can` bus is reserved for 1.1 (`J0290`) | 19 §6 |
| `addr` | `Integer` | no | `0` on a point-to-point serial connection, `1` on CAN | address **of the MCU** on the bus: 1–247 on a shared RS-485, 1–253 on CAN (`J0736`, `ref/07`); only with `bus` | `ref/07` §15 |
| `address` | `"udp:<host>:<port>"` \| `"tcp:<host>:<port>"` | cond. | — | only on M2 MCUs; `address` is always an IP address, and never the address on the bus. It is the address **of the Linux node** (the peer): with `tcp:`, the MCU connects to it; with `udp:`, it sends it the datagrams and only accepts those that come from it. On the Linux side, the `address` of the `joker-link` device is the opposite, the IP and port **of the MCU** (appendix A; `ref/07` §15) | 19 §6 |
| `peer` | `DnsName` of the Linux node, or `"deck"` | no | accepts the first authenticated peer | `"deck"` belongs to the standalone MCU profile and is reserved for 1.1 (`J0290`; ADR 0035) | new |
| `auth` | `"required"` \| `"none"` | no | `"required"` | `none` on an IP transport `J0179` | 19 U10 |

The form of the messages, the negotiation, the link key and the defaults of the timings are in `ref/07`.

**`[net.wifi]`** (only an M2 MCU on a board with `[net] wifi = true`, `ref/03` §2.3.3): the Wi-Fi network that the MCU connects to at boot. It is what makes the IP transports of an MCU possible: the `[link] address` (`udp:`/`tcp:`), the `[telemetry.uplink]` and the `rssi` signal of the `esp32` driver (appendix A). On a Linux target, the network belongs to the operating system, and `[net]` is `J0145`; on a board without Wi-Fi (`mcu_class = "m1"`, or `[net] wifi = false`), `J0828`.

```toml
# micro/esp/joker.toml (excerpt)
[net.wifi]
ssid     = "bench"
password = "secret:wifi_bench"
country  = "PT"
hostname = "esp-bench"
```

| Key | Type | Req. | Default | Validation | Origin |
|---|---|---|---|---|---|
| `ssid` | `Text`, 1–32 B in UTF-8 | yes | — | empty or more than 32 B `J0240` | new |
| `password` **(secret)** | `Ref` `secret:<name>` | no | — (open network) | plaintext `J0294`; the value must have 8–63 characters (WPA2/WPA3-Personal): since `jk` does not see it, it is the MCU that checks it when decrypting it, and any other length leaves it without a network, with the reason in the MCU state; without `password`, the network is open: `J0242` (`error-in-prod`); name missing or not encrypted for the MCU `J0920`, `J0922` (`ref/02` §2.4.6) | new |
| `country` | `Text`, ISO 3166-1 alpha-2 code in uppercase (`"PT"`) | yes | — | outside the ISO list `J0240`. It fixes the channels and the power that the radio may use; without it, the radio would stay in the most restrictive domain and might not see the network | new |
| `hostname` | `DnsName`, a single label (`[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?`) | no | the `[project] name`, reduced to this grammar | outside the grammar `J0240`; it is the name that the MCU requests over DHCP (option 12) | new |

- **The secret reaches the MCU sealed for it.** `jk build --generation` puts the `password` entry in the `SECRETS` section of the blob, encrypted for the MCU's X25519 key (`ref/08` §2.13.3, §2.17; `ref/02` §2.4.8). Neither the Linux node that carries the blob over the link, nor the Deck, can read it. An MCU that is not yet a recipient (`jk node enroll <mcu> --link <d> --ca <directory>`, or the enrollment of a standalone MCU) is `J0922`.
- **The first blob arrives by a path other than Wi-Fi:** written with the firmware (`jk flash`) or over the serial link. After that, the MCU connects to the network at boot, before opening an IP link.
- An IP transport on an MCU without `[net.wifi]` (`[link] address`, `[telemetry.uplink]`) is `J0241`.

---

## 6. Reserved for 1.1

The keys of this section **already exist in the 1.0 schema** (document 11, "Prepared for ML"; ADR 0019): `jk` 1.0 validates their form, and using them with a 1.0 runtime is `J0290` ("reserved for 1.1; this runtime is 1.0"). So a manifest written for 1.1 is rejected with a clear message, and 1.1 is a minor change: it only adds optional keys to those that are here.

The semantics are those of document 06.

### 6.1 `[models.<model>]`

```toml
[models.load_forecast]
source    = "models/load-tcn-v3.jkm"
backend   = ["burn:cuda", "burn:vulkan", "burn:cpu"]
resources = "ml"
deadline  = "2s"
isolation = "thread"
warmup    = 3
```

| Key | Type | Req. | Default | Validation |
|---|---|---|---|---|
| `source` | `File` `.jkm`, `Ref` `oci://…` or `"training:<job>"` | yes | — | `J0157` |
| `digest` | `"sha256:…"` | no | pinned in the lock | |
| `backend` | `List<string>` (ordered): `"burn:cpu"`, `"burn:vulkan"`, `"burn:cuda"`, `"burn:metal"`, `"ort:cpu"`, `"ort:tensorrt"`, `"tract"`, `"hailo"`, `"tensorrt"` | yes | — | fallback chain; none available on the board `J0501` |
| `resources` | resource class | no | `"ml"` | `rt` `J0145` |
| `deadline` | `Duration` | yes | — | |
| `isolation` | `"thread"` \| `"process"` | no | `"thread"` | |
| `warmup` | `Integer` | no | `1` | |
| `max_inflight` | `Integer` | no | 1 on NPU/GPU; number of `ml` CPUs on CPU | |
| `batch` | `{ max = Integer, wait = Duration }` | no | — (no micro-batching) | |
| `fallback_after` | `Integer` | no | `5` | consecutive failures or deadlines before moving to the next backend |

`[models.<m>.shadow]` (M16, models):

| Key | Type | Req. | Default | Validation |
|---|---|---|---|---|
| `candidate` | `File` or `Ref` | cond. | — | required without `auto` |
| `auto` | `Bool` | no | `false` | each candidate from `[training]` enters shadow on its own |
| `until` | `{ duration, frames }` | yes | — | at least one |
| `promote_if` | `List<string>` (KPI grammar, 06 §10.1) | yes | — | |
| `kpi` | string (KPI grammar) | no | — | demotion after being promoted |
| `on_regress` | `"demote"` \| `"keep"` | no | `"demote"` | |
| `record_disagreements` | `Bool` | no | `false` | stores the frames in disagreement, with short retention |

### 6.2 `[streams."<path>"]`

| Key | Type | Req. | Default | Validation |
|---|---|---|---|---|
| `pool` | `Integer` (buffers) | yes | — | the memory counts in `J0380` |
| `mode` | `"latest"` or `"queue(<n>)"` | no | `"latest"` | |
| `record` | `Bool` | no | `false` | streams are not recorded by default (document 12, §10) |

### 6.3 `[pipelines.<pipeline>]`

```toml
[pipelines.inspect]
from     = "cam0.frames"
steps    = [
  { op = "correlate", with = "line.photoeye", window = "5ms" },
  { op = "crop", roi = [480, 0, 960, 1080] },
  { op = "letterbox", size = [640, 640] },
  { op = "model", name = "detector" },
  { op = "nms", iou = 0.45, score = 0.30, max = 16 },
]
to       = "vision.detections"
deadline = "120ms"
```

| Key | Type | Req. | Default | Validation |
|---|---|---|---|---|
| `from` | `Path` of a stream, block signal or event, or `List<Path>` | yes | — | |
| `steps` | `List<Step>` (ordered) | yes | — | incompatible formats between steps `J0510` |
| `to` | `Path` | yes | — | creates the path (16, P4); unique in the site (`J0174`) |
| `deadline` | `Duration` | no | — | output `Bad(Deadline)` when exceeded |

**Steps** (`op` and the keys of each, document 06, §5):

| `op` | Keys |
|---|---|
| `crop` | `roi = [x, y, w, h]` |
| `resize` | `size = [w, h]`, `keep_aspect` (`false`), `method` (`"bilinear"` \| `"area"`) |
| `letterbox` | `size = [w, h]` |
| `convert` | `to` (`"rgb"`, `"bgr"`, `"gray"`) |
| `normalize` | `mean = [f, …]`, `std = [f, …]` |
| `to_tensor` | `layout` (`"NCHW"`, `"NHWC"`, `"NTC"`) |
| `blur_faces` | — |
| `window` | `n`, `hop` |
| `detrend` | — |
| `fft` | `n`, `window` (`"hann"`, `"hamming"`, `"rect"`) |
| `bands` | `edges_hz = [f, …]` |
| `stats` | `fields = ["rms", "peak", "crest", "kurtosis", …]` |
| `resample` | `rate` (`Quantity<Hz>`) |
| `append` | `signals = [Path, …]` |
| `model` | `name` (`Name` from `[models]`) |
| `nms` | `iou`, `score`, `max` |
| `decode_boxes` | `anchors` |
| `argmax` | — |
| `topk` | `k` |
| `threshold` | `value` |
| `softmax` | — |
| `to_event` | `schema` |
| `to_signal` | `field` |
| `every` | `n` |
| `when` | `expr` (`Expr<bool>`) |
| `correlate` | `with` (`Path`), `window` (`Duration`) |

### 6.4 `[training.<job>]`

| Key | Type | Req. | Default | Validation |
|---|---|---|---|---|
| `arch` | string (catalog, `"joker.ae@1"`) | yes | — | |
| `from` | `File` `.jkm` | no | — | initial weights |
| `hyper` | `Table` (hyperparameters of the architecture) | no | those of the architecture | |
| `data.signals` | `List<Path \| Ref pipe:>` | yes | — | |
| `data.window` | `Duration` | yes | — | |
| `data.where` | `Expr<bool>` | no | — | filter over the historian |
| `schedule` | string `"after: <duration>[ of data]; then: <daily\|weekly\|every <duration>>"` | yes | — | |
| `resources` | class | no | `"ml"` | |
| `priority` | `"idle"` \| `"normal"` | no | `"idle"` | |
| `budget` | `{ max_time = Duration, max_memory = Size }` | no | — | |
| `checkpoint` | `Duration` | no | `"10m"` | |
| `validate` | `{ holdout = Percentage, metric, max, threshold }` | yes | — | `threshold` is a percentile (`"p99.9"`) stored in the `.jkm` |
| `output` | `"candidate"` | no | `"candidate"` | never direct production (M16) |

### 6.5 Other reserved keys

| Key | Where it is described | What is missing for 1.1 |
|---|---|---|
| `apps.*.fallback = { controller = "app:<a>", when = "<KPI>" }` | 06 §7 | `when` in the KPI grammar, evaluated in `bg`; the result passes to the arbiter on the next tick (the 30 min windows of document 08, R6, only fit there) |
| `apps.*.writer.kind = "proposal"` | §5.12 | M18 |
| `outputs.*.schedule_depth`, `timed`, `on_late`, `late_margin` | §5.10 | M6 |
| `devices.*.signals.*.kind = "block"`, `block` | §5.8.1 | M7 |
| `telemetry.record.*.blocks`, `trigger`, `pre`, `post` | §5.17.1 | M7 |
| `resources.ml.accel` | §5.4 | accelerators |
| `bind = "model:<m>"`, `"pipe:<p>"` | §5.12.1 | model and pipeline ports |
| `security.trust.models` | §5.19 | `joker.model.v1+jkm` artifacts |
| drivers `v4l2`, `libcamera`, `spi-adc` | appendix A | 1.1 (document 21, §3) |

### 6.6 Reserved by the 1.0 cut

ADR 0028 keeps stable in 1.0 only what is implemented and passed its gate; the cut is the table of document 26, §5, and ADR 0035 applies it to this reference. The keys below are described in §5 and appendix A with their final form, and they exist in the 1.0 schema with the `since` of the version that brings them; using one on a runtime that does not implement it is `J0290`, as for the keys of §6.1–§6.5. A key reserved for `1.0.x` gets the number of the patch release that implements it.

| Key | Where it is described | Version |
|---|---|---|
| `buses.*.kind = "spi"` and its keys | §5.6 | 1.0.x |
| drivers `revpi` (implemented, ADR 0078), `spi`, `opcua`, `m-bus`, `onewire`, `serial-line`; `modbus-slave` on an MCU | appendix A | 1.0.x |
| `instances.*.sim` (class simulators) | §5.13 | 1.0.x |
| `apps.*.shadow` | §5.12.3 | 1.1 |
| `telemetry.uplink.*.budget` (M12) | §5.17.3 | 1.1 |
| `telemetry.uplink.*.kind = "otlp"` and `"http"`, with their keys | §5.17.3 | 1.1 |
| `buses.*.kind = "can"` and its keys; drivers `can` and `canopen`; `joker-link` over CAN (`bus` of kind `can`, `can_id_base`) | §5.6, §5.22, appendix A | 1.1 |
| `buses.*.duplex = "half"` with a `joker-link` device; `joker-link` keys `turn_timeout` and `turn_frames` | §5.6, appendix A | 1.1 |
| `[link] peer = "deck"` (standalone MCU profile) | §5.22 | 1.1 |
| `joker-link` key `usb` (USB CDC transport) | appendix A | 1.0.x |
| `[drivers.<driver>]` and the `w:<name>`, `x:<name>` drivers (tier W and X hosts) | §5.7, appendix A | 1.2 |
| `[bridges.<bridge>]` (OPC UA server, Modbus server, Zenoh/ROS 2) | §5.18 | 1.2 |
| `telemetry.uplink.*.kind = "zenoh"`, with its keys | §5.17.3 | 1.2 |
| `buses.*.kind = "ethercat"` and its keys | §5.6 | 1.2 |
| `data_class` on device signals and on `[signals]` | §5.8.1, §5.9, §6.7 | 1.0 (until implemented) |
| `telemetry.uplink.*.accepts` and `bridges.*.accepts` | §5.17.3, §5.18, §6.7 | 1.0 (until implemented); bridges 1.2 |

### 6.7 Data classes

Document 26, §8 puts data protection on Joker's side as "not letting it leak" (D1–D4 in 1.0). ADR 0079 decides the rules; the keys exist in the 1.0 schema and are reserved until the runtime implements them (`J0290`, §6.6). Their form:

```toml
[signals."door.badge_id"]
data_class = "personal"

[telemetry.uplink.cloud]
kind    = "mqtt"
broker  = "mqtts://broker.example.com:8883"
classes = { low = "**" }
accepts = ["operational"]
```

- **Classes.** `operational` (the default), `personal` and `sensitive`, in this order of protection. A device signal declares its class in `[devices.<d>.signals.<s>]` or in `[signals."<path>"]`; profiles and classes do not.
- **Inheritance.** A derived signal (`calc.*`, `alarm.*`, the outputs and published signals of an app, the members of a virtual instance) gets the most protective class of the signals it reads. `[signals."<path>"] data_class` may raise it; lowering it is `J0260`.
- **Egress.** `accepts` on an uplink or a bridge lists the classes it may send; the default is `["operational"]`. A signal that the uplink's `classes` or the bridge's `expose` selects, of a class the egress does not accept, is `J0261`: an error, never a silent filter.
- **Exports, traces and logs.** Exports and traces leave the node with `personal` or `sensitive` values only encrypted per recipient (D3, Phase 2; ref/09 §5.5, §5.16), and logs, error messages and diagnostics carry the path with `<redacted:personal>` or `<redacted:sensitive>` instead of the value (D4).

---

## 7. Overlays (`extends`)

### 7.1 Resolution order

```
jk check --manifest sites/lisboa-01/joker.toml
   │
   1. read the top-level file
   2. resolve `extends`, recursively, depth-first and from left to right
   3. linearize: post-order, each file only once (in the first position where it appears)
   4. merge the layers in that order, and the top-level file last
   5. apply the directives (`append`, `unset`) and check "only tighten" layer by layer
   6. remove `extends`; the result is the effective manifest (§8)
```

**Example.** The site `S` extends `[A, B]`; `A` extends `[T]`; `B` extends `[T, P]`.

```
S ─ extends ─▶ A ─ extends ─▶ T
          └──▶ B ─ extends ─▶ T, P
linearization (post-order, no repetitions):  T, A, P, B, S
```

`T` enters only once, before `A`. An `append` from `T` is not applied twice, and a diamond is not an error.

- **Paths in `extends`** are relative to the file that declares them (this is the decision on the "base of `extends` paths", document 16, §10). Only local files: `oci://` and URLs are `J0160`.
- A file that appears in its own chain is `J0161`. More than 8 levels or 32 files is `J0162`.
- Each layer is valid TOML on its own (`J0146`) and is validated on its own for unknown keys (`J0140`); required keys and references are only validated in the effective manifest.

### 7.2 Merge rules

| What is in the previous layer | What is in the new layer | Result |
|---|---|---|
| table | table | recursive merge, key by key |
| scalar | scalar | the new one replaces |
| list | list | the new one **replaces** |
| list | `{ append = [ … ] }` | the previous one plus the new elements, at the end (§7.3) |
| any value | `{ unset = true }` | the key disappears |
| nothing | `{ append = [ … ] }` | the list `[ … ]` |
| nothing | `{ unset = true }` | nothing, with the warning `J0163` |
| table | scalar, or the reverse | `J0165` |

- **Directives are recognized by the schema.** `{ append = … }` is only a directive in a key whose type is a list; `{ unset = true }` is a directive in any key, because no table of the schema has an `unset` key. `append` in a key that is not a list, a table with `append` and other keys, or `unset` with a value other than `true`, are `J0164`.
- **Removing a whole section** is done with a dotted key at the root of the overlay, before the table headers:

```toml
# joker.rpi.toml
extends = ["joker.toml"]
devices.tank = { unset = true }

[devices.adc]
driver  = "i2c"
bus     = "i2c1"
address = 0x48
profile = "ti/ads1115@1"
```

### 7.3 `append` in lists and sets

- In a **set** (`requires_good`, `alias`, `capabilities`, `net.allow`, `actions.trip`…), elements already present are not repeated.
- In an **ordered list** (`backend`, `order`, `steps`, `extends`), a repeated element is `J0164`: in a fallback chain or in a task order, a duplicate is almost always an error.

```toml
[outputs."pcs.p_set"]
requires_good = { append = ["bms.soh"] }
```

### 7.4 Envelopes only tighten

An overlay may make the envelope of an output **more restrictive**. To make it less restrictive, it needs `widen` with a justification, which `jk lock` copies to `[accepted] widen."<output>.<key>"` (document 03, §14). Without `widen`, it is `J0220`.

**Comparison.** Each layer is compared with the **accumulated** value of the previous layers, with the units already converted (`"125000 W"` is equal to `"125 kW"`). The output is identified by the literal key of `[outputs]` (§7.6).

| Key | Tightens when | Widens (needs `widen`) when |
|---|---|---|
| `range` | the new interval is contained in the previous one (constant limits); or it starts to exist | it contains values outside the previous one; it is removed; one of the limits is an expression and changed |
| `rate`, `safe_ramp` | the new value is smaller; or it starts to exist | it is larger; it is removed |
| `deadband` | — | — (it is not a limit: it is free) |
| `min_interval` | it is larger; or it starts to exist | it is smaller; it is removed |
| `max_actuations` | fewer actuations in the same window (compared per hour); or it starts to exist | more; it is removed |
| `interlock` | the new expression is `(<previous>) && <term>`, textually; or it starts to exist | any other change; it is removed |
| `permit_uncertain` | `true` → `false` | `false` → `true` |
| `requires_good` | the new set contains the previous one | some element of the previous one is missing |
| `grace` | it is smaller | it is larger |
| `max_lease`, `max_manual` | it is smaller | it is larger; it is removed (goes back to the larger default) |
| `on_range` | `clamp` → `reject` → `trip` (in this direction) | in the opposite direction |
| `on_interlock` | `hold` → `safe` | `safe` → `hold` |
| `verify` | it starts to exist; smaller `tolerance` or `within` | it is removed; larger `tolerance` or `within` |
| `reset_when` | it is removed (it starts to require `jk reset`) | it starts to exist or changes |
| `safe`, `safer` | — | **any change**: the safe side is a decision, and not an order |
| `accept_missing_fallback` | — | — (**never a `widen`**: the key itself is the justification, and it requires the entry `[accepted] accept_missing_fallback."<output>"` in the lock, whether it comes from the base manifest or from an overlay; `ref/02` §2.1.14. A `widen` that names it is `J0167`) |
| `keepalive`, `group`, `pulse`, `remote_levels`, `schedule_depth`, `timed`, `on_late`, `late_margin` | — | — (free; validated by the other rules) |

- The **first** layer that declares an output is not compared with anything: it is the origin of the envelope.
- **An envelope that moves to the MCU widens nothing.** When a layer swaps the device of an output for a `joker-link` (part 9 of the track: the LED becomes governed by Joker Micro), the envelope of that output can no longer be in the Linux manifest (`J0234`) and moves to the `[outputs]` of the MCU manifest. Removing it from the Linux manifest is **not** a widening and does not ask for `widen`: the envelope still exists, only in the node that governs the output, and it is the `jk check` of the MCU that validates it (and that compares its layers with each other, if it has any). The `jk check` of the Linux node confirms that the output really belongs to a `joker-link` (with the `micro` key, appendix A, also that the MCU manifest declares the envelope; without it, only with `--node`); if it does not, the removal is the one of the last column of the table and requires `widen`.
- Removing a key with `unset` counts as the change of the last column. It is not possible to widen in two layers (remove in one, put it back in another) without `widen`: the removal already requires it.
- `widen` is written in the table of the output, in the layer that widens:

```toml
[outputs."pcs.p_set"]
range = ["-150 kW", "150 kW"]
widen = { range = "New PCS with 150 kW rated power; approved by DL on 2026-09-18" }
```

The short form `widen = "<justification>"` covers all the keys that this layer widens in this output. The justification has at least 10 characters (`J0166`). A `widen` in a layer that widens nothing is `J0167` (warning). In `prod`, a widening without the corresponding entry in the lock (or with different text) is `J0168`. `widen` does not pass into the effective manifest: it stays in the lock.

### 7.5 File paths

Values of type `File` (`model`, `path`, `eds`, `config`, `trust.*`, `profile` of bridges, `exec`) are relative to the **file that declares them**, and not to the top-level file. In the effective manifest, they are rewritten relative to the directory of the top-level file (the project root). A file that does not exist is `J0160`.

### 7.6 Literal keys and selectors

Merging is done by **literal key**: `[outputs."inv*.w_max_lim_pct"]` in a template and `[outputs."inv2.w_max_lim_pct"]` in a site are two different entries, and after the expansion of the selectors the output `inv2.w_max_lim_pct` has two envelopes (`J0215`). To adjust only `inv2`, the site removes the template entry and declares the ones it wants (stress, case 3).

### 7.7 Layer names and choice of the input manifest

**Names** (ADR 0026). A manifest layer is named `joker.toml` or `joker.<variant>.toml`, in **any** directory: `joker.sim.toml` and `joker.lab.toml` at the root, `templates/joker.bess-v2.toml`, `sites/lisboa-01/joker.summer.toml`, `micro/esp/joker.toml`. `<variant>` follows the grammar of board names (`[a-z0-9]+ ("-" [a-z0-9]+)*`, ≤ 32 characters; ref/00 §2); outside it, `J0150`. The content of a layer has only manifest sections (§4) and no identification table of another type (`[profile]`, `[board]`, `[scenario]`, `[suite]`, …); a `joker.*.toml` that is something else is `J0803` (ref/03 §2.1.6).

- **In `extends`**, only files with one of these names are accepted; another name is `J0803` (the reference asks for a manifest layer). This is what lets the overlay chain be read in a listing (`ls joker.*`).
- **As input manifest**, a file with another name is only accepted through `--manifest` (or `JK_MANIFEST`), with the warning `J0169`; `jk migrate` proposes the new name.

**Choice of the input manifest** (the same in all `jk` commands, document 07, principle 5):

| Form | Input manifest | Lock (ref/02 §2.1.1) |
|---|---|---|
| none | `joker.toml` (or `defaults.manifest` / `defaults.env` of `.joker/config.toml`, ref/02 §2.3.2) | `joker.lock` |
| `--env <variant>` or `JK_ENV=<variant>` | `joker.<variant>.toml` in the current directory | `joker.<variant>.lock` |
| `--manifest <file>` or `JK_MANIFEST=<file>` | the given file | the same name with `.lock` in place of `.toml`, in the same directory |

- `--env lab` is exactly `--manifest joker.lab.toml`. For an overlay in another directory, `--manifest sites/lisboa-01/joker.summer.toml` (or `cd sites/lisboa-01` and `--env summer`).
- **Precedence:** option > `JK_*` variable > `.joker/config.toml` > default (that of ref/02 §2.3.3). `--env` and `--manifest` are the same choice: an option of either of the two wins over either of the variables, and a variable of either of the two wins over the project configuration. Both options together, or both variables together, are a usage error (exit code 1, document 07, §8), and not a precedence rule that someone has to guess.
- `--env <variant>` for a file that does not exist is `J0160`, with the list of the `joker.*.toml` in the directory.
- `--lock <file>` (or `JK_LOCK`) replaces only the lock, with any of the forms.

---

## 8. The effective manifest

### 8.1 Construction

After the merge (§7), `jk check` builds the effective manifest in this order. Each step only runs if the previous ones have no errors, and within each step the diagnostics are all collected (it does not stop at the first).

| # | Step | Codes |
|---|---|---|
| 1 | read and merge the layers | `J0140`, `J0141`, `J0146`, `J016x` |
| 2 | types, enums, required keys, combinations | `J0142`–`J0145`, `J0147`–`J0149` |
| 3 | grammar of the values (durations, quantities, paths, selectors, references, syntax of the expressions) | `J015x` |
| 4 | load the lock, the profiles, the classes and the board; build the path registry (canonical, aliases, instances, `calc`, alarms, bridges) | `J0101`, `J017x`, `J0193`, `J0196`, `J04xx`, `J06xx` |
| 5 | expand the selectors, materialize the defaults (A9, A11, writers, leases, implicit groups) | `J0155`, `J0156` |
| 6 | bindings, parameters, instances | `J0107`–`J0130`, `J018x`, `J019x`, `J06xx` |
| 7 | envelopes, arbiter, fallback, atomic groups | `J02xx` |
| 8 | expressions (types, units, windows) | `J03xx` (0301–0307) |
| 9 | cycles, RT admission, buses, memory | `J028x`, `J0350`–`J0380` |
| 10 | `prod` rules (signatures, lock, secrets, TLS) | `error-in-prod` |

Diagnostics are output sorted by file, line and code, so that the same input always gives the same output (invariant 9 of `AGENTS.md`).

### 8.2 Form

The effective manifest is a TOML document with:

- the sections in the order of the table of §4, and the keys of each table in the order of the tables of this reference; the free keys (names, paths) in lexicographic order;
- **all defaults materialized**, each one marked as such in `jk diff` (`# default`);
- the selectors of `[outputs]` and `[signals]` kept as they are (the expansion appears in `jk explain`);
- file paths relative to the project root;
- without `extends`, without `widen`, without `accept_missing_fallback` (which live in the lock), with the `x_*` keys.

`jk diff` shows it; `jk explain <key>` shows the origin of each value, layer by layer:

```
$ jk explain 'outputs."pcs.p_set".range'
outputs."pcs.p_set".range = ["-150 kW", "150 kW"]
  templates/joker.bess-v2.toml:31   (not declared)
  sites/lisboa-01/joker.toml:74   ["-125 kW", "125 kW"]     origin
  sites/lisboa-01/joker.summer.toml:9    ["-150 kW", "150 kW"]     widens · widen: "New PCS with 150 kW rated power; …" (joker.lock:88)
```

### 8.3 Hash of the effective manifest

This reference defines the hash of the **effective manifest**; the hash of the **generation** (what `jk`, `jokerd`, the Deck and the `HELLO` of an MCU show, `ref/00` §5) is that of the `joker.config.v1+cbor` descriptor, defined in `ref/10` §2.6.2, and it covers this hash, the canonical digest of the lock (`ref/02` §2.1.16), the contracts of the classes and the artifacts by digest.

The hash of the effective manifest is the SHA-256 of the deterministic CBOR (RFC 8949, §4.2.1) of the effective manifest:

1. **with the defaults materialized** and **without the `x_*` keys**;
2. with the values normalized (durations in ns, quantities in the canonical unit, selectors and paths as they are, free tables in lexicographic order);
3. with each `File` that changes the behavior (simulation models, EDS, PiCtory configuration, local profiles, `.pub` of `[security.trust]`) replaced by the SHA-256 of its content.

Therefore, reordering keys, changing comments or splitting a file into two overlays **does not change** the hash, and neither does changing a default by declaring it explicitly. The values of secrets never enter: only the names (`secret:<name>`). The Deck compares generations by the generation hash (`ref/10`).

### 8.4 In the API

The local API exposes the effective manifest of the active generation and of each stored generation, in JSON with the same keys, and the provenance in a separate object indexed by JSON Pointer (`ref/09`). Keys starting with `_` are reserved precisely so that they do not collide with metadata of this kind.

---

## 9. Versions and compatibility

| Rule | Detail |
|---|---|
| `[project] joker = "X.Y"` | minimum version of the runtime and of the schema. An older runtime rejects the manifest (`J0147`) before validating the rest |
| each key has an entry version (`since`) | in the schema generated from the Rust types. Using a key with `since` > `joker` is `J0148`, with the suggestion to raise `joker`. This is what prevents a manifest from looking compatible with a runtime that would ignore it |
| only additions in a minor | new optional keys, new variants at the end of an enum, new sections. The default of a new key reproduces the previous behavior (golden rule, ADR 0022) |
| **defaults do not change within a major** | changing a default is changing the behavior of all the manifests that do not declare it |
| before 1.0 | the runtime accepts the current minor and the previous one of the schema (document 09, attack 8); `jk migrate` rewrites between minors |
| in the 1 series | a 1.x runtime accepts any 1.y manifest with y ≤ x; `jk migrate` is optional |
| in 2.0 | `jk migrate` is required; 2.0 reads the last 1.x (document 21, §1.1) |
| reserved | the reserved keys (§6, including those of the 1.0 cut in §6.6) exist in the 1.0 schema with the `since` of the version that brings them (`"1.1"`, `"1.2"`, or the patch release of a `1.0.x` key): `J0290` on a runtime older than that, and `J0148` with an older `joker` on a runtime that has them |
| codes | a code is never reused after being removed (`ref/00` §1) |

---

## 10. Manifest code registry

All of them live in `crates/joker-manifest/codes.toml` (document 16, §9). Severities: `error`; `warning` (in all modes); `error-in-prod` (error in `prod`; `W` warning with the same number in `dev`).

```toml
# excerpt of the registry format (crates/joker-manifest/codes.toml; document 16, §9)
[[code]]
id       = "J0140"
severity = "error"
message  = "unknown key"
explain  = "The manifest is strict: an unknown key is almost always a typo. Extension keys start with x_."
doc      = "ref/01 §2.3"

[[code]]
id       = "J0205"
severity = "error-in-prod"
message  = "device fallback missing or inconsistent with the safe"
explain  = "The equipment fallback must be as safe as the safe, or safer, in the sense of safer (M1)."
doc      = "01 §8"
```

### 10.1 Existing codes (collected from the documents)

| Code | Severity | Message | Where it is used here | Document |
|---|---|---|---|---|
| `J0101` | error | unknown signal (with suggestion) | paths, actions and profile signals | 03 §15 |
| `J0107` | error | incompatible unit (dimension) | quantities, bindings, `unit` | 03 §15, 02 §3 |
| `J0108` | error | incompatible quantity (W vs var) | quantities, bindings | 03 §15 |
| `J0109` | error | unit not in the catalog | quantities | 16 §5 |
| `J0110` | error | required port without a binding | `bind` | 03 §15 |
| `J0120` | error | parameter outside the range declared by the app | `params` | 03 §15 |
| `J0121` | error | unknown parameter | `params` | 03 §15 |
| `J0130` | warning | group period outside the task's `period_tolerance` | `cycles` | 04 §3.2 |
| `J0140` | error | unknown key | the whole file | `ref/00` §1 |
| `J0201` | error | output without `safe` | `outputs` | 03 §15 |
| `J0202` | error | `rate` (or `safe_ramp`) with a unit inconsistent with the output | `outputs` | 03 §15 |
| `J0203` | error | `safe` outside the `range` | `outputs` | 16 E5 |
| `J0205` | error-in-prod | device fallback missing or inconsistent with `safe` | `outputs`, `devices.*.fallback` | 03 §15, 01 §8 |
| `J0206` | error | scheduled writes without `on_late` | `outputs` (1.1) | 02 §7.2 |
| `J0207` | error | `safe = "relinquish"` on a driver without support | `outputs` | 03 §6.1 |
| `J0210` | error | two writers at the same level of the same output | `writer` | 16 A5 |
| `J0211` | error | output with several writers and one without `writer` | `writer` | 03 §8 |
| `J0212` | error | proposal-only writer with a direct write | `writer.kind` (1.1) | 03 §8 |
| `J0213` | error | lease above `max_lease` (on an MCU output, the link's `max_lease`) | `writer.lease` | 16 A9, A12 |
| `J0215` | error | two envelopes for the same signal | `outputs` | 16 P2 |
| `J0220` | error | overlay widens the envelope without `widen` | overlays | 03 §15 |
| `J0301` | error | expression with incompatible dimensions | expressions | 03 §15 |
| `J0302` | error | unknown signal in an expression | expressions, `message` | 02 §10 |
| `J0303` | error | enum without that variant | expressions, `Value` | 02 §10 |
| `J0304` | error | expression with incompatible types (operand, enum of another type, unreadable signal, result of the wrong type) | expressions | ADR 0034 |
| `J0305` | error | window above the maximum (15 min) | expressions | 16 §4 |
| `J0306` | error | `own()` outside a class method | expressions | 16 §4 |
| `J0307` | error | unknown function, wrong arity, or an argument that must be a path, duration or constant | expressions | ADR 0034 |
| `J0350` | error | cycle group not schedulable (M23) | `cycles` | 03 §15 |
| `J0351` | error | group < 10 ms without `kernel = "preempt_rt"` | `cycles`, `target` | 03 §15 |
| `J0352` | warning | group ≥ 10 ms without PREEMPT_RT: jitter not guaranteed | `cycles`, `target` | 16 §9 |
| `J0360` | error | resource class without CPUs, or with CPUs overlapping `rt` (except on a board with `cpus = 1`, where it is a note, §5.4) | `resources` | 03 §15 |
| `J0370` | error | bus overloaded | `buses`, `devices` | 05 §5 |
| `J0380` | error | estimated memory above 70% of the board's RAM | the whole manifest | 03 §15 |
| `J0381` | warning | projected write load above what the board's `storage` withstands (also in `jk doctor`, measured) | `[telemetry]`, `[apps.*] kv`, `trace` | `ref/03` §2.3.1, 01 §12 |

Referred to, from other ranges: `J0401` (drift against the lock), `J0501`, `J0502`, `J0510` (ML, 1.1), `J0601`–`J0604`, `J0610`, `J0620`, `J0630`, `J0650`–`J0656` (classes and instances), `J0710`, `J0720`, `J0721` (Joker Micro).

### 10.2 New codes (ranges `J0140`–`J0199` and `J0230`–`J0299`)

**Structure (`J014x`)**

| Code | Severity | Message |
|---|---|---|
| `J0140` | error | unknown key (already fixed in `ref/00`) |
| `J0141` | error | reserved key or name (`_…`, `joker…`, prefixes `calc.`, `alarm.`, `app.`, `bridge.`, `joker.`, `@`) |
| `J0142` | error | required key missing in the effective manifest |
| `J0143` | error | wrong value type |
| `J0144` | error | value outside the allowed set |
| `J0145` | error | invalid key combination (exclusive, dependent, or not applicable on this target) |
| `J0146` | error | file is not valid TOML 1.0 or is not UTF-8 |
| `J0147` | error | invalid `[project] joker`, or it requires a newer runtime than this one |
| `J0148` | error | key introduced in a version later than the one declared in `joker` |
| `J0149` | error | manifest limit exceeded (§11.3) |

**Value grammar (`J015x`)**

| Code | Severity | Message |
|---|---|---|
| `J0150` | error | invalid name (grammar, length, dots in a name key) |
| `J0151` | error | number without a unit where a quantity or duration is required |
| `J0152` | error | invalid duration (syntax, zero, negative, above the key's maximum) |
| `J0153` | error | invalid path |
| `J0154` | error | invalid selector |
| `J0155` | error | selector without matches (outputs, bindings, envelopes, atomics, trips, bridges) |
| `J0156` | warning | telemetry, exposure or security selector without matches |
| `J0157` | error | invalid reference, unknown scheme, or reference in a key that does not accept it |
| `J0158` | error | expression with a syntax error |
| `J0159` | error | invalid budget, size or data volume literal |

**Overlays (`J016x`)**

| Code | Severity | Message |
|---|---|---|
| `J0160` | error | referenced file not found (`extends`, `path`, `model`, `eds`, keys…), or non-local `extends` |
| `J0161` | error | `extends` in a cycle |
| `J0162` | error | `extends` with more than 8 levels or 32 files |
| `J0163` | warning | `unset` with nothing to remove |
| `J0164` | error | invalid merge directive, or repeated element in an ordered list |
| `J0165` | error | overlay changes the type of a key (table ↔ value) |
| `J0166` | error | `widen` without sufficient justification (< 10 characters) or on a key that is not an envelope key |
| `J0167` | warning | `widen` in a layer that widens nothing |
| `J0168` | error-in-prod | widening without the matching entry in `[accepted]` in the lock |
| `J0169` | warning | input manifest with a name outside the `joker.toml` / `joker.<variant>.toml` rule (accepted only by `--manifest`, §7.7) |

**Devices, buses and signals (`J017x`)**

| Code | Severity | Message |
|---|---|---|
| `J0170` | error | driver unknown in this build, or tier W/X without `[drivers]` |
| `J0171` | error | bus does not exist, has an incompatible type, or has incompatible protocols |
| `J0172` | error | profile or board not found, or profile without a major |
| `J0173` | error | key that this driver does not accept |
| `J0174` | error | created alias or path collides with another path |
| `J0175` | error | adjustment of the same signal declared in two places with different values |
| `J0176` | error | adjustment incompatible with the signal's type, direction or origin |
| `J0177` | error | `discover = true` on a driver without discovery |
| `J0178` | error | two devices with the same address on the same bus, or a device with the same kernel resource as a referenced `board.<name>` (`ref/03` §2.3.2) |
| `J0179` | error | Joker Link without authentication on an IP transport |

**Apps and bindings (`J018x`)**

| Code | Severity | Message |
|---|---|---|
| `J0180` | error | invalid app source (none or two; digest different from the lock) |
| `J0181` | error | invalid binding form for the port type |
| `J0182` | error | unknown task or group in `[apps.*.cycles]` |
| `J0183` | error | `as = "fraction"`/`"percent"` between units that are not `%` and `1` |
| `J0184` | error | invalid `net.allow` |
| `J0185` | error-in-prod | referenced secret does not exist in the node's vault |
| `J0186` | error | app resource class inconsistent with that of the groups, or `rt` in async |
| `J0187` | error | `native` app that is not in this runtime build |
| `J0188` | error | `shadow.of` not resolvable (reserved 1.1) |
| `J0189` | error | port bound in two places |

**Instances, `calc`, alarms, telemetry, bridges (`J019x`)**

| Code | Severity | Message |
|---|---|---|
| `J0190` | error | circular dependency between `calc` and alarms |
| `J0191` | error | `implemented_by` points to an app that does not exist or does not implement the class |
| `J0192` | error | instance with more than one source (`map`, `implemented_by`, `sim`); in a virtual instance, a `map` may only bind class members |
| `J0193` | error | unknown class or capability |
| `J0194` | error | invalid adapter for the member's direction or type |
| `J0195` | error | ambiguous or incomplete recording policy |
| `J0196` | error | colliding alarm name or created path (including an alarm named `app`: app alarms live in `alarm.app.<app>.<name>`, §5.15) |
| `J0197` | error | `keep` with an unknown layer |
| `J0198` | error | uplink with `classes`, `live` or `history` invalid for its `kind` |
| `J0199` | error | invalid bridge (`to`, `writable`, `listen`/`bus`, `fields`) (reserved 1.2) |

**Envelope (`J023x`)**

| Code | Severity | Message |
|---|---|---|
| `J0230` | error | `range` with minimum ≥ maximum, or without two elements |
| `J0231` | error | `safe` with a type incompatible with the output |
| `J0232` | error | non-constant `safe` |
| `J0233` | error | envelope key that does not apply to the output type |
| `J0234` | error | envelope on a path that is not an output of this node (or governed by a Joker Micro) |
| `J0235` | error | invalid `verify` |
| `J0236` | warning | `reset_when` on an output that never trips |
| `J0237` | error | `group` does not exist |
| `J0238` | warning | `safer` without a device fallback (ignored) |
| `J0239` | error | scheduled writes above 256, or `timed` on a driver without timed outputs (1.1) |

**Joker Micro network (`J024x`)**

| Code | Severity | Message |
|---|---|---|
| `J0240` | error | invalid `[net.wifi]`: `ssid` empty or longer than 32 B, `country` outside ISO 3166-1 alpha-2, or `hostname` outside the grammar |
| `J0241` | error | IP transport on an MCU without `[net.wifi]` (`[link] address`, `[telemetry.uplink]`) |
| `J0242` | error-in-prod | open Wi-Fi network: `[net.wifi]` without `password` |

**Board without a Wasm engine (`J0243`)**

| Code | Severity | Message |
|---|---|---|
| `J0243` | warning | the board has no Wasm engine for its architecture: native apps only |

`jk check` emits `J0243` when the `arch` of the `[target]` board has no Wasm engine in this runtime (neither Cranelift nor Pulley; document 01, §22): today, `armv6` if Pulley does not compile for ARMv6 (document 23, §5.2). The warning appears once, with the list of the manifest's Wasm apps; those apps are not loaded on the node (they stay `failed`, ref/09 §5.14), and only the `kind = "native"` apps of the runtime build run (document 04, §5; `J0187`). `jk doctor` repeats it on the node.

**Arbiter and writers (`J025x`)**

| Code | Severity | Message |
|---|---|---|
| `J0250` | error | invalid `level`, or compact form inconsistent with `priority` |
| `J0251` | error | `priority` outside the band |
| `J0252` | error | app or bridge in the `manual` band |
| `J0253` | error | bridge in the `safety` band (reserved 1.2) |
| `J0254` | error | `proposal` writer with `level`, `priority` or `lease` |
| `J0255` | error | `max_lease` > 24 h or `max_manual` > 7 d |
| `J0256` | error | lease shorter than the owning group's period |
| `J0257` | warning | `writer` on an app without outputs |

**Data classes (`J026x`, reserved, §6.7)**

| Code | Severity | Message |
|---|---|---|
| `J0260` | error | derived signal declares a data class less protective than one of its inputs |
| `J0261` | error | egress sends a signal whose data class it does not accept |

**Fallback and atomic groups (`J027x`)**

| Code | Severity | Message |
|---|---|---|
| `J0270` | error | output in two atomic groups |
| `J0271` | error | atomic group without a common transport |
| `J0272` | error | atomic group with outputs from different owning groups |
| `J0273` | error | atomic group `order` is not a permutation of `outputs` |
| `J0274` | error | invalid `commit.signal` |
| `J0275` | error | `fallback.configure = true` on a profile without `[fallback]` |
| `J0276` | error-in-prod | `keepalive` inconsistent with the equipment fallback timeout |
| `J0277` | warning | `accept_missing_fallback` on an output that has a fallback |

**Cycles and resources (`J028x`)**

| Code | Severity | Message |
|---|---|---|
| `J0280` | error | `period` outside the target's limits |
| `J0281` | error | `order` references an app or task that does not run in this group |
| `J0282` | error | group with a resource class that is not `rt` |
| `J0283` | error | explicit `priority` inverts the rate-monotonic order |
| `J0284` | error | `cpu` outside `[resources.rt]` |
| `J0285` | error | `trace.continuous` above 7 d |
| `J0286` | warning | `overrun = "degrade"` without `optional` tasks |
| `J0287` | warning | `overrun = "catch_up"` in a group with time-dependent outputs |
| `J0288` | error | reserved group name used in a way that is not allowed |

**Reservations and security (`J029x`)**

| Code | Severity | Message |
|---|---|---|
| `J0290` | error | feature reserved for a later version of this runtime |
| `J0291` | — | withdrawn in this round: the `api.mtls` key is gone (TCP is always mTLS; the key gives `J0140`). The number is not reused |
| `J0292` | error | `security.trust` references a key that is not in the node's trust bundle with that role (the same code in `jk check` and on the node, on apply) |
| `J0293` | warning | role granted to any principal |
| `J0294` | error-in-prod | plaintext secret in a secret-type key |
| `J0295` | error | invalid principal in `[security.roles]` |
| `J0296` | error-in-prod | connection without TLS or without security (`mqtt://`, `http://`, OPC UA `None`) |
| `J0297` | warning | bridge with writes listening on all interfaces (reserved 1.2) |
| `J0298` | warning | action with `role = "viewer"`: counts as `operator`, which is the API minimum |

`J0244`–`J0249`, `J0258`, `J0259`, `J0262`–`J0269`, `J0278`, `J0279`, `J0289` and `J0299` stay free, so that this reference can grow without leaving the range.

---

## 11. Expected behavior

### 11.1 When applying a new generation

`jk apply` plans the minimal diff (document 01, §15). What each change causes:

| Change | Effect on the node | `--confirm` required in `prod` |
|---|---|---|
| `[outputs.*]` | hot, at the tick boundary of the owning group; the last applied value is inherited (16 A8) | no |
| `[calc]`, `[alarms]` | hot; the state of windows and edges starts over (it is volatile, document 14, A7); active alarms keep their state if the expression did not change | no |
| `[signals]`, `[devices.*.signals]` (adjustments) | hot, in the acquisition stage | no |
| `[devices.*]` (address, profile, bus, driver) | restarts only that driver; the signals go through `Uncertain(Initial)` | no |
| `[buses.*]` | restarts the drivers of that bus | no |
| `[apps.*.params]` | `on-params`, without a restart | no |
| `[apps.*] source`/`digest` | hot swap at a tick boundary (document 01, §9) | no |
| `[apps.*] bind`, `cycles`, `writer`, `resources` | new instance of the app at a tick boundary, with `migrate` | no |
| `[cycles.*]` | the group is recreated at a tick boundary; leases start counting with the new period | no |
| `[instances.*]` | the apps that use the instances are reinstantiated (state of `Many` by name, document 14, A10) | no |
| `[telemetry.record]`, `[telemetry.store]` | hot | no |
| `[telemetry.uplink.*]`, `[bridges.*]`, `[security]`, `[clock]`, `[project] mode` | reconnects the affected service | **yes** (ADR 0015) |
| `[resources]`, `[target]` | the cpusets are reconfigured; the affected groups are recreated | no |

### 11.2 Determinism

- All free tables are `BTreeMap`: the processing order is lexicographic, not that of the file.
- The same set of files always gives the same diagnostics, in the same order, and the same effective manifest hash (§8.3).
- No validation depends on the wall clock, the network or the node, except those of step 10 (§8.1) when `jk check` runs with `--node`.

### 11.3 v1 limits of the manifest

| Thing | Limit | Code |
|---|---|---|
| size of a file | 4 MiB | `J0149` |
| `extends` layers | 8 levels, 32 files | `J0162` |
| devices | 1 024 | `J0149` |
| apps | 50 (document 09, attack 11) | `J0149` |
| outputs, `calc`, alarms, instances | 16 384 each | `J0149` |
| matches of a selector | 4 096 | `J0149` |
| expression | 4 KiB | `J0149` |
| `message`, justifications | 1 KiB | `J0149` |
| `schedule_depth` | 256 | `J0239` |
| path segments | `ref/00` §10 | `J0153` |

### 11.4 Error messages

In the rustc style (`ref/00` §8), always with a code, the location in the **layer** where the key is and, when there is one, help:

```
error[J0151]: number without a unit where a quantity or duration is required
  --> sites/lisboa-01/joker.toml:41:8
   |
41 | safe = 0
   |        ^ `pcs.p_set` is in kW
   = help: write `safe = "0 kW"`
```

---

## 12. Sketch of the Rust types (`joker-manifest`)

Two phases: the layers are read as untyped `toml::Spanned` (so that the merge keeps the provenance), and the effective manifest is deserialized into the types below. The free tables are `BTreeMap` (invariant 9). The published JSON schema is generated from these types, with the `since` of each field.

```rust
use std::collections::BTreeMap;
use serde::Deserialize;

/// Unknown keys are errors (J0140) except `x_*`, which are kept for tooling.
#[derive(Deserialize, Default)]
pub struct Ext(BTreeMap<String, toml::Value>); // custom Deserialize: rejects keys not starting with "x_"

#[derive(Deserialize)]
pub struct Manifest {
    pub project: Project,
    pub target: Option<Target>,
    #[serde(default)] pub resources: BTreeMap<ResourceClass, ResourceSpec>,
    pub clock: Option<Clock>,
    #[serde(default)] pub buses: BTreeMap<Name, Bus>,
    #[serde(default)] pub drivers: BTreeMap<DriverName, DriverDecl>,
    #[serde(default)] pub devices: BTreeMap<Name, Device>,
    #[serde(default)] pub signals: BTreeMap<PathOrSelector, SignalTuning>,
    #[serde(default)] pub outputs: BTreeMap<PathOrSelector, Envelope>,
    #[serde(default)] pub cycles: BTreeMap<Name, CycleGroup>,
    #[serde(default)] pub apps: BTreeMap<Name, App>,
    #[serde(default)] pub instances: BTreeMap<Name, Instance>,
    #[serde(default)] pub calc: BTreeMap<Name, Calc>,
    #[serde(default)] pub alarms: BTreeMap<Name, Alarm>,
    #[serde(default)] pub atomic: BTreeMap<Name, AtomicGroup>,
    #[serde(default)] pub telemetry: Telemetry,
    #[serde(default)] pub bridges: BTreeMap<Name, Bridge>,
    #[serde(default)] pub security: Security,
    pub link: Option<Link>,
    // Reserved for 1.1 (J0290 on a 1.0 runtime).
    #[serde(default)] pub models: BTreeMap<Name, Reserved<Model>>,
    #[serde(default)] pub streams: BTreeMap<Path, Reserved<Stream>>,
    #[serde(default)] pub pipelines: BTreeMap<Name, Reserved<Pipeline>>,
    #[serde(default)] pub training: BTreeMap<Name, Reserved<Training>>,
    #[serde(flatten)] pub ext: Ext,
}

#[derive(Deserialize)]
pub struct Project {
    pub name: DnsName,
    pub joker: Option<SchemaVersion>,          // "MAJOR.MINOR"
    #[serde(default = "Mode::prod")] pub mode: Mode,
    #[serde(flatten)] pub ext: Ext,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode { Dev, Prod }

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Bus {
    Ethernet { iface: String },
    Serial { port: FilePath, baud: u32, #[serde(default)] parity: Parity, #[serde(default = "one")] stop: u8,
             #[serde(default = "eight")] data_bits: u8, turnaround: Option<Duration>,
             #[serde(default)] duplex: Duplex },
    Can { iface: String, bitrate: u32, #[serde(default)] fd: bool, data_bitrate: Option<u32> },
    Ethercat { iface: String, cycle: Duration, #[serde(default)] dc: bool },
    I2c { dev: FilePath },
    Spi { dev: FilePath, #[serde(default)] mode: u8, speed: Option<Quantity> },
}

#[derive(Deserialize)]
pub struct Device {
    pub driver: DriverRef,                    // "modbus-tcp" | "w:<name>" | "x:<name>" | "extern" (MCU)
    pub bus: Option<Name>,
    pub profile: Option<ProfileRef>,
    pub poll: Option<Duration>,
    pub timeout: Option<Duration>,
    pub retries: Option<u8>,
    pub align: Option<Name>,
    #[serde(default)] pub discover: bool,
    pub fallback: Option<DeviceFallback>,
    #[serde(default)] pub signals: BTreeMap<Name, SignalTuning>,
    #[serde(default)] pub actions: BTreeMap<Name, ActionConfig>,
    /// Driver-specific keys (appendix A), validated against the driver's published schema (ref/06).
    #[serde(flatten)] pub driver_config: BTreeMap<String, toml::Value>,
}

#[derive(Deserialize)]
pub struct Envelope {
    pub safe: Option<Safe>,                   // None -> J0201 (kept optional to report, not to parse-fail)
    #[serde(default)] pub safer: Safer,
    pub safe_ramp: Option<Rate>,
    pub interlock: Option<Expr>,
    #[serde(default)] pub permit_uncertain: bool,
    #[serde(default)] pub on_interlock: OnInterlock,
    #[serde(default)] pub requires_good: Vec<PathOrSelector>,
    pub grace: Option<Duration>,
    pub range: Option<[Bound; 2]>,
    #[serde(default)] pub on_range: OnRange,
    pub rate: Option<Rate>,
    pub deadband: Option<Quantity>,
    pub min_interval: Option<Duration>,
    pub max_actuations: Option<ActuationBudget>,
    pub max_lease: Option<Duration>,
    pub max_manual: Option<Duration>,
    pub verify: Option<Verify>,
    pub reset_when: Option<Expr>,
    pub group: Option<Name>,
    pub keepalive: Option<Duration>,
    pub pulse: Option<Duration>,
    pub accept_missing_fallback: Option<String>,
    pub widen: Option<Widen>,                 // consumed by the merger; never in the effective manifest
    pub schedule_depth: Option<Reserved<u16>>,
    pub timed: Option<Reserved<bool>>,
    pub on_late: Option<Reserved<OnLate>>,
    pub late_margin: Option<Reserved<Duration>>,
    #[serde(flatten)] pub ext: Ext,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Safe { Relinquish(RelinquishLit), Value(SignalValue) } // "relinquish" | bool | int | float | "0 kW" | "E.v"

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Bound { Const(Quantity), Expr(Expr) }   // a string that parses entirely as a quantity is Const

#[derive(Deserialize)]
pub struct App {
    pub source: Option<OciRef>,
    pub path: Option<FilePath>,
    pub digest: Option<Digest>,
    #[serde(default)] pub kind: AppKind,              // wasm | native | extern (MCU)
    pub writer: Option<Writer>,
    #[serde(default)] pub bind: BTreeMap<Name, Binding>,
    #[serde(default)] pub params: BTreeMap<Name, ParamValue>,   // value | { value, runtime, persist } (MCU)
    #[serde(default)] pub cycles: BTreeMap<Name, Name>,
    pub period_tolerance: Option<Percent>,
    pub resources: Option<AppResources>,
    pub net: Option<Net>,
    #[serde(default)] pub secrets: BTreeMap<Name, SecretRef>,   // local name -> "secret:<name>" (D6)
    #[serde(default)] pub writes: Vec<PathOrSelector>,           // kind = "extern" only
    #[serde(default)] pub reads: Vec<Selector>,                  // kind = "extern" only
    #[serde(default)] pub requires_clock: ClockRequirement,
    pub keep_previous: Option<Duration>,
    pub crashloop: Option<Crashloop>,
    pub shadow: Option<AppShadow>,
    pub fallback: Option<Reserved<AppFallback>>,
    #[serde(flatten)] pub ext: Ext,
}

#[derive(Deserialize)]
pub struct Writer {
    pub level: Option<Level>,                 // "control" | "control:10"
    pub priority: Option<u8>,
    pub lease: Option<Duration>,
    #[serde(default)] pub kind: WriterKind,   // direct | proposal (1.1)
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Binding {
    Target(String),                           // path, instance, selector, "class:…", "model:…", "pipe:…"
    List(Vec<String>),
    Signal { signal: Path, #[serde(rename = "as")] as_: Option<AsConversion>, #[serde(default)] optional: bool },
    Each { each: Selector, map: BTreeMap<Name, String> },
}

#[derive(Deserialize)]
pub struct Instance {
    pub class: ClassRef,
    #[serde(default)] pub capabilities: Vec<Name>,
    pub map: Option<BTreeMap<Name, MemberMapping>>,
    pub implemented_by: Option<ImplementedBy>,
    pub sim: Option<SimSpec>,                 // bool | table
    #[serde(default)] pub actions: BTreeMap<Name, ActionConfig>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum ImplementedBy {
    App(AppRef),                              // "app:<name>": one app instance
    PerInstance { app: Name, #[serde(default)] bind: BTreeMap<Name, Binding>,
                  #[serde(default)] params: BTreeMap<Name, SignalValue> },
}

/// A value that exists in the 1.0 schema but is rejected by a 1.0 runtime (J0290).
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Reserved<T>(pub T);

// Value newtypes: each implements FromStr with the grammar of §3 and carries its span for diagnostics.
pub struct Duration(u64);                     // ns
pub struct Quantity { pub value: f64, pub unit: joker_units::Unit }
pub struct Rate(Quantity);                    // unit/s after normalization
pub struct ActuationBudget { pub count: u32, pub window: Duration }
pub struct Level { pub band: Band, pub priority: Option<u8> }
pub struct Expr(String);                      // compiled by joker-expr after the registry is built
pub struct Path(String);
pub struct Selector(String);
```

---

## 13. Stress: eleven attacks on the reference itself

**1 — The typo that turns off a protection.** `max_leas = "15m"` in a site. In a tolerant format, the key would be ignored and `max_lease` would stay at 15 min by default, by chance the same; with `requse_good`, the protection would disappear silently. **What happens:** `J0140` with "did you mean `requires_good`?". **Holds**, and it is the reason the TOML is strict (`ref/00` §1). Tools that want to annotate the manifest use `x_*`.

**2 — The `safe` without a unit and the profile that changes.** An integrator writes `safe = 0` and `range = [-125, 125]`; the PCS profile changes from `kW` to `W` in version 4. **Broke:** the range would become ±125 W without anyone touching the site. **Fix incorporated:** a number without a unit is `J0151` whenever the dimension is not `1` (§3.2), with the suggestion of the signal's current unit.

**3 — The template with a selector and the site that wants to change one inverter.** The template declares `[outputs."inv*.w_max_lim_pct"]`; the site wants `max_lease = "5s"` only on `inv2` and writes `[outputs."inv2.w_max_lim_pct"]`. **What happens:** `J0215` (two envelopes for `inv2`), with the provenance of both entries and the help: "remove the template's entry with `outputs.\"inv*.w_max_lim_pct\" = { unset = true }` and declare `inv1`…`inv3` explicitly". "The most specific wins" was considered and **rejected** for envelopes: an ambiguity in an envelope must be an error (it is the Cravo rule). In recordings (§5.17.1) specificity is accepted, because recording is not a safety decision.

**4 — Widening in two layers.** Layer 2: `range = { unset = true }`; layer 3: `range = ["-200 kW", "200 kW"]`. If layer 3 were compared with layer 2 (without a range), "coming into existence" would be tightening, and the widening would pass without `widen`. **Fix incorporated:** the `unset` of an envelope key is already a widening (§7.4), and requires `widen` in layer 2.

**5 — Different units in the comparison.** The template has `["-125 kW", "125 kW"]` and a site writes `["-125000 W", "125000 W"]`. **What happens:** equal, after conversion; it is not `J0220`. With `["-125 kW", "125.001 kW"]`, it is a widening of 1 W, and it is `J0220`: the rule is exact, and the help shows the difference.

**6 — The diamond with `append`.** `S` extends `A` and `B`, which both extend `T`, and `T` does `requires_good = { append = ["bms.soc"] }`. A naive merge would apply the `append` twice. **Fix incorporated:** post-order linearization with each file only once (§7.1), and `append` on sets without duplicates (§7.3).

**7 — A 1.1 manifest on a 1.0 node.** The Deck sends a 1.0 node a site that uses `schedule_depth`. **What happens:** the 1.0 `jk check` knows the key (reserved) and gives `J0290`; the agent rejects the generation before applying it. With a key that 1.0 does not know at all (a new key from 1.3), it gives `J0148` if the manifest declares `joker = "1.3"`, or `J0147` ("this manifest requires runtime 1.3"). A key is never silently ignored because it is new.

**8 — Security: a site's overlay adds a trust key.** A site repository with broad permissions adds `trust.apps = ["keys/mine.pub"]` to install an app signed by a key of its own. **Broke** in the design of document 03, §13: the configuration declared its own trust. **Fix incorporated:** `[security.trust]` only restricts the node's anchors (`J0292`, §5.19). A role `admin = ["cert:CN=*"]` passes, but with `J0293`, and in `prod` `jk apply` requires `--confirm` for any change in `[security]`.

**9 — A secret pasted into the manifest.** `password = "hunter2"` in an uplink. **What happens:** `J0294` (error in `prod`): the key is of the secret type and only accepts `secret:<name>`. The value never appears in the diagnostics, in `jk diff` or in the API: it is masked (`"****"`) in the message.

**10 — Performance: 3,000 sites and the Deck.** Each site has 3 layers and ~400 lines. The merge is linear in the size of the layers, and the full `jk check` is dominated by the RT admission and by loading the profiles (cached by the lock digest). The hash of the effective manifest, and therefore that of the generation, does not depend on the order of the keys or on the comments (§8.3), and therefore a mass reformat of the repositories does not generate 3,000 rollouts. **Holds**; the target is < 200 ms per site on a laptop, to be measured in task 16 of document 13.

**11 — Relative paths in layers from different directories.** `templates/joker.bess-v2.toml` declares `model = "sim/plant.sim.toml"` and is extended by `sites/lisboa-01/joker.toml`. Relative to what? **Fix incorporated:** to the file that declares the key (§7.5). The template carries its files with it, and the site does not have to copy them. `jk explain` shows the resolved path.

---

## 14. Decisions and rejected alternatives

| # | Decision | Why | Rejected alternative |
|---|---|---|---|
| D1 | `mode` defaults to `"prod"` | forgetting the key in a real site must not turn off signatures nor turn errors into warnings | default `"dev"`: more convenient, and dangerous |
| D2 | number without a unit only in dimensionless quantities | the meaning of a number must not depend on the version of the profile | accepting numbers "in the signal's unit", as in the text interfaces of `ref/00` §4.1 (which keep accepting them, because there the signal is in view) |
| D3 | selector overlaps: an error in envelopes and adjustments; specificity in telemetry | an ambiguity in an envelope is a safety decision taken by chance | specificity everywhere (CSS); first declaration wins |
| D4 | `extends` relative to the declaring file; post-order linearization without repetitions | templates are portable; diamonds are normal in fleets | relative to the project root (broke shared templates); diamonds as an error |
| D5 | "only tighten" compared layer by layer, with `unset` counting as a widening | closes two-layer widenings | comparing only with the first layer (left an intermediate overlay unchecked) |
| D6 | no interpolation of environment variables | the generation must be a function of the files only (replay, hash, Deck); secrets go through `secret:` | `${ENV}` in the Docker Compose style |
| D7 | `[security.trust]` only restricts the node's anchors | a configuration must not be its own root of trust | keeping document 03, §13, as it is |
| D8 | tasks outside `order` run in lexicographic order | the result must not depend on the order of the layers | "in manifest order" (document 03, §7), undefined with overlays |
| D9 | implicit groups `default` (100 ms) and `default_<period>` | reconciles A11 of document 16 with §8 of document 03, which contradicted each other | a single `default` with the requested period (conflict with A11) |
| D10 | `[drivers]` section for tier W and X | an external driver needs a declared artifact or executable, and nothing said so | registering the drivers on the node, outside the manifest (the generation would stop describing them) |
| D11 | the `commit` of an atomic group has no arbiter nor envelope | without writers, the arbiter would write the `safe` on every tick (A6) | treating it as an ordinary output |
| D12 | writer level in two forms (`level` + `priority`, or `"band:n"`) | the compact form is that of `ref/00`, of the CLI and of conformance; the long one is that of document 03 | only one of the forms (broke one of the two sets of examples) |
| D13 | the hash of the effective manifest materializes the defaults and excludes `x_*` | declaring a default does not change the behavior, and neither do tool annotations | hash of the bytes of the files |
| D14 | the local API is configured in `[security.api]`, and not in `[api]` | every choice about the API (TCP listeners, roles) is a security one, and is reviewed in the same place; mTLS is not a choice (always on over TCP) | a `[api]` section of its own |
| D15 | `severity` required in alarms | ISA-18.2 requires a priority, and a default would hide the decision | default `"medium"` |

---

## 15. Version markers

| Phase | What |
|---|---|
| **1.0** | everything in §5, except what is listed below (the cut of document 26, §5; ADR 0035); `[link]` on Joker Micro |
| **1.0.x** (reserved in the 1.0 schema, `J0290`) | buses `spi`; drivers `spi`, `opcua`, `m-bus`, `onewire`, `serial-line`, and `modbus-slave` on an MCU; the `usb` transport of `joker-link`; class simulators (`instances.*.sim`) (§6.6). The driver `revpi` is 1.0.x and implemented (ADR 0078) |
| **1.1** (reserved in the 1.0 schema, `J0290`) | app shadow (`apps.*.shadow`); uplink budgets; uplinks `otlp` and `http`; buses `can`, drivers `can` and `canopen`, `joker-link` over CAN and with RS-485 turn-taking; `[link] peer = "deck"` (§6.6); `[models]`, `[streams]`, `[pipelines]`, `[training]`; `writer.kind = "proposal"`; `apps.*.fallback`; `outputs.*.schedule_depth`, `timed`, `on_late`, `late_margin`; signals `kind = "block"`; `telemetry.record` with `blocks`/`trigger`/`pre`/`post`; `resources.ml.accel`; `bind` with `model:`/`pipe:`; `source = "training:…"`; `security.trust.models`; drivers `v4l2`, `libcamera`, `spi-adc` |
| **1.2** (reserved in the 1.0 schema, `J0290`) | `[drivers]` and the tier W and X drivers (the hosts arrive in 1.2; tier W is open to third parties when it becomes stable, `ref/06` §4); `[bridges]`; uplink `zenoh`; buses `ethercat`; drivers `ethercat` and native `bacnet-ip` (document 21, §4) |
| **1.4** | Wasm apps on M2 MCUs: `[apps.*] source` on an MCU target (today `J0720`) |
| **2.0** | node-qualified paths (`@boiler/pump.run_cmd`, `@` prefix already reserved); site manifest for several nodes (document 21, §8) |

---

## Appendix A — Driver-specific keys (v1 catalog)

The common keys are in §5.8. For each catalog driver, the "Device keys" column **is** the `config` schema of its descriptor, the "Per-signal keys" column is the `address` schema (in the drivers without a profile) and the "`[discovered]`" column is the `discovered` schema (`ref/06` §2.3; `ref/02` §2.1.11). In the drivers with a profile, the address keys of the points are in the profile (`ref/03` §2.2.5), with the same names. A tier W or X driver publishes its schemas in the descriptor, and `jk check` validates against them. The keys marked **(secret)** only accept `secret:<name>` (§3.10). The "Phase" column follows the 1.0 cut (§6.6, ADR 0035): a driver of a later phase is reserved, and using it on a runtime that does not implement it is `J0290`.

| Driver | Tier | Phase | `bus` | `profile` | Device keys | Per-signal keys | `[discovered]` |
|---|---|---|---|---|---|---|---|
| `sim` | N | 1.0 | — | — (the model declares the signals) | `model` (`File` of the simulation model, `ref/03` §2.4, required), `params` (table with the model's `[params]`) | `signal` (the name of the signal in the model, set by the core) | — |
| `modbus-tcp` | N | 1.0 | `ethernet`, optional | required (`"sunspec"` with `discover = true`) | `address` (`"<host>:<port>"`, required; port 502 by default), `unit_id` (0–255, default 1) | — | with `sunspec`: `models` |
| `modbus-rtu` | N | 1.0 | `serial`, required | required (`"sunspec"` with `discover = true`) | `unit_id` (1–247, required) | — | with `sunspec`: `models` |
| `gpio` | N | 1.0 | — | — (expanders: profile of the `gpio` family) | `chip` (`"gpiochip<N>"`, label, or `"auto"`, default `"auto"`: the board chooses, `ref/03` §2.3.2; on an MCU it does not apply, `J0145`) | `line` (BCM name `"GPIO17"`, line name, or integer offset; on an MCU, the name of a pin from the board's `[pins]` with the `gpio` function, such as `"gpio2"` on the `esp32-devkit`, and a pin that does not exist or lacks that function is `J0828`; required), `direction`, `bias` (`"as-is"` \| `"pull-up"` \| `"pull-down"` \| `"disabled"`, default `"as-is"`), `active_low` (`false`), `drive` (`"push-pull"` \| `"open-drain"` \| `"open-source"`, outputs only), `edges` (`"none"` \| `"rising"` \| `"falling"` \| `"both"`, default `"none"`), `timestamp` (`"kernel"` \| `"none"`, default `"kernel"` when there are `edges`), `debounce` | — |
| `sysfs` | N | 1.0 | — | — | none | exactly one of `led` (name in `/sys/class/leds`, e.g. `"ACT"`; `bool` signal, `direction = "output"` or `"input"`; writes `max_brightness` or 0; on an output, on open it sets the LED's `trigger` to `none` and restores the previous one on close, see the note after the table) and `thermal` (zone in `/sys/class/thermal`, e.g. `"thermal_zone0"`; `f64` input in `°C`, read in m°C); `direction` (default `"input"`) | — |
| `i2c` | N | 1.0 | `i2c`, required | required | `address` (integer 0x03–0x77, required), `config` (table validated by the profile's `[config]`, e.g. `pga`, `rate`) | — | — |
| `spi` | N | 1.0.x | `spi`, required | required | `config` (as in `i2c`) | — | — |
| `revpi` | N | 1.0.x (implemented, ADR 0078) | — | — (PiCtory modules) | none: the variables are those of the configuration piControl has loaded (`/dev/piControl0`) | `var` (PiCtory variable name, 1–31 bytes; default: the signal's name), `type` (`bool` for a 1-bit variable, `i64` or `u64` for one of 8, 16 or 32 bits), `direction` | — |
| `mqtt` (input) | N | 1.0 | — | optional | `broker` (`mqtt://`/`mqtts://` URL, required), `username`, `password` **(secret)** | `topic` (required), `json` (RFC 6901 JSON pointer; by default: the whole payload), `qos` (0–1, default 1), `type`, `unit` | — |
| `opcua` (client) | N | 1.0.x | — | optional (imported from NodeSet) | `endpoint` (`"opc.tcp://…"`, required), `security` (`{ policy, mode }`, as in the bridges), `username`, `password` **(secret)**, `sampling` (`Duration`, default = `poll`) | without a profile: `node` (`"ns=2;s=…"`), `attribute` (default `"Value"`), `sampling`, `type`, `unit` | `server_uri`, `namespaces` |
| `can` | N | 1.1 | `can`, required | required (imported from DBC) | — | — | — |
| `canopen` | N | 1.1 | `can`, required | `profile` or `eds` | `node_id` (1–127, required), `eds` (`File`) | — | `vendor_id`, `product_code`, `revision` |
| `m-bus` | N | 1.0.x | `serial`, required | required | `address` (primary 0–250, or secondary `"<16 hex>"`) | — | — |
| `onewire` | N | 1.0.x | — | required | `rom` (`"<16 hex>"`, required) | — | — |
| `serial-line` | N | 1.0.x | `serial`, required | required | — | — | — |
| `joker-link` | N | 1.0 (over CAN and with RS-485 turn-taking, 1.1; `usb`, 1.0.x) | `serial` or `can`; or none, with `usb` or `address` | — (generated from the MCU's catalog, `ref/03` §2.2.16) | exactly one of `bus`, `usb` (`File`, `/dev/ttyACM0`) and `address` (`"udp:<ip>:<port>"` \| `"tcp:<ip>:<port>"`, port 7471 by default; M2 MCUs); `addr` (address of the MCU on the bus, integer; default 0 over serial, 1 on CAN; `J0736`); `can_id_base` (0–255, default `0x4A`; `J0737`); `auth` (`"required"` \| `"none"`, default `"required"`; `J0179`, `J0738`); `key` **(secret)** (default `"secret:link.<device>"`); `node` (`DnsName` expected in the `HELLO`, optional); timings `report`, `refresh`, `keepalive`, `link_timeout`, `max_lease`, `rto_min`, `ack_delay`, `time_interval`, `time_tolerance`, `time_max_delay`, `turn_timeout` (`Duration`); sizes and counts `max_frame`, `max_msg`, `window`, `bulk_max`, `retransmit`, `turn_frames` (`Integer`); `instances` (`Table<name on the MCU, name on the node>`); `micro` (`File` of the MCU's manifest: `jk check` without a connection to the node validates the MCU side from it, and `jk apply` builds and carries the MCU's generation blob). With `address`, it is the IP and port **of the MCU** (in the MCU's manifest, `[link] address` is that of the node, §5.22). Defaults and validation in `ref/07` §15 | — | none beyond the common ones; the `digest` is the hash of the catalog (`ref/02` §2.1.11) |
| `ethercat` | N | 1.2 | `ethercat`, required | required | `position` (integer, required) | — | — |
| `bacnet-ip` | X → N | 1.2 (both tiers; document 21, §4) | `ethernet`, optional | required or by discovery | `network` (CIDR), `device_instance` (integer), `priority` (1–16: the BACnet priority that Joker uses; required with outputs) | — | according to the driver's schema |
| `v4l2` | N | **1.1** | — | — | `device` (`File`), `format` (`{ width, height, pixfmt, fps }`), `trigger` (`{ kind = "hardware" \| "software", input = Path }`) | — | — |
| `libcamera` | X | **1.1** | — | — | `camera` (id), `format`, `trigger` | — | — |
| `spi-adc` | N | **1.1** | `spi`, required | required | `drdy` (`Path` of a GPIO input), `rate` (`Quantity<Hz>`) | `kind`, `block`, `unit` | — |
| `adc`, `pwm` | N (MCU only) | 1.0 | — | — | — | `adc`: `pin` (pin with the board's `adc` function, `ref/03` §2.3.3), `unit`, `scale`; `pwm`: `pin`, `frequency` (`Quantity<Hz>`) | — |
| `esp32` | N (MCU only) | 1.0 | — | — | none; only on ESP32 boards (`arch` = `xtensa-esp32-none-elf` or `xtensa-esp32s3-none-elf`), on another `J0145` | exactly one of: `touch` (`Integer` 0–9: the capacitive touch pad, resolved by the `touch_channel` of the board's `[pins]`, `ref/03` §2.3.3; `touch = 0` is T0, on GPIO4 of the `esp32-devkit`; a pad without a pin on the board is `J0828`; `u64` input, without a unit: the raw value of the peripheral, which **drops** when touched), `rssi = true` (Wi-Fi RSSI in dBm; `i64` input with `unit = "1"`, because the scale is logarithmic; without `[net.wifi]` or without a connection, `bad(not_connected)`), `uptime = true` (time since boot; `f64` input in `s`, `monotonic`); `direction` only `"input"` (another is `J0145`); `poll` (by default: that of the device); the common signal adjustments (`deadband`, `stuck_detect`, `limits`, §5.8.1). The types are the same that a Modbus profile gives to `u16`, `i16` and `u32` registers with `unit = "s"` (`ref/03` §2.2.7): replacing the bench firmware with Joker Micro does not change the types of the signals (document 23, §3 and §6.5) | — |
| `modbus-slave` | N (MCU only) | 1.0.x | `serial`, required | required (the served map, in the profile format) | `unit_id` (1–247, required) | — | — |
| `extern` | — (MCU only) | 1.0 | — | — | none | `type`, `unit`, `direction` (§5.22; `J0785`) | — |
| `w:<name>`, `x:<name>` | W, X | 1.2 | according to the descriptor | according to the descriptor | those of the `config` schema | those of the `address` schema | those of the `discovered` schema |

**What the `sim` driver receives.** The driver does no I/O (`ref/06` §2.4), so the core reads the files and passes their content in the device's `config` (ADR 0051, ADR 0054): `model` holds the **source text** of the model instead of its path; `params` is the CBOR of the `params` table; `profile` is the source text of the profile named by `[model] profile` (the one pinned in the lock); `files` maps each path as written in the model (`replay` `file`, `[plant] component`) to the file's bytes; `seed` is the seed override of a scenario. The seed is `config.seed`, else `[model] seed`, else the FNV-1a of the device name. A missing profile is `J0803`, a missing file `J0831` (`replay`) or `J0837` (`[plant]`). The address of each point is `{ signal = "<name>" }`; `plan` refuses an unknown signal (`J0853`) and a point whose direction or raw kind differs from the model (`J0809`).

**`sysfs` and the LEDs' `trigger`.** A kernel LED may be driven by a `trigger` (`mmc0`, `heartbeat`, `default-on`…), which overrides the written `brightness`. When opening a `led` signal with `direction = "output"`, the driver reads the current `trigger`, saves it, and writes `none`; on close (orderly stop of `jokerd`, a generation swap that removes the signal, `jk device stop`), it restores the saved `trigger`. An abrupt stop leaves the `trigger` at `none` and the last `brightness`: this is why the behavior of the LED with `jokerd` stopped is the `fallback` of the board's `[io]` (ref/03 §2.3.2), and not the original `trigger`. An input-only LED (`direction = "input"`) does not touch the `trigger`. **Permissions:** the `jokerd` package installs the udev rule `/usr/lib/udev/rules.d/60-joker-leds.rules`, which gives the `joker` group write access to `brightness` and `trigger` of `/sys/class/leds/*`; without it, opening the LED fails with `bad(config_error)` and `jk doctor` says so (document 05, §10).

`"sunspec"` is not a driver: it is the short form of the built-in profile `joker/sunspec@1`, discovered by the Modbus drivers (document 05, §3 and §7; `ref/03` §2.2.15).

**`retries` and `retransmit` in `joker-link`.** The common key `retries` (§5.8, core retries per driver request) does not apply to a `joker-link` (`J0145`): the link does not make per-batch requests. The number of retransmissions of the link's reliable messages is its own key `retransmit` (`ref/07` §5.6, §15).

---

## Appendix B — Coverage table

**Method.** A script extracted **all** the ` ```toml ` blocks from all the project's `.md` files outside `docs/ref/` (area documents, examples, track, ADRs, proposals; the blocks of the references are the definitions themselves, and are not included): 125 blocks, all valid TOML. Of these, 80 are manifests, overlays or manifest fragments (2 fragments of `[apps.light]` from the track, read as `apps.*.…`); the others are locks, profiles, boards, simulation models, scenarios, conformance cases, the configuration of `jk` and of the node, `fleet.toml` and `model.toml`, which belong to `ref/02`, `ref/03` and `ref/10`. The script walked through each manifest and normalized the keys, replacing the free names with `*` (`devices.esp.poll` → `devices.*.poll`; the elements of arrays of tables with `[]`; the overlay directives `unset`, `append` and `widen` stay as the last component). The result is **371 distinct keys**. Each one was assigned to the section of this reference that defines it, and a second pass confirmed that the name of the key appears in that section; none was left without a section or without a definition. The table was regenerated after the rewrite of the track (eleven parts on the bench of document 23).

| Status | Keys |
|---|---|
| covered | 256 |
| covered (1.1) | 94 |
| covered (Joker Micro only) | 11 |
| covered: overlay directive | 4 |
| covered (1.1): free key of `hyper` | 3 |
| covered (C21 applied: 08 uses `path = "apps/…"`) | 1 |
| covered (`proposal` is 1.1) | 1 |
| covered; only restricts the node's bundle (C9, resolved: D7) | 1 |

The "Uses" column counts the blocks where the key appears; "Where" shows the first occurrence and at most two more (file:line of the block, in file order).

| Key | Uses | Where | Section | Status |
|---|---|---|---|---|
| `alarms` | 10 | 02-signal-model:274, 03-manifest:372, 08-stress-tests:253 (+7) | §5.15 | covered |
| `alarms.*` | 10 | 02-signal-model:274, 03-manifest:372, 08-stress-tests:253 (+7) | §5.15 | covered |
| `alarms.*.actions` | 3 | 02-signal-model:274, 03-manifest:372, examples/01-bess-peak-shaving:24 | §5.15 | covered |
| `alarms.*.actions.trip` | 3 | 02-signal-model:274, 03-manifest:372, examples/01-bess-peak-shaving:24 | §5.15 | covered |
| `alarms.*.delay_off` | 3 | 02-signal-model:274, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.15 | covered |
| `alarms.*.delay_on` | 9 | 02-signal-model:274, 08-stress-tests:253, 23-development-bench:195 (+6) | §5.15 | covered |
| `alarms.*.message` | 6 | 02-signal-model:274, 23-development-bench:195, examples/02-visual-inspection:35 (+3) | §5.15 | covered |
| `alarms.*.severity` | 10 | 02-signal-model:274, 03-manifest:372, 08-stress-tests:253 (+7) | §5.15 | covered |
| `alarms.*.suppress_when` | 2 | 02-signal-model:274, examples/03-predictive-maintenance:11 | §5.15 | covered |
| `alarms.*.when` | 10 | 02-signal-model:274, 03-manifest:372, 08-stress-tests:253 (+7) | §5.15 | covered |
| `apps` | 28 | 03-manifest:251, 06-ml:171, 08-stress-tests:23 (+25) | §5.12 | covered |
| `apps.*` | 28 | 03-manifest:251, 06-ml:171, 08-stress-tests:23 (+25) | §5.12 | covered |
| `apps.*.bind` | 25 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+22) | §5.12.1 | covered |
| `apps.*.bind.*` | 25 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+22) | §5.12.1 | covered |
| `apps.*.bind.*.as` | 3 | 03-manifest:251, 08-stress-tests:23, examples/01-bess-peak-shaving:103 | §5.12.1 | covered |
| `apps.*.bind.*.each` | 1 | 14-classes-and-instances:300 | §5.12.1 | covered |
| `apps.*.bind.*.map` | 1 | 14-classes-and-instances:300 | §5.12.1 | covered |
| `apps.*.bind.*.map.*` | 1 | 14-classes-and-instances:300 | §5.12.1 | covered |
| `apps.*.bind.*.optional` | 1 | 08-stress-tests:444 | §5.12.1 | covered |
| `apps.*.bind.*.signal` | 4 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:444 (+1) | §5.12.1 | covered |
| `apps.*.cycles` | 15 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:341 (+12) | §5.12 | covered |
| `apps.*.cycles.*` | 15 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:341 (+12) | §5.12 | covered |
| `apps.*.digest` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.fallback` | 2 | 06-ml:171, 08-stress-tests:518 | §6.5 | covered (1.1) |
| `apps.*.fallback.controller` | 2 | 06-ml:171, 08-stress-tests:518 | §6.5 | covered (1.1) |
| `apps.*.fallback.when` | 2 | 06-ml:171, 08-stress-tests:518 | §6.5 | covered (1.1) |
| `apps.*.kind` | 1 | 08-stress-tests:341 | §5.12 | covered |
| `apps.*.net` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.net.allow` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.params` | 15 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+12) | §5.12.2 | covered |
| `apps.*.params.*` | 15 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+12) | §5.12.2 | covered |
| `apps.*.path` | 17 | 08-stress-tests:158, 08-stress-tests:341, 08-stress-tests:518 (+14) | §5.12 | covered |
| `apps.*.requires_clock` | 2 | 03-manifest:251, 08-stress-tests:444 | §5.12 | covered |
| `apps.*.resources` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.resources.async` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.resources.cycle` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.resources.memory` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.secrets` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.secrets.*` | 1 | 03-manifest:251 | §5.12 | covered |
| `apps.*.shadow` | 2 | 03-manifest:251, 08-stress-tests:518 | §5.12.3 | covered |
| `apps.*.shadow.of` | 2 | 03-manifest:251, 08-stress-tests:518 | §5.12.3 | covered |
| `apps.*.shadow.promote` | 1 | 08-stress-tests:518 | §5.12.3 | covered |
| `apps.*.shadow.promote.outputs` | 1 | 08-stress-tests:518 | §5.12.3 | covered |
| `apps.*.shadow.until` | 2 | 03-manifest:251, 08-stress-tests:518 | §5.12.3 | covered |
| `apps.*.shadow.until.duration` | 2 | 03-manifest:251, 08-stress-tests:518 | §5.12.3 | covered |
| `apps.*.shadow.until.max_divergence` | 1 | 03-manifest:251 | §5.12.3 | covered |
| `apps.*.source` | 5 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:444 (+2) | §5.12 | covered (C21 applied: 08 uses `path = "apps/…"`) |
| `apps.*.writer` | 12 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+9) | §5.12 | covered |
| `apps.*.writer.kind` | 1 | 08-stress-tests:518 | §5.12 | covered (`proposal` is 1.1) |
| `apps.*.writer.lease` | 11 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+8) | §5.12 | covered |
| `apps.*.writer.level` | 10 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+7) | §5.12 | covered |
| `apps.*.writer.priority` | 9 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+6) | §5.12 | covered |
| `atomic` | 3 | 02-signal-model:162, 03-manifest:372, 08-stress-tests:341 | §5.16 | covered |
| `atomic.*` | 3 | 02-signal-model:162, 03-manifest:372, 08-stress-tests:341 | §5.16 | covered |
| `atomic.*.commit` | 1 | 02-signal-model:162 | §5.16 | covered |
| `atomic.*.commit.signal` | 1 | 02-signal-model:162 | §5.16 | covered |
| `atomic.*.commit.value` | 1 | 02-signal-model:162 | §5.16 | covered |
| `atomic.*.on_rate` | 2 | 03-manifest:372, 08-stress-tests:341 | §5.16 | covered |
| `atomic.*.order` | 2 | 02-signal-model:162, 03-manifest:372 | §5.16 | covered |
| `atomic.*.outputs` | 3 | 02-signal-model:162, 03-manifest:372, 08-stress-tests:341 | §5.16 | covered |
| `bridges` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.expose` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.expose.*` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.kind` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.listen` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.security` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.security.mode` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.security.policy` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.security.trust` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.subscribe` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.subscribe.*` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.subscribe.*.fields` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.subscribe.*.max_age` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.subscribe.*.to` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.subscribe.*.type` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | covered |
| `bridges.*.writer` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.writer.lease` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.writer.level` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.writer.priority` | 1 | 03-manifest:423 | §5.18 | covered |
| `bridges.*.writer.writable` | 1 | 03-manifest:423 | §5.18 | covered |
| `buses` | 11 | 03-manifest:83, 08-stress-tests:253, 08-stress-tests:341 (+8) | §5.6 | covered |
| `buses.*` | 11 | 03-manifest:83, 08-stress-tests:253, 08-stress-tests:341 (+8) | §5.6 | covered |
| `buses.*.baud` | 8 | 03-manifest:83, 23-development-bench:255, 23-development-bench:304 (+5) | §5.6 | covered |
| `buses.*.bitrate` | 1 | 03-manifest:83 | §5.6 | covered |
| `buses.*.cycle` | 2 | 03-manifest:83, 08-stress-tests:341 | §5.6 | covered |
| `buses.*.dc` | 1 | 08-stress-tests:341 | §5.6 | covered |
| `buses.*.dev` | 3 | 08-stress-tests:253, examples/00-first-project:194, examples/03-predictive-maintenance:11 | §5.6 | covered |
| `buses.*.dtr` | 2 | track/04-esp32-modbus:375, track/06-switching-boards:31 | §5.6 | covered |
| `buses.*.iface` | 2 | 03-manifest:83, 08-stress-tests:341 | §5.6 | covered |
| `buses.*.kind` | 9 | 03-manifest:83, 08-stress-tests:253, 08-stress-tests:341 (+6) | §5.6 | covered |
| `buses.*.parity` | 2 | 03-manifest:83, examples/00-first-project:194 | §5.6 | covered |
| `buses.*.port` | 6 | 03-manifest:83, 23-development-bench:255, examples/00-first-project:194 (+3) | §5.6 | covered |
| `buses.*.rts` | 2 | track/04-esp32-modbus:375, track/06-switching-boards:31 | §5.6 | covered |
| `buses.*.stop` | 1 | 03-manifest:83 | §5.6 | covered |
| `buses.*.turnaround` | 1 | 03-manifest:83 | §5.6 | covered |
| `calc` | 6 | 03-manifest:372, 08-stress-tests:23, 23-development-bench:195 (+3) | §5.14 | covered |
| `calc.*` | 6 | 03-manifest:372, 08-stress-tests:23, 23-development-bench:195 (+3) | §5.14 | covered |
| `clock` | 2 | 03-manifest:53, track/09-joker-micro:48 | §5.5 | covered |
| `clock.max_holdover` | 1 | 03-manifest:53 | §5.5 | covered |
| `clock.servers` | 1 | 03-manifest:53 | §5.5 | covered |
| `clock.sync` | 2 | 03-manifest:53, track/09-joker-micro:48 | §5.5 | covered |
| `cycles` | 9 | 03-manifest:229, 08-stress-tests:23, 08-stress-tests:341 (+6) | §5.11 | covered |
| `cycles.*` | 9 | 03-manifest:229, 08-stress-tests:23, 08-stress-tests:341 (+6) | §5.11 | covered |
| `cycles.*.order` | 2 | 03-manifest:229, track/08-your-own-class:222 | §5.11 | covered |
| `cycles.*.overrun` | 2 | 03-manifest:229, 08-stress-tests:341 | §5.11 | covered |
| `cycles.*.period` | 9 | 03-manifest:229, 08-stress-tests:23, 08-stress-tests:341 (+6) | §5.11 | covered |
| `cycles.*.resources` | 4 | 03-manifest:229, 08-stress-tests:23, 08-stress-tests:341 (+1) | §5.11 | covered |
| `cycles.*.trace` | 2 | 03-manifest:229, examples/01-bess-peak-shaving:24 | §5.11 | covered |
| `cycles.*.trace.continuous` | 2 | 03-manifest:229, examples/01-bess-peak-shaving:24 | §5.11 | covered |
| `cycles.*.trip_after` | 2 | 03-manifest:229, 08-stress-tests:341 | §5.11 | covered |
| `devices` | 32 | 02-signal-model:179, 03-manifest:113, 03-manifest:159 (+29) | §5.8 | covered |
| `devices.*` | 32 | 02-signal-model:179, 03-manifest:113, 03-manifest:159 (+29) | §5.8 | covered |
| `devices.*.actions` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | covered |
| `devices.*.actions.*` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | covered |
| `devices.*.actions.*.permit` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | covered |
| `devices.*.actions.*.role` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | covered |
| `devices.*.actions.*.timeout` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | covered |
| `devices.*.address` | 8 | 03-manifest:113, 08-stress-tests:23, 08-stress-tests:341 (+5) | appendix A | covered |
| `devices.*.align` | 1 | examples/01-bess-peak-shaving:103 | §5.8 | covered |
| `devices.*.auth` | 2 | 23-development-bench:304, track/09-joker-micro:199 | appendix A | covered |
| `devices.*.bus` | 11 | 03-manifest:113, 08-stress-tests:253, 08-stress-tests:341 (+8) | §5.8 | covered |
| `devices.*.bus.unset` | 1 | track/10-logic-on-the-mcu:236 | §7.2 | covered: overlay directive |
| `devices.*.chip` | 3 | 08-stress-tests:158, 08-stress-tests:444, examples/02-visual-inspection:35 | appendix A | covered |
| `devices.*.config` | 1 | 03-manifest:113 | appendix A | covered |
| `devices.*.device` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.discover` | 3 | 03-manifest:113, 08-stress-tests:23, examples/01-bess-peak-shaving:103 | §5.8 | covered |
| `devices.*.drdy` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | appendix A | covered (1.1) |
| `devices.*.driver` | 27 | 03-manifest:113, 08-stress-tests:23, 08-stress-tests:158 (+24) | §5.8 | covered |
| `devices.*.eds` | 1 | 03-manifest:113 | appendix A | covered |
| `devices.*.fallback` | 3 | 03-manifest:113, 08-stress-tests:23, examples/01-bess-peak-shaving:103 | §5.8 | covered |
| `devices.*.fallback.configure` | 3 | 03-manifest:113, 08-stress-tests:23, examples/01-bess-peak-shaving:103 | §5.8 | covered |
| `devices.*.fallback.keepalive` | 1 | 03-manifest:113 | §5.8 | covered |
| `devices.*.format` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.format.fps` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.format.height` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.format.pixfmt` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.format.width` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.micro` | 1 | track/09-joker-micro:199 | appendix A | covered |
| `devices.*.model` | 4 | 03-manifest:113, examples/00-first-project:41, track/01-hello-bench:97 (+1) | appendix A | covered |
| `devices.*.network` | 1 | 08-stress-tests:518 | appendix A | covered |
| `devices.*.node` | 1 | track/09-joker-micro:199 | appendix A | covered |
| `devices.*.node_id` | 1 | 03-manifest:113 | appendix A | covered |
| `devices.*.poll` | 4 | 03-manifest:113, 08-stress-tests:23, 23-development-bench:238 (+1) | §5.8 | covered |
| `devices.*.position` | 1 | 08-stress-tests:341 | appendix A | covered |
| `devices.*.priority` | 1 | 08-stress-tests:518 | appendix A | covered |
| `devices.*.profile` | 16 | 03-manifest:113, 08-stress-tests:23, 08-stress-tests:253 (+13) | §5.8 | covered |
| `devices.*.profile.unset` | 2 | 23-development-bench:304, track/09-joker-micro:199 | §7.2 | covered: overlay directive |
| `devices.*.rate` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | appendix A | covered (1.1) |
| `devices.*.refresh` | 3 | 23-development-bench:304, track/09-joker-micro:199, track/10-logic-on-the-mcu:236 | appendix A | covered |
| `devices.*.report` | 3 | 23-development-bench:304, track/09-joker-micro:199, track/10-logic-on-the-mcu:236 | appendix A | covered |
| `devices.*.retries` | 1 | 03-manifest:113 | §5.8 | covered |
| `devices.*.signals` | 16 | 03-manifest:159, 08-stress-tests:23, 08-stress-tests:158 (+13) | §5.8.1 | covered |
| `devices.*.signals.*` | 16 | 03-manifest:159, 08-stress-tests:23, 08-stress-tests:158 (+13) | §5.8.1 | covered |
| `devices.*.signals.*.active_low` | 1 | track/09-joker-micro:48 | §5.8.1, appendix A | covered |
| `devices.*.signals.*.alias` | 11 | 03-manifest:159, 08-stress-tests:23, 08-stress-tests:158 (+8) | §5.8.1 | covered |
| `devices.*.signals.*.bias` | 1 | track/09-joker-micro:48 | §5.8.1, appendix A | covered |
| `devices.*.signals.*.block` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.8.1 | covered (1.1) |
| `devices.*.signals.*.deadband` | 1 | track/09-joker-micro:48 | §5.8.1 | covered |
| `devices.*.signals.*.debounce` | 1 | track/09-joker-micro:48 | §5.8.1 | covered |
| `devices.*.signals.*.direction` | 9 | 08-stress-tests:158, 08-stress-tests:444, 09-architecture-under-stress:308 (+6) | §5.8.1 | covered |
| `devices.*.signals.*.edges` | 2 | 08-stress-tests:158, examples/02-visual-inspection:35 | §5.8.1, appendix A | covered |
| `devices.*.signals.*.kind` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.8.1 | covered (1.1) |
| `devices.*.signals.*.led` | 4 | 23-development-bench:238, track/02-the-pi-as-a-node:165, track/03-temperature-sensor:300 (+1) | §5.8.1, appendix A | covered |
| `devices.*.signals.*.limits` | 1 | 03-manifest:159 | §5.8.1 | covered |
| `devices.*.signals.*.line` | 5 | 08-stress-tests:158, 08-stress-tests:444, 09-architecture-under-stress:308 (+2) | §5.8.1, appendix A | covered |
| `devices.*.signals.*.max_age` | 1 | 03-manifest:159 | §5.8.1 | covered |
| `devices.*.signals.*.record` | 1 | 03-manifest:159 | §5.8.1 | covered |
| `devices.*.signals.*.record.every` | 1 | 03-manifest:159 | §5.8.1 | covered |
| `devices.*.signals.*.rssi` | 1 | track/09-joker-micro:48 | §5.8.1, appendix A | covered |
| `devices.*.signals.*.scale` | 2 | 03-manifest:159, examples/00-first-project:194 | §5.8.1 | covered |
| `devices.*.signals.*.scale.from` | 2 | 03-manifest:159, examples/00-first-project:194 | §5.8.1 | covered |
| `devices.*.signals.*.scale.to` | 2 | 03-manifest:159, examples/00-first-project:194 | §5.8.1 | covered |
| `devices.*.signals.*.stuck_detect` | 1 | 03-manifest:159 | §5.8.1 | covered |
| `devices.*.signals.*.thermal` | 3 | 23-development-bench:238, track/03-temperature-sensor:300, track/06-switching-boards:19 | §5.8.1, appendix A | covered |
| `devices.*.signals.*.timestamp` | 2 | 08-stress-tests:158, examples/02-visual-inspection:35 | §5.8.1, appendix A | covered |
| `devices.*.signals.*.touch` | 1 | track/09-joker-micro:48 | §5.8.1, appendix A | covered |
| `devices.*.signals.*.unit` | 3 | 08-stress-tests:253, examples/03-predictive-maintenance:11, track/09-joker-micro:48 | §5.8.1 | covered |
| `devices.*.signals.*.uptime` | 1 | track/09-joker-micro:48 | §5.8.1, appendix A | covered |
| `devices.*.timeout` | 1 | 03-manifest:113 | §5.8 | covered |
| `devices.*.trigger` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.trigger.input` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.trigger.kind` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | appendix A | covered (1.1) |
| `devices.*.unit_id` | 12 | 03-manifest:113, 08-stress-tests:444, 14-classes-and-instances:168 (+9) | appendix A | covered |
| `devices.*.unit_id.unset` | 2 | 23-development-bench:304, track/09-joker-micro:199 | §7.2 | covered: overlay directive |
| `drivers` | 2 | 08-stress-tests:341, 08-stress-tests:518 | §5.7 | covered |
| `drivers.*` | 2 | 08-stress-tests:341, 08-stress-tests:518 | §5.7 | covered |
| `drivers.*.exec` | 2 | 08-stress-tests:341, 08-stress-tests:518 | §5.7 | covered |
| `drivers.*.tier` | 2 | 08-stress-tests:341, 08-stress-tests:518 | §5.7 | covered |
| `extends` | 12 | 03-manifest:33, 23-development-bench:238, 23-development-bench:255 (+9) | §5.1 | covered |
| `instances` | 7 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:425 (+4) | §5.13 | covered |
| `instances.*` | 7 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:425 (+4) | §5.13 | covered |
| `instances.*.actions` | 1 | 17-jkc:228 | §5.13 | covered |
| `instances.*.actions.*` | 1 | 17-jkc:228 | §5.13 | covered |
| `instances.*.actions.*.role` | 1 | 17-jkc:228 | §5.13 | covered |
| `instances.*.capabilities` | 3 | 03-manifest:352, 14-classes-and-instances:181, 17-jkc:228 | §5.13 | covered |
| `instances.*.class` | 7 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:425 (+4) | §5.13 | covered |
| `instances.*.implemented_by` | 5 | 03-manifest:352, 14-classes-and-instances:425, 14-classes-and-instances:533 (+2) | §5.13 | covered |
| `instances.*.implemented_by.app` | 2 | 14-classes-and-instances:533, 17-jkc:228 | §5.13 | covered |
| `instances.*.implemented_by.bind` | 2 | 14-classes-and-instances:533, 17-jkc:228 | §5.13 | covered |
| `instances.*.implemented_by.bind.*` | 2 | 14-classes-and-instances:533, 17-jkc:228 | §5.13 | covered |
| `instances.*.map` | 5 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:533 (+2) | §5.13 | covered |
| `instances.*.map.*` | 5 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:533 (+2) | §5.13 | covered |
| `link` | 2 | track/09-joker-micro:48, track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `link.address` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `link.auth` | 2 | track/09-joker-micro:48, track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `link.bus` | 1 | track/09-joker-micro:48 | §5.22 | covered (Joker Micro only) |
| `link.peer` | 2 | track/09-joker-micro:48, track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `models` | 7 | 03-manifest:298, 06-ml:220, 08-stress-tests:23 (+4) | §6.1 | covered (1.1) |
| `models.*` | 7 | 03-manifest:298, 06-ml:220, 08-stress-tests:23 (+4) | §6.1 | covered (1.1) |
| `models.*.backend` | 6 | 03-manifest:298, 08-stress-tests:23, 08-stress-tests:158 (+3) | §6.1 | covered (1.1) |
| `models.*.deadline` | 6 | 03-manifest:298, 08-stress-tests:23, 08-stress-tests:158 (+3) | §6.1 | covered (1.1) |
| `models.*.isolation` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.1 | covered (1.1) |
| `models.*.resources` | 6 | 03-manifest:298, 08-stress-tests:23, 08-stress-tests:158 (+3) | §6.1 | covered (1.1) |
| `models.*.shadow` | 2 | 06-ml:220, examples/03-predictive-maintenance:11 | §6.1 | covered (1.1) |
| `models.*.shadow.auto` | 1 | examples/03-predictive-maintenance:11 | §6.1 | covered (1.1) |
| `models.*.shadow.candidate` | 1 | 06-ml:220 | §6.1 | covered (1.1) |
| `models.*.shadow.kpi` | 1 | 06-ml:220 | §6.1 | covered (1.1) |
| `models.*.shadow.on_regress` | 1 | 06-ml:220 | §6.1 | covered (1.1) |
| `models.*.shadow.promote_if` | 2 | 06-ml:220, examples/03-predictive-maintenance:11 | §6.1 | covered (1.1) |
| `models.*.shadow.until` | 2 | 06-ml:220, examples/03-predictive-maintenance:11 | §6.1 | covered (1.1) |
| `models.*.shadow.until.duration` | 2 | 06-ml:220, examples/03-predictive-maintenance:11 | §6.1 | covered (1.1) |
| `models.*.shadow.until.frames` | 1 | 06-ml:220 | §6.1 | covered (1.1) |
| `models.*.source` | 6 | 03-manifest:298, 08-stress-tests:23, 08-stress-tests:158 (+3) | §6.1 | covered (1.1) |
| `models.*.warmup` | 2 | 03-manifest:298, examples/02-visual-inspection:35 | §6.1 | covered (1.1) |
| `net` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `net.wifi` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `net.wifi.country` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `net.wifi.password` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `net.wifi.ssid` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | covered (Joker Micro only) |
| `outputs` | 20 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:158 (+17) | §5.10 | covered |
| `outputs.*` | 20 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:158 (+17) | §5.10 | covered |
| `outputs.*.accept_missing_fallback` | 2 | examples/02-visual-inspection:35, track/11-production-and-fleet:40 | §5.10 | covered |
| `outputs.*.deadband` | 1 | 03-manifest:189 | §5.10 | covered |
| `outputs.*.grace` | 2 | 03-manifest:189, examples/01-bess-peak-shaving:24 | §5.10 | covered |
| `outputs.*.group` | 2 | 03-manifest:189, track/09-joker-micro:48 | §5.10 | covered |
| `outputs.*.interlock` | 7 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:341 (+4) | §5.10 | covered |
| `outputs.*.max_actuations` | 3 | 03-manifest:189, 14-classes-and-instances:425, examples/02-visual-inspection:35 | §5.10 | covered |
| `outputs.*.max_lease` | 5 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:444 (+2) | §5.10 | covered |
| `outputs.*.max_manual` | 2 | 03-manifest:189, track/11-production-and-fleet:40 | §5.10 | covered |
| `outputs.*.min_interval` | 1 | 03-manifest:189 | §5.10 | covered |
| `outputs.*.on_interlock` | 1 | 03-manifest:189 | §5.10 | covered |
| `outputs.*.on_late` | 3 | 03-manifest:189, 08-stress-tests:158, examples/02-visual-inspection:35 | §5.10 | covered (1.1) |
| `outputs.*.on_range` | 1 | 03-manifest:189 | §5.10 | covered |
| `outputs.*.pulse` | 3 | 03-manifest:189, 08-stress-tests:158, examples/02-visual-inspection:35 | §5.10 | covered |
| `outputs.*.range` | 7 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:341 (+4) | §5.10 | covered |
| `outputs.*.rate` | 6 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:341 (+3) | §5.10 | covered |
| `outputs.*.remote_levels` | 1 | track/09-joker-micro:48 | §5.22 | covered (Joker Micro only) |
| `outputs.*.requires_good` | 4 | 03-manifest:189, 08-stress-tests:23, examples/00-first-project:41 (+1) | §5.10 | covered |
| `outputs.*.reset_when` | 1 | 03-manifest:189 | §5.10 | covered |
| `outputs.*.safe` | 17 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:158 (+14) | §5.10 | covered |
| `outputs.*.safe_ramp` | 2 | 03-manifest:189, 08-stress-tests:341 | §5.10 | covered |
| `outputs.*.safer` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | covered |
| `outputs.*.schedule_depth` | 3 | 03-manifest:189, 08-stress-tests:158, examples/02-visual-inspection:35 | §5.10 | covered (1.1) |
| `outputs.*.timed` | 3 | 03-manifest:189, 08-stress-tests:158, examples/02-visual-inspection:35 | §5.10 | covered (1.1) |
| `outputs.*.unset` | 2 | 23-development-bench:304, track/09-joker-micro:246 | §7.2 | covered: overlay directive |
| `outputs.*.verify` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | covered |
| `outputs.*.verify.readback` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | covered |
| `outputs.*.verify.tolerance` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | covered |
| `outputs.*.verify.within` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | covered |
| `pipelines` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | covered (1.1) |
| `pipelines.*` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | covered (1.1) |
| `pipelines.*.deadline` | 2 | 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | covered (1.1) |
| `pipelines.*.from` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | covered (1.1) |
| `pipelines.*.steps` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | covered (1.1) |
| `pipelines.*.steps.[]` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].edges_hz` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].expr` | 1 | examples/03-predictive-maintenance:11 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].field` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].fields` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].hop` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].iou` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].keep_aspect` | 1 | 03-manifest:298 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].max` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].mean` | 1 | 03-manifest:298 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].n` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].name` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].op` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].roi` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].score` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].signals` | 1 | examples/03-predictive-maintenance:11 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].size` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].std` | 1 | 03-manifest:298 | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].window` | 4 | 08-stress-tests:158, 08-stress-tests:253, examples/02-visual-inspection:35 (+1) | §6.3 | covered (1.1) |
| `pipelines.*.steps.[].with` | 2 | 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | covered (1.1) |
| `pipelines.*.to` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | covered (1.1) |
| `project` | 13 | 03-manifest:53, 08-stress-tests:444, 09-architecture-under-stress:308 (+10) | §5.2 | covered |
| `project.joker` | 9 | 03-manifest:53, 23-development-bench:195, examples/01-bess-peak-shaving:24 (+6) | §5.2 | covered |
| `project.mode` | 10 | 03-manifest:53, 09-architecture-under-stress:308, 23-development-bench:195 (+7) | §5.2 | covered |
| `project.name` | 12 | 03-manifest:53, 08-stress-tests:444, 09-architecture-under-stress:308 (+9) | §5.2 | covered |
| `resources` | 7 | 03-manifest:53, 23-development-bench:255, examples/01-bess-peak-shaving:24 (+4) | §5.4 | covered |
| `resources.*` | 7 | 03-manifest:53, 23-development-bench:255, examples/01-bess-peak-shaving:24 (+4) | §5.4 | covered |
| `resources.*.accel` | 1 | examples/02-visual-inspection:35 | §5.4 | covered (1.1) |
| `resources.*.cpus` | 7 | 03-manifest:53, 23-development-bench:255, examples/01-bess-peak-shaving:24 (+4) | §5.4 | covered |
| `resources.*.memory` | 3 | 03-manifest:53, examples/01-bess-peak-shaving:24, examples/03-predictive-maintenance:11 | §5.4 | covered |
| `security` | 5 | 03-manifest:445, track/02-the-pi-as-a-node:165, track/06-switching-boards:31 (+2) | §5.19 | covered |
| `security.api` | 4 | 03-manifest:445, track/02-the-pi-as-a-node:165, track/06-switching-boards:31 (+1) | §5.19 | covered |
| `security.api.listen` | 4 | 03-manifest:445, track/02-the-pi-as-a-node:165, track/06-switching-boards:31 (+1) | §5.19 | covered |
| `security.roles` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.20 | covered |
| `security.roles.*` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.20 | covered |
| `security.trust` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.19 | covered |
| `security.trust.apps` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.19 | covered |
| `security.trust.config` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.19 | covered; only restricts the node's bundle (C9, resolved: D7) |
| `security.trust.secrets` | 1 | track/11-production-and-fleet:112 | §5.19 | covered |
| `signals` | 3 | track/04-esp32-modbus:226, track/05-touch-and-faults:35, track/05-touch-and-faults:63 | §5.9 | covered |
| `signals.*` | 3 | track/04-esp32-modbus:226, track/05-touch-and-faults:35, track/05-touch-and-faults:63 | §5.9 | covered |
| `signals.*.debounce` | 1 | track/04-esp32-modbus:226 | §5.9 | covered |
| `signals.*.stuck_detect` | 2 | track/05-touch-and-faults:35, track/05-touch-and-faults:63 | §5.9 | covered |
| `streams` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.2 | covered (1.1) |
| `streams.*` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.2 | covered (1.1) |
| `streams.*.mode` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.2 | covered (1.1) |
| `streams.*.pool` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.2 | covered (1.1) |
| `streams.*.record` | 1 | examples/02-visual-inspection:35 | §6.2 | covered (1.1) |
| `target` | 13 | 03-manifest:53, 08-stress-tests:23, 23-development-bench:255 (+10) | §5.3 | covered |
| `target.arch` | 1 | 03-manifest:53 | §5.3 | covered |
| `target.board` | 12 | 03-manifest:53, 08-stress-tests:23, 23-development-bench:255 (+9) | §5.3 | covered |
| `target.kernel` | 5 | 03-manifest:53, 08-stress-tests:23, examples/01-bess-peak-shaving:24 (+2) | §5.3 | covered |
| `telemetry` | 7 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+4) | §5.17 | covered |
| `telemetry.record` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | covered |
| `telemetry.record.*` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | covered |
| `telemetry.record.*.blocks` | 3 | 03-manifest:394, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.17.1 | covered (1.1) |
| `telemetry.record.*.every` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | covered |
| `telemetry.record.*.keep` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | covered |
| `telemetry.record.*.keep.*` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | covered |
| `telemetry.record.*.on_change` | 2 | 03-manifest:394, examples/01-bess-peak-shaving:24 | §5.17.1 | covered |
| `telemetry.record.*.post` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.17.1 | covered (1.1) |
| `telemetry.record.*.pre` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.17.1 | covered (1.1) |
| `telemetry.record.*.trigger` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.17.1 | covered (1.1) |
| `telemetry.store` | 1 | 03-manifest:394 | §5.17.2 | covered |
| `telemetry.store.fsync` | 1 | 03-manifest:394 | §5.17.2 | covered |
| `telemetry.store.max_disk` | 1 | 03-manifest:394 | §5.17.2 | covered |
| `telemetry.uplink` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*.broker` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*.budget` | 2 | 03-manifest:394, examples/01-bess-peak-shaving:24 | §5.17.3 | covered |
| `telemetry.uplink.*.classes` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*.classes.*` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*.group_id` | 2 | 03-manifest:394, examples/01-bess-peak-shaving:24 | §5.17.3 | covered |
| `telemetry.uplink.*.history` | 1 | 03-manifest:394 | §5.17.3 | covered |
| `telemetry.uplink.*.history.delivery` | 1 | 03-manifest:394 | §5.17.3 | covered |
| `telemetry.uplink.*.kind` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*.live` | 2 | 03-manifest:394, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*.live.every` | 2 | 03-manifest:394, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*.node_id` | 1 | 03-manifest:394 | §5.17.3 | covered |
| `telemetry.uplink.*.password` | 2 | 03-manifest:394, track/11-production-and-fleet:127 | §5.17.3 | covered |
| `telemetry.uplink.*.username` | 1 | track/11-production-and-fleet:127 | §5.17.3 | covered |
| `training` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.arch` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.budget` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.budget.max_memory` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.budget.max_time` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.checkpoint` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.data` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.data.signals` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.data.where` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.data.window` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.from` | 1 | 06-ml:197 | §6.4 | covered (1.1) |
| `training.*.hyper` | 1 | examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.hyper.epochs` | 1 | examples/03-predictive-maintenance:11 | §6.4 | covered (1.1): free key of `hyper` |
| `training.*.hyper.layers` | 1 | examples/03-predictive-maintenance:11 | §6.4 | covered (1.1): free key of `hyper` |
| `training.*.hyper.lr` | 1 | examples/03-predictive-maintenance:11 | §6.4 | covered (1.1): free key of `hyper` |
| `training.*.output` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.priority` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.resources` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.schedule` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.validate` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.validate.holdout` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.validate.max` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.validate.metric` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |
| `training.*.validate.threshold` | 1 | examples/03-predictive-maintenance:11 | §6.4 | covered (1.1) |

**Keys referred to only in text** (searched for with a second script, outside the code blocks):

| Key | Where | Status |
|---|---|---|
| `accept_missing_fallback` | 01 §8, 03 §14, track 02 (also in blocks: example 02, track 11) | §5.10 (in the manifest) and lock (`ref/02`) |
| `align` | 03 §9.1, 08 R1 | §5.8 |
| `batch`, `max_inflight` | 06 §4 | §6.1 (1.1) |
| `dedup_window` | 02 §8, 16 E9 | §5.8.2 |
| `period_tolerance` | 04 §3.2 | §5.12 |
| `permit_uncertain` | 02 §4, 03 §9.1, 16 §3.2 | §5.10 |
| `trace.async` | 03 §9.1, 09 A10 | §5.11 |
| `record = false` (streams) | 03 §9.1, 08 R2 | §6.2 (1.1) |
| `keep_previous` | 01 §16 | §5.12 |
| `health_window` | 01 §16 | node configuration (`ref/02`): it belongs to the runtime, not to the site |
| `kind = "native"` | 01 §4, 04 §5 | §5.12 |
| `memory.max` | 01 §9, 09 A3 | §5.12 (`cycle_memory`, `async_memory`) |
| `margin` (of `on_late`) | 02 §7.2 | §5.10 (`late_margin`, 1.1) |
| `record_disagreements` | example 02 §5 | §6.1 (1.1) |
| `optional` (tasks, ports) | 01 §4, 03 §8 | declared in the code (`joker:meta`, `ref/04`); `bind.optional` in §5.12.1 |
| `relinquish = { … }`, `coherent`, `quirks`, `implements` | 03 §6.1, 05 §6 | profiles (`ref/03`) |
| `bounce`, `panel`, `gen`, `noise` | 04 §6, track 1, 3, 4, 5 | simulation models (`ref/03`) |
| `halt_if` | 20 D2 | rollout rule of the Deck (`ref/10`), not of the manifest |
| `--skip-shadow` | ADR 0012 | CLI option |


---

## Appendix C — Findings: changes proposed in other documents

| # | Document, section | Problem | Proposed change |
|---|---|---|---|
| C1 | 03 §11 | `classes = { alarms = "**alarm.**", … }`: `**` inside a segment is not a valid selector | `alarms = "alarm.**"` |
| C2 | 08, round 5 (`sites/joker.hb-01234.toml`) | `name = "HB-01234"` violates the site name grammar (lowercase) | `name = "hb-01234"`, and the file `sites/joker.hb-01234.toml` (the variant of an overlay follows the same lowercase rule, §7.7) |
| C3 | 02 §8 | `kind = "modbus.write"` in `[devices.pcs.actions.reset_fault]` is not a manifest key (the implementation belongs to the profile) | remove the line; the note "implemented by the profile" moves to the text |
| C4 | old track, part 4, §4 (`joker.rpi.toml`) | `config = { pga = "4.096 V", rate = "128 SPS" }`: `SPS` is not in the unit catalog | **Superseded by `ref/03` A20:** `rate` is a `choice` of the `[config]` of the `ti/ads1115@1` profile, and `"128 SPS"` is a label, not a quantity (`ref/03` §2.2.10); the track stays as it is, with a comment saying so |
| C5 | 08 R3, example 03 §1 | `spi = "/dev/spidev0.0"` in the device: the manifest expects a bus (the same fix as for I²C, document 18, P7) | `[buses.spi0] kind = "spi"`, `dev = "/dev/spidev0.0"`, and `bus = "spi0"` in the device |
| C6 | 03 §8 | "implicit group `default` with the period requested by the app" contradicts 16 A11 (`default` = 100 ms) | "in an implicit group `default_<period>` with the period requested by the app; the group `default` (100 ms) is the one of A11" |
| C7 | example 01 (template), 08 R1 | `forecast = "model:load_forecast"` and `[models.load_forecast]` are 1.1 (ADR 0019): the example does not pass on a 1.0 runtime | mark the blocks "requires 1.1", or remove the model binding and declare the `forecast` port as `optional` |
| C8 | 03 §7 | "the ones not listed run afterwards, in manifest order" is undefined with overlays | "in lexicographic order of `<app>.<task>`" |
| C9 | 03 §13 | `trust.runtime` and `trust.config` in the manifest: the configuration declares its own root of trust | **Resolved: D7 (ADR 0025).** "`[security.trust]` only restricts the node's trust bundle (`ref/02` §2.5.4): lists of `.pub` per role and `revoked`; it never adds keys" |
| C10 | `ref/00` §9 | "writes coming from outside the node (…, bridges, …) carry `nonce` and `ts`": Modbus and OPC UA do not carry them | **Resolved: D16.** `ref/00` §9 already says that bridges authenticate through their own protocol, with level and lease, and carry no nonce; this reference points there (§5.18) |
| C11 | 03 §12 | the ROS 2 `subscribe` does not say the message type nor which field goes to which signal | add `type` and `fields` (§5.18) |
| C12 | example 03 §1 | `source = "state:training/ae_p1/current.jkm"` points to an internal file of the node (document 15, §2.5) | `source = "training:ae_p1"` |
| C13 | 08 R6 | `fallback.when` with a 30 min window: above the maximum of the control expressions (`J0305`) | define `fallback.when` in the KPI grammar (§6.5), evaluated in `bg`, as here |
| C14 | 09 §12.2 | the minimal manifest does not declare `mode`; with the default `prod`, GPIO without a fallback is an error (`J0205`) | add `mode = "dev"` to the `[project]` of the example |
| C15 | 01 §6, item 5 | "TTL required (by default 8 h…)" mixes the TTL (required, with no default; A10) with its maximum | "TTL required on each write; the maximum is `max_manual` (default 8 h)" |
| C16 | 05 §11 (board profile) | `relay = { …, safe = false }` in a board profile: `safe` only lives in the manifest (ADR 0006) | **Resolved by `ref/03` A6:** `fallback = false`. What the board knows is the state that the hardware imposes on its own, and that is a device fallback (M1, §5.8), not a `safe` suggestion |
| C17 | 16 §10 | "base of the `extends` paths" and "exact definition of only tightening" were undecided | point to this reference, §7.1 and §7.4 |
| C18 | 03 §9.1 | `record = false` in `[streams.*]` is the default, but the table does not say it is 1.1 | mark the row as 1.1 |
| C19 | 03 §6, §8 | `schedule_depth`, `timed`, `on_late` and `writer.kind = "proposal"` appear without a version marker, and are 1.1 (ADR 0019) | mark as 1.1 |
| C20 | old track, part 3, §8 | the text says the LED does not light up again on its own after the fault, and the scenario expects `equals = true` at 3.6 s | fix the text: it lights up again, because the app writes its state again |
| C21 | 08 R2, R4, R6 | `source = "apps/inspector"` (and `apps/diff-base`, `apps/ops-assistant`): `source` is for OCI artifacts | `path = "apps/…"` |
| C22 | 03 §13 | `api = { …, mtls = true }`: with TCP always in mTLS, the key only serves to make mistakes | remove `mtls` (§5.19); `J0291` withdrawn |
| C23 | 03 §8, 01 §17 | `secrets = ["…"]` as a list of names | **D6:** table `[apps.<app>.secrets] <local_name> = "secret:<name>"` (§5.12, `ref/02` §2.4.7) |
| C24 | 03 §4 | "a bus serializes access" with no exceptions | only `serial`, `i2c`, `spi` and `ethernet` with a declared `bus`; `can` and TCP without `bus` do not serialize (§5.6, `ref/06` §2.14) |
