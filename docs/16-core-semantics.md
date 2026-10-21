# 16 — Core semantics: the reference for Phase 0

This document closes the ambiguities that blocked Phase 0 (document 18, §3). It is **normative** and takes precedence over the other documents in everything that concerns: arbiter, envelope, quality, expressions, units, aliases, trace format, conformance cases and code registry. Where an example in another document contradicts it, the example is wrong.

The common representation (versions, names, time, values, numeric codes, errors) is in `ref/00`, with the same precedence (ADR 0023). This document fixes the **semantics**; `ref/00` fixes the **form**.

---

## 1. Arbiter

| # | Rule |
|---|---|
| A1 | A **writer** is the pair (origin, level). The origin is an app (cycle instance and async instance count as **the same writer**), a user's CLI/API, a bridge, an active implementation or, on an output of an MCU, a link (A12). The text and binary forms are in `ref/00` §5. A write that arrives through a composite instance (a method of the composite, or a member through the nested path) belongs to the writer that made it, and not to the composition (P6). |
| A2 | Each proposal renews the lease of its writer: `lease_until = t_tick + lease`. The lease expires when `t_latch ≥ lease_until`, evaluated at the latch of the owning group. |
| A3 | Any proposal accepted by the arbiter renews the lease, **whatever the result in the envelope** (`Accepted`, `Clamped`, `Suppressed`, `Rejected(Interlock)`, `Rejected(Quality)`, `Rejected(ClockUnsynced)`…). Only `Rejected(NotOwner)` and `Rejected(WrongType)` do not renew. |
| A4 | Several proposals from the same writer on the same output in the same tick: **the last one wins**. The async proposals drained in step 2 come before those of the cycle tasks in step 4, and so the cycle one wins. |
| A5 | The effective value is that of the active level with the highest priority (1 = highest). There are no ties: two writers at the same level of the same output is `J0210`. At the manual levels, held at runtime by people, the rule applies through A13. |
| A6 | With no active level, the effective value is `safe`. |
| A7 | **The effective value goes through the envelope on every tick**, even if no one has proposed anything new. A rate ramp keeps advancing toward the effective value, tick by tick. |
| A8 | The **last applied value** at the startup of a generation is the `safe`. A new generation that keeps the output inherits the last applied value of the previous generation. |
| A9 | `lease` of each writer ≤ `max_lease` of the output (`J0213` if greater). Defaults: `lease` = 3 × period of the owning group; `max_lease` = 15 min. |
| A10 | Manual writes: `--ttl` is **required** on every write (CLI, API, HMI). `max_manual` (default 8 h) is the upper bound of the TTL. |
| A11 | The owning group of an output is, in order: the `group` declared on the output; the group of the highest-priority app that writes it in cycle; the implicit group `default` (100 ms), for outputs written only by async, bridges, CLI or implementations in async. |
| A12 | **Remote writers.** On an output of an MCU, the one that arbitrates and applies the envelope is the MCU (document 19, §6); the Linux node only proposes, over Joker Link. Each output delegates a set of levels to the link (default: those that no local writer of the MCU uses), and each (link, level) is a writer of the MCU. **Ownership of a delegated level belongs to the level**, and not to the origin on the Linux node: a new generation of the node, which renumbers the origins, holds the same level without `NotOwner`. A proposal at a non-delegated level is `Rejected(NotOwner)`. **The lease counts at the consumer**: `t_tick` in A2 is the tick of the MCU that consumes the proposal, and the link latency does not eat into it. The effective lease is `min(lease, max_lease of the link)`: the `max_lease` of the link (that of the session) is never greater than that of the output (`J0733`), and it is against it that `J0213` checks the `lease` of a remote writer. A remote manual write keeps the TTL on the Linux node, which sends it with `lease = min(remaining TTL, max_lease of the link)` and renews it. The MCU **does not persist** the remote manual levels: after a reset of the MCU, the level disappears until the Linux node renews it (at most one `renew`, `ref/07` §8.1); the persistence of A14 is that of the Linux node. The results come back through `OUTCOMES`: on the Linux node, the `last_outcome()` of a cycle task on an output of the MCU can lag by one or two ticks. Detail: `ref/07` §7.4 and §8.1. |
| A13 | **Holding the manual levels.** A manual level of an output is held by **one** principal. The same principal at the same level renews (replaces value and TTL). Another principal at the same level is refused, with who holds it, until when and why (`J-E1105`), unless it asks for `takeover`: in that case, the holder's *relinquish* and the new proposal enter the same inbox in order, and the decision log keeps both (`kind = takeover`). Releasing another principal's level requires `force` and is recorded in the audit. Detail: `ref/09` §5.6. |
| A14 | **Manual levels persist across restarts** of `jokerd`. Each accepted manual write records, with fsync (like trips), the output, the level, the principal, the value, the reason and the **deadline in UTC**. At startup, before the first tick, a level with a deadline in the future is restored; one with a past deadline is discarded (`kind = expiry`). If the clock is not `synced` at startup, UTC cannot be used to decide: the **remaining TTL of the last record** (what was left when it was recorded) is restored, counted from startup in monotonic time, and the alarm `joker.output.<s>.manual_restored` (severity `medium`) is raised, so that an operator confirms or releases it. A renewal rewrites the record. Write tickets do not survive the restart; levels do. On an output of an MCU, the record is that of the Linux node (A12): at the startup of `jokerd`, the restored level is sent again over the link with `lease = min(remaining TTL, max_lease of the link)`; the MCU does not record it. |

## 2. Envelope

| # | Rule |
|---|---|
| E1 | Order of the steps: that of document 01, §7. The first step that fails decides the result. **Step 0** is the clock step: on an output whose winning writer is an app with `requires_clock = "synced"`, with the node's clock out of `synced`, the proposal is `Rejected(ClockUnsynced)` before any other step (type, permissive, range), the output goes to `safe` and the lease is renewed (A3), as in the other envelope rejections. |
| E2 | The rate is measured against the **last applied value**; on the first tick of a new generation, against the `safe` (A8). |
| E3 | `on_interlock = "hold"` keeps the last applied value; `"safe"` (default) applies the `safe`. |
| E4 | `grace` (default 0): `requires_good` only fails after the quality has been bad for `grace`. When it fails (step 3), the result is `Rejected(Quality)` and the `on_interlock` of the output applies (E3), as with a permissive that fails: `hold` keeps the last applied value, `safe` applies the `safe`. It renews the lease (A3). |
| E5 | A `safe` outside the `range` is `J0203`. |
| E6 | `range` accepts **expressions** (`range = ["-bess.p_max_discharge", "bess.p_max_charge"]`), evaluated on every tick. A limit with quality other than `Good` counts as a failure of the permissive (step 2) and applies `on_interlock`. |
| E7 | A trip ends with `jk reset` or with the `reset_when` condition (an expression, on the output), evaluated on every tick while the output is in trip. |
| E8 | `Suppressed` (deadband, `min_interval`) sends nothing to the driver, but renews the lease (A3) and does not count toward `max_actuations`. The keepalive (document 01, §8) is never suppressed. |
| E9 | `dedup_window` of actions: default 60 s. |
| E10 | A proposal with a value that is **NaN, infinite, or a nonexistent enum variant** (and, in general, a value that does not fit the type of the output: a `bool` other than 0 and 1, a `string` with invalid UTF-8) is `Rejected(WrongType)` at step 1, and does not renew the lease (A3). It is neither a trap nor an error of the task: only that proposal is rejected (`ref/04` §3.4). It is the only way, besides a wrong binding, in which step 1 fails at runtime. |

## 3. Quality

### 3.1 Access through the SDK

| Call | `Good` | `Uncertain` | `Bad` |
|---|---|---|---|
| `get()` | `Ok(v)` | `Err` | `Err` |
| `get_uncertain()` | `Ok(v)` | `Ok(v)` | `Err` |
| `get_or(d)` | `v` | `d` | `d` |
| `sample()` | full sample | full sample | full sample |

`get_or` records in the trace that the read was substituted. There is no automatic contamination of the quality of the app's outputs: whoever wants to communicate that publishes a signal.

### 3.2 In expressions

| Situation | Rule |
|---|---|
| operands | the result has the worst quality of the operands, with the short-circuit exceptions |
| `a && b`, `a \|\| b` | **symmetric** short-circuit: a `false (Good)` operand decides `&&` and a `true (Good)` one decides `\|\|`, whatever the quality of the other and the side it is on (`false && Bad` and `Bad && false` both give `false (Good)`). Operands have no effects, and so the order does not matter. In the other cases, the worst quality |
| `any(sel)`, `all(sel)` | generalize the short-circuit: `any` gives `true (Good)` if some element is `true (Good)`; `all` gives `false (Good)` if some element is `false (Good)`; otherwise, the worst quality of the elements |
| `count(sel)`, `sum(sel)`, `maxs(sel)`, `mins(sel)` | the worst quality of the elements, without exceptions (a `Bad` element makes the result `Bad`) |
| `if c then a else b` | `c` `Bad` → result `Bad`; `c` `Uncertain` → evaluates the branch and the result is, at most, `Uncertain` |
| permissives | only `Good` permits (unless `permit_uncertain = true`) |
| alarms | `Uncertain` evaluates normally (with the flag on the event); `Bad` → `Unknown` |
| windows | the quality of the result is the worst quality `q` such that the samples with quality equal to or worse than `q` cover ≥ 10% of the duration of the window |
| `held`, `rose`, `fell`, `hyst` | initial state `false`; with a `Bad` input, the timer of `held` restarts and `rose`/`fell` give `false` |

### 3.3 When a sample carries `clock_unsynced`

The state of the node's clock does **not** change the quality of samples: it goes separately, in the `instant.clock` of apps, in the `clock_q` of the trace and in the runtime metrics (document 01, §19). The reason `uncertain(clock_unsynced)` only appears in three cases:

1. the driver reports it, because the **source timestamp** (`ts_src`) comes from a remote clock that the equipment itself says is not synchronized (ref/06 §2.8);
2. a derived signal (`calc`) or a signal published by an app with `requires_clock = "synced"` is produced while the node's clock is not `synced` (the value depends on the wall-clock time, as in a tariff);
3. a sample that arrives from an MCU whose `TIME` has not yet been established and whose `ts_src` is the only time reference (ref/07 §6.23).

The writes of an app with `requires_clock = "synced"` are something else: they are `Rejected(ClockUnsynced)` at step 0 of the envelope, and the output goes to `safe` (E1; document 01, §7 and §18).

### 3.4 `debounce` on a `bool` read by poll

When the debounce is done by the core (a driver without the `debounce` capability, ref/06 §3.6, step 6) and the `bool` is **read by poll**, and not by edges:

| # | Rule |
|---|---|
| D1 | The published value only changes from `v` to `¬v` after **consecutive reads** equal to `¬v` whose acquisition instants (`ts_acq`) cover at least the `debounce`: `ts_acq(last) − ts_acq(first) ≥ debounce`. With a 50 ms poll and `debounce = "20ms"`, two reads in a row are enough; with `debounce = "100ms"`, three. |
| D2 | A read equal to `v` in between restarts the count. A `Bad` read also restarts it, and is published as `Bad` (quality never waits for the debounce). |
| D3 | While it waits, the signal keeps the previous value with the quality of the last read, and the `seq` does not increase (it does not count as a new sample, like a sample in the deadband). |
| D4 | The first read after `connecting` (with no previous value) is published immediately. |
| D5 | The instant of the published sample is the `ts_acq` of the **first** read of the sequence that confirmed the change: that is when the value changed, and not when the debounce ended. |

A `debounce` smaller than the `poll` of the signal is accepted and is equivalent to two equal reads in a row. On edges (`edges`, with a kernel timestamp), the debounce is the kernel's or, without it, the minimum interval between edges, and these rules do not apply.

## 4. Expressions: edge cases

| Case | Rule |
|---|---|
| division by zero, root of a negative number, NaN at the input | result `Bad(NaN)` |
| integers | arithmetic is done in `f64` (an `i64` or `u64` above 2⁵³ loses precision, and that is accepted). **Comparisons** between two integers of the same signedness (`i64` with `i64`, `u64` with `u64`) are **exact**, without going through `f64`; the others compare in `f64` |
| short-circuit | symmetric (§3.2): `Bad && false` = `false (Good)`, just like `false && Bad` |
| `°C` (affine scale) | `°C + K → °C`; `°C − °C → K`; `°C − K → °C`; `°C + °C` and `°C * x` are `J0301`; comparisons require the same dimension |
| `%` against `1` | they do not mix: `bms.soc >= 5 %` requires `bms.soc` in `%`; for fractions, `0.05` |
| literals without a unit | are only accepted with dimensionless operands, as exponents or as factors (`2 * x`); `clamp(x, 0, 1)` requires a dimensionless `x` |
| durations | `"200ms"` and `"200 ms"` are equivalent, everywhere |
| windows | maximum 15 min (`J0305` above it); the KPI of models has another grammar (document 06, §10.1) |
| `own(x)` | is only valid inside class methods (document 17); outside them it is `J0306`. It is the caller's last proposal at its level (including the earlier ones of the same tick, 17 §4.1); **with no earlier proposal** (lease expired or first call), it returns the **applied value** of `x` |

The cases this table does not settle (types, unit alignment, `J0304` and `J0307`, window sampling, the state of `held`, `rose`, `fell` and `hyst`) are in ADR 0034.

## 5. Units: the v1 catalog

- **Base:** `s`, `m`, `g` (gram), `A`, `K`, `mol`, `cd`, and `1` (dimensionless).
- **Derived and commonly used:** `W`, `Wh`, `J`, `V`, `Ω`, `Hz`, `var`, `varh`, `VA`, `Pa`, `bar`, `N`, `Nm`, `lx`, `°C`, `%`, `rpm`, `l`, `h`, `min`, `d`, `m2`, `m3`, `m/s`, `m/s2`, `m3/h`, `l/min`, `gn` (standard acceleration of gravity, 9.80665 m/s²).
- **Prefixes** (on any unit of the list, except `°C`, `%`, `1`, `rpm`, `gn`): `µ`, `m`, `k`, `M`, `G`.
- **Compound:** products and quotients of the above (`kW/s`, `%/s`, `m3/h/%`, `°C/min`).
- **Quantities** (quantity kinds that distinguish units of the same dimension): `active_power`, `reactive_power`, `apparent_power`, `energy`, `reactive_energy`, `temperature`, `temperature_difference`.

A unit outside this catalog is `J0109`. Adding units is a patch change in `joker-units`. The syntax of compounds, the quantity kinds of compounds and the codes of malformed literals are in ADR 0030.

## 6. Paths, aliases and instances

| # | Rule |
|---|---|
| P1 | An **alias** is a second path to the **same** `SignalId`. The registry sorts by canonical path (the device's); aliases are resolved in `jk check`. |
| P2 | An output has **a single** envelope, declared through any one of its paths. Declaring it through two is `J0215`. |
| P3 | Instances and devices share the namespace of the first segment. The members of an instance are aliases `<instance>.<member>`. An instance name equal to that of a device is only allowed for the implicit instance of that same device; in that case, a member with the same name as a different signal of the device is `J0604`, and the same holds for the nested paths of P6. |
| P4 | The `to` of pipelines, the `calc`, the `alarm` and the `bridge` create paths under their prefixes; `pipe:`, `model:` and `class:` are **references** in bindings, and not paths. |
| P5 | The names of apps, instances, devices and groups are `snake_case` (`[a-z][a-z0-9_]*`). |
| P6 | **Nested paths.** A class member (document 17, §8) creates nested aliases: `<instance>.<member>.<submember>` for a fixed member, and `<instance>.<member>.<element>.<submember>` in a collection, with `<element>` = the **instance name** of the element, and never an index (`station.pumps.p1.running`). They are aliases of the same `SignalId` (P1): `button.clicked` and `esp_io.boot.clicked` are the same signal, and the queries of the members also have a nested path. An output still has a single envelope, declared through only one of its paths (P2, `J0215`). With the maximum depth of 3 (`JC0501`), a nested path has at most 6 segments, and always fits in the 8 of `ref/00` §10. An instance may be a member of several compositions: it has the paths of all of them, and its `out` have **a single** envelope and **a single** arbiter. The writes of each composition arrive as those of the writer that called the method or wrote through the path (A1), and two apps at the same level are `J0210`. When an instance with `out` members reaches different apps through different paths, and at least one goes through a composition, `jk check` gives the warning `J0655`, because the sharing is not visible in the `bind` of any of the apps. In a **virtual** instance, the class members belong to the implementation: an app that binds the virtual instance is a writer only of the `out` of the class itself (the virtual commands), reads the members through the nested paths and does not write to them (`J-E0507` at runtime); therefore, a virtual instance of a class whose methods call methods of the members is `J0656`. |

## 7. Trace format (`.jktrace`) v1

```
file := header [state_segment] segment*
header := "JKTR" u16:version(=1) u32:flags
          [32]u8:generation hash  [32]u8:registry hash (sorted list of SignalId, types, groups)
          u64:monotonic epoch  i64:UTC epoch  u8:clock quality
segment := u32:group  u32:n_ticks  u32:compressed_bytes  zstd(tick*)
tick := u64:seq  u64:t_mono  i64:t_utc  u8:clock_q
        inputs: for each port of the group, in SignalId order: u32:seq  u32:quality  u64:value   (16 B)
        u16:n_proposals   (u32:writer u32:SignalId u64:value u16:flags)*        (18 B each)
        u16:n_outcomes    (u32:SignalId u8:outcome u64:applied_value)*          (13 B each)
        u16:n_ml_outputs  (u32:model u32:bytes bytes)*                          (M22)
        u16:n_methods     (u32:writer u32:instance u16:method u16:bytes args)*  (document 17)
state_segment := u32:0xFFFF_FFFF  u32:n_apps  u32:compressed_bytes  zstd(app_state*)
app_state := u8:name bytes  name(UTF-8)  u32:group  u64:seq of the first recorded tick
             u8:state(0 = exported, 1 = no persist, 2 = failed)  u32:bytes  joker-state (CBOR, ref/04 §5)
```

- Segments are compressed with zstd on Linux; on Joker Micro they go uncompressed (bit 0 of the header `flags`), and `jokerd` or `jk` compress them when they receive them.
- All integers in little-endian. The input `seq` carry only the low 32 bits; the reader extends them as in document 01, §3.2.
- **Writers:** the `u32` is the number of the writer in this generation (`ref/00` §5). The remote writers of an MCU (A12) appear in the MCU's trace as `0x8000_0000 | slot`; the decision log of the Linux node keeps the slot → (origin, level) mapping of each session (`ref/07` §8.7). Numbers below `0x8000_0000` are the local writers.
- **Trace of an MCU:** the same format, read with `jk_trace_read` and transferred by the `TRACE` of Joker Link. Instants are in **ns** (the MCU multiplies its µs by 1000) and on the MCU's clock; the hashes are those of the generation and of the registry of the MCU; `n_ml_outputs` is always 0 (`ref/08`).
- **Cost:** 16 B per input per tick (and not 12 B), plus proposals and results. Document 03, §7, uses this value.
- **State segment** (bit 1 of the `flags`). `jk record` (`source = "record"`, ref/09 §5.16) records it right after the header: for each app with cycle tasks in the recorded groups, the `export-state` blob that already exists for the hot swap (ref/04 §4.12), requested from the cycle instance **between two ticks** of its group, with the same 100 ms deadline, and the `seq` of the first recorded tick of that group, which is the next one. `jk replay` starts each app **from that state** (through `migrate`, as in a hot swap, and with `--app a@version` through the migration of the new version) and only then replays the ticks. An app without `persist` (state 1) or whose `export-state` failed or exceeded the deadline (state 2) starts from `init`, and the replay warns. Traces from the continuous ring (`jk trace-dump`, `source = "ring"`), those reconstructed from the historian and those of an MCU **do not have** a state segment: the replay starts from `init`, and the first ticks can diverge from the recorded ones until the state of the app converges (an integrator, a filter, a state machine). `--diff` marks those ticks as "before the state" and does not count them as divergence.
- **Phase 0 exit gate (determinism):** given the same file, the replayed sequence of `outcomes` must be **byte for byte** equal to the recorded one.

## 8. Conformance cases

The schema of the cases is **a single one** for the core and for classes (D10): it is in `ref/03` §2.6, and that is the reference. The rules that hold for the core cases:

- One file per suite, `conformance/<area>/<suite>.conformance.toml`, with `schema = 1` as the first key/value pair, a `[suite]` (`kind = "core"`, `id`, `spec`, `period`) and one or more `[[case]]`.
- The fixture uses the manifest syntax in lists (`[[signals]]`, `[[outputs]]`, `[[writers]]`, `[[atomic]]`, `[[alarms]]`, `[[calc]]`). Writers are declared once, with the level in the `band:priority` notation and `kind` = `cycle` \| `async` \| `manual` \| `bridge` \| `impl` (`proposal` reserved for 1.1); `manual` requires `ttl`.
- Each case is a timeline: actions in `do` and expectations in `expect`, all with `at` aligned with the `period`. An `async` proposal enters at step 2 of the indicated tick; a `cycle` one, at step 4.
- `equals` on an output is the **applied value** at the tick; `outcome` is the write result; `level` is the active level (`"none"` with no active writer).
- Literals are written in the canonical lowercase form (`"good"`, `"clamped_rate"`, `"bad(comm_failure)"`); the CLI forms (`Clamped(Rate)`) are exact synonyms (`ref/00` §2).
- The same cases run on Linux and on an MCU (Joker Micro), and must give the same bytes (ADR 0020).
- The expected values are **written by hand** from the specification, never generated by the core (the Cravo rule).

```toml
# conformance/envelope/rate.conformance.toml
schema = 1

[suite]
kind   = "core"
id     = "envelope.rate"
spec   = ["01 §7 step 5", "16 A7", "16 E2"]
period = "100ms"

[[outputs]]
path  = "x.p_set"
type  = "f64"
unit  = "kW"
range = ["-100 kW", "100 kW"]
rate  = "10 kW/s"
safe  = "0 kW"

[[writers]]
id    = "a"
level = "optimizer:13"
lease = "2s"
kind  = "cycle"

[[case]]
id = "clamp_then_safe"
do = [{ at = "0ms", propose = "x.p_set", writer = "a", value = "50 kW" }]
expect = [
  { at = "0ms",    output = "x.p_set", outcome = "clamped_rate", equals = "1 kW", level = "optimizer:13" },
  { at = "2200ms", output = "x.p_set", outcome = "none",         equals = "0 kW", level = "none" },
]
```

## 9. Code registry

All codes live in a single file. The registry, with the entries of all the references (including retired codes, which stay so that the number is not reused), is in **`crates/joker-manifest/codes.toml`** (the seed was in `spec/codes.toml` and moved there in task 0b), which is the source of the counts (the prose does not repeat them; document 13, §5.1). Each `[[code]]` entry has `id`, `severity` (`error`, `warning`, `error-in-prod`, or `trap` for the `J-E09xx`), `message`, `doc`, and optionally `explain` (long explanation), `since` (version in which the code starts to exist, for reserved features), `status` (`"active"`, the default, or `"reserved"`: a code that only exists for a surface reserved by the 1.0 cut or for a later version, which always has `since`, and every code with `since` later than 1.0 is reserved; ADR 0028, ADR 0035), `also` (other references that cite it) and `retired` with `replaced_by` (a retired code is never reused). A reserved code becomes active, with no other change, when its surface is implemented. **A code has a single severity**: when one case is a warning and another is an error, they are two codes. The messages of the seed are descriptions for the documentation; the product messages (in English, like the identifiers) are written in task 0b. `jk explain <code>` reads from there, and a CI test fails if a code appears in the documentation or in the code without being in the registry (the `A–B` ranges and the lists of free numbers do not count).

| Range | Area |
|---|---|
| `J01xx` | signals, bindings, units, parameters |
| `J02xx` | outputs, envelope, arbiter, fallback |
| `J03xx` | expressions, cycles, RT, buses, memory |
| `J04xx` | lock and drift |
| `J05xx` | ML and pipelines |
| `J06xx` | classes and instances |
| `J07xx` | Joker Micro and Joker Link, validation (`J073x`–`J076x` in `ref/07`, `J077x`–`J079x` in `ref/08`) |
| `J08xx` | profiles, boards, simulation, scenarios, conformance, packages (`ref/03`); drivers (`ref/06`) |
| `J09xx` | node, client, trust, secrets (`ref/02`); enrollment, agent, sessions (`ref/10`) |
| `JCxxxx` / `JWxxxx` | class compiler `jkc` (errors and warnings) |
| `J-Exxxx` | runtime errors seen by apps, by the SDKs, by the API and by Joker Micro |
| `Wxxxx` | warnings with the same number as the corresponding error, in `mode = "dev"` |

The subdivisions of these ranges, one per reference and without overlaps, are in `ref/00` §8. Each reference lists its codes; the `codes.toml` registry gathers them all (document 22, §8, task 0b).

## 10. Decided during Phase 0 (they do not block the start)

| Topic | Where it is decided |
|---|---|
| exact formula of the RT admission (the 30% margin, what goes into the WCET) | **decided:** ADR 0048 |
| `dt()` after an `overrun = skip` (nominal or actual period) | **decided:** ADR 0032 (whole periods since the previous executed tick) |
| alarm transitions from `Unknown` | ADR in task 12 |
| bus budget model beyond RTU | **decided:** ADR 0049 |
| JSON schema of the manifest | **generated** from the types of `joker-manifest` (`ref/01` is the source; document 03 is the explanation) |

**Already decided in this round:** the base of the `extends` paths (relative to the file that declares them, `ref/01` §7.1 and §7.5) and the exact definition of "only tighten" (`ref/01` §7.4). The tier 2 admission uses the measured **maximum**, and not the p99.9 (D15, `ref/05`); the formula for tier 1 apps remains in the ADR of task 17.
