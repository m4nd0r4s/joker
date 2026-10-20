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
| **Phase** | `1.0`, or `1.1` (reserved: the key exists in the 1.0 schema, but using it on a 1.0 runtime is `J0290`) |

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
- **Evaluation group:** the expressions of an output run in its owning group (A11); `calc` and alarms run in the fastest group that consumes them, or in the implicit `calc` group (100 ms), or in the group indicated by `group` (§5.13, §5.14).
- **Dependencies between `calc`** form an acyclic graph; a cycle is `J0190`.
- An expression with a syntax error is `J0158`; unknown signal `J0302`; unknown variant `J0303`; dimensions `J0301`; window above the maximum `J0305`.

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

`jokerd` does not implement NTP or PTP: it reads the state of the system daemon (chrony, systemd-timesyncd, linuxptp) and derives the quality of the clock (document 01, §18). With `sync = "none"`, the quality is always `unsynced`. The time of the Deck only serves as a coarse source, never beyond `holdover` (`ref/02` §3.2.4). On a Joker Micro, synchronization comes from the `TIME` message of Joker Link, or from the firmware with `extern`.

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
| all | `kind` | `"ethernet"` \| `"serial"` \| `"can"` \| `"ethercat"` \| `"i2c"` \| `"spi"` | yes | — | `J0144` | 03 §4 | 1.0 |
| `ethernet` | `iface` | string | yes | — | | 03 §4 | 1.0 |
| `serial` | `port` | `File` (device) on Linux; `Name` on an MCU | yes | — | on Linux, an absolute path; **on an MCU** (§5.22), the name of a `[uart.<name>]` of the board (`ref/03` §2.3.3), such as `port = "uart0"`, and never a path (nonexistent `J0828`; a path `J0145`) | 03 §4 | 1.0 |
| `serial` | `baud` | `Integer` | yes | — | 300–4 000 000 | 03 §4 | 1.0 |
| `serial` | `parity` | `"none"` \| `"even"` \| `"odd"` | no | `"none"` | | 03 §4 | 1.0 |
| `serial` | `stop` | `1` \| `2` | no | `1` | | 03 §4 | 1.0 |
| `serial` | `data_bits` | `7` \| `8` | no | `8` | | new | 1.0 |
| `serial` | `turnaround` | `Duration` | no | 3.5 characters at the declared `baud` (Modbus RTU frame silence) | | 03 §4 | 1.0 |
| `serial` | `dtr`, `rts` | `"keep"` \| `"low"` \| `"high"` | no | `"low"` | state of the control lines **when the port is opened** and while it is open. `"low"` (deasserted) on both is what lets the port be opened without triggering the *auto-reset* of ESP32 boards (the DTR/RTS circuit of the USB bridge pulls `EN` and `GPIO0` to zero, and the ESP32 restarts or enters flashing mode); `"keep"` does not touch the lines (whatever state the bridge controller has); `"high"` asserts it (some RS-485 adapters use RTS for the direction). Only on Linux; on an MCU `J0145`. With `duplex = "half"` and an adapter that does the direction through RTS, the driver controls RTS and `rts` is `J0145` | new (track, part 4) | 1.0 |
| `serial` | `duplex` | `"full"` \| `"half"` | no | `"full"` | `half` on two-wire RS-485: Joker Link uses turn-taking (`ref/07` §15); Modbus RTU ignores it | `ref/07` §15 | 1.0 |
| `can` | `iface` | string | yes | — | | 03 §4 | 1.0 |
| `can` | `bitrate` | `Integer` (bit/s) | yes | — | | 03 §4 | 1.0 |
| `can` | `fd` | `Bool` | no | `false` | | new | 1.0 |
| `can` | `data_bitrate` | `Integer` | cond. | — | only with `fd = true` (`J0145`) | new | 1.0 |
| `ethercat` | `iface` | string | yes | — | | 03 §4 | 1.0 |
| `ethercat` | `cycle` | `Duration` | yes | — | ≥ 100 µs | 03 §4 | 1.0 |
| `ethercat` | `dc` | `Bool` (distributed clocks) | no | `false` | | 08 R4 | 1.0 |
| `i2c` | `dev` | `File` | yes | — | | example 00, old track (part 4) | 1.0 |
| `spi` | `dev` | `File` (`/dev/spidevB.C`) | yes | — | | new (fixes 08 R3) | 1.0 |
| `spi` | `mode` | `0`–`3` | no | `0` | | new | 1.0 |
| `spi` | `speed` | `Quantity<Hz>` | no | `"1 MHz"` | | new | 1.0 |

A TCP device without `bus` uses the system routing and does **not** serialize with the others (each one has its own connection). With a `bus` of `kind = "ethernet"`, it connects through that interface and shares the scheduler (useful when a PLC only accepts one TCP connection, or behind a Modbus TCP/RTU gateway).

### 5.7 `[drivers.<driver>]`

Tier N drivers are compiled into `jokerd`. Tier W and X drivers are referred to with a prefix in `driver` (`"w:dlms-cosem"`, `"x:bacnet-ip"`) and are declared here. In 1.0, tier W is **unstable**: only in-tree drivers pass in `prod`, and a W driver from outside the tree is `J0860` (`ref/06` §4); stable and open to third parties in 1.2.

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
| `tier` | w, x | `"w"` \| `"x"` | yes | — | must match the prefix used in `driver` (`J0170`) | new | 1.0 |
| `source` | w, x | `Ref` `oci://…` or `File` | cond. | — | required in `w`; in `x`, an alternative to `exec` (`J0145`) | new | 1.0 |
| `digest` | w, x | `"sha256:…"` | no | pinned in the lock (`ref/02` §2.1.8) | different from the lock `J0420` | new | 1.0 |
| `exec` | x | `File` (absolute) | cond. | — | | new | 1.0 |
| `args` | x | `List<string>` (ordered) | no | `[]` | | new | 1.0 |
| `launch` | x | `"systemd"` \| `"spawn"` \| `"external"` | no | `"systemd"` where there is systemd, otherwise `"spawn"` | `ref/06` §5.2.1; `spawn` with the `jokerd` user in `prod` `J0865` | `ref/06` | 1.0 |
| `user` | x | `"dynamic"` or a user name | no | `"dynamic"` with `launch = "systemd"` (`DynamicUser`); `"joker-drv-<driver>"` with `launch = "spawn"` | nonexistent user `J0864` | 01 §17, `ref/06` §5.2.4 | 1.0 |
| `groups` | x | `List<string>` (set) | no | `[]` | only device-access groups (`dialout`, `video`, `gpio`, `i2c`, `spi`); `joker`, `root`, `wheel`, `sudo` `J0865` | `ref/06` | 1.0 |
| `memory` | w, x | `Size` | no | W: that of the descriptor (16 MiB); X: `"256 MiB"` | W ≤ `"64 MiB"` (`J0874`) | `ref/06` | 1.0 |
| `cpu` | x | `Percentage` of one CPU | no | `"100 %"` | `"1 %"`–`"400 %"` (`J0144`) | `ref/06` | 1.0 |
| `cpu_per_call` | w | `Duration` | no | `"50ms"` | ≤ `"1s"` (`J0152`) | `ref/06` | 1.0 |
| `plan_fuel` | w | `Integer` | no | 10⁹ | ≤ 10¹⁰ (`J0144`) | `ref/06` | 1.0 |
| `isolation` | x | `"process"` \| `"per_device"` | no | `"process"` | `per_device`: one process per device | `ref/06` | 1.0 |
| `net` | x | `List<string>` (IP or CIDR; set) | no | `[]` | destinations beyond the addresses of the devices (`J0184` if malformed) | `ref/06` | 1.0 |
| `devices` | x | `List<File>` (`/dev` nodes; set) | no | `[]` | extra nodes for `DeviceAllow` | `ref/06` | 1.0 |
| `raw_net` | x | `Bool` | no | `false` | `AF_PACKET` (GigE Vision, external EtherCAT) | `ref/06` | 1.0 |
| `env` | x | `Table<string, string>` | no | `{}` | `JDP_*` keys reserved (`J0141`) | `ref/06` | 1.0 |
| `restart` | w, x | `{ min = Duration, max = Duration }` | no | `{ min = "1s", max = "5m" }` | `min ≤ max` (`J0145`) | `ref/06` §4.5, §5.7 | 1.0 |

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
| `signals` | `Table<signal name, adjustment>` | no | — | §5.8.1 | 03 §5.1 | 1.0 |
| `actions` | `Table<action name, configuration>` | no | — | §5.8.2 | 03 §5.2 | 1.0 |

The keys specific to each driver (`address`, `unit_id`, `chip`, `model`, `params`, `node_id`, `eds`, `config`, `position`…) are in **appendix A**, which is the `config` schema of the descriptor of each catalog driver (`ref/06` §2.3). For a W or X driver, those of the schema it publishes apply. A key that exists in another driver but not in this one is `J0173`; one that exists in none is `J0140`; a value outside the driver's schema is `J0852` (`ref/06` §2.3.1). Two devices with the same address on the same bus are `J0178`; so is a device that uses the same kernel resource (LED, GPIO line, thermal zone) as a fixed IO `board.<name>` referred to by the manifest. The fixed IO of the board only exists when the manifest refers to `board.<name>`, and its `fallback` only counts through that path (`ref/03` §2.3.2).

**Two keys with the same name and different roles.** `config` is a **table** validated by the `[config]` of the **profile** (chip keys, `ref/03` §2.2.10), except in `revpi`, where it is the PiCtory file (appendix A). `model` is the **file** of the simulation model in the `sim` driver; the commercial designation of a piece of equipment is `[profile] model`, in the profile, and never appears in the manifest.

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
| `deadband` | `Quantity` (difference, in the dimension of the signal) | no | `0` | acquisition deadband (document 01, §3.3); does not apply to `bool` or `enum` (`J0176`) | track 05 | 1.0 |
| `debounce` | `Duration` | no | `0` | only `bool` (`J0176`); in a driver that supports it (GPIO chardev), it is done by the kernel, and the acquisition stage does not repeat it. On a `bool` **read by poll** (Modbus, `sysfs`, a GPIO without `edges`), the published value only changes after consecutive reads equal to the new value that cover at least the `debounce` (document 16, §3.4) | track 04 | 1.0 |
| `scale` | `Number` (factor) or `{ from = [Quantity, Quantity], to = [Quantity, Quantity] }` | no | `1` | linear, applied **after** the profile's scale (`ref/03` §3, "Acquisition"); with `from`/`to`, `from` is in the profile's unit, and the unit of the signal becomes that of `to` (the canonical one and the aliases); it is the same grammar as the two-point `scale` of profiles (`ref/03` §2.2.4) | example 00 | 1.0 |
| `offset` | `Number` or `Quantity` | no | `0` | applied after `scale`; incompatible with the `from`/`to` form (`J0145`) | 17 §5.2 | 1.0 |
| `unit` | unit | cond. | that of the profile | on a profile signal, only for another unit of the **same** dimension (`J0107`); required on the numeric signals declared in drivers without a profile | 08 R3 | 1.0 |
| `type` | signal type (`ref/00` §6) | cond. | `bool` in `gpio` | only in drivers without a profile (`J0176`) | new | 1.0 |
| `direction` | `"input"` \| `"output"` \| `"param"` | cond. | `"input"` | only in drivers without a profile; on a profile signal the direction is not changed (`J0176`) | track 02 | 1.0 |
| `poll` | `Duration` or `"on_demand"` | no | that of the device | `on_demand` only in `param` (document 05, §3) | 05 §3 | 1.0 |
| `record` | recording table (§5.17.1) | no | — | shorthand for `[telemetry.record."<canonical path>"]`; the two together for the same signal are `J0195` | 03 §5.1 | 1.0 |
| `kind` | `"scalar"` \| `"block"` | no | `"scalar"` | `block` is M7 | 08 R3 | **1.1** |
| `block` | `Integer` (samples per block) | cond. | — | required with `kind = "block"` | 08 R3 | **1.1** |
| driver address keys (`line`, `bias`, `active_low`, `edges`, `timestamp`, `drive`, `topic`, `node`…) | appendix A | | | only in drivers without a profile; on a profile signal, the address belongs to the profile (`J0176`) | track 02, 09, 08 R2 | 1.0 |

**Adjusting a profile signal or declaring a signal.** On a signal that comes from a profile, the manifest only accepts `alias`, `max_age`, `limits`, `stuck_detect`, `deadband`, `debounce`, `scale`, `offset`, `unit` (same dimension), `poll` and `record`; it is the list of `ref/03` §2.2.4. The other keys (`type`, `direction`, address keys) are only used in the drivers without a profile, and the address keys are the `address` schema of the driver's descriptor (`ref/06` §2.3), with the same names as in `ref/03` §2.2.5.

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
| `permit` | `Expr<bool>` | no | `"true"` | permissive evaluated before each invocation (`Uncertain` counts as false) | 03 §5.2 | 1.0 |
| `role` | `"viewer"` \| `"operator"` \| `"admin"` | no | `"operator"` | minimum role of whoever invokes through the CLI, API, HMI or bridge; `"viewer"` is accepted with the warning `J0298` and counts as `"operator"`, because the API requires at least `operator` for any action (`ref/09`) | 03 §5.2 | 1.0 |
| `timeout` | `Duration` | cond. | that of the profile | without either of the two, `J0142` | 03 §5.2 | 1.0 |
| `dedup_window` | `Duration` | no | `"60s"` | window of the idempotency key (16, E9) | 16 E9 | 1.0 |

An **app** only invokes an action through an `Action` port bound in its `bind`: the binding is the authorization, and `role` does not apply to it.

### 5.9 `[signals."<caminho>"]`

Ajustes independentes do dispositivo que fornece o sinal. Servem para o manifesto comum de um par de overlays sim/hardware (trilha, partes 4–5):

```toml
[signals."painel.botao"]
debounce = "20ms"

[signals."painel.pot"]
deadband = "0.01 V"
```

- A chave é um caminho (canónico, alias ou membro de instância) ou um seletor (`J0155` se não casar com nada).
- Chaves aceites: `debounce`, `deadband`, `max_age`, `limits`, `stuck_detect`, `record`, com as regras do §5.8.1. Outra é `J0140`.
- **Um ajuste por chave e por sinal.** Se `[signals]` e `[devices.*.signals]`, ou duas entradas de `[signals]` (por seletores), definirem a mesma chave para o mesmo sinal com valores **diferentes**, é `J0175`. Com o mesmo valor, é aceite: é o caso de um manifesto comum e de um overlay que declaram o mesmo `debounce`.
- Em sinais derivados (`calc.*`, `app.*`, `alarm.*`), só `record` se aplica (`J0176`).

### 5.10 `[outputs."<caminho>"]`: o envelope

Todas as saídas precisam de uma entrada aqui (invariante 5 do `AGENTS.md`). A chave é o caminho da saída (canónico, alias ou membro `out` de uma instância) ou um seletor; o envelope de um seletor aplica-se a cada **saída** que ele apanha (as entradas apanhadas são ignoradas, e um seletor que não apanha nenhuma saída é `J0155`). **Uma saída tem um só envelope** (16, P2): declará-lo por dois caminhos, ou por um caminho e um seletor que também a apanhe, é `J0215`.

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

**Chaves do envelope**, pela ordem dos passos do documento 01, §7:

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `safe` | `Valor<S>` ou `"relinquish"` | **sim** | — (não há; ADR 0006) | falta `J0201`; tipo `J0231`; tem de ser constante `J0232`; fora da `range` constante `J0203`; `relinquish` só em drivers que o suportam `J0207` | 03 §6 | 1.0 |
| `safer` | `"lower"` \| `"higher"` \| `"equal"` | não | `"equal"` | como comparar o fallback do equipamento com o `safe` (M1, `J0205`); com `relinquish` não se aplica; sem fallback de dispositivo é ignorado (`J0238`, aviso) | 03 §6 | 1.0 |
| `safe_ramp` | `Taxa<S>` | não | — (ir para `safe` é imediato) | `J0202` | 03 §6 | 1.0 |
| `interlock` | `Expr<bool>` | não | — (sempre permitido) | passo 2; `Bad` conta como falso; `Uncertain` também, salvo `permit_uncertain` | 03 §6 | 1.0 |
| `permit_uncertain` | `Bool` | não | `false` | aplica-se a `interlock` e aos limites de `range` com expressões (E6) | 03 §9.1 | 1.0 |
| `on_interlock` | `"safe"` \| `"hold"` | não | `"safe"` | 16 E3; vale também para o `requires_good` (16 E4) | 03 §6 | 1.0 |
| `requires_good` | `Lista<Caminho \| Seletor>` (conjunto) | não | `[]` | passo 3; caminho desconhecido `J0101` | 03 §6 | 1.0 |
| `grace` | `Duração` | não | `"0s"` | 16 E4; aceita zero | 03 §6 | 1.0 |
| `range` | `[Limite, Limite]`, com `Limite` = `Quantidade<S>` ou `Expr<número>` | não | — (sem passo de gama) | dois elementos, mínimo < máximo quando ambos constantes (`J0230`); dimensão `J0107`; expressões E6; num `bool` ou `enum` `J0233` | 03 §6 | 1.0 |
| `on_range` | `"clamp"` \| `"reject"` \| `"trip"` | não | `"clamp"` | passo 4 | 03 §6 | 1.0 |
| `rate` | `Taxa<S>` | não | — | passo 5; face ao último valor aplicado (16 E2); `J0202`; num `bool` ou `enum` `J0233` | 03 §6 | 1.0 |
| `deadband` | `Quantidade` (diferença) | não | — | passo 6; num `bool` ou `enum` `J0233` | 03 §6 | 1.0 |
| `min_interval` | `Duração` | não | `"0s"` | passo 6 | 03 §6 | 1.0 |
| `max_actuations` | `Orçamento` | não | — | passo 6: uma atuação é uma escrita aplicada com valor diferente do anterior; `Suppressed` e keepalive não contam (16 E8); esgotado → `Rejected(Budget)` e alarme `joker.output.<s>.budget`; sintaxe `J0159` | 03 §6 | 1.0 |
| `max_lease` | `Duração` | não | `"15m"` | ≤ `"24h"` (`J0255`); lease de cada escritor ≤ `max_lease` (`J0213`) | 03 §6 | 1.0 |
| `max_manual` | `Duração` | não | `"8h"` | ≤ `"7d"` (`J0255`); limite do TTL das escritas manuais (16 A10); 15 min antes do fim de um TTL, alarme `joker.output.<s>.manual_expiring`; um nível manual reposto no arranque sem relógio `synced`, alarme `joker.output.<s>.manual_restored` (16 A14; ambos `medium`, §5.15) | 03 §6 | 1.0 |
| `verify` | tabela | não | — | abaixo | 03 §6 | 1.0 |
| `reset_when` | `Expr<bool>` | não | — (só `jk reset`) | 16 E7; numa saída sem `on_range = "trip"` nem alarme com `trip` sobre ela é `J0236` (aviso) | 03 §6 | 1.0 |
| `group` | `Nome` de `[cycles]` | não | a regra A11 | grupo inexistente `J0237` | 03 §6 | 1.0 |
| `keepalive` | `Duração` | não | o `fallback.keepalive` do dispositivo | só em saídas de dispositivos com fallback; `J0276` | 01 §8, nova | 1.0 |
| `pulse` | `Duração` | não | — | só `bool` (`J0233`); depois de aplicar um valor ≠ `safe`, a saída volta a `safe` ao fim de `pulse` | 03 §6 | 1.0 |
| `remote_levels` | `Lista<Nível>` (conjunto) ou string `"<n>-<m>"` (`"3-16"`) | não | os níveis que nenhum escritor local do MCU usa | **só num Joker Micro**: níveis do árbitro delegados ao link (16 A12, `ref/07` §15); num nó Linux `J0145`; um nível também usado por um escritor local `J0210` | `ref/07` §15 | 1.0 |
| `accept_missing_fallback` | `Texto` (justificação ≥ 10 caracteres) | não | — | transforma o `J0205` por fallback em falta num aviso, e o `jk lock` copia-a para `[accepted]`; numa saída que tem fallback é `J0277` (aviso) | 01 §8, trilha 02 | 1.0 |
| `widen` | `Texto` ou `Tabela<chave, Texto>` | não | — | overlays (§7.4) | 03 §2 | 1.0 |
| `schedule_depth` | `Inteiro` | não | `0` (escritas agendadas desligadas) | 0–256 (`J0239`) | 02 §7.2 | **1.1** |
| `timed` | `Bool` | não | `false` | driver sem saídas temporizadas `J0239` | 02 §7.2 | **1.1** |
| `on_late` | `"apply_now"` \| `"drop"` \| `{ apply = Valor<S> }` | cond. | — (não há) | obrigatória com `schedule_depth > 0` (`J0206`) | 02 §7.2 | **1.1** |
| `late_margin` | `Duração` | não | um período do grupo dono (`timed = false`) ou a latência declarada do driver (`timed = true`) | o `margin` do documento 02, §7.2 | nova | **1.1** |

**`verify`:**

| Chave | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|
| `verify.readback` | `Caminho` de uma entrada | sim | — | tem de ser entrada, com a mesma dimensão (`J0235`) |
| `verify.tolerance` | `Quantidade` (diferença) | cond. | — (igualdade exata) | obrigatória em saídas `f64`/`f32` (`J0235`) |
| `verify.within` | `Duração` | não | 2 × `poll` do dispositivo do `readback` | ≥ um `poll` desse dispositivo (`J0235`) |

Uma saída que não confirma passa a `Uncertain(NotConfirmed)` e dispara `joker.output.<s>.unconfirmed` (documento 01, §7).

**Regras que envolvem várias chaves:**

- Uma chave que não é caminho de saída (direção ≠ `output`) é `J0234`. As saídas de um dispositivo `joker-link` são governadas pelo envelope do Joker Micro: declará-las aqui também é `J0234` (documento 19, §6).
- Os comandos virtuais de uma instância implementada por app (`p1.run_cmd`, `luz.mode`) são saídas como as outras e precisam de envelope (documento 14, §7.2).
- **Grupo dono** (A11): `group`, se declarado; senão, o grupo da app de maior prioridade que escreve a saída em ciclo; senão, o grupo implícito `default` (100 ms).
- **Omissões do árbitro** (A9, A10): `lease` = 3 × período do grupo dono; `max_lease` = 15 min; `max_manual` = 8 h.

### 5.11 `[cycles.<grupo>]`

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

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `period` | `Duração` | sim | — | Linux: 1 ms–1 h; Joker Micro: 100 µs–1 h (`J0280`); < 10 ms sem `kernel = "preempt_rt"` `J0351`; ≥ 10 ms sem ele `J0352` (aviso); admissão `J0350` | 03 §7 | 1.0 |
| `resources` | `"rt"` | não | `"rt"` quando `[resources.rt]` existe; senão, sem classe (sem RT garantido) | outra classe `J0282` | 03 §7 | 1.0 |
| `overrun` | `"skip"` \| `"catch_up"` \| `"degrade"` \| `"trip"` | não | `"skip"` | documento 01, §4; `degrade` sem tarefas `optional` `J0286` (aviso); `catch_up` com saídas com `rate`, `min_interval` ou `max_actuations` `J0287` (aviso) | 03 §7 | 1.0 |
| `trip_after` | `Inteiro` ≥ 1 | não | `1` com `overrun = "trip"`; senão, sem trip | N overruns **consecutivos** põem as saídas do grupo em trip | 03 §7 | 1.0 |
| `alarm_after` | `Inteiro` ≥ 1 | não | `10` | N overruns consecutivos disparam `joker.group.<g>.overrun` | 01 §4, nova | 1.0 |
| `recover_after` | `Inteiro` ≥ 1 | não | `10` | ticks limpos para `degrade` voltar a ligar as tarefas `optional` | 01 §4, nova | 1.0 |
| `order` | `Lista<"<app>" \| "<app>.<tarefa>">` (ordenada) | não | `[]` | entradas que não correm neste grupo `J0281`; duplicados `J0164` | 03 §7 | 1.0 |
| `trace.continuous` | `Duração` | não | — (sem anel contínuo) | ≤ `"7d"`; o disco estimado entra no `J0380` (`J0285` acima do máximo) | 03 §7 | 1.0 |
| `trace.async` | `Bool` | não | `false` | grava também as leituras das tarefas async (documento 09, ataque 10) | 03 §9.1 | 1.0 |
| `cpu` | `Inteiro` | não | atribuída pelo `jk check` (a CPU `rt` menos ocupada, pela ordem dos períodos) | tem de pertencer a `[resources.rt].cpus` (`J0284`) | nova | 1.0 |
| `priority` | `Inteiro` 1–98 (`SCHED_FIFO`) | não | rate-monotonic automático | uma prioridade que inverta a ordem rate-monotonic entre grupos da mesma CPU é `J0283` | nova | 1.0 |

**Ordem das tarefas no tick.** Primeiro as de `order`, pela ordem da lista (uma entrada `"<app>"` corre as tarefas dessa app neste grupo pela ordem em que a app as declara); depois as restantes, por ordem lexicográfica de `<app>.<tarefa>`. A ordem lexicográfica, e não "a ordem do manifesto", torna o resultado independente da ordem das camadas de overlays (anexo C).

**Grupos implícitos** (nomes reservados, `J0288`):

| Nome | Período | Para quê | Pode declarar-se? |
|---|---|---|---|
| `default` | 100 ms | saídas escritas só por tarefas async, pontes, CLI ou implementações async (A11) | sim, `[cycles.default]`, para mudar `period`, `trace` ou `cpu` |
| `default_<período>` (p. ex. `default_10ms`) | o pedido pela tarefa | tarefas de ciclo de apps sem `[apps.<a>.cycles]`: um grupo por período distinto, sem RT garantido | não |
| `calc` | 100 ms | `calc` e alarmes que nenhum grupo consome | sim, `[cycles.calc]` |

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

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `source` | `Ref` `oci://…` | cond. | — | exatamente uma de `source` e `path`, salvo `kind = "native"` (`J0180`) | 03 §8 | 1.0 |
| `path` | `Ficheiro` (pasta da app) | cond. | — | a pasta existe (`J0160`); o `jk build` produz o componente | 03 §8 | 1.0 |
| `digest` | `"sha256:…"` | não | fixado no lock | diferente do lock ou do artefacto `J0180` | 03 §8 | 1.0 |
| `kind` | `"wasm"` \| `"native"` \| `"extern"` | não | `"wasm"` | `native`: a app tem de estar compilada neste build da runtime (feature `native-apps`; num MCU, o backend nativo do SDK, `ref/08` §2.11.5), sem `source` nem `path` (`J0187`); `extern`: **só num Joker Micro**, um escritor externo alimentado pelo código C ou Rust do integrador, com `writes`/`reads` e sem `source`, `path`, `bind` nem `cycles` (§5.22, `ref/08` §2.17; num nó Linux `J0145`) | 04 §5, 08 R4, `ref/08` | 1.0 |
| `writer` | tabela (abaixo) | cond. | um só escritor por saída: `control:12`, lease de 3 períodos | obrigatória se alguma saída que a app escreve tem mais de um escritor (`J0211`); numa app sem saídas `J0257` (aviso) | 03 §8 | 1.0 |
| `bind` | `Tabela<porta, Ligação>` | cond. | — | §5.12.1; porta obrigatória sem ligação `J0110` | 03 §8 | 1.0 |
| `params` | `Tabela<parâmetro, Valor \| { value, runtime, persist }>` | não | as omissões da app | parâmetro desconhecido `J0121`; fora da gama `J0120`; unidade `J0107`; parâmetro sem omissão e sem valor `J0142`; a forma em tabela (`runtime = true`: alterável sem geração nova; `persist = true`: o valor alterado sobrevive a reinícios) só num Joker Micro (§5.22, `ref/08` §2.6.l; num nó Linux `J0145`) | 03 §8, `ref/08` | 1.0 |
| `cycles` | `Tabela<tarefa, Nome de grupo>` | não | grupos implícitos `default_<período>` (§5.11) | tarefa ou grupo desconhecido `J0182`; período fora da tolerância `J0130` (aviso) | 03 §8 | 1.0 |
| `period_tolerance` | `Percentagem` | não | a da app; sem ela, `"50 %"` | `J0130` | 04 §3.2 | 1.0 |
| `resources.cycle` | `"rt"` | não | a do grupo | tem de coincidir com a de cada grupo onde as tarefas correm (`J0186`) | 03 §8 | 1.0 |
| `resources.async` | classe de recursos | não | `"bg"` | não pode ser `rt` (`J0186`) | 03 §8 | 1.0 |
| `resources.memory` | `Tamanho` | não | — | define `cycle_memory` e `async_memory` ao mesmo tempo | 03 §8 | 1.0 |
| `resources.cycle_memory` | `Tamanho` | não | a sugestão da app no `joker:meta`; sem ela, a do tier: Rust `"16 MiB"`, TypeScript `"64 MiB"`, Python `"128 MiB"` (`ref/05` §19) | `memory.max` da instância de ciclo (documento 01, §9); o manifesto vence a sugestão | nova | 1.0 |
| `resources.async_memory` | `Tamanho` | não | idem: Rust `"64 MiB"`, TypeScript `"64 MiB"`, Python `"128 MiB"` | `memory.max` da instância async | nova | 1.0 |
| `net.allow` | `Lista<"<host>:<porta>">` (conjunto) | não | `[]` (sem rede) | host DNS ou IP, porta obrigatória, sem curingas (`J0184`); só tarefas async | 03 §8 | 1.0 |
| `secrets` | `Tabela<nome local, "secret:<nome>">` **(segredo)** | não | `{}` | liga cada segredo que a app declara no `joker:meta` (`ref/04`) a um segredo do site; segredo obrigatório da app sem ligação, ou nome local que a app não declara, `J0927`; valor que não é `secret:` `J0294`; nome em falta no cofre do nó `J0185` (`error-in-prod`) (`ref/02` §2.4.7, D6) | 01 §17, `ref/02` | 1.0 |
| `requires_clock` | `"any"` \| `"holdover"` \| `"synced"` | não | `"any"` | vale a **mais estrita** entre esta chave e a declarada pela app no `joker:meta` (`ref/04`); sem a qualidade pedida, as saídas da app vão para `safe` (documento 01, §18) | 03 §9.1, `ref/04` | 1.0 |
| `keep_previous` | `Duração` | não | `"10m"` | tempo em que a versão anterior fica quente depois de um hot-swap (documento 01, §16) | 01 §16 | 1.0 |
| `crashloop` | `{ traps, window, max_backoff }` | não | `{ traps = 3, window = "10s", max_backoff = "5m" }` | documento 01, §9 | 01 §9, nova | 1.0 |
| `shadow` | tabela (§5.12.3) | não | — | M16 para apps | 03 §8 | 1.0 |
| `writes` | `Lista<Caminho \| Seletor>` (conjunto) de saídas | cond. | — | só com `kind = "extern"` (obrigatória aí; noutro `kind` `J0145`): as saídas em que o firmware propõe (`jk_propose`), com o `writer` da app | `ref/08` §2.17 | 1.0 |
| `reads` | `Lista<Seletor>` (conjunto) | não | `[]` | só com `kind = "extern"`: os sinais que o firmware lê por `jk_read`, além das próprias saídas | `ref/08` §2.17 | 1.0 |
| `fallback` | tabela (§6.5) | não | — | M17 | 06 §7 | **1.1** |

**`writer`:**

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `writer.level` | `Nível` | cond. | `"control"` | `J0250`; faixa `manual` `J0252` | 03 §8 | 1.0 |
| `writer.priority` | `Inteiro` 1–16 | não | a omissão da faixa (§3.11) | fora da faixa `J0251`; dois escritores no mesmo nível da mesma saída `J0210` | 03 §8 | 1.0 |
| `writer.lease` | `Duração` | não | 3 × período do grupo dono de cada saída | ≤ `max_lease` de cada saída e, numa saída de um MCU, ≤ `max_lease` do link (`J0213`, 16 A12); < período do grupo dono `J0256` | 03 §8 | 1.0 |
| `writer.kind` | `"direct"` \| `"proposal"` | não | `"direct"` | `proposal` (M18): sem `level`, `priority` nem `lease` (`J0254`); escritas diretas `J0212` | 03 §8 | **1.1** (`proposal`) |

Uma app tem **um** escritor (A1): as instâncias de ciclo e async contam como o mesmo, no mesmo nível, em todas as saídas que escreve.

#### 5.12.1 Formas de `bind`

| Forma | Porta | Exemplo | Validação |
|---|---|---|---|
| `"<caminho>"` | escalar (`Input`, `Output`, `Action`, `EventInput`, `EventOutput`) | `grid_power = "meter.p_total"` | tipo e unidade (`J0107`, `J0108`); caminho desconhecido `J0101` |
| `"<instância>"` | classe (`#[device] led: Led`) | `led = "painel_led"` | classe e major (`J0630`); capacidades pedidas (`J0602`) |
| `"<seletor>"` | `InputMany`, `OutputMany`, `Many<T>` | `inverters = "inv*.w_max_lim_pct"`, `vavs = "vav_3*"` | sem correspondências `J0155`; limites de `Many` `J0610` |
| `["a", "b", …]` | `InputMany`, `OutputMany`, `Many<T>` | `pumps = ["p1", "p2", "p3"]` | a ordem da lista é a ordem na app |
| `"class:<classe>"` | `Many<T>` | `valves = "class:valve.motor@1"` | todas as instâncias da classe, por ordem de nome |
| `{ signal, as, optional }` | escalar | `soc = { signal = "bms.soc", as = "fraction" }` | abaixo |
| `{ each, map }` | `Many<ClasseLocal>` (documento 14, §6.5) | `zones = { each = "bms.zone_*", map = { temp = "{}.temp" } }` | abaixo |
| `"model:<m>"`, `"pipe:<p>"` | porta de modelo, porta de eventos de pipeline | `forecast = "model:load_forecast"` | **1.1** (`J0290`) |

Chaves da forma em tabela escalar:

| Chave | Tipo | Obr. | Omissão | Validação | Origem |
|---|---|---|---|---|---|
| `signal` | `Caminho` | sim | — | | 03 §8 |
| `as` | `"fraction"` \| `"percent"` | não | — | `fraction` liga um sinal em `%` a uma porta em `1` (÷100); `percent` o contrário (×100); outras unidades `J0183` | 02 §3, nova (`percent`) |
| `optional` | `Bool` | não | `false` | com `true`, um `signal` que não existe neste site deixa a porta por ligar, o que só é aceite se a porta for `optional` no código (senão `J0110`) | 08 R5 |

Chaves da forma `{ each, map }`:

| Chave | Tipo | Obr. | Validação |
|---|---|---|---|
| `each` | `Seletor` com `*` num segmento | sim | casa com **prefixos** de caminhos (nós da árvore), e não com sinais; cada prefixo é um elemento, por ordem lexicográfica |
| `map` | `Tabela<membro, modelo>` | sim | cada modelo contém `{}`, substituído pelo prefixo (`"{}.temp"` → `bms.zone_01.temp`); membro da classe local em falta `J0601` |

Uma forma que não serve para o tipo da porta (lista numa porta escalar, `each` numa porta que não é `Many` de classe local, `as` numa porta `bool`) é `J0181`. Uma porta ligada em dois sítios (no `bind` da app e no `implemented_by.bind`) é `J0189`.

#### 5.12.2 Parâmetros

Os valores de `params` são `Valor` (§3.6) validados contra a declaração da app em `joker:meta`: quantidade com unidade da mesma dimensão, número em `1`, `bool`, string ou variante de enum. Um parâmetro mudado numa geração nova chega à app por `on-params`, sem reiniciar (trilha, parte 1).

#### 5.12.3 `shadow` (M16, apps)

```toml
[apps.hvac_policy.shadow]
of      = "none"
until   = { duration = "14d" }
promote = { outputs = "ahu3.**" }
```

| Chave | Tipo | Obr. | Omissão | Validação | Origem |
|---|---|---|---|---|---|
| `of` | `"<app>@<versão>"` ou `"none"` | sim | — | a versão ativa, instalada no nó ou fixada no lock (`J0188`); `none` quando não há versão ativa (a sombra compara-se com o que o sistema existente faz) | 03 §8, 08 R6 |
| `until.duration` | `Duração` | cond. | — | pelo menos um critério em `until` (`J0142`) | 03 §8 |
| `until.max_divergence` | string `"<quantidade> <p50\|p95\|p99\|max>"` | não | — | diferença entre as propostas da sombra e as aplicadas; dimensão da saída | 03 §8 |
| `promote.outputs` | `Seletor` ou `Lista<Caminho>` | não | todas as saídas da app | promoção parcial (documento 08, R6b) | 08 R6 |
| `promote.auto` | `Bool` | não | `false` | com `true`, promove ao cumprir `until`; senão, espera por `jk shadow promote` | 06 §10, nova |

A versão declarada em `source` é a candidata, e corre com escritas `Shadowed`; a de `of` continua a escrever.

### 5.13 `[instances.<instância>]`

Uma instância nasce de **uma** de três maneiras: composta (`map`), virtual (`implemented_by`) ou simulada (`sim`). Mais de uma é `J0192`, com uma exceção: uma instância virtual de uma classe com membros de classe liga esses membros num `map` que só tem membros de classe (abaixo). As implícitas vêm dos perfis e não se declaram aqui (documento 14, §5.1).

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

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `class` | `Ref` de classe (`"<pacote>/<nome>@<major>"`, `std/` opcional) | sim | — | classe desconhecida `J0193`; major diferente da pedida pela app `J0630` | 03 §9.2 | 1.0 |
| `capabilities` | `Lista<Nome>` (conjunto) | não | `[]` | capacidade que a classe não tem `J0193`; membros de capacidade sem mapeamento `J0602` | 03 §9.2 | 1.0 |
| `map` | `Tabela<membro, Mapeamento>` | cond. | — | membro obrigatório em falta `J0601`; `const:` num `out` `J0603`; adaptador errado `J0194`; membros de classe `J0650`–`J0654` (abaixo), `J0656` numa virtual; numa instância virtual, só membros de classe (`J0192`) | 03 §9.2, 17 §8 | 1.0 |
| `implemented_by` | `"app:<app>"` ou `{ app, bind, params }` | cond. | — | `J0191`; cadeia > 3 camadas `J0620` (a profundidade da classe é o `JC0501`, à parte) | 03 §9.2, 17 §5.3 | 1.0 |
| `sim` | `Bool` ou `Tabela` (parâmetros do simulador da classe, `ref/03`) | não | `false` | só com o driver `sim` disponível; em `prod` `J0145` | 03 §9.2 | 1.0 |
| `actions` | `Tabela<ação, configuração>` | não | as sugestões da classe (`@timeout`, `@permit_hint`, `@role_hint`) | as chaves do §5.8.2; o manifesto vence (17, §4.2) | 17 §4.2 | 1.0 |

**`implemented_by`:**

| Forma | Efeito |
|---|---|
| `"app:<app>"` | a instância é servida pela app `[apps.<app>]`, com o `bind`, `params` e `cycles` dela. Uma app assim serve **uma** instância |
| `{ app = "<app>", bind = { … }, params = { … } }` | cria **uma instância da app por instância virtual** (documento 17, §5.3). `bind` e `params` fundem-se sobre os de `[apps.<app>]`, chave a chave; `cycles`, `writer` e `resources` são os da app |

A app tem de implementar a classe (`#[implements]`, com a mesma major e o hash de contrato no lock); senão, `J0191`.

**`Mapeamento`** (valores de `map`):

| Forma | Membro | Exemplo |
|---|---|---|
| `"<caminho>"` | qualquer | `run_cmd = "io.do_1"` |
| `"<dispositivo>.<ação>"` | `action` | `reset = "vfd1.reset_fault"` |
| `"const:<valor>"` | `in`, `param` | `rated_flow = "const:30 m3/h"` |
| `{ signal, scale, offset, invert }` | `in`, `out`, `param` | `speed_set = { signal = "speed_ref", scale = 0.01 }` |
| `{ signal, enum_map = { variante = n } }` | enum | `mode = { signal = "op_mode", enum_map = { off = 0, standby = 3 } }` |
| `{ edge = { rise = "<ação>", fall = "<ação>" } }` | `out` `bool` | documento 17, §5.2 |
| `{ on_rise = "<caminho>" }`, `{ on_change = "<caminho>" }`, com `fields = { campo = "<caminho>" }` | `event` | `tripped = { on_rise = "io.di_2" }` |
| `"<instância>"` | membro de classe fixo (`member boot: std/button@1`) | `boot = "botao"` |
| `["<instância>", …]` | membro de classe coleção (`member pumps: std/pump@1[1..8]`) | `pumps = ["p1", "p2", "p3"]` |

Os membros de uma instância são aliases `<instância>.<membro>` (16, P3); as regras de colisão são as de P3 (`J0604`).

**Membros de classe** (documento 17, §8). O valor é o **nome de uma instância** do site (implícita, composta, virtual ou simulada), e não um caminho; numa coleção, uma **lista** de nomes, e a ordem da lista é a ordem da coleção (a de `iter()` e de `index()`). O `jk check` verifica:

| Verificação | Código |
|---|---|
| o nome é o de uma instância (um caminho de sinal aqui dá a sugestão da instância que o tem) | `J0650` |
| a classe e a major da instância são as do membro (`std/button@1`); o minor pode ser outro (17 §9) | `J0651` |
| a instância tem as capacidades que o membro exige com `@requires` | `J0652` |
| o número de elementos cabe em `[min..max]`; um membro fixo não aceita lista, e uma coleção aceita uma lista com um só nome | `J0653` |
| não há ciclos: uma instância não é membro de si mesma, direta ou indiretamente | `J0654` |
| membro obrigatório por ligar; membro de uma capacidade declarada por ligar | `J0601`; `J0602` |

- Os caminhos aninhados (`esp_io.boot.clicked`) são aliases dos membros da instância ligada (16, P6).
- Uma instância pode ser membro de várias composições. Os `out` dela têm um só envelope e um só árbitro, e as escritas que chegam por cada composição são do escritor que chamou o método (16, P6). O aviso `J0655` assinala a partilha entre apps diferentes.
- **Numa instância virtual**, os membros escalares são da implementação, e os de classe ligam-se num `map` só com membros de classe, ao lado do `implemented_by`. Um membro escalar nesse `map` é `J0192`. Os membros de classe de uma virtual são da implementação: quem liga a instância lê-os, mas não escreve neles (16, P6); uma classe cujos métodos chamam métodos dos membros não pode ter instâncias virtuais (`J0656`).
- **Profundidade:** a da classe conta-se no `jkc` (`JC0501`, pela cadeia de membros de classe, 17 §8); as camadas de implementação contam-se aqui (`J0620`, pelas instâncias virtuais em cadeia). São dois limites de 3 que não se somam.

### 5.14 `[calc]`

```toml
[calc]
site_load  = "meter.p_total - pcs.p_ac"
rse_limit  = "if rse.k4 then 0 % else if rse.k3 then 30 % else if rse.k2 then 60 % else 100 %"
grid_limit = "400 kW"
velocidade = { expr = "clamp(avg(painel.pot, 200ms) / 3.3 V, 0, 1)", group = "main" }
```

| Forma | Tipo | Validação | Origem | Fase |
|---|---|---|---|---|
| `<nome> = "<expr>"` | `Expr<escalar>` | cria `calc.<nome>` com o tipo e a unidade inferidos; `J03xx`; ciclo entre `calc` `J0190`; nome com pontos `J0150` | 03 §10 | 1.0 |
| `<nome> = { expr, group }` | `expr`: `Expr`; `group`: `Nome` de `[cycles]` | `group` fixa onde o `calc` é avaliado (grupo inexistente `J0237`) | nova | 1.0 |

Uma constante (`"400 kW"`) é uma expressão válida e dá um sinal `Good` com o valor fixo. Sem `group`, um `calc` corre no grupo mais rápido que o consome, ou no grupo implícito `calc`.

### 5.15 `[alarms.<alarme>]`

```toml
[alarms.grid_overfreq]
when          = "meter.f > 50.2 Hz"
delay_on      = "2s"
delay_off     = "5s"
severity      = "high"
suppress_when = "site.mode == SiteMode.maintenance"
message       = "Sobre-frequência na rede: {meter.f}"
actions       = { trip = ["pcs.p_set"] }
```

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `when` | `Expr<bool>` | sim | — | `J03xx`; `Bad` → estado `Unknown` (16 §3.2) | 02 §11 | 1.0 |
| `severity` | `"low"` \| `"medium"` \| `"high"` \| `"critical"` | sim | — | `J0144` | 02 §11 | 1.0 |
| `delay_on` | `Duração` | não | `"0s"` | | 02 §11 | 1.0 |
| `delay_off` | `Duração` | não | `"0s"` | | 02 §11 | 1.0 |
| `suppress_when` | `Expr<bool>` | não | — | com `true`, o alarme não passa a ativo (continua a avaliar) | 02 §11 | 1.0 |
| `message` | `Texto` com marcadores `{<caminho>}` (`{{` e `}}` escapam) | não | o nome do alarme | caminho desconhecido `J0302` | 02 §11 | 1.0 |
| `actions.trip` | `Lista<Caminho \| Seletor>` de saídas (conjunto) | não | `[]` | trip das saídas ao **entrar** em ativo (depois de `delay_on`); não saída `J0234`; sem correspondências `J0155` | 02 §11 | 1.0 |
| `max_shelve` | `Duração` | não | `"24h"` | TTL máximo de `jk alarm shelve` | nova | 1.0 |
| `group` | `Nome` de `[cycles]` | não | o mais rápido que o consome, ou `calc` | `J0237` | nova | 1.0 |

Cada alarme cria `alarm.<alarme>.state` (`enum`, `ref/00` §6) e `alarm.<alarme>.active` (`bool`), e uma série de eventos. Os alarmes declarados por uma app têm o id `app.<app>.<nome>` e os caminhos `alarm.app.<app>.<nome>.state` e `.active` (`ref/04`); por isso o nome `app` está reservado aos alarmes do manifesto, e um alarme chamado `app`, ou um nome que colida com outro caminho criado, é `J0196`. Um `jk reset` de uma saída em trip por um alarme ainda ativo volta a dar trip no tick seguinte: o reset só "pega" quando a condição passou.

**Alarmes da runtime (`joker.*`).** Não se declaram no manifesto, mas são as chaves desta referência que os ligam ou os ajustam. É a lista para que a `ref/09` §5.8 remete; o documento 01, §19, dá os sinais `joker.*` de métricas.

| Alarme | Quando | Chave que o controla | Onde está definido |
|---|---|---|---|
| `joker.output.<s>.budget` | orçamento de atuações esgotado (`Rejected(Budget)`) | `outputs.*.max_actuations` | §5.10, 16 E8 |
| `joker.output.<s>.unconfirmed` | a saída não confirmou pelo `readback` | `outputs.*.verify` | §5.10, 01 §7 |
| `joker.output.<s>.manual_expiring` | 15 min antes do fim do TTL de uma escrita manual (`medium`) | `outputs.*.max_manual` | 01 §6, `ref/09` §5.6 |
| `joker.output.<s>.manual_restored` | nível manual reposto no arranque sem relógio `synced` (`medium`) | `outputs.*.max_manual` | 16 A14 |
| `joker.group.<g>.overrun` | `alarm_after` overruns consecutivos | `cycles.*.alarm_after` | §5.11, 01 §4 |
| `joker.device.<d>.degraded`, `.offline`, `.drift` | estado do link do dispositivo | `devices.*` | 05 §12, `ref/06` §2.12 |
| `joker.driver.<n>.crashloop` | driver W ou X em crashloop (crítico) | — (limiar fixo; o backoff é `drivers.*.restart`) | `ref/06` §4.5, §5.7 |
| `joker.node.config_invalid`, `joker.node.crl_expired` | `node.toml` inválido; CRL expirada | — (configuração do nó) | `ref/02` §2.2.2, §3.2.1 |
| `joker.agent.*` | estado do agente | — (configuração do nó) | `ref/10` §2.18 |

### 5.16 `[atomic.<grupo>]` (M20)

```toml
[atomic.pcs_cmd]
outputs = ["pcs.mode", "pcs.p_set", "pcs.q_set"]
order   = ["pcs.mode", "pcs.p_set", "pcs.q_set"]
on_rate = "independent"
commit  = { signal = "pcs.apply", value = 1 }
```

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `outputs` | `Lista<Caminho>` (conjunto, ≥ 2) | sim | — | cada uma com envelope; uma saída em dois grupos `J0270`; saídas de dispositivos sem transporte comum (mesmo dispositivo ou mesmo barramento com trama comum, como o EtherCAT) `J0271`; grupos donos diferentes `J0272` | 02 §7.3 | 1.0 |
| `order` | `Lista<Caminho>` (ordenada) | não | a ordem de `outputs` | permutação de `outputs` (`J0273`) | 02 §7.3 | 1.0 |
| `on_rate` | `"independent"` \| `"proportional"` | não | `"independent"` | 02 §7.3 | 03 §10 | 1.0 |
| `commit.signal` | `Caminho` | cond. | — | saída do mesmo dispositivo, fora de `outputs`, sem `[outputs]` próprio nem ligações (`J0274`) | 02 §7.3 | 1.0 |
| `commit.value` | `Valor` | cond. | — | obrigatória com `commit.signal` | 02 §7.3 | 1.0 |

O sinal de `commit` é um **registo de comando do grupo**: só o núcleo o escreve, a seguir às escritas do grupo, e não tem árbitro nem envelope. Os métodos `atomic` das classes geram grupos destes automaticamente sobre os membros da instância (documento 14, §8, A5).

### 5.17 `[telemetry]`

#### 5.17.1 `[telemetry.record."<seletor>"]`: o historian local

```toml
[telemetry.record]
"meter.**"          = { every = "1s", keep = { raw = "7d", "1m" = "400d" } }
"pcs.p_set"         = { on_change = true, keep = { raw = "90d" } }
"vision.detections" = { keep = { raw = "7d" } }
```

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `every` | `Duração` | cond. | — | amostragem periódica do último valor; ≥ `"100ms"` | 03 §11 | 1.0 |
| `on_change` | `Bool` | cond. | `false` | uma amostra por `seq` nova, com a deadband de aquisição do sinal | 03 §11 | 1.0 |
| `keep` | `Tabela<camada, Duração>` | não | `{ raw = "7d" }` | camadas `raw`, `1s`, `1m`, `15m` (documento 01, §12); outra camada `J0197` | 03 §11 | 1.0 |
| `blocks` | `true` \| `"on_trigger"` | cond. | — | sinais em bloco (M7) | 03 §11, 08 R3 | **1.1** |
| `trigger` | `Expr<bool>` | cond. | — | obrigatória com `blocks = "on_trigger"` | 08 R3 | **1.1** |
| `pre` | `Duração` | não | `"0s"` | anel em RAM antes do gatilho; entra no `J0380` | 08 R3 | **1.1** |
| `post` | `Duração` | não | `"0s"` | | 08 R3 | **1.1** |

- Num sinal escalar, exatamente uma de `every` e `on_change` (`J0195`). Num evento, nenhuma: gravam-se todas as ocorrências. Num sinal em bloco, `blocks`.
- **Seletores sobrepostos:** para cada sinal vale a entrada **mais específica**: um caminho exato vence um seletor; entre seletores, o que tem mais segmentos literais, e depois o prefixo literal mais longo. Um empate é `J0195`. Esta é a única secção em que a especificidade resolve sobreposições, porque gravar não é uma decisão de segurança; em `[outputs]` e `[signals]`, a sobreposição é erro.
- O registo de decisões, os trips e as transições de alarmes são sempre gravados, independentemente desta secção. O registo guarda cada mudança de resultado, e o `fsync = "always"` vale para os trips, as escritas manuais, as rejeições e as mudanças de nível; o resto vai em lote (documento 01, §12 e §19).
- Um sinal que nenhuma entrada apanha não é gravado.

#### 5.17.2 `[telemetry.store]`

```toml
[telemetry.store]
max_disk = "8 GiB"
fsync    = "batch"
```

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `max_disk` | `Tamanho` | não | 25% da partição de dados do nó, medido no `jk apply` | a retenção por tamanho vence a retenção por tempo (documento 01, §12) | 03 §11 | 1.0 |
| `fsync` | `"batch"` \| `"always"` \| `"never"` | não | `"batch"` | | 03 §11 | 1.0 |
| `fsync_interval` | `Duração` | não | `"1s"` | só com `batch` | 01 §12, nova | 1.0 |

O caminho do historian no disco é configuração do nó (`ref/02`), e não do site.

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

**Chaves comuns:**

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `kind` | `"mqtt"` \| `"mqtt-sparkplug"` \| `"zenoh"` \| `"otlp"` \| `"http"` | sim | — | `J0144` | 03 §11, 01 §13 | 1.0 |
| `classes` | `Tabela<classe, Seletor \| Lista<Seletor>>` | não | `{ alarms = "alarm.**" }` | classes `alarms`, `events`, `high`, `low`, `bulk`, por esta prioridade (documento 01, §13); outra `J0198`; um sinal fica na classe mais prioritária que o apanha | 03 §11 | 1.0 |
| `budget` | `VolumeDados` | não | — (sem orçamento) | M12; os alarmes nunca são degradados | 03 §11 | 1.0 |
| `live.every` | `Duração` | não | `"5s"` | valores ao vivo, entrega `latest` | 03 §11 | 1.0 |
| `history.delivery` | `"at_least_once"` | não | `"at_least_once"` | store-and-forward a partir do historian; um sinal enviado que não está gravado só vai ao vivo (nota do `jk check`) | 03 §11 | 1.0 |
| `history.enabled` | `Bool` | não | `true` | | nova | 1.0 |
| `username` | string | não | — | | nova | 1.0 |
| `password`, `token` **(segredo)** | `Ref` `secret:` | não | — | em claro `J0294` | nova | 1.0 |

A omissão de `classes` só envia alarmes: nada sai do nó sem ser pedido (documento 12, §10).

**Chaves por `kind`:**

| `kind` | Chave | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|---|
| `mqtt`, `mqtt-sparkplug` | `broker` | URL `mqtt://` ou `mqtts://` com porta | sim | — | `mqtt://` em `prod` `J0296` |
| `mqtt-sparkplug` | `group_id` | string | sim | — | |
| `mqtt-sparkplug` | `node_id` | string | não | o nome do nó (enrolamento) | |
| `mqtt` | `topic` | modelo com `{node}`, `{site}`, `{path}` | não | `"joker/{node}/{path}"` | |
| `mqtt` | `encoding` | `"json"` \| `"cbor"` | não | `"json"` | |
| `mqtt` | `qos` | `0` \| `1` | não | `1` | |
| `zenoh` | `endpoints` | `Lista<string>` (`"tls/host:7447"`) | sim | — | |
| `zenoh` | `key_prefix` | modelo | não | `"joker/{node}"` | |
| `zenoh` | `mode` | `"client"` \| `"peer"` | não | `"client"` | |
| `otlp` | `endpoint` | URL `https://` | sim | — | `http://` em `prod` `J0296` |
| `otlp` | `signals` | `Lista<"metrics" \| "logs" \| "traces">` | não | `["metrics"]` | métricas da própria runtime (documento 01, §19) |
| `otlp` | `every` | `Duração` | não | `"10s"` | |
| `http` | `url` | URL `https://` | sim | — | `J0296` |
| `http` | `events` | `Lista<"alarms" \| "events">` | não | `["alarms"]` | webhook |

`classes`, `live` e `history` não se aplicam a `otlp` (`J0198`). As credenciais TLS de cliente são a identidade do nó (`ref/02`), e não chaves do manifesto.

### 5.18 `[bridges.<ponte>]` (M11)

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

**Chaves comuns:**

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `kind` | `"opcua-server"` \| `"modbus-server"` \| `"zenoh-ros2"` | sim | — | | 03 §12 | 1.0 |
| `expose` | `Lista<Seletor>` ou, em `zenoh-ros2`, `Tabela<Caminho, tópico \| { topic, type, fields }>` | não | `[]` | sem correspondências `J0156` (aviso); caminhos de instâncias expõem a instância inteira (métodos incluídos, no OPC UA) | 03 §12 | 1.0 |
| `writer.level`, `writer.priority`, `writer.lease` | como em `[apps.*.writer]` | cond. | — | obrigatório se `writable` ou algum `to` for uma saída; faixa `safety` `J0253`; `manual` `J0252` | 03 §12 | 1.0 |
| `writer.writable` | `Lista<Caminho \| Seletor>` (conjunto) de saídas | não | `[]` (só leitura) | só saídas com envelope (`J0199`); sem correspondências `J0155` | 03 §12 | 1.0 |

**Chaves por `kind`:**

| `kind` | Chave | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|---|
| `opcua-server` | `listen` | `"<host>:<porta>"` | não | `"127.0.0.1:4840"` | todas as interfaces com `writable` não vazio em `prod` `J0297` (aviso) |
| `opcua-server` | `security.policy` | `"None"` \| `"Basic256Sha256"` \| `"Aes128_Sha256_RsaOaep"` \| `"Aes256_Sha256_RsaPss"` | não | `"Basic256Sha256"` | `None` em `prod` `J0296` |
| `opcua-server` | `security.mode` | `"None"` \| `"Sign"` \| `"SignAndEncrypt"` | não | `"SignAndEncrypt"` | `None` em `prod` `J0296` |
| `opcua-server` | `security.trust` | `Lista<Ficheiro>` (certificados de clientes) | cond. | — | obrigatória com `writable` não vazio |
| `modbus-server` | `listen` | `"<host>:<porta>"` | cond. | — | exatamente um de `listen` (Modbus TCP) e `bus` (RTU escravo) (`J0199`) |
| `modbus-server` | `bus` | `Nome` de barramento `serial` | cond. | — | |
| `modbus-server` | `unit_id` | `Inteiro` 1–247 | não | `1` | |
| `modbus-server` | `profile` | perfil (`ref/03`) | sim | — | o mapa de registos servido, no formato dos perfis; cada sinal do perfil é um caminho exposto ou escrito |
| `zenoh-ros2` | `subscribe` | `Tabela<tópico, { type, fields, to, max_age }>` | não | `{}` | `fields` e `to` com o mesmo comprimento (`J0199`) |
| `zenoh-ros2` | `subscribe.*.to` | `Lista<Caminho>` | sim | — | em `bridge.<ponte>.…` (cria o sinal) ou uma saída (precisa de `writer`) (`J0199`) |
| `zenoh-ros2` | `subscribe.*.type` | string (tipo ROS 2) | sim | — | |
| `zenoh-ros2` | `subscribe.*.fields` | `Lista<string>` (campos da mensagem) | não | o campo `data` (mensagens `std_msgs`) | |
| `zenoh-ros2` | `subscribe.*.max_age` | `Duração` | não | 3 × o período observado | `max_age` dos sinais criados |
| `zenoh-ros2` | `endpoints` | `Lista<string>` | não | as do `rmw_zenoh` local | |

**Uma escrita que chega por uma ponte é uma escrita de um escritor declarado** (documento 01, §14): tem nível, lease e envelope, e fica no registo de decisões como `bridge:<ponte>`. A autenticação é a do protocolo da ponte (sessão OPC UA com certificado; Modbus não tem nenhuma, e por isso uma ponte Modbus com `writable` só deve escutar numa interface de campo). As pontes não levam nonce: autenticam-se pelo protocolo delas e escrevem com nível e lease (`ref/00` §9; D16 do documento 22).

### 5.19 `[security]`: confiança e API local

```toml
[security]
trust = { apps = ["keys/apps.pub"], profiles = ["keys/profiles.pub"], revoked = ["ed25519:3f2a…"] }
api   = { listen = ["tcp:0.0.0.0:7443"] }
```

| Chave | Tipo | Obr. | Omissão | Validação | Origem | Fase |
|---|---|---|---|---|---|---|
| `trust.<papel>` | `Lista<Ficheiro>` de chaves públicas (`.pub`, `ref/02` §2.5.2; conjunto) | não | sem restrição: as chaves do bundle de confiança do nó com esse papel | papéis: `config`, `runtime`, `firmware`, `apps`, `drivers`, `profiles`, `boards`, `classes`, `secrets`, `deck`, `models` (1.1) (`ref/02` §2.5.3); outro `J0144`; só pode **restringir**: cada chave tem de estar no bundle com esse papel (`J0292`) | 03 §13 | 1.0 (`models` 1.1) |
| `trust.revoked` | `Lista<string>` de ids de chave (`"ed25519:<32 hex>"`; conjunto) | não | `[]` | acrescenta revogações às do nó; nunca retira (`ref/02` §2.5.4) | `ref/02` | 1.0 |
| `api.listen` | `Lista<"unix:<caminho>" \| "tcp:<host>:<porta>">` (conjunto) | não | `[]` | **acrescenta** escutas às do `node.toml` (`ref/02` §3.2.2); nunca as retira; `J0157`. TCP é sempre TLS 1.3 com mTLS (`ref/09` §2.1): não há chave para o desligar | 03 §13 | 1.0 |

Não há `api.mtls`: a chave saiu, porque um TCP sem mTLS não existe. Um manifesto que a declare dá `J0140` ("chave desconhecida"), com a ajuda "TCP é sempre mTLS".

**Porque a confiança só restringe.** Uma configuração assinada com uma chave que ela própria declara não prova nada. As chaves por papel vêm do **bundle de confiança** do nó, assinado por uma âncora da organização (`ref/02` §2.5.4; D7 do documento 22, ADR 0025), e o Deck só o transporta (documento 20, princípio 5); o manifesto pode dizer "neste site, só aceito apps assinadas por esta chave", mas nunca acrescentar uma chave que o bundle não tem. O `jk check` dá `J0292` quando conhece o bundle (`--node`, ou um bundle local); o nó verifica sempre, ao aplicar, com o mesmo código (`J0292`, `ref/02` §3.5.1, passo 3). O `J0916` é outra verificação: um artefacto ou uma geração assinados por uma chave sem esse papel. É uma correção ao documento 03, §13 (anexo C).

### 5.20 `[security.roles]`

```toml
[security.roles]
operator = ["cert:CN=ops-*", "local:group=joker-ops"]
admin    = ["cert:CN=admin-*"]
```

| Chave | Tipo | Obr. | Omissão em `prod` | Omissão em `dev` | Validação |
|---|---|---|---|---|---|
| `viewer` | `Lista<Principal>` | não | qualquer principal autenticado (certificado da CA de clientes do nó, ou quem abre o socket Unix) | idem | |
| `operator` | `Lista<Principal>` | não | `["local:group=joker-ops"]` | qualquer principal autenticado | |
| `admin` | `Lista<Principal>` | não | `["local:group=joker-admin"]` | qualquer principal autenticado | |

```
principal := "cert:" atributo "=" padrão
           | "local:user=" nome | "local:group=" nome
           | "deck:" deck_id "/user:" id | "deck:" deck_id "/group:" nome
atributo  := "CN" | "OU" | "O" | "SAN"
padrão    := texto com "*" como curinga
```

- `cert:SAN=<padrão>` casa com um URI, DNS ou email do SAN do certificado; é a forma de pôr um papel no certificado (`cert:SAN=urn:joker:role:operator`, `ref/09` §2.3).
- `deck:<deck_id>/user:<id>` e `deck:<deck_id>/group:<g>` são os principais das sessões remotas pelo túnel do Deck (`ref/10`, agent/1.3); numa runtime sem túnel são aceites e não casam com nada.
- Os papéis são cumulativos: `admin` ⊇ `operator` ⊇ `viewer` (documento 01, §14).
- **Acrescenta ao nó.** Os papéis efetivos são os de `[roles]` do `node.toml` **unidos** aos daqui (`ref/02` §3.2.2): uma geração nunca retira os administradores mínimos do nó. O uid 0 e o utilizador `joker` no socket são sempre `admin` (`ref/09` §2.3).
- Um principal inválido é `J0295`. Um papel concedido a qualquer certificado (`"cert:CN=*"`) é `J0293` (aviso).
- Em `dev`, sem `[security.roles]`, qualquer principal autenticado é `admin`: é o que permite `jk write` a partir do portátil na trilha. Em `prod`, as omissões são as restritivas.

### 5.21 Chaves de extensão

Qualquer tabela aceita chaves `x_*`, com qualquer valor TOML. A runtime ignora-as, o `jk check` não as valida, e o manifesto efetivo mantém-nas (com proveniência) para ferramentas de terceiros, como o Deck (`x_owner`, `x_ticket`). Não entram no hash do manifesto efetivo (§8.3).

### 5.22 Num Joker Micro

O manifesto de um nó Joker Micro é um `joker.toml` como os outros, validado pelo mesmo `jk check` e compilado para o blob de geração (documento 19, §4; `ref/08` §2.13). Aplicam-se as secções `[project]`, `[target]` (com `[target.capacity]`, `[target.extern]` e `[target.micro]`), `[clock]` (`sync = "link" | "extern" | "none"`), `[buses]` (`serial`, `can`, `i2c`, `spi`), `[devices]` (drivers do MCU e `driver = "extern"`), `[signals]`, `[outputs]` (com `remote_levels`), `[cycles]`, `[apps]` (só `path`, apps nativas, ou `kind = "extern"`), `[instances]`, `[calc]`, `[alarms]`, `[atomic]`, `[telemetry.uplink]` (`mqtt`, só em MCUs M2), `[security.trust]`, `[link]` e `[net.wifi]` (só em MCUs M2 com Wi-Fi). As outras (`[resources]`, `[telemetry.record]`, `[telemetry.store]`, `[bridges]`, `[security.api]`, `[security.roles]`, `[drivers]`) são `J0720` num alvo MCU (código do Joker Micro, `ref/08`). A lista de chaves que o `jk build` espera é a da `ref/08` §2.17; a sintaxe é esta.

**`[target]` num MCU:** `board` é uma placa com `os = "none"` (`ref/03` §2.3.3); `arch` e `kernel` não se aplicam (`J0145`).

**A versão do núcleo manda no que o manifesto pode usar.** O `[project] joker` diz a versão mínima do `jk`; num MCU, o que conta é o núcleo compilado no firmware, fixado no lock do nó em `[mcu.<d>] core` (`ref/02` §2.1.10). Uma chave, secção ou regra que esse núcleo não conhece é `J0796` no `jk check`, mesmo que o `jk` a aceite (`ref/08` §3.11).

**O modo do MCU e o do nó.** O `[project] mode` do manifesto do MCU é o modo do seu blob de geração (bit `DEV` do cabeçalho, `ref/08` §2.13.2). **Um nó em `prod` só envia a um MCU blobs de geração `prod`:** um dispositivo `joker-link` cujo `micro` aponta para um manifesto em `dev`, num manifesto do nó em `prod`, é `J0794` no `jk check`, e o nó recusa a geração com o mesmo código no `jk apply` (e o Deck, na validação). Um nó em `dev` aceita os dois. Em `prod`, as verificações `error-in-prod` do MCU são erros como as do nó, e em particular o `J0786` (placa com `reset_safe = false`).

**`accept_unsafe_reset`** (chave de `[target]`, só MCU): `Texto`, justificação com ≥ 10 caracteres (`J0442`). Aceita, com justificação, uma placa sem saídas seguras no reset (`reset_safe = false`, `ref/03` §2.3.1): transforma o `J0786` num aviso, como o `accept_missing_fallback` faz ao `J0205` (§5.10). O `jk lock` copia-a para `[accepted] accept_unsafe_reset."<placa>"` do lock do manifesto do MCU (`ref/02` §2.1.14), com a placa resolvida (`"esp32-devkit@1"`): mudar de placa invalida a aceitação (`J0440`). Numa placa com `reset_safe = true`, ou num alvo Linux, é `J0145`. A justificação diz o que o hardware faz entre o reset e o passo 6 do arranque (`ref/08` §2.12.3) e porque é aceitável naquele site (sem carga, uma resistência de *pull-down* externa, um relé que só fecha com o pino ativo…); não muda o comportamento do firmware.

```toml
# micro/esp/joker.toml (excerto, em prod)
[project]
mode = "prod"

[target]
board               = "esp32-devkit@1"
accept_unsafe_reset = "bancada: nenhuma carga nos GPIO do ESP32; o LED flutuante no reset não tem consequências"
```

| Tabela | Chave | Tipo | Omissão | Validação e significado |
|---|---|---|---|---|
| `[target.capacity]` | os nomes da tabela de capacidades da `ref/08` §2.13.3 (`signals`, `outputs`, `groups`, `instances`, `apps`, `alarms`, `programs`, `trace_bytes`, `kv_bytes`, …) | `Inteiro`, ou `Tamanho` nos nomes `_bytes` | as `[capacities]` da placa (`ref/03` §2.3.3) | capacidades do build; nome desconhecido `J0140`; a geração acima delas `J0710` |
| `[target.extern]` | `expose` | `Lista<Seletor>` (conjunto) | `[]` | sinais que o firmware do integrador pode ler além dos das apps `extern` (`ref/08` §2.8.2) |
| `[target.micro]` | `service_deadline` | `Duração` | `"100ms"` | prazo do serviço (`ref/08` §2.6.q) |
| `[target.micro]` | `freeze_on_trip` | `Bool` | `false` | um trip congela os anéis do traço (`jk_trace_freeze`, `ref/08` §2.6.o) |
| `[target.micro]` | `samples_ring` | `Inteiro` | `256` | entradas do anel de `SAMPLES` do link |
| `[target.micro]` | `event_queue` | `Inteiro` | `16` | profundidade da fila de eventos do firmware que esta geração usa (secção `NODE`); não pode passar da capacidade `event_queue` do build (`[target.capacity]`, ou a da placa; `J0710`) |
| `[target.micro]` | `confirm` | `Duração` | `"5m"` | janela de confirmação por omissão de uma geração recebida por OTA (`ref/08` §2.12.4) |

**`[devices.<d>]` com `driver = "extern"`:** um dispositivo sem driver, alimentado pelo código do integrador (`jk_set_input`, `ref/08`). Não tem `bus`, `profile` nem chaves de driver; os sinais declaram-se em `[devices.<d>.signals]` com `type`, `unit` e `direction` (sinal sem `type`, `direction` ou, se numérico, `unit`: `J0785`, `ref/08`).

**`[apps.<a>]` com `kind = "extern"`:** um escritor externo (§5.12: `writer`, `writes`, `reads`). Os `params` de qualquer app aceitam, num MCU, a forma `{ value, runtime, persist }` (§5.12).

**`[link]`** (só MCU; do lado Linux, o MCU é um dispositivo `joker-link`, anexo A):

| Chave | Tipo | Obr. | Omissão | Validação | Origem |
|---|---|---|---|---|---|
| `bus` | `Nome` de barramento `serial` ou `can` | cond. | — | exatamente um de `bus` e `address` | 19 §6 |
| `addr` | `Inteiro` | não | `0` numa ligação série ponto a ponto, `1` em CAN | endereço **do MCU** no barramento: 1–247 num RS-485 partilhado, 1–253 em CAN (`J0736`, `ref/07`); só com `bus` | `ref/07` §15 |
| `address` | `"udp:<host>:<porta>"` \| `"tcp:<host>:<porta>"` | cond. | — | só em MCUs M2; `address` é sempre um endereço IP, e nunca o endereço no barramento. É o endereço **do nó Linux** (o par): em `tcp:`, o MCU liga-se a ele; em `udp:`, envia-lhe os datagramas e só aceita os que vêm dele. Do lado Linux, o `address` do dispositivo `joker-link` é o contrário, o IP e a porta **do MCU** (anexo A; `ref/07` §15) | 19 §6 |
| `peer` | `NomeDns` do nó Linux, ou `"deck"` | não | aceita o primeiro par autenticado | | nova |
| `auth` | `"required"` \| `"none"` | não | `"required"` | `none` num transporte IP `J0179` | 19 U10 |

A forma das mensagens, a negociação, a chave do link e as omissões das temporizações estão na `ref/07`.

**`[net.wifi]`** (só MCU M2 numa placa com `[net] wifi = true`, `ref/03` §2.3.3): a rede Wi-Fi a que o MCU se liga no arranque. É o que torna possíveis os transportes IP de um MCU: o `[link] address` (`udp:`/`tcp:`), o `[telemetry.uplink]` e o sinal `rssi` do driver `esp32` (anexo A). Num alvo Linux, a rede é do sistema operativo, e `[net]` é `J0145`; numa placa sem Wi-Fi (`mcu_class = "m1"`, ou `[net] wifi = false`), `J0828`.

```toml
# micro/esp/joker.toml (excerto)
[net.wifi]
ssid     = "bancada"
password = "secret:wifi_bancada"
country  = "PT"
hostname = "esp-bancada"
```

| Chave | Tipo | Obr. | Omissão | Validação | Origem |
|---|---|---|---|---|---|
| `ssid` | `Texto`, 1–32 B em UTF-8 | sim | — | vazio ou mais de 32 B `J0240` | nova |
| `password` **(segredo)** | `Ref` `secret:<nome>` | não | — (rede aberta) | em claro `J0294`; o valor tem de ter 8–63 caracteres (WPA2/WPA3-Personal): como o `jk` não o vê, é o MCU que o verifica ao decifrá-lo, e outro comprimento deixa-o sem rede, com o motivo no estado do MCU; sem `password`, a rede é aberta: `J0242` (`error-in-prod`); nome em falta ou não cifrado para o MCU `J0920`, `J0922` (`ref/02` §2.4.6) | nova |
| `country` | `Texto`, código ISO 3166-1 alfa-2 em maiúsculas (`"PT"`) | sim | — | fora da lista ISO `J0240`. Fixa os canais e a potência que o rádio pode usar; sem ele, o rádio ficaria no domínio mais restritivo e podia não ver a rede | nova |
| `hostname` | `NomeDns`, um só rótulo (`[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?`) | não | o `[project] name`, reduzido a esta gramática | fora da gramática `J0240`; é o nome que o MCU pede por DHCP (opção 12) | nova |

- **O segredo chega ao MCU selado para ele.** O `jk build --generation` põe a entrada `password` na secção `SECRETS` do blob, cifrada para a chave X25519 do MCU (`ref/08` §2.13.3, §2.17; `ref/02` §2.4.8). Nem o nó Linux que leva o blob pelo link, nem o Deck, o conseguem ler. Um MCU que ainda não é destinatário (`jk node enroll <mcu> --link <d> --ca <pasta>`, ou o enrolamento de um MCU autónomo) é `J0922`.
- **O primeiro blob chega por um caminho que não é o Wi-Fi:** gravado com o firmware (`jk flash`) ou pelo link série. Depois disso, o MCU liga-se à rede no arranque, antes de abrir um link IP.
- Um transporte IP num MCU sem `[net.wifi]` (`[link] address`, `[telemetry.uplink]`) é `J0241`.

---

## 6. Reservado para a 1.1

As chaves desta secção **já existem no esquema da 1.0** (documento 11, "Preparado para ML"; ADR 0019): o `jk` 1.0 valida a forma delas, e usá-las com uma runtime 1.0 é `J0290` ("reservado para a 1.1; esta runtime é a 1.0"). Assim, um manifesto escrito para a 1.1 é rejeitado com uma mensagem clara, e a 1.1 é uma mudança minor: só acrescenta chaves opcionais às que estão aqui.

A semântica é a do documento 06.

### 6.1 `[models.<modelo>]`

```toml
[models.load_forecast]
source    = "models/load-tcn-v3.jkm"
backend   = ["burn:cuda", "burn:vulkan", "burn:cpu"]
resources = "ml"
deadline  = "2s"
isolation = "thread"
warmup    = 3
```

| Chave | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|
| `source` | `Ficheiro` `.jkm`, `Ref` `oci://…` ou `"training:<trabalho>"` | sim | — | `J0157` |
| `digest` | `"sha256:…"` | não | fixado no lock | |
| `backend` | `Lista<string>` (ordenada): `"burn:cpu"`, `"burn:vulkan"`, `"burn:cuda"`, `"burn:metal"`, `"ort:cpu"`, `"ort:tensorrt"`, `"tract"`, `"hailo"`, `"tensorrt"` | sim | — | cadeia de fallback; nenhum disponível na placa `J0501` |
| `resources` | classe de recursos | não | `"ml"` | `rt` `J0145` |
| `deadline` | `Duração` | sim | — | |
| `isolation` | `"thread"` \| `"process"` | não | `"thread"` | |
| `warmup` | `Inteiro` | não | `1` | |
| `max_inflight` | `Inteiro` | não | 1 em NPU/GPU; nº de CPUs `ml` em CPU | |
| `batch` | `{ max = Inteiro, wait = Duração }` | não | — (sem micro-batching) | |
| `fallback_after` | `Inteiro` | não | `5` | falhas ou deadlines seguidos antes de passar ao backend seguinte |

`[models.<m>.shadow]` (M16, modelos):

| Chave | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|
| `candidate` | `Ficheiro` ou `Ref` | cond. | — | obrigatória sem `auto` |
| `auto` | `Bool` | não | `false` | cada candidato de `[training]` entra em sombra sozinho |
| `until` | `{ duration, frames }` | sim | — | pelo menos um |
| `promote_if` | `Lista<string>` (gramática KPI, 06 §10.1) | sim | — | |
| `kpi` | string (gramática KPI) | não | — | despromoção depois de promovido |
| `on_regress` | `"demote"` \| `"keep"` | não | `"demote"` | |
| `record_disagreements` | `Bool` | não | `false` | guarda os frames em desacordo, com retenção curta |

### 6.2 `[streams."<caminho>"]`

| Chave | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|
| `pool` | `Inteiro` (buffers) | sim | — | a memória entra no `J0380` |
| `mode` | `"latest"` ou `"queue(<n>)"` | não | `"latest"` | |
| `record` | `Bool` | não | `false` | os streams não se gravam por omissão (documento 12, §10) |

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

| Chave | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|
| `from` | `Caminho` de stream, sinal em bloco ou evento, ou `Lista<Caminho>` | sim | — | |
| `steps` | `Lista<Passo>` (ordenada) | sim | — | formatos incompatíveis entre passos `J0510` |
| `to` | `Caminho` | sim | — | cria o caminho (16, P4); único no site (`J0174`) |
| `deadline` | `Duração` | não | — | saída `Bad(Deadline)` quando excedido |

**Passos** (`op` e as chaves de cada um, documento 06, §5):

| `op` | Chaves |
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
| `resample` | `rate` (`Quantidade<Hz>`) |
| `append` | `signals = [Caminho, …]` |
| `model` | `name` (`Nome` de `[models]`) |
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
| `correlate` | `with` (`Caminho`), `window` (`Duração`) |

### 6.4 `[training.<trabalho>]`

| Chave | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|
| `arch` | string (catálogo, `"joker.ae@1"`) | sim | — | |
| `from` | `Ficheiro` `.jkm` | não | — | pesos iniciais |
| `hyper` | `Tabela` (hiperparâmetros da arquitetura) | não | os da arquitetura | |
| `data.signals` | `Lista<Caminho \| Ref pipe:>` | sim | — | |
| `data.window` | `Duração` | sim | — | |
| `data.where` | `Expr<bool>` | não | — | filtro sobre o historian |
| `schedule` | string `"after: <duração>[ of data]; then: <daily\|weekly\|every <duração>>"` | sim | — | |
| `resources` | classe | não | `"ml"` | |
| `priority` | `"idle"` \| `"normal"` | não | `"idle"` | |
| `budget` | `{ max_time = Duração, max_memory = Tamanho }` | não | — | |
| `checkpoint` | `Duração` | não | `"10m"` | |
| `validate` | `{ holdout = Percentagem, metric, max, threshold }` | sim | — | `threshold` é um percentil (`"p99.9"`) guardado no `.jkm` |
| `output` | `"candidate"` | não | `"candidate"` | nunca produção direta (M16) |

### 6.5 Outras chaves reservadas

| Chave | Onde está descrita | O que falta para a 1.1 |
|---|---|---|
| `apps.*.fallback = { controller = "app:<a>", when = "<KPI>" }` | 06 §7 | `when` na gramática KPI, avaliado em `bg`; o resultado passa ao árbitro no tick seguinte (as janelas de 30 min do documento 08, R6, só cabem aí) |
| `apps.*.writer.kind = "proposal"` | §5.12 | M18 |
| `outputs.*.schedule_depth`, `timed`, `on_late`, `late_margin` | §5.10 | M6 |
| `devices.*.signals.*.kind = "block"`, `block` | §5.8.1 | M7 |
| `telemetry.record.*.blocks`, `trigger`, `pre`, `post` | §5.17.1 | M7 |
| `resources.ml.accel` | §5.4 | aceleradores |
| `bind = "model:<m>"`, `"pipe:<p>"` | §5.12.1 | portas de modelo e de pipeline |
| `security.trust.models` | §5.19 | artefactos `joker.model.v1+jkm` |
| drivers `v4l2`, `libcamera`, `spi-adc` | anexo A | 1.1 (documento 21, §3) |

---

## 7. Overlays (`extends`)

### 7.1 Ordem de resolução

```
jk check --manifest sites/lisboa-01/joker.toml
   │
   1. ler o ficheiro de topo
   2. resolver `extends`, recursivamente, em profundidade e da esquerda para a direita
   3. linearizar: pós-ordem, cada ficheiro uma só vez (na primeira posição em que aparece)
   4. fundir as camadas por essa ordem, e o ficheiro de topo por último
   5. aplicar as diretivas (`append`, `unset`) e verificar o "só apertar" camada a camada
   6. retirar `extends`; o resultado é o manifesto efetivo (§8)
```

**Exemplo.** O site `S` estende `[A, B]`; `A` estende `[T]`; `B` estende `[T, P]`.

```
S ─ extends ─▶ A ─ extends ─▶ T
          └──▶ B ─ extends ─▶ T, P
linearização (pós-ordem, sem repetições):  T, A, P, B, S
```

`T` entra uma só vez, antes de `A`. Um `append` de `T` não é aplicado duas vezes, e um diamante não é um erro.

- **Caminhos em `extends`** são relativos ao ficheiro que os declara (é a decisão da "base dos caminhos de `extends`", documento 16, §10). Só ficheiros locais: `oci://` e URLs são `J0160`.
- Um ficheiro que aparece na sua própria cadeia é `J0161`. Mais de 8 níveis ou 32 ficheiros é `J0162`.
- Cada camada é TOML válido por si (`J0146`) e é validada por si quanto a chaves desconhecidas (`J0140`); as chaves obrigatórias e as referências só são validadas no manifesto efetivo.

### 7.2 Regras de fusão

| O que há na camada anterior | O que há na camada nova | Resultado |
|---|---|---|
| tabela | tabela | fusão recursiva, chave a chave |
| escalar | escalar | o novo substitui |
| lista | lista | a nova **substitui** |
| lista | `{ append = [ … ] }` | a anterior mais os novos elementos, no fim (§7.3) |
| qualquer valor | `{ unset = true }` | a chave desaparece |
| nada | `{ append = [ … ] }` | a lista `[ … ]` |
| nada | `{ unset = true }` | nada, com o aviso `J0163` |
| tabela | escalar, ou o contrário | `J0165` |

- **As diretivas reconhecem-se pelo esquema.** `{ append = … }` só é diretiva numa chave cujo tipo é lista; `{ unset = true }` é diretiva em qualquer chave, porque nenhuma tabela do esquema tem uma chave `unset`. `append` em chave que não é lista, uma tabela com `append` e outras chaves, ou `unset` com um valor que não seja `true`, são `J0164`.
- **Retirar uma secção inteira** faz-se com uma chave pontuada na raiz do overlay, antes dos cabeçalhos de tabela:

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

### 7.3 `append` em listas e conjuntos

- Num **conjunto** (`requires_good`, `alias`, `capabilities`, `net.allow`, `actions.trip`…), os elementos já presentes não se repetem.
- Numa **lista ordenada** (`backend`, `order`, `steps`, `extends`), um elemento repetido é `J0164`: numa cadeia de fallback ou numa ordem de tarefas, um duplicado é quase sempre um erro.

```toml
[outputs."pcs.p_set"]
requires_good = { append = ["bms.soh"] }
```

### 7.4 Os envelopes só apertam

Um overlay pode tornar o envelope de uma saída **mais restritivo**. Para o tornar menos restritivo, precisa de `widen` com uma justificação, que o `jk lock` copia para `[accepted] widen."<saída>.<chave>"` (documento 03, §14). Sem `widen`, é `J0220`.

**Comparação.** Cada camada compara-se com o valor **acumulado** das camadas anteriores, já com as unidades convertidas (`"125000 W"` é igual a `"125 kW"`). A saída identifica-se pela chave literal de `[outputs]` (§7.6).

| Chave | Aperta quando | Alarga (precisa de `widen`) quando |
|---|---|---|
| `range` | o novo intervalo está contido no anterior (limites constantes); ou passa a existir | contém valores fora do anterior; é retirada; um dos limites é uma expressão e mudou |
| `rate`, `safe_ramp` | o novo valor é menor; ou passa a existir | é maior; é retirada |
| `deadband` | — | — (não é limite: é livre) |
| `min_interval` | é maior; ou passa a existir | é menor; é retirada |
| `max_actuations` | menos atuações na mesma janela (comparadas por hora); ou passa a existir | mais; é retirada |
| `interlock` | a nova expressão é `(<anterior>) && <termo>`, textualmente; ou passa a existir | qualquer outra mudança; é retirada |
| `permit_uncertain` | `true` → `false` | `false` → `true` |
| `requires_good` | o novo conjunto contém o anterior | falta algum elemento do anterior |
| `grace` | é menor | é maior |
| `max_lease`, `max_manual` | é menor | é maior; é retirada (volta à omissão maior) |
| `on_range` | `clamp` → `reject` → `trip` (neste sentido) | no sentido contrário |
| `on_interlock` | `hold` → `safe` | `safe` → `hold` |
| `verify` | passa a existir; `tolerance` ou `within` menores | é retirada; `tolerance` ou `within` maiores |
| `reset_when` | é retirada (passa a exigir `jk reset`) | passa a existir ou muda |
| `safe`, `safer` | — | **qualquer mudança**: o lado seguro é uma decisão, e não uma ordem |
| `accept_missing_fallback` | — | — (**nunca é um `widen`**: a própria chave é a justificação, e exige a entrada `[accepted] accept_missing_fallback."<saída>"` no lock, venha do manifesto de base ou de um overlay; `ref/02` §2.1.14. Um `widen` que a nomeie é `J0167`) |
| `keepalive`, `group`, `pulse`, `remote_levels`, `schedule_depth`, `timed`, `on_late`, `late_margin` | — | — (livres; validados pelas outras regras) |

- A **primeira** camada que declara uma saída não é comparada com nada: é a origem do envelope.
- **Um envelope que passa para o MCU não alarga nada.** Quando uma camada troca o dispositivo de uma saída por um `joker-link` (a parte 9 da trilha: o LED passa a ser governado pelo Joker Micro), o envelope dessa saída deixa de poder estar no manifesto Linux (`J0234`) e passa para o `[outputs]` do manifesto do MCU. Retirá-lo do manifesto Linux **não** é um alargamento e não pede `widen`: o envelope continua a existir, só que no nó que governa a saída, e é o `jk check` do MCU que o valida (e que compara as suas camadas entre si, se as tiver). O `jk check` do nó Linux confirma que a saída é mesmo de um `joker-link` (com a chave `micro`, anexo A, também que o manifesto do MCU declara o envelope; sem ela, só com `--node`); se não for, a remoção é a da última coluna da tabela e exige `widen`.
- Retirar uma chave com `unset` conta como a mudança da última coluna. Não é possível alargar em duas camadas (retirar numa, voltar a pôr noutra) sem `widen`: a remoção já o exige.
- `widen` escreve-se na tabela da saída, na camada que alarga:

```toml
[outputs."pcs.p_set"]
range = ["-150 kW", "150 kW"]
widen = { range = "PCS novo com 150 kW nominal; aprovado por DL em 2026-09-18" }
```

A forma curta `widen = "<justificação>"` cobre todas as chaves que essa camada alarga nessa saída. A justificação tem pelo menos 10 caracteres (`J0166`). Um `widen` numa camada que não alarga nada é `J0167` (aviso). Em `prod`, um alargamento sem a entrada correspondente no lock (ou com texto diferente) é `J0168`. O `widen` não passa para o manifesto efetivo: fica no lock.

### 7.5 Caminhos de ficheiros

Os valores do tipo `Ficheiro` (`model`, `path`, `eds`, `config`, `trust.*`, `profile` de pontes, `exec`) são relativos ao **ficheiro que os declara**, e não ao ficheiro de topo. No manifesto efetivo, ficam reescritos relativamente à pasta do ficheiro de topo (a raiz do projeto). Um ficheiro que não existe é `J0160`.

### 7.6 Chaves literais e seletores

A fusão faz-se pela **chave literal**: `[outputs."inv*.w_max_lim_pct"]` num template e `[outputs."inv2.w_max_lim_pct"]` num site são duas entradas diferentes, e depois da expansão dos seletores a saída `inv2.w_max_lim_pct` tem dois envelopes (`J0215`). Para ajustar só o `inv2`, o site retira a entrada do template e declara as que quer (estresse, caso 3).

### 7.7 Nomes das camadas e escolha do manifesto de entrada

**Nomes** (ADR 0026). Uma camada de manifesto chama-se `joker.toml` ou `joker.<variante>.toml`, em **qualquer** pasta: `joker.sim.toml` e `joker.lab.toml` na raiz, `templates/joker.bess-v2.toml`, `sites/lisboa-01/joker.verao.toml`, `micro/esp/joker.toml`. `<variante>` segue a gramática dos nomes de placa (`[a-z0-9]+ ("-" [a-z0-9]+)*`, ≤ 32 caracteres; ref/00 §2); fora dela, `J0150`. O conteúdo de uma camada só tem secções do manifesto (§4) e nenhuma tabela de identificação de outro tipo (`[profile]`, `[board]`, `[scenario]`, `[suite]`, …); um `joker.*.toml` que seja outra coisa é `J0803` (ref/03 §2.1.6).

- **Em `extends`**, só se aceitam ficheiros com um destes nomes; outro nome é `J0803` (a referência pede uma camada de manifesto). É o que deixa ler a cadeia de overlays numa listagem (`ls joker.*`).
- **Como manifesto de entrada**, um ficheiro com outro nome só se aceita por `--manifest` (ou `JK_MANIFEST`), com o aviso `J0169`; o `jk migrate` propõe o nome novo.

**Escolha do manifesto de entrada** (a mesma em todos os comandos da `jk`, documento 07, princípio 5):

| Forma | Manifesto de entrada | Lock (ref/02 §2.1.1) |
|---|---|---|
| nenhuma | `joker.toml` (ou `defaults.manifest` / `defaults.env` do `.joker/config.toml`, ref/02 §2.3.2) | `joker.lock` |
| `--env <variante>` ou `JK_ENV=<variante>` | `joker.<variante>.toml` na pasta atual | `joker.<variante>.lock` |
| `--manifest <ficheiro>` ou `JK_MANIFEST=<ficheiro>` | o ficheiro dado | o mesmo nome com `.lock` no lugar de `.toml`, na mesma pasta |

- `--env lab` é exatamente `--manifest joker.lab.toml`. Para um overlay noutra pasta, `--manifest sites/lisboa-01/joker.verao.toml` (ou `cd sites/lisboa-01` e `--env verao`).
- **Precedência:** opção > variável `JK_*` > `.joker/config.toml` > omissão (a de ref/02 §2.3.3). `--env` e `--manifest` são a mesma escolha: uma opção de qualquer das duas ganha a qualquer das variáveis, e uma variável de qualquer das duas ganha à configuração do projeto. As duas opções juntas, ou as duas variáveis juntas, são um erro de utilização (código de saída 1, documento 07, §8), e não uma regra de precedência que alguém tenha de adivinhar.
- `--env <variante>` para um ficheiro que não existe é `J0160`, com a lista dos `joker.*.toml` da pasta.
- `--lock <ficheiro>` (ou `JK_LOCK`) substitui só o lock, com qualquer das formas.

---

## 8. O manifesto efetivo

### 8.1 Construção

Depois da fusão (§7), o `jk check` constrói o manifesto efetivo por esta ordem. Cada passo só corre se os anteriores não tiverem erros, e dentro de cada passo os diagnósticos são todos recolhidos (não pára no primeiro).

| # | Passo | Códigos |
|---|---|---|
| 1 | ler e fundir as camadas | `J0140`, `J0141`, `J0146`, `J016x` |
| 2 | tipos, enums, chaves obrigatórias, combinações | `J0142`–`J0145`, `J0147`–`J0149` |
| 3 | gramática dos valores (durações, quantidades, caminhos, seletores, referências, sintaxe das expressões) | `J015x` |
| 4 | carregar o lock, os perfis, as classes e a placa; construir o registo de caminhos (canónicos, aliases, instâncias, `calc`, alarmes, pontes) | `J0101`, `J017x`, `J0193`, `J0196`, `J04xx`, `J06xx` |
| 5 | expandir os seletores, materializar as omissões (A9, A11, escritores, leases, grupos implícitos) | `J0155`, `J0156` |
| 6 | ligações, parâmetros, instâncias | `J0107`–`J0130`, `J018x`, `J019x`, `J06xx` |
| 7 | envelopes, árbitro, fallback, grupos atómicos | `J02xx` |
| 8 | expressões (tipos, unidades, janelas) | `J03xx` (0301–0306) |
| 9 | ciclos, admissão RT, barramentos, memória | `J028x`, `J0350`–`J0380` |
| 10 | regras de `prod` (assinaturas, lock, segredos, TLS) | `error-in-prod` |

Os diagnósticos saem ordenados por ficheiro, linha e código, para a mesma entrada dar sempre a mesma saída (invariante 9 do `AGENTS.md`).

### 8.2 Forma

O manifesto efetivo é um documento TOML com:

- as secções pela ordem da tabela do §4, e as chaves de cada tabela pela ordem das tabelas desta referência; as chaves livres (nomes, caminhos) por ordem lexicográfica;
- **todas as omissões materializadas**, cada uma marcada como tal no `jk diff` (`# omissão`);
- os seletores de `[outputs]` e `[signals]` mantidos como estão (a expansão aparece no `jk explain`);
- os caminhos de ficheiros relativos à raiz do projeto;
- sem `extends`, sem `widen`, sem `accept_missing_fallback` (que vivem no lock), com as chaves `x_*`.

`jk diff` mostra-o; `jk explain <chave>` mostra a origem de cada valor, camada a camada:

```
$ jk explain 'outputs."pcs.p_set".range'
outputs."pcs.p_set".range = ["-150 kW", "150 kW"]
  templates/joker.bess-v2.toml:31   (não declarada)
  sites/lisboa-01/joker.toml:74   ["-125 kW", "125 kW"]     origem
  sites/lisboa-01/joker.verao.toml:9    ["-150 kW", "150 kW"]     alarga · widen: "PCS novo com 150 kW nominal; …" (joker.lock:88)
```

### 8.3 Hash do manifesto efetivo

Esta referência define o hash do **manifesto efetivo**; o hash da **geração** (o que o `jk`, o `jokerd`, o Deck e o `HELLO` de um MCU mostram, `ref/00` §5) é o do descritor `joker.config.v1+cbor`, definido na `ref/10` §2.6.2, e cobre este hash, o digest canónico do lock (`ref/02` §2.1.16), os contratos das classes e os artefactos por digest.

O hash do manifesto efetivo é o SHA-256 do CBOR determinístico (RFC 8949, §4.2.1) do manifesto efetivo:

1. **com as omissões materializadas** e **sem as chaves `x_*`**;
2. com os valores normalizados (durações em ns, quantidades na unidade canónica, seletores e caminhos como estão, tabelas livres por ordem lexicográfica);
3. com cada `Ficheiro` que muda o comportamento (modelos de simulação, EDS, configuração do PiCtory, perfis locais, `.pub` de `[security.trust]`) substituído pelo SHA-256 do seu conteúdo.

Por isso, reordenar chaves, mudar comentários ou partir um ficheiro em dois overlays **não muda** o hash, e mudar uma omissão por passar a declará-la explicitamente também não. Os valores dos segredos nunca entram: só os nomes (`secret:<nome>`). O Deck compara gerações pelo hash da geração (`ref/10`).

### 8.4 Na API

A API local expõe o manifesto efetivo da geração ativa e de cada geração guardada, em JSON com as mesmas chaves, e a proveniência num objeto à parte indexado por JSON Pointer (`ref/09`). As chaves que começam por `_` são reservadas precisamente para não colidirem com metadados deste tipo.

---

## 9. Versões e compatibilidade

| Regra | Detalhe |
|---|---|
| `[project] joker = "X.Y"` | versão mínima da runtime e do esquema. Uma runtime mais antiga recusa o manifesto (`J0147`) antes de validar o resto |
| cada chave tem uma versão de entrada (`since`) | no esquema gerado dos tipos Rust. Usar uma chave com `since` > `joker` é `J0148`, com a sugestão de subir o `joker`. É o que impede um manifesto de parecer compatível com uma runtime que o ignoraria |
| só se acrescenta numa minor | chaves opcionais novas, variantes novas no fim de um enum, secções novas. A omissão de uma chave nova reproduz o comportamento anterior (regra de ouro, ADR 0022) |
| **as omissões não mudam dentro de uma major** | mudar uma omissão é mudar o comportamento de todos os manifestos que não a declaram |
| antes da 1.0 | a runtime aceita o minor atual e o anterior do esquema (documento 09, ataque 8); `jk migrate` reescreve entre minors |
| na série 1 | uma runtime 1.x aceita qualquer manifesto 1.y com y ≤ x; `jk migrate` é opcional |
| na 2.0 | `jk migrate` é obrigatório; a 2.0 lê a última 1.x (documento 21, §1.1) |
| reservadas | as chaves 1.1 (§6) existem no esquema 1.0 com `since = "1.1"`: `J0290` numa runtime 1.0, e `J0148` com `joker = "1.0"` numa runtime 1.1 |
| códigos | um código nunca se reutiliza depois de removido (`ref/00` §1) |

---

## 10. Registo de códigos do manifesto

Todos vivem em `crates/joker-manifest/codes.toml` (documento 16, §9). Gravidades: `error`; `warning` (em todos os modos); `error-in-prod` (erro em `prod`; aviso `W` com o mesmo número em `dev`).

```toml
# excerto do formato do registo (spec/codes.toml; documento 16, §9)
[[code]]
id       = "J0140"
severity = "error"
message  = "chave desconhecida"
explain  = "O manifesto é estrito: uma chave desconhecida é quase sempre um erro de escrita. Chaves de extensão começam por x_."
doc      = "ref/01 §2.3"

[[code]]
id       = "J0205"
severity = "error-in-prod"
message  = "fallback do dispositivo em falta ou incoerente com o safe"
explain  = "O fallback do equipamento tem de ser tão seguro ou mais do que o safe, no sentido de safer (M1)."
doc      = "01 §8"
```

### 10.1 Códigos existentes (recolhidos dos documentos)

| Código | Gravidade | Mensagem | Onde é usado aqui | Documento |
|---|---|---|---|---|
| `J0101` | error | sinal desconhecido (com sugestão) | caminhos, ações e sinais de perfil | 03 §15 |
| `J0107` | error | unidade incompatível (dimensão) | quantidades, ligações, `unit` | 03 §15, 02 §3 |
| `J0108` | error | grandeza incompatível (W vs var) | quantidades, ligações | 03 §15 |
| `J0109` | error | unidade fora do catálogo | quantidades | 16 §5 |
| `J0110` | error | porta obrigatória sem ligação | `bind` | 03 §15 |
| `J0120` | error | parâmetro fora da gama declarada pela app | `params` | 03 §15 |
| `J0121` | error | parâmetro desconhecido | `params` | 03 §15 |
| `J0130` | warning | período do grupo fora da `period_tolerance` da tarefa | `cycles` | 04 §3.2 |
| `J0140` | error | chave desconhecida | todo o ficheiro | `ref/00` §1 |
| `J0201` | error | saída sem `safe` | `outputs` | 03 §15 |
| `J0202` | error | `rate` (ou `safe_ramp`) com unidade incoerente com a saída | `outputs` | 03 §15 |
| `J0203` | error | `safe` fora da `range` | `outputs` | 16 E5 |
| `J0205` | error-in-prod | fallback do dispositivo em falta ou incoerente com `safe` | `outputs`, `devices.*.fallback` | 03 §15, 01 §8 |
| `J0206` | error | escritas agendadas sem `on_late` | `outputs` (1.1) | 02 §7.2 |
| `J0207` | error | `safe = "relinquish"` num driver sem suporte | `outputs` | 03 §6.1 |
| `J0210` | error | dois escritores no mesmo nível da mesma saída | `writer` | 16 A5 |
| `J0211` | error | saída com vários escritores e um sem `writer` | `writer` | 03 §8 |
| `J0212` | error | escritor só-proposta com escrita direta | `writer.kind` (1.1) | 03 §8 |
| `J0213` | error | lease acima de `max_lease` (numa saída de um MCU, o `max_lease` do link) | `writer.lease` | 16 A9, A12 |
| `J0215` | error | dois envelopes para o mesmo sinal | `outputs` | 16 P2 |
| `J0220` | error | overlay alarga o envelope sem `widen` | overlays | 03 §15 |
| `J0301` | error | expressão com dimensões incompatíveis | expressões | 03 §15 |
| `J0302` | error | sinal desconhecido numa expressão | expressões, `message` | 02 §10 |
| `J0303` | error | enum sem essa variante | expressões, `Valor` | 02 §10 |
| `J0305` | error | janela acima do máximo (15 min) | expressões | 16 §4 |
| `J0306` | error | `own()` fora de um método de classe | expressões | 16 §4 |
| `J0350` | error | grupo de ciclo não escalonável (M23) | `cycles` | 03 §15 |
| `J0351` | error | grupo < 10 ms sem `kernel = "preempt_rt"` | `cycles`, `target` | 03 §15 |
| `J0352` | warning | grupo ≥ 10 ms sem PREEMPT_RT: jitter não garantido | `cycles`, `target` | 16 §9 |
| `J0360` | error | classe de recursos sem CPUs, ou com CPUs sobrepostas a `rt` (exceto numa placa com `cpus = 1`, onde é uma nota, §5.4) | `resources` | 03 §15 |
| `J0370` | error | barramento sobrecarregado | `buses`, `devices` | 05 §5 |
| `J0380` | error | memória estimada acima de 70% da RAM da placa | todo o manifesto | 03 §15 |
| `J0381` | warning | carga de escrita projetada acima do que o `storage` da placa aguenta (também no `jk doctor`, medida) | `[telemetry]`, `[apps.*] kv`, `trace` | `ref/03` §2.3.1, 01 §12 |

Referidos, de outras gamas: `J0401` (deriva face ao lock), `J0501`, `J0502`, `J0510` (ML, 1.1), `J0601`–`J0604`, `J0610`, `J0620`, `J0630`, `J0650`–`J0656` (classes e instâncias), `J0710`, `J0720`, `J0721` (Joker Micro).

### 10.2 Códigos novos (gamas `J0140`–`J0199` e `J0230`–`J0299`)

**Estrutura (`J014x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0140` | error | chave desconhecida (já fixado no `ref/00`) |
| `J0141` | error | chave ou nome reservado (`_…`, `joker…`, prefixos `calc.`, `alarm.`, `app.`, `bridge.`, `joker.`, `@`) |
| `J0142` | error | chave obrigatória em falta no manifesto efetivo |
| `J0143` | error | tipo de valor errado |
| `J0144` | error | valor fora do conjunto permitido |
| `J0145` | error | combinação de chaves inválida (exclusivas, dependentes, ou que não se aplicam neste alvo) |
| `J0146` | error | ficheiro não é TOML 1.0 válido ou não é UTF-8 |
| `J0147` | error | `[project] joker` inválido, ou exige uma runtime mais nova do que esta |
| `J0148` | error | chave introduzida numa versão posterior à declarada em `joker` |
| `J0149` | error | limite do manifesto excedido (§11.3) |

**Gramática dos valores (`J015x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0150` | error | nome inválido (gramática, comprimento, pontos numa chave de nome) |
| `J0151` | error | número sem unidade onde se exige uma quantidade ou duração |
| `J0152` | error | duração inválida (sintaxe, zero, negativa, acima do máximo da chave) |
| `J0153` | error | caminho inválido |
| `J0154` | error | seletor inválido |
| `J0155` | error | seletor sem correspondências (saídas, ligações, envelopes, atómicos, trips, pontes) |
| `J0156` | warning | seletor de telemetria, exposição ou segurança sem correspondências |
| `J0157` | error | referência inválida, esquema desconhecido, ou referência numa chave que não a aceita |
| `J0158` | error | expressão com erro de sintaxe |
| `J0159` | error | literal de orçamento, tamanho ou volume de dados inválido |

**Overlays (`J016x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0160` | error | ficheiro referido não encontrado (`extends`, `path`, `model`, `eds`, chaves…), ou `extends` não local |
| `J0161` | error | `extends` em ciclo |
| `J0162` | error | `extends` com mais de 8 níveis ou 32 ficheiros |
| `J0163` | warning | `unset` sem nada a remover |
| `J0164` | error | diretiva de fusão inválida, ou elemento repetido numa lista ordenada |
| `J0165` | error | overlay muda o tipo de uma chave (tabela ↔ valor) |
| `J0166` | error | `widen` sem justificação suficiente (< 10 caracteres) ou numa chave que não é de envelope |
| `J0167` | warning | `widen` numa camada que não alarga nada |
| `J0168` | error-in-prod | alargamento sem a entrada correspondente em `[accepted]` no lock |
| `J0169` | warning | manifesto de entrada com nome fora da regra `joker.toml` / `joker.<variante>.toml` (aceite só por `--manifest`, §7.7) |

**Dispositivos, barramentos e sinais (`J017x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0170` | error | driver desconhecido neste build, ou tier W/X sem `[drivers]` |
| `J0171` | error | barramento inexistente, de tipo incompatível, ou com protocolos incompatíveis |
| `J0172` | error | perfil ou placa não encontrados, ou perfil sem major |
| `J0173` | error | chave que este driver não aceita |
| `J0174` | error | alias ou caminho criado colide com outro caminho |
| `J0175` | error | ajuste do mesmo sinal declarado em dois sítios com valores diferentes |
| `J0176` | error | ajuste incompatível com o tipo, a direção ou a origem do sinal |
| `J0177` | error | `discover = true` num driver sem descoberta |
| `J0178` | error | dois dispositivos com o mesmo endereço no mesmo barramento, ou um dispositivo com o mesmo recurso do kernel que um `board.<nome>` referido (`ref/03` §2.3.2) |
| `J0179` | error | Joker Link sem autenticação num transporte IP |

**Apps e ligações (`J018x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0180` | error | origem da app inválida (nenhuma ou duas; digest diferente do lock) |
| `J0181` | error | forma de ligação inválida para o tipo da porta |
| `J0182` | error | tarefa ou grupo desconhecido em `[apps.*.cycles]` |
| `J0183` | error | `as = "fraction"`/`"percent"` entre unidades que não são `%` e `1` |
| `J0184` | error | `net.allow` inválido |
| `J0185` | error-in-prod | segredo referido que não existe no cofre do nó |
| `J0186` | error | classe de recursos da app incoerente com a dos grupos, ou `rt` em async |
| `J0187` | error | app `native` que não está neste build da runtime |
| `J0188` | error | `shadow.of` não resolúvel |
| `J0189` | error | porta ligada em dois sítios |

**Instâncias, `calc`, alarmes, telemetria, pontes (`J019x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0190` | error | dependência circular entre `calc` e alarmes |
| `J0191` | error | `implemented_by` aponta para uma app inexistente ou que não implementa a classe |
| `J0192` | error | instância com mais de uma origem (`map`, `implemented_by`, `sim`); numa instância virtual, um `map` só pode ligar membros de classe |
| `J0193` | error | classe ou capacidade desconhecida |
| `J0194` | error | adaptador inválido para a direção ou o tipo do membro |
| `J0195` | error | política de gravação ambígua ou incompleta |
| `J0196` | error | nome de alarme ou caminho criado em colisão (incluindo um alarme chamado `app`: os alarmes de apps vivem em `alarm.app.<app>.<nome>`, §5.15) |
| `J0197` | error | `keep` com uma camada desconhecida |
| `J0198` | error | uplink com `classes`, `live` ou `history` inválidos para o seu `kind` |
| `J0199` | error | ponte inválida (`to`, `writable`, `listen`/`bus`, `fields`) |

**Envelope (`J023x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0230` | error | `range` com mínimo ≥ máximo, ou sem dois elementos |
| `J0231` | error | `safe` com tipo incompatível com a saída |
| `J0232` | error | `safe` não constante |
| `J0233` | error | chave de envelope que não se aplica ao tipo da saída |
| `J0234` | error | envelope num caminho que não é uma saída deste nó (ou governada por um Joker Micro) |
| `J0235` | error | `verify` inválido |
| `J0236` | warning | `reset_when` numa saída que nunca entra em trip |
| `J0237` | error | `group` inexistente |
| `J0238` | warning | `safer` sem fallback de dispositivo (ignorado) |
| `J0239` | error | escritas agendadas acima de 256, ou `timed` num driver sem saídas temporizadas (1.1) |

**Rede do Joker Micro (`J024x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0240` | error | `[net.wifi]` inválido: `ssid` vazio ou com mais de 32 B, `country` fora da ISO 3166-1 alfa-2, ou `hostname` fora da gramática |
| `J0241` | error | transporte IP num MCU sem `[net.wifi]` (`[link] address`, `[telemetry.uplink]`) |
| `J0242` | error-in-prod | rede Wi-Fi aberta: `[net.wifi]` sem `password` |

**Placa sem motor Wasm (`J0243`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0243` | warning | a placa não tem motor Wasm para a sua arquitetura: só apps nativas |

O `jk check` emite o `J0243` quando a `arch` da placa de `[target]` não tem motor Wasm nesta runtime (nem Cranelift nem Pulley; documento 01, §22): hoje, `armv6` se o Pulley não compilar para ARMv6 (documento 23, §5.2). O aviso aparece uma vez, com a lista das apps Wasm do manifesto; essas apps não se carregam no nó (ficam `failed`, ref/09 §5.14), e só as apps `kind = "native"` do build da runtime correm (documento 04, §5; `J0187`). O `jk doctor` repete-o no nó.

**Árbitro e escritores (`J025x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0250` | error | `level` inválido, ou forma compacta incoerente com `priority` |
| `J0251` | error | `priority` fora da faixa |
| `J0252` | error | app ou ponte na faixa `manual` |
| `J0253` | error | ponte na faixa `safety` |
| `J0254` | error | escritor `proposal` com `level`, `priority` ou `lease` |
| `J0255` | error | `max_lease` > 24 h ou `max_manual` > 7 d |
| `J0256` | error | lease menor do que o período do grupo dono |
| `J0257` | warning | `writer` numa app sem saídas |

**Fallback e grupos atómicos (`J027x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0270` | error | saída em dois grupos atómicos |
| `J0271` | error | grupo atómico sem transporte comum |
| `J0272` | error | grupo atómico com saídas de grupos donos diferentes |
| `J0273` | error | `order` do grupo atómico não é uma permutação de `outputs` |
| `J0274` | error | `commit.signal` inválido |
| `J0275` | error | `fallback.configure = true` num perfil sem `[fallback]` |
| `J0276` | error-in-prod | `keepalive` incoerente com o timeout do fallback do equipamento |
| `J0277` | warning | `accept_missing_fallback` numa saída que tem fallback |

**Ciclos e recursos (`J028x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0280` | error | `period` fora dos limites do alvo |
| `J0281` | error | `order` refere uma app ou tarefa que não corre neste grupo |
| `J0282` | error | grupo com classe de recursos que não é `rt` |
| `J0283` | error | `priority` explícita inverte a ordem rate-monotonic |
| `J0284` | error | `cpu` fora de `[resources.rt]` |
| `J0285` | error | `trace.continuous` acima de 7 d |
| `J0286` | warning | `overrun = "degrade"` sem tarefas `optional` |
| `J0287` | warning | `overrun = "catch_up"` num grupo com saídas dependentes do tempo |
| `J0288` | error | nome de grupo reservado usado de forma não permitida |

**Reservas e segurança (`J029x`)**

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0290` | error | funcionalidade reservada para uma versão posterior desta runtime |
| `J0291` | — | retirado nesta rodada: a chave `api.mtls` saiu (TCP é sempre mTLS; a chave dá `J0140`). O número não se reutiliza |
| `J0292` | error | `security.trust` refere uma chave que não está no bundle de confiança do nó com esse papel (o mesmo código no `jk check` e no nó, ao aplicar) |
| `J0293` | warning | papel concedido a qualquer principal |
| `J0294` | error-in-prod | segredo em claro numa chave do tipo segredo |
| `J0295` | error | principal inválido em `[security.roles]` |
| `J0296` | error-in-prod | ligação sem TLS ou sem segurança (`mqtt://`, `http://`, OPC UA `None`) |
| `J0297` | warning | ponte com escritas a escutar em todas as interfaces |
| `J0298` | warning | ação com `role = "viewer"`: vale como `operator`, que é o mínimo da API |

`J0244`–`J0249`, `J0258`–`J0269`, `J0278`, `J0279`, `J0289` e `J0299` ficam livres, para esta referência crescer sem sair da gama.

---

## 11. Comportamento esperado

### 11.1 Ao aplicar uma geração nova

O `jk apply` planeia o diff mínimo (documento 01, §15). O que cada mudança provoca:

| Mudança | Efeito no nó | `--confirm` obrigatório em `prod` |
|---|---|---|
| `[outputs.*]` | a quente, no limite de tick do grupo dono; o último valor aplicado é herdado (16 A8) | não |
| `[calc]`, `[alarms]` | a quente; o estado das janelas e dos flancos recomeça (é volátil, documento 14, A7); alarmes ativos mantêm o estado se a expressão não mudou | não |
| `[signals]`, `[devices.*.signals]` (ajustes) | a quente, no estágio de aquisição | não |
| `[devices.*]` (endereço, perfil, barramento, driver) | reinicia só esse driver; os sinais passam por `Uncertain(Initial)` | não |
| `[buses.*]` | reinicia os drivers desse barramento | não |
| `[apps.*.params]` | `on-params`, sem reinício | não |
| `[apps.*] source`/`digest` | hot-swap num limite de tick (documento 01, §9) | não |
| `[apps.*] bind`, `cycles`, `writer`, `resources` | nova instância da app num limite de tick, com `migrate` | não |
| `[cycles.*]` | o grupo é recriado num limite de tick; os leases passam a contar com o período novo | não |
| `[instances.*]` | as apps que usam as instâncias são reinstanciadas (estado de `Many` por nome, documento 14, A10) | não |
| `[telemetry.record]`, `[telemetry.store]` | a quente | não |
| `[telemetry.uplink.*]`, `[bridges.*]`, `[security]`, `[clock]`, `[project] mode` | religa o serviço afetado | **sim** (ADR 0015) |
| `[resources]`, `[target]` | os cpusets são reconfigurados; os grupos afetados são recriados | não |

### 11.2 Determinismo

- Todas as tabelas livres são `BTreeMap`: a ordem de processamento é lexicográfica, e não a do ficheiro.
- O mesmo conjunto de ficheiros dá sempre os mesmos diagnósticos, pela mesma ordem, e o mesmo hash do manifesto efetivo (§8.3).
- Nenhuma validação depende do relógio de parede, da rede ou do nó, exceto as do passo 10 (§8.1) quando o `jk check` corre com `--node`.

### 11.3 Limites v1 do manifesto

| Coisa | Limite | Código |
|---|---|---|
| tamanho de um ficheiro | 4 MiB | `J0149` |
| camadas de `extends` | 8 níveis, 32 ficheiros | `J0162` |
| dispositivos | 1 024 | `J0149` |
| apps | 50 (documento 09, ataque 11) | `J0149` |
| saídas, `calc`, alarmes, instâncias | 16 384 cada | `J0149` |
| correspondências de um seletor | 4 096 | `J0149` |
| expressão | 4 KiB | `J0149` |
| `message`, justificações | 1 KiB | `J0149` |
| `schedule_depth` | 256 | `J0239` |
| segmentos de caminho | `ref/00` §10 | `J0153` |

### 11.4 Mensagens de erro

No estilo do rustc (`ref/00` §8), sempre com código, localização na **camada** onde está a chave e, quando houver, ajuda:

```
error[J0151]: número sem unidade numa quantidade
  --> sites/lisboa-01/joker.toml:41:8
   |
41 | safe = 0
   |        ^ `pcs.p_set` está em kW
   = ajuda: escreve `safe = "0 kW"`
```

---

## 12. Esboço dos tipos Rust (`joker-manifest`)

Duas fases: as camadas são lidas como `toml::Spanned` sem tipos (para a fusão guardar a proveniência), e o manifesto efetivo é desserializado para os tipos abaixo. As tabelas livres são `BTreeMap` (invariante 9). O esquema JSON publicado é gerado a partir destes tipos, com o `since` de cada campo.

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
    /// Driver-specific keys (annex A), validated against the driver's published schema (ref/06).
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

/// A value that exists in the 1.0 schema but is refused by a 1.0 runtime (J0290).
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Reserved<T>(pub T);

// Value newtypes: each implements FromStr with the grammar of §3 and carries its span for diagnostics.
pub struct Duration(u64);                     // ns
pub struct Quantity { pub value: f64, pub unit: joker_units::Unit }
pub struct Rate(Quantity);                    // unit/s after normalisation
pub struct ActuationBudget { pub count: u32, pub window: Duration }
pub struct Level { pub band: Band, pub priority: Option<u8> }
pub struct Expr(String);                      // compiled by joker-expr after the registry is built
pub struct Path(String);
pub struct Selector(String);
```

---

## 13. Estresse: onze ataques à própria referência

**1 — O erro de escrita que desliga uma proteção.** `max_leas = "15m"` num site. Num formato tolerante, a chave era ignorada e o `max_lease` ficava em 15 min por omissão, por acaso igual; com `requse_good`, a proteção desaparecia em silêncio. **O que acontece:** `J0140` com "querias dizer `requires_good`?". **Aguenta**, e é a razão de o TOML ser estrito (`ref/00` §1). As ferramentas que querem anotar o manifesto usam `x_*`.

**2 — O `safe` sem unidade e o perfil que muda.** Um integrador escreve `safe = 0` e `range = [-125, 125]`; o perfil do PCS passa de `kW` para `W` na versão 4. **Quebrava:** a gama passava a ±125 W sem ninguém mexer no site. **Correção incorporada:** número sem unidade é `J0151` sempre que a dimensão não é `1` (§3.2), com a sugestão da unidade atual do sinal.

**3 — O template com seletor e o site que quer mexer num inversor.** O template declara `[outputs."inv*.w_max_lim_pct"]`; o site quer `max_lease = "5s"` só no `inv2` e escreve `[outputs."inv2.w_max_lim_pct"]`. **O que acontece:** `J0215` (dois envelopes para `inv2`), com a proveniência das duas entradas e a ajuda: "retira a entrada do template com `outputs.\"inv*.w_max_lim_pct\" = { unset = true }` e declara `inv1`…`inv3` explicitamente". Considerou-se "o mais específico vence" e **rejeitou-se** para envelopes: uma ambiguidade num envelope tem de ser erro (é a regra do Cravo). Nas gravações (§5.17.1) a especificidade é aceite, porque gravar não é uma decisão de segurança.

**4 — Alargar em duas camadas.** Camada 2: `range = { unset = true }`; camada 3: `range = ["-200 kW", "200 kW"]`. Se a camada 3 se comparasse com a 2 (sem gama), "passar a existir" seria apertar, e o alargamento passava sem `widen`. **Correção incorporada:** o `unset` de uma chave de envelope já é um alargamento (§7.4), e exige `widen` na camada 2.

**5 — Unidades diferentes na comparação.** O template tem `["-125 kW", "125 kW"]` e um site escreve `["-125000 W", "125000 W"]`. **O que acontece:** igual, depois de convertido; não é `J0220`. Com `["-125 kW", "125.001 kW"]`, é um alargamento de 1 W, e é `J0220`: a regra é exata, e a ajuda mostra a diferença.

**6 — O diamante com `append`.** `S` estende `A` e `B`, que estendem ambos `T`, e `T` faz `requires_good = { append = ["bms.soc"] }`. Uma fusão ingénua aplicava o `append` duas vezes. **Correção incorporada:** linearização em pós-ordem com cada ficheiro uma só vez (§7.1), e `append` em conjuntos sem duplicados (§7.3).

**7 — Um manifesto 1.1 num nó 1.0.** O Deck envia a um nó 1.0 um site que usa `schedule_depth`. **O que acontece:** o `jk check` 1.0 conhece a chave (reservada) e dá `J0290`; o agente recusa a geração antes de a aplicar. Com uma chave que a 1.0 não conhece de todo (uma chave nova da 1.3), dá `J0148` se o manifesto declarar `joker = "1.3"`, ou `J0147` ("este manifesto exige a runtime 1.3"). Nunca uma chave é ignorada em silêncio por ser nova.

**8 — Segurança: o overlay de um site acrescenta uma chave de confiança.** Um repositório de site com permissões largas acrescenta `trust.apps = ["keys/minha.pub"]` para instalar uma app assinada por uma chave própria. **Quebrava** no desenho do documento 03, §13: a configuração declarava a sua própria confiança. **Correção incorporada:** `[security.trust]` só restringe as âncoras do nó (`J0292`, §5.19). Um papel `admin = ["cert:CN=*"]` passa, mas com `J0293`, e em `prod` o `jk apply` exige `--confirm` para qualquer mudança em `[security]`.

**9 — Um segredo colado no manifesto.** `password = "hunter2"` num uplink. **O que acontece:** `J0294` (erro em `prod`): a chave é do tipo segredo e só aceita `secret:<nome>`. O valor nunca aparece nos diagnósticos, no `jk diff` nem na API: é mascarado (`"****"`) na mensagem.

**10 — Desempenho: 3 000 sites e o Deck.** Cada site tem 3 camadas e ~400 linhas. A fusão é linear no tamanho das camadas, e o `jk check` completo é dominado pela admissão RT e pela carga dos perfis (em cache pelo digest do lock). O hash do manifesto efetivo, e por isso o da geração, não depende da ordem das chaves nem dos comentários (§8.3), e por isso um reformat em massa dos repositórios não gera 3 000 rollouts. **Aguenta**; o alvo é < 200 ms por site num portátil, a medir na tarefa 16 do documento 13.

**11 — Caminhos relativos em camadas de pastas diferentes.** `templates/joker.bess-v2.toml` declara `model = "sim/plant.sim.toml"` e é estendido por `sites/lisboa-01/joker.toml`. Relativo a quê? **Correção incorporada:** ao ficheiro que declara a chave (§7.5). O template leva consigo os seus ficheiros, e o site não tem de os copiar. O `jk explain` mostra o caminho resolvido.

---

## 14. Decisões e alternativas rejeitadas

| # | Decisão | Porquê | Alternativa rejeitada |
|---|---|---|---|
| D1 | `mode` tem omissão `"prod"` | esquecer a chave num site real não pode desligar assinaturas nem transformar erros em avisos | omissão `"dev"`: mais cómodo, e perigoso |
| D2 | número sem unidade só em grandezas adimensionais | o significado de um número não pode depender da versão do perfil | aceitar números "na unidade do sinal", como nas interfaces de texto do `ref/00` §4.1 (que continuam a aceitá-los, porque aí o sinal está à vista) |
| D3 | sobreposições de seletores: erro em envelopes e ajustes; especificidade na telemetria | ambiguidade num envelope é uma decisão de segurança tomada por acaso | especificidade em todo o lado (CSS); primeira declaração vence |
| D4 | `extends` relativo ao ficheiro que declara; linearização em pós-ordem sem repetições | os templates são portáteis; diamantes são normais em frotas | relativo à raiz do projeto (partia templates partilhados); diamantes como erro |
| D5 | "só apertar" comparado camada a camada, com `unset` a contar como alargamento | fecha os alargamentos em duas camadas | comparar só com a primeira camada (deixava um overlay intermédio sem controlo) |
| D6 | sem interpolação de variáveis de ambiente | a geração tem de ser função só dos ficheiros (replay, hash, Deck); os segredos vão por `secret:` | `${ENV}` à Docker Compose |
| D7 | `[security.trust]` só restringe as âncoras do nó | uma configuração não pode ser a sua própria raiz de confiança | manter o documento 03, §13, como está |
| D8 | tarefas fora de `order` correm por ordem lexicográfica | o resultado não pode depender da ordem das camadas | "pela ordem do manifesto" (documento 03, §7), indefinida com overlays |
| D9 | grupos implícitos `default` (100 ms) e `default_<período>` | concilia o A11 do documento 16 com o §8 do documento 03, que se contradiziam | um só `default` com o período pedido (conflito com A11) |
| D10 | secção `[drivers]` para tier W e X | um driver externo precisa de artefacto ou executável declarado, e nada o dizia | registar os drivers no nó, fora do manifesto (a geração deixava de os descrever) |
| D11 | o `commit` de um grupo atómico não tem árbitro nem envelope | sem escritores, o árbitro escreveria o `safe` em cada tick (A6) | tratá-lo como uma saída comum |
| D12 | nível do escritor em duas formas (`level` + `priority`, ou `"faixa:n"`) | a forma compacta é a do `ref/00`, da CLI e da conformidade; a longa é a do documento 03 | só uma das formas (partia um dos dois conjuntos de exemplos) |
| D13 | o hash do manifesto efetivo materializa as omissões e exclui `x_*` | declarar uma omissão não muda o comportamento, e as anotações de ferramentas também não | hash dos bytes dos ficheiros |
| D14 | a API local configura-se em `[security.api]`, e não em `[api]` | cada escolha sobre a API (escutas TCP, papéis) é de segurança, e revê-se no mesmo sítio; o mTLS não é escolha (sempre ligado em TCP) | uma secção `[api]` própria |
| D15 | `severity` obrigatória nos alarmes | a ISA-18.2 exige prioridade, e uma omissão escondia a decisão | omissão `"medium"` |

---

## 15. Marcações de versão

| Fase | O quê |
|---|---|
| **1.0** | tudo o que está no §5, exceto o que se lista abaixo; `[drivers]`; `[link]` no Joker Micro; pontes (Fase 2); sombra de apps e orçamentos de uplink (Fase 3) |
| **1.1** (reservado no esquema 1.0, `J0290`) | `[models]`, `[streams]`, `[pipelines]`, `[training]`; `writer.kind = "proposal"`; `apps.*.fallback`; `outputs.*.schedule_depth`, `timed`, `on_late`, `late_margin`; sinais `kind = "block"`; `telemetry.record` com `blocks`/`trigger`/`pre`/`post`; `resources.ml.accel`; `bind` com `model:`/`pipe:`; `source = "training:…"`; `security.trust.models`; drivers `v4l2`, `libcamera`, `spi-adc` |
| **1.2** | drivers `ethercat` e `bacnet-ip` nativo (documento 21, §4); o tier W passa a estável e aberto a terceiros (`ref/06` §4; na 1.0 é instável, só para drivers da árvore) |
| **1.4** | apps Wasm em MCUs M2: `[apps.*] source` num alvo MCU (hoje `J0720`) |
| **2.0** | caminhos qualificados por nó (`@caldeira/pump.run_cmd`, prefixo `@` já reservado); manifesto de site para vários nós (documento 21, §8) |

---

## Anexo A — Chaves específicas por driver (catálogo v1)

As chaves comuns estão no §5.8. Para cada driver do catálogo, a coluna "Chaves do dispositivo" **é** o esquema `config` do seu descritor, a coluna "Chaves por sinal" é o esquema `address` (nos drivers sem perfil) e a coluna "`[discovered]`" é o esquema `discovered` (`ref/06` §2.3; `ref/02` §2.1.11). Nos drivers com perfil, as chaves de endereço dos pontos estão no perfil (`ref/03` §2.2.5), com os mesmos nomes. Um driver tier W ou X publica os seus esquemas no descritor, e o `jk check` valida contra eles. As chaves marcadas **(segredo)** só aceitam `secret:<nome>` (§3.10).

| Driver | Tier | Fase | `bus` | `profile` | Chaves do dispositivo | Chaves por sinal | `[discovered]` |
|---|---|---|---|---|---|---|---|
| `sim` | N | 1.0 | — | — (o modelo declara os sinais) | `model` (`Ficheiro` do modelo de simulação, `ref/03` §2.4, obrigatória), `params` (tabela com os `[params]` do modelo) | — | — |
| `modbus-tcp` | N | 1.0 | `ethernet`, opcional | obrigatório (`"sunspec"` com `discover = true`) | `address` (`"<host>:<porta>"`, obrigatória; porta por omissão 502), `unit_id` (0–255, omissão 1) | — | com `sunspec`: `models` |
| `modbus-rtu` | N | 1.0 | `serial`, obrigatório | obrigatório (`"sunspec"` com `discover = true`) | `unit_id` (1–247, obrigatória) | — | com `sunspec`: `models` |
| `gpio` | N | 1.0 | — | — (expansores: perfil da família `gpio`) | `chip` (`"gpiochip<N>"`, label, ou `"auto"`, omissão `"auto"`: a placa escolhe, `ref/03` §2.3.2; num MCU não se aplica, `J0145`) | `line` (nome BCM `"GPIO17"`, nome da linha, ou deslocamento inteiro; num MCU, o nome de um pino dos `[pins]` da placa com a função `gpio`, como `"gpio2"` no `esp32-devkit`, e um pino inexistente ou sem essa função é `J0828`; obrigatória), `direction`, `bias` (`"as-is"` \| `"pull-up"` \| `"pull-down"` \| `"disabled"`, omissão `"as-is"`), `active_low` (`false`), `drive` (`"push-pull"` \| `"open-drain"` \| `"open-source"`, só saídas), `edges` (`"none"` \| `"rising"` \| `"falling"` \| `"both"`, omissão `"none"`), `timestamp` (`"kernel"` \| `"none"`, omissão `"kernel"` quando há `edges`), `debounce` | — |
| `sysfs` | N | 1.0 | — | — | nenhuma | exatamente um de `led` (nome em `/sys/class/leds`, p. ex. `"ACT"`; sinal `bool`, `direction = "output"` ou `"input"`; escreve `max_brightness` ou 0; numa saída, ao abrir põe o `trigger` do LED a `none` e repõe o anterior ao fechar, ver a nota a seguir à tabela) e `thermal` (zona em `/sys/class/thermal`, p. ex. `"thermal_zone0"`; entrada `f64` em `°C`, lida em m°C); `direction` (omissão `"input"`) | — |
| `i2c` | N | 1.0 | `i2c`, obrigatório | obrigatório | `address` (inteiro 0x03–0x77, obrigatória), `config` (tabela validada pelo `[config]` do perfil, p. ex. `pga`, `rate`) | — | — |
| `spi` | N | 1.0 | `spi`, obrigatório | obrigatório | `config` (como no `i2c`) | — | — |
| `revpi` | N | 1.0 | — | — (módulos do PiCtory) | `config` (`Ficheiro` `.rsc`, omissão `"/var/www/revpi/pictory/projects/_config.rsc"`) | — (os sinais são as variáveis do PiCtory, com o mesmo nome) | — |
| `mqtt` (entrada) | N | 1.0 | — | opcional | `broker` (URL `mqtt://`/`mqtts://`, obrigatória), `username`, `password` **(segredo)** | `topic` (obrigatória), `json` (ponteiro JSON RFC 6901; omissão: o payload inteiro), `qos` (0–1, omissão 1), `type`, `unit` | — |
| `opcua` (cliente) | N | 1.0 (Fase 2) | — | opcional (importado de NodeSet) | `endpoint` (`"opc.tcp://…"`, obrigatória), `security` (`{ policy, mode }`, como nas pontes), `username`, `password` **(segredo)**, `sampling` (`Duração`, omissão = `poll`) | sem perfil: `node` (`"ns=2;s=…"`), `attribute` (omissão `"Value"`), `sampling`, `type`, `unit` | `server_uri`, `namespaces` |
| `can` | N | 1.0 (Fase 2) | `can`, obrigatório | obrigatório (importado de DBC) | — | — | — |
| `canopen` | N | 1.0 (Fase 2) | `can`, obrigatório | `profile` ou `eds` | `node_id` (1–127, obrigatória), `eds` (`Ficheiro`) | — | `vendor_id`, `product_code`, `revision` |
| `m-bus` | N | 1.0 (Fase 2) | `serial`, obrigatório | obrigatório | `address` (primário 0–250, ou secundário `"<16 hex>"`) | — | — |
| `onewire` | N | 1.0 (Fase 2) | — | obrigatório | `rom` (`"<16 hex>"`, obrigatória) | — | — |
| `serial-line` | N | 1.0 (Fase 2) | `serial`, obrigatório | obrigatório | — | — | — |
| `joker-link` | N | 1.0 (Fase 2) | `serial` ou `can`; ou nenhum, com `usb` ou `address` | — (gerado a partir do catálogo do MCU, `ref/03` §2.2.16) | exatamente um de `bus`, `usb` (`Ficheiro`, `/dev/ttyACM0`) e `address` (`"udp:<ip>:<porta>"` \| `"tcp:<ip>:<porta>"`, porta por omissão 7471; MCUs M2); `addr` (endereço do MCU no barramento, inteiro; omissão 0 em série, 1 em CAN; `J0736`); `can_id_base` (0–255, omissão `0x4A`; `J0737`); `auth` (`"required"` \| `"none"`, omissão `"required"`; `J0179`, `J0738`); `key` **(segredo)** (omissão `"secret:link.<dispositivo>"`); `node` (`NomeDns` esperado no `HELLO`, opcional); temporizações `report`, `refresh`, `keepalive`, `link_timeout`, `max_lease`, `rto_min`, `ack_delay`, `time_interval`, `time_tolerance`, `time_max_delay`, `turn_timeout` (`Duração`); tamanhos e contagens `max_frame`, `max_msg`, `window`, `bulk_max`, `retransmit`, `turn_frames` (`Inteiro`); `instances` (`Tabela<nome no MCU, nome no nó>`); `micro` (`Ficheiro` do manifesto do MCU: o `jk check` sem ligação ao nó valida o lado do MCU a partir dele, e o `jk apply` constrói e leva o blob de geração do MCU). Com `address`, é o IP e a porta **do MCU** (no manifesto do MCU, `[link] address` é o do nó, §5.22). Omissões e validação na `ref/07` §15 | — | nenhum além dos comuns; o `digest` é o hash do catálogo (`ref/02` §2.1.11) |
| `ethercat` | N | 1.2 | `ethercat`, obrigatório | obrigatório | `position` (inteiro, obrigatória) | — | — |
| `bacnet-ip` | X → N | 1.2 (os dois tiers; documento 21, §4) | `ethernet`, opcional | obrigatório ou por descoberta | `network` (CIDR), `device_instance` (inteiro), `priority` (1–16: a prioridade BACnet que o Joker usa; obrigatória com saídas) | — | segundo o esquema do driver |
| `v4l2` | N | **1.1** | — | — | `device` (`Ficheiro`), `format` (`{ width, height, pixfmt, fps }`), `trigger` (`{ kind = "hardware" \| "software", input = Caminho }`) | — | — |
| `libcamera` | X | **1.1** | — | — | `camera` (id), `format`, `trigger` | — | — |
| `spi-adc` | N | **1.1** | `spi`, obrigatório | obrigatório | `drdy` (`Caminho` de uma entrada GPIO), `rate` (`Quantidade<Hz>`) | `kind`, `block`, `unit` | — |
| `adc`, `pwm` | N (só MCU) | 1.0 | — | — | — | `adc`: `pin` (pino com função `adc` da placa, `ref/03` §2.3.3), `unit`, `scale`; `pwm`: `pin`, `frequency` (`Quantidade<Hz>`) | — |
| `esp32` | N (só MCU) | 1.0 | — | — | nenhuma; só em placas ESP32 (`arch` = `xtensa-esp32-none-elf` ou `xtensa-esp32s3-none-elf`), noutra `J0145` | exatamente um de: `touch` (`Inteiro` 0–9: o pad de toque capacitivo, resolvido pelo `touch_channel` dos `[pins]` da placa, `ref/03` §2.3.3; `touch = 0` é o T0, no GPIO4 do `esp32-devkit`; pad sem pino na placa `J0828`; entrada `u64`, sem unidade: o valor cru do periférico, que **desce** quando se toca), `rssi = true` (RSSI do Wi-Fi em dBm; entrada `i64` com `unit = "1"`, porque a escala é logarítmica; sem `[net.wifi]` ou sem ligação, `bad(not_connected)`), `uptime = true` (tempo desde o arranque; entrada `f64` em `s`, `monotonic`); `direction` só `"input"` (outra `J0145`); `poll` (omissão: o do dispositivo); os ajustes comuns de sinal (`deadband`, `stuck_detect`, `limits`, §5.8.1). Os tipos são os mesmos que um perfil Modbus dá a registos `u16`, `i16` e `u32` com `unit = "s"` (`ref/03` §2.2.7): trocar o firmware de bancada pelo Joker Micro não muda os tipos dos sinais (documento 23, §3 e §6.5) | — |
| `modbus-slave` | N (só MCU) | 1.0 | `serial`, obrigatório | obrigatório (o mapa servido, no formato dos perfis) | `unit_id` (1–247, obrigatória) | — | — |
| `extern` | — (só MCU) | 1.0 | — | — | nenhuma | `type`, `unit`, `direction` (§5.22; `J0785`) | — |
| `w:<nome>`, `x:<nome>` | W, X | 1.0 | segundo o descritor | segundo o descritor | as do esquema `config` | as do esquema `address` | as do esquema `discovered` |

**`sysfs` e o `trigger` dos LEDs.** Um LED do kernel pode estar a ser comandado por um `trigger` (`mmc0`, `heartbeat`, `default-on`…), que sobrepõe o `brightness` escrito. Ao abrir um sinal `led` com `direction = "output"`, o driver lê o `trigger` atual, guarda-o, e escreve `none`; ao fechar (paragem ordenada do `jokerd`, troca de geração que retira o sinal, `jk device stop`), repõe o `trigger` guardado. Uma paragem abrupta deixa o `trigger` a `none` e o último `brightness`: é por isso que o comportamento do LED com o `jokerd` parado é o `fallback` do `[io]` da placa (ref/03 §2.3.2), e não o `trigger` de origem. Um LED só de entrada (`direction = "input"`) não mexe no `trigger`. **Permissões:** o pacote do `jokerd` instala a regra udev `/usr/lib/udev/rules.d/60-joker-leds.rules`, que dá ao grupo `joker` escrita em `brightness` e `trigger` de `/sys/class/leds/*`; sem ela, abrir o LED falha com `bad(config_error)` e o `jk doctor` di-lo (documento 05, §10).

`"sunspec"` não é um driver: é a forma curta do perfil embutido `joker/sunspec@1`, descoberto pelos drivers Modbus (documento 05, §3 e §7; `ref/03` §2.2.15).

**`retries` e `retransmit` no `joker-link`.** A chave comum `retries` (§5.8, repetições do núcleo por pedido de driver) não se aplica a um `joker-link` (`J0145`): o link não faz pedidos por lote. O número de retransmissões das mensagens fiáveis do link é a chave própria `retransmit` (`ref/07` §5.6, §15).

---

## Anexo B — Tabela de cobertura

**Método.** Um script extraiu **todos** os blocos ` ```toml ` de todos os `.md` do projeto fora de `docs/ref/` (documentos por área, exemplos, trilha, ADRs, propostas; os blocos das referências são as próprias definições, e não entram): 125 blocos, todos TOML válido. Destes, 80 são manifestos, overlays ou fragmentos de manifesto (2 fragmentos de `[apps.luz]` da trilha, lidos como `apps.*.…`); os outros são locks, perfis, placas, modelos de simulação, cenários, casos de conformidade, a configuração da `jk` e do nó, o `fleet.toml` e o `model.toml`, que pertencem à `ref/02`, à `ref/03` e à `ref/10`. O script percorreu cada manifesto e normalizou as chaves, substituindo os nomes livres por `*` (`devices.esp.poll` → `devices.*.poll`; os elementos de listas de tabelas por `[]`; as diretivas de overlay `unset`, `append` e `widen` ficam como a última componente). O resultado são **371 chaves distintas**. Cada uma foi atribuída à secção desta referência que a define, e um segundo passo confirmou que o nome da chave aparece nessa secção; nenhuma ficou sem secção nem sem definição. A tabela foi regenerada depois da reescrita da trilha (onze partes sobre a bancada do documento 23).

| Estado | Chaves |
|---|---|
| coberta | 256 |
| coberta (1.1) | 94 |
| coberta (só Joker Micro) | 11 |
| coberta: diretiva de overlay | 4 |
| coberta (1.1): chave livre de `hyper` | 3 |
| coberta (C21 aplicado: o 08 usa `path = "apps/…"`) | 1 |
| coberta (`proposal` é 1.1) | 1 |
| coberta; só restringe o bundle do nó (C9, resolvido: D7) | 1 |

A coluna "Usos" conta os blocos onde a chave aparece; "Onde" mostra a primeira ocorrência e mais duas, no máximo (ficheiro:linha do bloco, pela ordem dos ficheiros).

| Chave | Usos | Onde | Secção | Estado |
|---|---|---|---|---|
| `alarms` | 10 | 02-signal-model:274, 03-manifest:372, 08-stress-tests:253 (+7) | §5.15 | coberta |
| `alarms.*` | 10 | 02-signal-model:274, 03-manifest:372, 08-stress-tests:253 (+7) | §5.15 | coberta |
| `alarms.*.actions` | 3 | 02-signal-model:274, 03-manifest:372, examples/01-bess-peak-shaving:24 | §5.15 | coberta |
| `alarms.*.actions.trip` | 3 | 02-signal-model:274, 03-manifest:372, examples/01-bess-peak-shaving:24 | §5.15 | coberta |
| `alarms.*.delay_off` | 3 | 02-signal-model:274, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.15 | coberta |
| `alarms.*.delay_on` | 9 | 02-signal-model:274, 08-stress-tests:253, 23-development-bench:195 (+6) | §5.15 | coberta |
| `alarms.*.message` | 6 | 02-signal-model:274, 23-development-bench:195, examples/02-visual-inspection:35 (+3) | §5.15 | coberta |
| `alarms.*.severity` | 10 | 02-signal-model:274, 03-manifest:372, 08-stress-tests:253 (+7) | §5.15 | coberta |
| `alarms.*.suppress_when` | 2 | 02-signal-model:274, examples/03-predictive-maintenance:11 | §5.15 | coberta |
| `alarms.*.when` | 10 | 02-signal-model:274, 03-manifest:372, 08-stress-tests:253 (+7) | §5.15 | coberta |
| `apps` | 28 | 03-manifest:251, 06-ml:171, 08-stress-tests:23 (+25) | §5.12 | coberta |
| `apps.*` | 28 | 03-manifest:251, 06-ml:171, 08-stress-tests:23 (+25) | §5.12 | coberta |
| `apps.*.bind` | 25 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+22) | §5.12.1 | coberta |
| `apps.*.bind.*` | 25 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+22) | §5.12.1 | coberta |
| `apps.*.bind.*.as` | 3 | 03-manifest:251, 08-stress-tests:23, examples/01-bess-peak-shaving:103 | §5.12.1 | coberta |
| `apps.*.bind.*.each` | 1 | 14-classes-and-instances:300 | §5.12.1 | coberta |
| `apps.*.bind.*.map` | 1 | 14-classes-and-instances:300 | §5.12.1 | coberta |
| `apps.*.bind.*.map.*` | 1 | 14-classes-and-instances:300 | §5.12.1 | coberta |
| `apps.*.bind.*.optional` | 1 | 08-stress-tests:444 | §5.12.1 | coberta |
| `apps.*.bind.*.signal` | 4 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:444 (+1) | §5.12.1 | coberta |
| `apps.*.cycles` | 15 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:341 (+12) | §5.12 | coberta |
| `apps.*.cycles.*` | 15 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:341 (+12) | §5.12 | coberta |
| `apps.*.digest` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.fallback` | 2 | 06-ml:171, 08-stress-tests:518 | §6.5 | coberta (1.1) |
| `apps.*.fallback.controller` | 2 | 06-ml:171, 08-stress-tests:518 | §6.5 | coberta (1.1) |
| `apps.*.fallback.when` | 2 | 06-ml:171, 08-stress-tests:518 | §6.5 | coberta (1.1) |
| `apps.*.kind` | 1 | 08-stress-tests:341 | §5.12 | coberta |
| `apps.*.net` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.net.allow` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.params` | 15 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+12) | §5.12.2 | coberta |
| `apps.*.params.*` | 15 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+12) | §5.12.2 | coberta |
| `apps.*.path` | 17 | 08-stress-tests:158, 08-stress-tests:341, 08-stress-tests:518 (+14) | §5.12 | coberta |
| `apps.*.requires_clock` | 2 | 03-manifest:251, 08-stress-tests:444 | §5.12 | coberta |
| `apps.*.resources` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.resources.async` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.resources.cycle` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.resources.memory` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.secrets` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.secrets.*` | 1 | 03-manifest:251 | §5.12 | coberta |
| `apps.*.shadow` | 2 | 03-manifest:251, 08-stress-tests:518 | §5.12.3 | coberta |
| `apps.*.shadow.of` | 2 | 03-manifest:251, 08-stress-tests:518 | §5.12.3 | coberta |
| `apps.*.shadow.promote` | 1 | 08-stress-tests:518 | §5.12.3 | coberta |
| `apps.*.shadow.promote.outputs` | 1 | 08-stress-tests:518 | §5.12.3 | coberta |
| `apps.*.shadow.until` | 2 | 03-manifest:251, 08-stress-tests:518 | §5.12.3 | coberta |
| `apps.*.shadow.until.duration` | 2 | 03-manifest:251, 08-stress-tests:518 | §5.12.3 | coberta |
| `apps.*.shadow.until.max_divergence` | 1 | 03-manifest:251 | §5.12.3 | coberta |
| `apps.*.source` | 5 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:444 (+2) | §5.12 | coberta (C21 aplicado: o 08 usa `path = "apps/…"`) |
| `apps.*.writer` | 12 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+9) | §5.12 | coberta |
| `apps.*.writer.kind` | 1 | 08-stress-tests:518 | §5.12 | coberta (`proposal` é 1.1) |
| `apps.*.writer.lease` | 11 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+8) | §5.12 | coberta |
| `apps.*.writer.level` | 10 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+7) | §5.12 | coberta |
| `apps.*.writer.priority` | 9 | 03-manifest:251, 08-stress-tests:23, 08-stress-tests:158 (+6) | §5.12 | coberta |
| `atomic` | 3 | 02-signal-model:162, 03-manifest:372, 08-stress-tests:341 | §5.16 | coberta |
| `atomic.*` | 3 | 02-signal-model:162, 03-manifest:372, 08-stress-tests:341 | §5.16 | coberta |
| `atomic.*.commit` | 1 | 02-signal-model:162 | §5.16 | coberta |
| `atomic.*.commit.signal` | 1 | 02-signal-model:162 | §5.16 | coberta |
| `atomic.*.commit.value` | 1 | 02-signal-model:162 | §5.16 | coberta |
| `atomic.*.on_rate` | 2 | 03-manifest:372, 08-stress-tests:341 | §5.16 | coberta |
| `atomic.*.order` | 2 | 02-signal-model:162, 03-manifest:372 | §5.16 | coberta |
| `atomic.*.outputs` | 3 | 02-signal-model:162, 03-manifest:372, 08-stress-tests:341 | §5.16 | coberta |
| `bridges` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.expose` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.expose.*` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.kind` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.listen` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.security` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.security.mode` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.security.policy` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.security.trust` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.subscribe` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.subscribe.*` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.subscribe.*.fields` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.subscribe.*.max_age` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.subscribe.*.to` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.subscribe.*.type` | 2 | 03-manifest:423, 08-stress-tests:341 | §5.18 | coberta |
| `bridges.*.writer` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.writer.lease` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.writer.level` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.writer.priority` | 1 | 03-manifest:423 | §5.18 | coberta |
| `bridges.*.writer.writable` | 1 | 03-manifest:423 | §5.18 | coberta |
| `buses` | 11 | 03-manifest:83, 08-stress-tests:253, 08-stress-tests:341 (+8) | §5.6 | coberta |
| `buses.*` | 11 | 03-manifest:83, 08-stress-tests:253, 08-stress-tests:341 (+8) | §5.6 | coberta |
| `buses.*.baud` | 8 | 03-manifest:83, 23-development-bench:255, 23-development-bench:304 (+5) | §5.6 | coberta |
| `buses.*.bitrate` | 1 | 03-manifest:83 | §5.6 | coberta |
| `buses.*.cycle` | 2 | 03-manifest:83, 08-stress-tests:341 | §5.6 | coberta |
| `buses.*.dc` | 1 | 08-stress-tests:341 | §5.6 | coberta |
| `buses.*.dev` | 3 | 08-stress-tests:253, examples/00-first-project:194, examples/03-predictive-maintenance:11 | §5.6 | coberta |
| `buses.*.dtr` | 2 | track/04-esp32-modbus:375, track/06-switching-boards:31 | §5.6 | coberta |
| `buses.*.iface` | 2 | 03-manifest:83, 08-stress-tests:341 | §5.6 | coberta |
| `buses.*.kind` | 9 | 03-manifest:83, 08-stress-tests:253, 08-stress-tests:341 (+6) | §5.6 | coberta |
| `buses.*.parity` | 2 | 03-manifest:83, examples/00-first-project:194 | §5.6 | coberta |
| `buses.*.port` | 6 | 03-manifest:83, 23-development-bench:255, examples/00-first-project:194 (+3) | §5.6 | coberta |
| `buses.*.rts` | 2 | track/04-esp32-modbus:375, track/06-switching-boards:31 | §5.6 | coberta |
| `buses.*.stop` | 1 | 03-manifest:83 | §5.6 | coberta |
| `buses.*.turnaround` | 1 | 03-manifest:83 | §5.6 | coberta |
| `calc` | 6 | 03-manifest:372, 08-stress-tests:23, 23-development-bench:195 (+3) | §5.14 | coberta |
| `calc.*` | 6 | 03-manifest:372, 08-stress-tests:23, 23-development-bench:195 (+3) | §5.14 | coberta |
| `clock` | 2 | 03-manifest:53, track/09-joker-micro:48 | §5.5 | coberta |
| `clock.max_holdover` | 1 | 03-manifest:53 | §5.5 | coberta |
| `clock.servers` | 1 | 03-manifest:53 | §5.5 | coberta |
| `clock.sync` | 2 | 03-manifest:53, track/09-joker-micro:48 | §5.5 | coberta |
| `cycles` | 9 | 03-manifest:229, 08-stress-tests:23, 08-stress-tests:341 (+6) | §5.11 | coberta |
| `cycles.*` | 9 | 03-manifest:229, 08-stress-tests:23, 08-stress-tests:341 (+6) | §5.11 | coberta |
| `cycles.*.order` | 2 | 03-manifest:229, track/08-your-own-class:222 | §5.11 | coberta |
| `cycles.*.overrun` | 2 | 03-manifest:229, 08-stress-tests:341 | §5.11 | coberta |
| `cycles.*.period` | 9 | 03-manifest:229, 08-stress-tests:23, 08-stress-tests:341 (+6) | §5.11 | coberta |
| `cycles.*.resources` | 4 | 03-manifest:229, 08-stress-tests:23, 08-stress-tests:341 (+1) | §5.11 | coberta |
| `cycles.*.trace` | 2 | 03-manifest:229, examples/01-bess-peak-shaving:24 | §5.11 | coberta |
| `cycles.*.trace.continuous` | 2 | 03-manifest:229, examples/01-bess-peak-shaving:24 | §5.11 | coberta |
| `cycles.*.trip_after` | 2 | 03-manifest:229, 08-stress-tests:341 | §5.11 | coberta |
| `devices` | 32 | 02-signal-model:179, 03-manifest:113, 03-manifest:159 (+29) | §5.8 | coberta |
| `devices.*` | 32 | 02-signal-model:179, 03-manifest:113, 03-manifest:159 (+29) | §5.8 | coberta |
| `devices.*.actions` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | coberta |
| `devices.*.actions.*` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | coberta |
| `devices.*.actions.*.permit` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | coberta |
| `devices.*.actions.*.role` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | coberta |
| `devices.*.actions.*.timeout` | 2 | 02-signal-model:179, 03-manifest:174 | §5.8.2 | coberta |
| `devices.*.address` | 8 | 03-manifest:113, 08-stress-tests:23, 08-stress-tests:341 (+5) | anexo A | coberta |
| `devices.*.align` | 1 | examples/01-bess-peak-shaving:103 | §5.8 | coberta |
| `devices.*.auth` | 2 | 23-development-bench:304, track/09-joker-micro:199 | anexo A | coberta |
| `devices.*.bus` | 11 | 03-manifest:113, 08-stress-tests:253, 08-stress-tests:341 (+8) | §5.8 | coberta |
| `devices.*.bus.unset` | 1 | track/10-logic-on-the-mcu:236 | §7.2 | coberta: diretiva de overlay |
| `devices.*.chip` | 3 | 08-stress-tests:158, 08-stress-tests:444, examples/02-visual-inspection:35 | anexo A | coberta |
| `devices.*.config` | 1 | 03-manifest:113 | anexo A | coberta |
| `devices.*.device` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.discover` | 3 | 03-manifest:113, 08-stress-tests:23, examples/01-bess-peak-shaving:103 | §5.8 | coberta |
| `devices.*.drdy` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | anexo A | coberta (1.1) |
| `devices.*.driver` | 27 | 03-manifest:113, 08-stress-tests:23, 08-stress-tests:158 (+24) | §5.8 | coberta |
| `devices.*.eds` | 1 | 03-manifest:113 | anexo A | coberta |
| `devices.*.fallback` | 3 | 03-manifest:113, 08-stress-tests:23, examples/01-bess-peak-shaving:103 | §5.8 | coberta |
| `devices.*.fallback.configure` | 3 | 03-manifest:113, 08-stress-tests:23, examples/01-bess-peak-shaving:103 | §5.8 | coberta |
| `devices.*.fallback.keepalive` | 1 | 03-manifest:113 | §5.8 | coberta |
| `devices.*.format` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.format.fps` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.format.height` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.format.pixfmt` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.format.width` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.micro` | 1 | track/09-joker-micro:199 | anexo A | coberta |
| `devices.*.model` | 4 | 03-manifest:113, examples/00-first-project:41, track/01-hello-bench:97 (+1) | anexo A | coberta |
| `devices.*.network` | 1 | 08-stress-tests:518 | anexo A | coberta |
| `devices.*.node` | 1 | track/09-joker-micro:199 | anexo A | coberta |
| `devices.*.node_id` | 1 | 03-manifest:113 | anexo A | coberta |
| `devices.*.poll` | 4 | 03-manifest:113, 08-stress-tests:23, 23-development-bench:238 (+1) | §5.8 | coberta |
| `devices.*.position` | 1 | 08-stress-tests:341 | anexo A | coberta |
| `devices.*.priority` | 1 | 08-stress-tests:518 | anexo A | coberta |
| `devices.*.profile` | 16 | 03-manifest:113, 08-stress-tests:23, 08-stress-tests:253 (+13) | §5.8 | coberta |
| `devices.*.profile.unset` | 2 | 23-development-bench:304, track/09-joker-micro:199 | §7.2 | coberta: diretiva de overlay |
| `devices.*.rate` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | anexo A | coberta (1.1) |
| `devices.*.refresh` | 3 | 23-development-bench:304, track/09-joker-micro:199, track/10-logic-on-the-mcu:236 | anexo A | coberta |
| `devices.*.report` | 3 | 23-development-bench:304, track/09-joker-micro:199, track/10-logic-on-the-mcu:236 | anexo A | coberta |
| `devices.*.retries` | 1 | 03-manifest:113 | §5.8 | coberta |
| `devices.*.signals` | 16 | 03-manifest:159, 08-stress-tests:23, 08-stress-tests:158 (+13) | §5.8.1 | coberta |
| `devices.*.signals.*` | 16 | 03-manifest:159, 08-stress-tests:23, 08-stress-tests:158 (+13) | §5.8.1 | coberta |
| `devices.*.signals.*.active_low` | 1 | track/09-joker-micro:48 | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.alias` | 11 | 03-manifest:159, 08-stress-tests:23, 08-stress-tests:158 (+8) | §5.8.1 | coberta |
| `devices.*.signals.*.bias` | 1 | track/09-joker-micro:48 | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.block` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.8.1 | coberta (1.1) |
| `devices.*.signals.*.deadband` | 1 | track/09-joker-micro:48 | §5.8.1 | coberta |
| `devices.*.signals.*.debounce` | 1 | track/09-joker-micro:48 | §5.8.1 | coberta |
| `devices.*.signals.*.direction` | 9 | 08-stress-tests:158, 08-stress-tests:444, 09-architecture-under-stress:308 (+6) | §5.8.1 | coberta |
| `devices.*.signals.*.edges` | 2 | 08-stress-tests:158, examples/02-visual-inspection:35 | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.kind` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.8.1 | coberta (1.1) |
| `devices.*.signals.*.led` | 4 | 23-development-bench:238, track/02-the-pi-as-a-node:165, track/03-temperature-sensor:300 (+1) | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.limits` | 1 | 03-manifest:159 | §5.8.1 | coberta |
| `devices.*.signals.*.line` | 5 | 08-stress-tests:158, 08-stress-tests:444, 09-architecture-under-stress:308 (+2) | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.max_age` | 1 | 03-manifest:159 | §5.8.1 | coberta |
| `devices.*.signals.*.record` | 1 | 03-manifest:159 | §5.8.1 | coberta |
| `devices.*.signals.*.record.every` | 1 | 03-manifest:159 | §5.8.1 | coberta |
| `devices.*.signals.*.rssi` | 1 | track/09-joker-micro:48 | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.scale` | 2 | 03-manifest:159, examples/00-first-project:194 | §5.8.1 | coberta |
| `devices.*.signals.*.scale.from` | 2 | 03-manifest:159, examples/00-first-project:194 | §5.8.1 | coberta |
| `devices.*.signals.*.scale.to` | 2 | 03-manifest:159, examples/00-first-project:194 | §5.8.1 | coberta |
| `devices.*.signals.*.stuck_detect` | 1 | 03-manifest:159 | §5.8.1 | coberta |
| `devices.*.signals.*.thermal` | 3 | 23-development-bench:238, track/03-temperature-sensor:300, track/06-switching-boards:19 | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.timestamp` | 2 | 08-stress-tests:158, examples/02-visual-inspection:35 | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.touch` | 1 | track/09-joker-micro:48 | §5.8.1, anexo A | coberta |
| `devices.*.signals.*.unit` | 3 | 08-stress-tests:253, examples/03-predictive-maintenance:11, track/09-joker-micro:48 | §5.8.1 | coberta |
| `devices.*.signals.*.uptime` | 1 | track/09-joker-micro:48 | §5.8.1, anexo A | coberta |
| `devices.*.timeout` | 1 | 03-manifest:113 | §5.8 | coberta |
| `devices.*.trigger` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.trigger.input` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.trigger.kind` | 3 | 03-manifest:113, 08-stress-tests:158, examples/02-visual-inspection:35 | anexo A | coberta (1.1) |
| `devices.*.unit_id` | 12 | 03-manifest:113, 08-stress-tests:444, 14-classes-and-instances:168 (+9) | anexo A | coberta |
| `devices.*.unit_id.unset` | 2 | 23-development-bench:304, track/09-joker-micro:199 | §7.2 | coberta: diretiva de overlay |
| `drivers` | 2 | 08-stress-tests:341, 08-stress-tests:518 | §5.7 | coberta |
| `drivers.*` | 2 | 08-stress-tests:341, 08-stress-tests:518 | §5.7 | coberta |
| `drivers.*.exec` | 2 | 08-stress-tests:341, 08-stress-tests:518 | §5.7 | coberta |
| `drivers.*.tier` | 2 | 08-stress-tests:341, 08-stress-tests:518 | §5.7 | coberta |
| `extends` | 12 | 03-manifest:33, 23-development-bench:238, 23-development-bench:255 (+9) | §5.1 | coberta |
| `instances` | 7 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:425 (+4) | §5.13 | coberta |
| `instances.*` | 7 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:425 (+4) | §5.13 | coberta |
| `instances.*.actions` | 1 | 17-jkc:228 | §5.13 | coberta |
| `instances.*.actions.*` | 1 | 17-jkc:228 | §5.13 | coberta |
| `instances.*.actions.*.role` | 1 | 17-jkc:228 | §5.13 | coberta |
| `instances.*.capabilities` | 3 | 03-manifest:352, 14-classes-and-instances:181, 17-jkc:228 | §5.13 | coberta |
| `instances.*.class` | 7 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:425 (+4) | §5.13 | coberta |
| `instances.*.implemented_by` | 5 | 03-manifest:352, 14-classes-and-instances:425, 14-classes-and-instances:533 (+2) | §5.13 | coberta |
| `instances.*.implemented_by.app` | 2 | 14-classes-and-instances:533, 17-jkc:228 | §5.13 | coberta |
| `instances.*.implemented_by.bind` | 2 | 14-classes-and-instances:533, 17-jkc:228 | §5.13 | coberta |
| `instances.*.implemented_by.bind.*` | 2 | 14-classes-and-instances:533, 17-jkc:228 | §5.13 | coberta |
| `instances.*.map` | 5 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:533 (+2) | §5.13 | coberta |
| `instances.*.map.*` | 5 | 03-manifest:352, 14-classes-and-instances:181, 14-classes-and-instances:533 (+2) | §5.13 | coberta |
| `link` | 2 | track/09-joker-micro:48, track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `link.address` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `link.auth` | 2 | track/09-joker-micro:48, track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `link.bus` | 1 | track/09-joker-micro:48 | §5.22 | coberta (só Joker Micro) |
| `link.peer` | 2 | track/09-joker-micro:48, track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `models` | 7 | 03-manifest:298, 06-ml:220, 08-stress-tests:23 (+4) | §6.1 | coberta (1.1) |
| `models.*` | 7 | 03-manifest:298, 06-ml:220, 08-stress-tests:23 (+4) | §6.1 | coberta (1.1) |
| `models.*.backend` | 6 | 03-manifest:298, 08-stress-tests:23, 08-stress-tests:158 (+3) | §6.1 | coberta (1.1) |
| `models.*.deadline` | 6 | 03-manifest:298, 08-stress-tests:23, 08-stress-tests:158 (+3) | §6.1 | coberta (1.1) |
| `models.*.isolation` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.1 | coberta (1.1) |
| `models.*.resources` | 6 | 03-manifest:298, 08-stress-tests:23, 08-stress-tests:158 (+3) | §6.1 | coberta (1.1) |
| `models.*.shadow` | 2 | 06-ml:220, examples/03-predictive-maintenance:11 | §6.1 | coberta (1.1) |
| `models.*.shadow.auto` | 1 | examples/03-predictive-maintenance:11 | §6.1 | coberta (1.1) |
| `models.*.shadow.candidate` | 1 | 06-ml:220 | §6.1 | coberta (1.1) |
| `models.*.shadow.kpi` | 1 | 06-ml:220 | §6.1 | coberta (1.1) |
| `models.*.shadow.on_regress` | 1 | 06-ml:220 | §6.1 | coberta (1.1) |
| `models.*.shadow.promote_if` | 2 | 06-ml:220, examples/03-predictive-maintenance:11 | §6.1 | coberta (1.1) |
| `models.*.shadow.until` | 2 | 06-ml:220, examples/03-predictive-maintenance:11 | §6.1 | coberta (1.1) |
| `models.*.shadow.until.duration` | 2 | 06-ml:220, examples/03-predictive-maintenance:11 | §6.1 | coberta (1.1) |
| `models.*.shadow.until.frames` | 1 | 06-ml:220 | §6.1 | coberta (1.1) |
| `models.*.source` | 6 | 03-manifest:298, 08-stress-tests:23, 08-stress-tests:158 (+3) | §6.1 | coberta (1.1) |
| `models.*.warmup` | 2 | 03-manifest:298, examples/02-visual-inspection:35 | §6.1 | coberta (1.1) |
| `net` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `net.wifi` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `net.wifi.country` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `net.wifi.password` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `net.wifi.ssid` | 1 | track/10-logic-on-the-mcu:222 | §5.22 | coberta (só Joker Micro) |
| `outputs` | 20 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:158 (+17) | §5.10 | coberta |
| `outputs.*` | 20 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:158 (+17) | §5.10 | coberta |
| `outputs.*.accept_missing_fallback` | 2 | examples/02-visual-inspection:35, track/11-production-and-fleet:40 | §5.10 | coberta |
| `outputs.*.deadband` | 1 | 03-manifest:189 | §5.10 | coberta |
| `outputs.*.grace` | 2 | 03-manifest:189, examples/01-bess-peak-shaving:24 | §5.10 | coberta |
| `outputs.*.group` | 2 | 03-manifest:189, track/09-joker-micro:48 | §5.10 | coberta |
| `outputs.*.interlock` | 7 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:341 (+4) | §5.10 | coberta |
| `outputs.*.max_actuations` | 3 | 03-manifest:189, 14-classes-and-instances:425, examples/02-visual-inspection:35 | §5.10 | coberta |
| `outputs.*.max_lease` | 5 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:444 (+2) | §5.10 | coberta |
| `outputs.*.max_manual` | 2 | 03-manifest:189, track/11-production-and-fleet:40 | §5.10 | coberta |
| `outputs.*.min_interval` | 1 | 03-manifest:189 | §5.10 | coberta |
| `outputs.*.on_interlock` | 1 | 03-manifest:189 | §5.10 | coberta |
| `outputs.*.on_late` | 3 | 03-manifest:189, 08-stress-tests:158, examples/02-visual-inspection:35 | §5.10 | coberta (1.1) |
| `outputs.*.on_range` | 1 | 03-manifest:189 | §5.10 | coberta |
| `outputs.*.pulse` | 3 | 03-manifest:189, 08-stress-tests:158, examples/02-visual-inspection:35 | §5.10 | coberta |
| `outputs.*.range` | 7 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:341 (+4) | §5.10 | coberta |
| `outputs.*.rate` | 6 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:341 (+3) | §5.10 | coberta |
| `outputs.*.remote_levels` | 1 | track/09-joker-micro:48 | §5.22 | coberta (só Joker Micro) |
| `outputs.*.requires_good` | 4 | 03-manifest:189, 08-stress-tests:23, examples/00-first-project:41 (+1) | §5.10 | coberta |
| `outputs.*.reset_when` | 1 | 03-manifest:189 | §5.10 | coberta |
| `outputs.*.safe` | 17 | 03-manifest:189, 08-stress-tests:23, 08-stress-tests:158 (+14) | §5.10 | coberta |
| `outputs.*.safe_ramp` | 2 | 03-manifest:189, 08-stress-tests:341 | §5.10 | coberta |
| `outputs.*.safer` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | coberta |
| `outputs.*.schedule_depth` | 3 | 03-manifest:189, 08-stress-tests:158, examples/02-visual-inspection:35 | §5.10 | coberta (1.1) |
| `outputs.*.timed` | 3 | 03-manifest:189, 08-stress-tests:158, examples/02-visual-inspection:35 | §5.10 | coberta (1.1) |
| `outputs.*.unset` | 2 | 23-development-bench:304, track/09-joker-micro:246 | §7.2 | coberta: diretiva de overlay |
| `outputs.*.verify` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | coberta |
| `outputs.*.verify.readback` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | coberta |
| `outputs.*.verify.tolerance` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | coberta |
| `outputs.*.verify.within` | 3 | 03-manifest:189, 08-stress-tests:23, examples/01-bess-peak-shaving:24 | §5.10 | coberta |
| `pipelines` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | coberta (1.1) |
| `pipelines.*` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | coberta (1.1) |
| `pipelines.*.deadline` | 2 | 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | coberta (1.1) |
| `pipelines.*.from` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | coberta (1.1) |
| `pipelines.*.steps` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[]` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].edges_hz` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].expr` | 1 | examples/03-predictive-maintenance:11 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].field` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].fields` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].hop` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].iou` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].keep_aspect` | 1 | 03-manifest:298 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].max` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].mean` | 1 | 03-manifest:298 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].n` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].name` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].op` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].roi` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].score` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].signals` | 1 | examples/03-predictive-maintenance:11 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].size` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].std` | 1 | 03-manifest:298 | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].window` | 4 | 08-stress-tests:158, 08-stress-tests:253, examples/02-visual-inspection:35 (+1) | §6.3 | coberta (1.1) |
| `pipelines.*.steps.[].with` | 2 | 08-stress-tests:158, examples/02-visual-inspection:35 | §6.3 | coberta (1.1) |
| `pipelines.*.to` | 5 | 03-manifest:298, 08-stress-tests:158, 08-stress-tests:253 (+2) | §6.3 | coberta (1.1) |
| `project` | 13 | 03-manifest:53, 08-stress-tests:444, 09-architecture-under-stress:308 (+10) | §5.2 | coberta |
| `project.joker` | 9 | 03-manifest:53, 23-development-bench:195, examples/01-bess-peak-shaving:24 (+6) | §5.2 | coberta |
| `project.mode` | 10 | 03-manifest:53, 09-architecture-under-stress:308, 23-development-bench:195 (+7) | §5.2 | coberta |
| `project.name` | 12 | 03-manifest:53, 08-stress-tests:444, 09-architecture-under-stress:308 (+9) | §5.2 | coberta |
| `resources` | 7 | 03-manifest:53, 23-development-bench:255, examples/01-bess-peak-shaving:24 (+4) | §5.4 | coberta |
| `resources.*` | 7 | 03-manifest:53, 23-development-bench:255, examples/01-bess-peak-shaving:24 (+4) | §5.4 | coberta |
| `resources.*.accel` | 1 | examples/02-visual-inspection:35 | §5.4 | coberta (1.1) |
| `resources.*.cpus` | 7 | 03-manifest:53, 23-development-bench:255, examples/01-bess-peak-shaving:24 (+4) | §5.4 | coberta |
| `resources.*.memory` | 3 | 03-manifest:53, examples/01-bess-peak-shaving:24, examples/03-predictive-maintenance:11 | §5.4 | coberta |
| `security` | 5 | 03-manifest:445, track/02-the-pi-as-a-node:165, track/06-switching-boards:31 (+2) | §5.19 | coberta |
| `security.api` | 4 | 03-manifest:445, track/02-the-pi-as-a-node:165, track/06-switching-boards:31 (+1) | §5.19 | coberta |
| `security.api.listen` | 4 | 03-manifest:445, track/02-the-pi-as-a-node:165, track/06-switching-boards:31 (+1) | §5.19 | coberta |
| `security.roles` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.20 | coberta |
| `security.roles.*` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.20 | coberta |
| `security.trust` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.19 | coberta |
| `security.trust.apps` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.19 | coberta |
| `security.trust.config` | 2 | 03-manifest:445, track/11-production-and-fleet:112 | §5.19 | coberta; só restringe o bundle do nó (C9, resolvido: D7) |
| `security.trust.secrets` | 1 | track/11-production-and-fleet:112 | §5.19 | coberta |
| `signals` | 3 | track/04-esp32-modbus:226, track/05-touch-and-faults:35, track/05-touch-and-faults:63 | §5.9 | coberta |
| `signals.*` | 3 | track/04-esp32-modbus:226, track/05-touch-and-faults:35, track/05-touch-and-faults:63 | §5.9 | coberta |
| `signals.*.debounce` | 1 | track/04-esp32-modbus:226 | §5.9 | coberta |
| `signals.*.stuck_detect` | 2 | track/05-touch-and-faults:35, track/05-touch-and-faults:63 | §5.9 | coberta |
| `streams` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.2 | coberta (1.1) |
| `streams.*` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.2 | coberta (1.1) |
| `streams.*.mode` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.2 | coberta (1.1) |
| `streams.*.pool` | 3 | 03-manifest:298, 08-stress-tests:158, examples/02-visual-inspection:35 | §6.2 | coberta (1.1) |
| `streams.*.record` | 1 | examples/02-visual-inspection:35 | §6.2 | coberta (1.1) |
| `target` | 13 | 03-manifest:53, 08-stress-tests:23, 23-development-bench:255 (+10) | §5.3 | coberta |
| `target.arch` | 1 | 03-manifest:53 | §5.3 | coberta |
| `target.board` | 12 | 03-manifest:53, 08-stress-tests:23, 23-development-bench:255 (+9) | §5.3 | coberta |
| `target.kernel` | 5 | 03-manifest:53, 08-stress-tests:23, examples/01-bess-peak-shaving:24 (+2) | §5.3 | coberta |
| `telemetry` | 7 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+4) | §5.17 | coberta |
| `telemetry.record` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | coberta |
| `telemetry.record.*` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | coberta |
| `telemetry.record.*.blocks` | 3 | 03-manifest:394, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.17.1 | coberta (1.1) |
| `telemetry.record.*.every` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | coberta |
| `telemetry.record.*.keep` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | coberta |
| `telemetry.record.*.keep.*` | 6 | 03-manifest:394, 08-stress-tests:253, examples/01-bess-peak-shaving:24 (+3) | §5.17.1 | coberta |
| `telemetry.record.*.on_change` | 2 | 03-manifest:394, examples/01-bess-peak-shaving:24 | §5.17.1 | coberta |
| `telemetry.record.*.post` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.17.1 | coberta (1.1) |
| `telemetry.record.*.pre` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.17.1 | coberta (1.1) |
| `telemetry.record.*.trigger` | 2 | 08-stress-tests:253, examples/03-predictive-maintenance:11 | §5.17.1 | coberta (1.1) |
| `telemetry.store` | 1 | 03-manifest:394 | §5.17.2 | coberta |
| `telemetry.store.fsync` | 1 | 03-manifest:394 | §5.17.2 | coberta |
| `telemetry.store.max_disk` | 1 | 03-manifest:394 | §5.17.2 | coberta |
| `telemetry.uplink` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*.broker` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*.budget` | 2 | 03-manifest:394, examples/01-bess-peak-shaving:24 | §5.17.3 | coberta |
| `telemetry.uplink.*.classes` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*.classes.*` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*.group_id` | 2 | 03-manifest:394, examples/01-bess-peak-shaving:24 | §5.17.3 | coberta |
| `telemetry.uplink.*.history` | 1 | 03-manifest:394 | §5.17.3 | coberta |
| `telemetry.uplink.*.history.delivery` | 1 | 03-manifest:394 | §5.17.3 | coberta |
| `telemetry.uplink.*.kind` | 3 | 03-manifest:394, examples/01-bess-peak-shaving:24, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*.live` | 2 | 03-manifest:394, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*.live.every` | 2 | 03-manifest:394, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*.node_id` | 1 | 03-manifest:394 | §5.17.3 | coberta |
| `telemetry.uplink.*.password` | 2 | 03-manifest:394, track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `telemetry.uplink.*.username` | 1 | track/11-production-and-fleet:127 | §5.17.3 | coberta |
| `training` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.arch` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.budget` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.budget.max_memory` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.budget.max_time` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.checkpoint` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.data` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.data.signals` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.data.where` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.data.window` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.from` | 1 | 06-ml:197 | §6.4 | coberta (1.1) |
| `training.*.hyper` | 1 | examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.hyper.epochs` | 1 | examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1): chave livre de `hyper` |
| `training.*.hyper.layers` | 1 | examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1): chave livre de `hyper` |
| `training.*.hyper.lr` | 1 | examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1): chave livre de `hyper` |
| `training.*.output` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.priority` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.resources` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.schedule` | 3 | 06-ml:197, 08-stress-tests:253, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.validate` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.validate.holdout` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.validate.max` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.validate.metric` | 2 | 06-ml:197, examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |
| `training.*.validate.threshold` | 1 | examples/03-predictive-maintenance:11 | §6.4 | coberta (1.1) |

**Chaves referidas só em texto** (procuradas com um segundo script, fora dos blocos de código):

| Chave | Onde | Estado |
|---|---|---|
| `accept_missing_fallback` | 01 §8, 03 §14, trilha 02 (também em blocos: exemplo 02, trilha 11) | §5.10 (no manifesto) e lock (`ref/02`) |
| `align` | 03 §9.1, 08 R1 | §5.8 |
| `batch`, `max_inflight` | 06 §4 | §6.1 (1.1) |
| `dedup_window` | 02 §8, 16 E9 | §5.8.2 |
| `period_tolerance` | 04 §3.2 | §5.12 |
| `permit_uncertain` | 02 §4, 03 §9.1, 16 §3.2 | §5.10 |
| `trace.async` | 03 §9.1, 09 A10 | §5.11 |
| `record = false` (streams) | 03 §9.1, 08 R2 | §6.2 (1.1) |
| `keep_previous` | 01 §16 | §5.12 |
| `health_window` | 01 §16 | configuração do nó (`ref/02`): é da runtime, não do site |
| `kind = "native"` | 01 §4, 04 §5 | §5.12 |
| `memory.max` | 01 §9, 09 A3 | §5.12 (`cycle_memory`, `async_memory`) |
| `margin` (de `on_late`) | 02 §7.2 | §5.10 (`late_margin`, 1.1) |
| `record_disagreements` | exemplo 02 §5 | §6.1 (1.1) |
| `optional` (tarefas, portas) | 01 §4, 03 §8 | declarado no código (`joker:meta`, `ref/04`); `bind.optional` no §5.12.1 |
| `relinquish = { … }`, `coherent`, `quirks`, `implements` | 03 §6.1, 05 §6 | perfis (`ref/03`) |
| `bounce`, `panel`, `gen`, `noise` | 04 §6, trilha 1, 3, 4, 5 | modelos de simulação (`ref/03`) |
| `halt_if` | 20 D2 | regra de rollout do Deck (`ref/10`), não do manifesto |
| `--skip-shadow` | ADR 0012 | opção da CLI |


---

## Anexo C — Achados: mudanças propostas noutros documentos

| # | Documento, secção | Problema | Mudança proposta |
|---|---|---|---|
| C1 | 03 §11 | `classes = { alarms = "**alarm.**", … }`: `**` dentro de um segmento não é um seletor válido | `alarms = "alarm.**"` |
| C2 | 08, rodada 5 (`sites/joker.hb-01234.toml`) | `name = "HB-01234"` viola a gramática de nome de site (minúsculas) | `name = "hb-01234"`, e o ficheiro `sites/joker.hb-01234.toml` (a variante de um overlay segue a mesma regra de minúsculas, §7.7) |
| C3 | 02 §8 | `kind = "modbus.write"` em `[devices.pcs.actions.reset_fault]` não é chave do manifesto (a implementação é do perfil) | retirar a linha; a nota "implementado pelo perfil" passa para o texto |
| C4 | trilha antiga, parte 4, §4 (`joker.rpi.toml`) | `config = { pga = "4.096 V", rate = "128 SPS" }`: `SPS` não está no catálogo de unidades | **Substituído pela `ref/03` A20:** `rate` é um `choice` do `[config]` do perfil `ti/ads1115@1`, e `"128 SPS"` é um rótulo, e não uma quantidade (`ref/03` §2.2.10); a trilha fica como está, com um comentário a dizê-lo |
| C5 | 08 R3, exemplo 03 §1 | `spi = "/dev/spidev0.0"` no dispositivo: o manifesto espera um barramento (a mesma correção do I²C, documento 18, P7) | `[buses.spi0] kind = "spi"`, `dev = "/dev/spidev0.0"`, e `bus = "spi0"` no dispositivo |
| C6 | 03 §8 | "grupo implícito `default` com o período pedido pela app" contradiz 16 A11 (`default` = 100 ms) | "num grupo implícito `default_<período>` com o período pedido pela app; o grupo `default` (100 ms) é o de A11" |
| C7 | exemplo 01 (template), 08 R1 | `forecast = "model:load_forecast"` e `[models.load_forecast]` são 1.1 (ADR 0019): o exemplo não passa numa runtime 1.0 | marcar os blocos "requer 1.1", ou retirar a ligação do modelo e declarar a porta `forecast` como `optional` |
| C8 | 03 §7 | "as não listadas correm depois, pela ordem do manifesto" é indefinido com overlays | "por ordem lexicográfica de `<app>.<tarefa>`" |
| C9 | 03 §13 | `trust.runtime` e `trust.config` no manifesto: a configuração declara a sua própria raiz de confiança | **Resolvido: D7 (ADR 0025).** "`[security.trust]` só restringe o bundle de confiança do nó (`ref/02` §2.5.4): listas de `.pub` por papel e `revoked`; nunca acrescenta chaves" |
| C10 | `ref/00` §9 | "escritas vindas de fora do nó (…, pontes, …) levam `nonce` e `ts`": o Modbus e o OPC UA não os transportam | **Resolvido: D16.** O `ref/00` §9 já diz que as pontes se autenticam pelo protocolo delas, com nível e lease, e não levam nonce; esta referência remete para lá (§5.18) |
| C11 | 03 §12 | `subscribe` do ROS 2 não diz o tipo da mensagem nem que campo vai para que sinal | acrescentar `type` e `fields` (§5.18) |
| C12 | exemplo 03 §1 | `source = "state:training/ae_p1/current.jkm"` aponta para um ficheiro interno do nó (documento 15, §2.5) | `source = "training:ae_p1"` |
| C13 | 08 R6 | `fallback.when` com janela de 30 min: acima do máximo das expressões de controlo (`J0305`) | definir `fallback.when` na gramática KPI (§6.5), avaliada em `bg`, como aqui |
| C14 | 09 §12.2 | o manifesto mínimo não declara `mode`; com a omissão `prod`, o GPIO sem fallback é erro (`J0205`) | acrescentar `mode = "dev"` ao `[project]` do exemplo |
| C15 | 01 §6, item 5 | "TTL obrigatório (por omissão 8 h…)" mistura o TTL (obrigatório, sem omissão; A10) com o seu máximo | "TTL obrigatório em cada escrita; o máximo é `max_manual` (omissão 8 h)" |
| C16 | 05 §11 (perfil de placa) | `relay = { …, safe = false }` num perfil de placa: o `safe` só vive no manifesto (ADR 0006) | **Resolvido pela `ref/03` A6:** `fallback = false`. O que a placa sabe é o estado que o hardware impõe sozinho, e isso é um fallback do dispositivo (M1, §5.8), e não uma sugestão de `safe` |
| C17 | 16 §10 | "base dos caminhos de `extends`" e "definição exata de só apertar" estavam por decidir | apontar para esta referência, §7.1 e §7.4 |
| C18 | 03 §9.1 | `record = false` em `[streams.*]` é a omissão, mas a tabela não diz que é 1.1 | marcar a linha como 1.1 |
| C19 | 03 §6, §8 | `schedule_depth`, `timed`, `on_late` e `writer.kind = "proposal"` aparecem sem marca de versão, e são 1.1 (ADR 0019) | marcar como 1.1 |
| C20 | trilha antiga, parte 3, §8 | o texto diz que o LED não volta a acender sozinho depois da falha, e o cenário espera `equals = true` aos 3,6 s | corrigir o texto: volta a acender, porque a app volta a escrever o seu estado |
| C21 | 08 R2, R4, R6 | `source = "apps/inspector"` (e `apps/diff-base`, `apps/ops-assistant`): `source` é para artefactos OCI | `path = "apps/…"` |
| C22 | 03 §13 | `api = { …, mtls = true }`: com TCP sempre em mTLS, a chave só serve para errar | retirar `mtls` (§5.19); `J0291` retirado |
| C23 | 03 §8, 01 §17 | `secrets = ["…"]` como lista de nomes | **D6:** tabela `[apps.<app>.secrets] <nome_local> = "secret:<nome>"` (§5.12, `ref/02` §2.4.7) |
| C24 | 03 §4 | "um barramento serializa o acesso" sem exceções | só `serial`, `i2c`, `spi` e `ethernet` com `bus` declarado; `can` e TCP sem `bus` não serializam (§5.6, `ref/06` §2.14) |
