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

### 5.9 `[signals."<path>"]`

Adjustments independent of the device that provides the signal. They serve the common manifest of a sim/hardware pair of overlays (track, parts 4–5):

```toml
[signals."painel.botao"]
debounce = "20ms"

[signals."painel.pot"]
deadband = "0.01 V"
```

- The key is a path (canonical, alias or instance member) or a selector (`J0155` if it matches nothing).
- Accepted keys: `debounce`, `deadband`, `max_age`, `limits`, `stuck_detect`, `record`, with the rules of §5.8.1. Any other is `J0140`.
- **One adjustment per key and per signal.** If `[signals]` and `[devices.*.signals]`, or two entries of `[signals]` (through selectors), define the same key for the same signal with **different** values, it is `J0175`. With the same value, it is accepted: that is the case of a common manifest and an overlay that declare the same `debounce`.
- On derived signals (`calc.*`, `app.*`, `alarm.*`), only `record` applies (`J0176`).

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
| `safer` | `"lower"` \| `"higher"` \| `"equal"` | no | `"equal"` | how to compare the equipment fallback with `safe` (M1, `J0205`); with `relinquish` it does not apply; without a device fallback it is ignored (`J0238`, warning) | 03 §6 | 1.0 |
| `safe_ramp` | `Rate<S>` | no | — (going to `safe` is immediate) | `J0202` | 03 §6 | 1.0 |
| `interlock` | `Expr<bool>` | no | — (always permitted) | step 2; `Bad` counts as false; `Uncertain` too, unless `permit_uncertain` | 03 §6 | 1.0 |
| `permit_uncertain` | `Bool` | no | `false` | applies to `interlock` and to the `range` limits with expressions (E6) | 03 §9.1 | 1.0 |
| `on_interlock` | `"safe"` \| `"hold"` | no | `"safe"` | 16 E3; also applies to `requires_good` (16 E4) | 03 §6 | 1.0 |
| `requires_good` | `List<Path \| Selector>` (set) | no | `[]` | step 3; unknown path `J0101` | 03 §6 | 1.0 |
| `grace` | `Duration` | no | `"0s"` | 16 E4; accepts zero | 03 §6 | 1.0 |
| `range` | `[Limit, Limit]`, with `Limit` = `Quantity<S>` or `Expr<number>` | no | — (no range step) | two elements, minimum < maximum when both are constant (`J0230`); dimension `J0107`; expressions E6; on a `bool` or `enum` `J0233` | 03 §6 | 1.0 |
| `on_range` | `"clamp"` \| `"reject"` \| `"trip"` | no | `"clamp"` | step 4 | 03 §6 | 1.0 |
| `rate` | `Rate<S>` | no | — | step 5; relative to the last applied value (16 E2); `J0202`; on a `bool` or `enum` `J0233` | 03 §6 | 1.0 |
| `deadband` | `Quantity` (difference) | no | — | step 6; on a `bool` or `enum` `J0233` | 03 §6 | 1.0 |
| `min_interval` | `Duration` | no | `"0s"` | step 6 | 03 §6 | 1.0 |
| `max_actuations` | `Budget` | no | — | step 6: an actuation is an applied write with a value different from the previous one; `Suppressed` and keepalive do not count (16 E8); exhausted → `Rejected(Budget)` and alarm `joker.output.<s>.budget`; syntax `J0159` | 03 §6 | 1.0 |
| `max_lease` | `Duration` | no | `"15m"` | ≤ `"24h"` (`J0255`); lease of each writer ≤ `max_lease` (`J0213`) | 03 §6 | 1.0 |
| `max_manual` | `Duration` | no | `"8h"` | ≤ `"7d"` (`J0255`); limit of the TTL of manual writes (16 A10); 15 min before the end of a TTL, alarm `joker.output.<s>.manual_expiring`; a manual level restored at startup without a `synced` clock, alarm `joker.output.<s>.manual_restored` (16 A14; both `medium`, §5.15) | 03 §6 | 1.0 |
| `verify` | table | no | — | below | 03 §6 | 1.0 |
| `reset_when` | `Expr<bool>` | no | — (only `jk reset`) | 16 E7; on an output without `on_range = "trip"` and without an alarm with `trip` on it, it is `J0236` (warning) | 03 §6 | 1.0 |
| `group` | `Name` from `[cycles]` | no | the A11 rule | nonexistent group `J0237` | 03 §6 | 1.0 |
| `keepalive` | `Duration` | no | the device's `fallback.keepalive` | only on outputs of devices with a fallback; `J0276` | 01 §8, new | 1.0 |
| `pulse` | `Duration` | no | — | only `bool` (`J0233`); after applying a value ≠ `safe`, the output returns to `safe` once `pulse` has elapsed | 03 §6 | 1.0 |
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
| `verify.readback` | `Path` of an input | yes | — | must be an input, with the same dimension (`J0235`) |
| `verify.tolerance` | `Quantity` (difference) | cond. | — (exact equality) | required on `f64`/`f32` outputs (`J0235`) |
| `verify.within` | `Duration` | no | 2 × `poll` of the `readback` device | ≥ one `poll` of that device (`J0235`) |

An output that does not confirm becomes `Uncertain(NotConfirmed)` and fires `joker.output.<s>.unconfirmed` (document 01, §7).

**Rules that involve several keys:**

- A key that is not an output path (direction ≠ `output`) is `J0234`. The outputs of a `joker-link` device are governed by the envelope of the Joker Micro: declaring them here too is `J0234` (document 19, §6).
- The virtual commands of an instance implemented by an app (`p1.run_cmd`, `luz.mode`) are outputs like the others and need an envelope (document 14, §7.2).
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
| `"<instance>"` | class (`#[device] led: Led`) | `led = "painel_led"` | class and major (`J0630`); requested capabilities (`J0602`) |
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

#### 5.12.3 `shadow` (M16, apps)

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
| `sim` | `Bool` or `Table` (parameters of the class simulator, `ref/03`) | no | `false` | only with the `sim` driver available; in `prod` `J0145` | 03 §9.2 | 1.0 |
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
| `"<instance>"` | fixed class member (`member boot: std/button@1`) | `boot = "botao"` |
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
velocidade = { expr = "clamp(avg(painel.pot, 200ms) / 3.3 V, 0, 1)", group = "main" }
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
| `joker.output.<s>.budget` | actuation budget exhausted (`Rejected(Budget)`) | `outputs.*.max_actuations` | §5.10, 16 E8 |
| `joker.output.<s>.unconfirmed` | the output did not confirm through the `readback` | `outputs.*.verify` | §5.10, 01 §7 |
| `joker.output.<s>.manual_expiring` | 15 min before the end of the TTL of a manual write (`medium`) | `outputs.*.max_manual` | 01 §6, `ref/09` §5.6 |
| `joker.output.<s>.manual_restored` | manual level restored at startup without a `synced` clock (`medium`) | `outputs.*.max_manual` | 16 A14 |
| `joker.group.<g>.overrun` | `alarm_after` consecutive overruns | `cycles.*.alarm_after` | §5.11, 01 §4 |
| `joker.device.<d>.degraded`, `.offline`, `.drift` | link state of the device | `devices.*` | 05 §12, `ref/06` §2.12 |
| `joker.driver.<n>.crashloop` | W or X driver in crashloop (critical) | — (fixed threshold; the backoff is `drivers.*.restart`) | `ref/06` §4.5, §5.7 |
| `joker.node.config_invalid`, `joker.node.crl_expired` | invalid `node.toml`; expired CRL | — (node configuration) | `ref/02` §2.2.2, §3.2.1 |
| `joker.agent.*` | agent state | — (node configuration) | `ref/10` §2.18 |

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
| `fsync` | `"batch"` \| `"always"` \| `"never"` | no | `"batch"` | | 03 §11 | 1.0 |
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
| `kind` | `"mqtt"` \| `"mqtt-sparkplug"` \| `"zenoh"` \| `"otlp"` \| `"http"` | yes | — | `J0144` | 03 §11, 01 §13 | 1.0 |
| `classes` | `Table<class, Selector \| List<Selector>>` | no | `{ alarms = "alarm.**" }` | classes `alarms`, `events`, `high`, `low`, `bulk`, in this order of priority (document 01, §13); another one `J0198`; a signal goes in the highest-priority class that catches it | 03 §11 | 1.0 |
| `budget` | `DataVolume` | no | — (no budget) | M12; alarms are never degraded | 03 §11 | 1.0 |
| `live.every` | `Duration` | no | `"5s"` | live values, `latest` delivery | 03 §11 | 1.0 |
| `history.delivery` | `"at_least_once"` | no | `"at_least_once"` | store-and-forward from the historian; a sent signal that is not recorded only goes live (`jk check` note) | 03 §11 | 1.0 |
| `history.enabled` | `Bool` | no | `true` | | new | 1.0 |
| `username` | string | no | — | | new | 1.0 |
| `password`, `token` **(secret)** | `Ref` `secret:` | no | — | plaintext `J0294` | new | 1.0 |

The default of `classes` only sends alarms: nothing leaves the node without being asked for (document 12, §10).

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

### 5.18 `[bridges.<bridge>]` (M11)

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
| `kind` | `"opcua-server"` \| `"modbus-server"` \| `"zenoh-ros2"` | yes | — | | 03 §12 | 1.0 |
| `expose` | `List<Selector>` or, in `zenoh-ros2`, `Table<Path, topic \| { topic, type, fields }>` | no | `[]` | without matches `J0156` (warning); instance paths expose the whole instance (methods included, in OPC UA) | 03 §12 | 1.0 |
| `writer.level`, `writer.priority`, `writer.lease` | as in `[apps.*.writer]` | cond. | — | required if `writable` or some `to` is an output; `safety` band `J0253`; `manual` `J0252` | 03 §12 | 1.0 |
| `writer.writable` | `List<Path \| Selector>` (set) of outputs | no | `[]` (read-only) | only outputs with an envelope (`J0199`); without matches `J0155` | 03 §12 | 1.0 |

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
| `bus` | `Name` of a `serial` or `can` bus | cond. | — | exactly one of `bus` and `address` | 19 §6 |
| `addr` | `Integer` | no | `0` on a point-to-point serial connection, `1` on CAN | address **of the MCU** on the bus: 1–247 on a shared RS-485, 1–253 on CAN (`J0736`, `ref/07`); only with `bus` | `ref/07` §15 |
| `address` | `"udp:<host>:<port>"` \| `"tcp:<host>:<port>"` | cond. | — | only on M2 MCUs; `address` is always an IP address, and never the address on the bus. It is the address **of the Linux node** (the peer): with `tcp:`, the MCU connects to it; with `udp:`, it sends it the datagrams and only accepts those that come from it. On the Linux side, the `address` of the `joker-link` device is the opposite, the IP and port **of the MCU** (appendix A; `ref/07` §15) | 19 §6 |
| `peer` | `DnsName` of the Linux node, or `"deck"` | no | accepts the first authenticated peer | | new |
| `auth` | `"required"` \| `"none"` | no | `"required"` | `none` on an IP transport `J0179` | 19 U10 |

The form of the messages, the negotiation, the link key and the defaults of the timings are in `ref/07`.

**`[net.wifi]`** (only an M2 MCU on a board with `[net] wifi = true`, `ref/03` §2.3.3): the Wi-Fi network that the MCU connects to at boot. It is what makes the IP transports of an MCU possible: the `[link] address` (`udp:`/`tcp:`), the `[telemetry.uplink]` and the `rssi` signal of the `esp32` driver (appendix A). On a Linux target, the network belongs to the operating system, and `[net]` is `J0145`; on a board without Wi-Fi (`mcu_class = "m1"`, or `[net] wifi = false`), `J0828`.

```toml
# micro/esp/joker.toml (excerpt)
[net.wifi]
ssid     = "bancada"
password = "secret:wifi_bancada"
country  = "PT"
hostname = "esp-bancada"
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
