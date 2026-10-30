# ref/06 — Drivers: tier N trait, the `joker:driver` world (tier W) and JDP (tier X)

Normative reference for the three ways of writing a driver. It applies `ref/00` (representation) and defers to document 16 in everything that is semantics. The machine-readable files are normative at the same level as this document:

| File | Contents |
|---|---|
| `wit/joker-driver.wit` | complete `joker:driver` world (tier W) |
| `spec/cddl/jdp.cddl` | JDP control messages, structures shared by the three tiers, descriptor, schema language |
| `spec/c/jdp.h` | C API of the `libjdp` SDK (tier X) |

---

## 1. Scope and sources

**Covers:** the contract between the core and a driver, the same in the three tiers (§2); the Rust API of native drivers and what the core does on top of them (§3); the WIT world of Wasm drivers, with versions, errors, types and limits (§4); the JDP protocol at the wire level, supervision, security and the C and Rust SDKs (§5); the driver conformance suite, the same for the three tiers (§6); the codes J0850–J0899 (§7).

**Does not cover:** the manifest keys (`[buses]`, `[devices]`, `[drivers]`: ref/01), the profile keys and the step language of actions (ref/03), the lock fields (ref/02), Joker Link (ref/07), the C ABI of Joker Micro (ref/08).

**Derives from:** 05 (all), 01 §2–§4, §7, §8, §11, §17, §20; 02 §4–§8; 09 attacks 3, 4, 7 and 8; 19 §2–§4; 16 §2, §3 and §9; 17 §5.2; 21 §4; ADR 0005, 0007, 0008, 0016, 0020, 0022.

**Connects to:**

| Reference | What it shares with this one |
|---|---|
| ref/01 | `[buses.<b>]`, `[devices.<d>]` (common keys; appendix A is the `config` schema of each catalog driver), `[drivers.<n>]` (§5.7 of ref/01 is the normative list of the keys; this reference says what each one does, §4.5, §5.2) |
| ref/02 | `[drivers.<n>]` of the lock (`tier`, `version`, `digest`, `descriptor`, `wit`, `jdp`) and `[discovered.<d>]` (`discovered` schema); drift `J0401` |
| ref/00 | media types of the artifacts (§7.1: `joker.driver.v1+wasm`, `joker.driver.v1+tar`, role `drivers`), `link_state` (§6), code range J0850–J0899 (§8) |
| ref/03 | point and address keys of the profiles (§2.2.4–§2.2.7), `quirks`, `[config]`, `[fallback]`, action steps, `[connect]`, `[relinquish]`; codes J0808–J0826 that `plan` returns |
| ref/04 | `joker:runtime/types` (`quality-code`, `instant`, `error`) and `joker:runtime/log`, imported by the `joker:driver` world |
| ref/08 | `no_std` drivers compiled into Joker Micro; the plan goes into the generation blob |
| ref/09 | `/v1/runtime/drivers`, `/v1/devices/<d>` (link state, metrics), `jk driver test` and `jk discover` as clients |

---

## 2. The common contract

A **driver** is code that speaks a protocol. A **driver instance** is that code bound to a device (`instance = device`, the common case) or to a whole bus (`instance = bus`: EtherCAT master, CANopen master). The contract is **the same in the three tiers**: the same operations, the same records, the same codes. Only the transport of the call changes (Rust call, Wasm call, JDP).

### 2.1 Lifecycle

```
                     (jk check, jk build, generation preparation: no I/O)
  descriptor ──▶ plan(request) ──▶ read-plan
                                        │
  (jokerd, in the io domain)            ▼
  open(request, plan, link) ──▶ connect() ──▶ ┌── read(batch)        ← bus scheduler
         ▲                         │          ├── write(writes)      ← latest-wins boxes, ordered queues
         │                         │          ├── read-addr / write-addr  ← steps (actions, connect, fallback)
         │                         │          ├── invoke(call)       ← native actions
         │                         │          └── health()           ← every 5 s
         │                         ▼
         │                  connection failure ──▶ disconnect() ──▶ backoff ──▶ connect()
         │
   new generation with changes on this device ──▶ disconnect() ──▶ close ──▶ open(…)
```

| Operation | Pure | May do I/O | Where it runs | Required |
|---|---|---|---|---|
| `plan` | yes | no | `jk check`, `jk build`, `jk lock`, generation preparation on the node | yes |
| `open` | no (creates state) | no | node, when activating the generation | yes |
| `connect` | no | yes | `io` domain | yes |
| `read` | no | yes | `io` domain | yes |
| `write` | no | yes | `io` domain | with the `write` capability |
| `read-addr`, `write-addr` | no | yes | `io` domain | with the `raw-access` capability |
| `invoke` | no | yes | `io` domain | with the `native-actions` capability |
| `scan` | no | yes (reads only) | separate instance (`jk discover`) | with the `scan` capability |
| `health` | yes (reads state) | no | `io` domain | yes (there is a default) |
| `disconnect` | no | yes | `io` domain | yes (there is a default) |

**Connection sequence** (the same in every tier; it is the core that drives it, and not the driver):

1. `connect()` → `connect-info` per device (identity and, with `discover = true`, the discovered map).
2. The core compares identity, firmware, certificate and map with the lock (ref/02 §3.1.5) and raises `J0401` with the right `kind`. In `prod`, with drift, the device's outputs stay blocked (05 §12, `drift`).
3. The core runs the profile's `[connect]` steps with `write-addr`/`read-addr` (ref/03 §2.2.13). A failure puts the link `down`.
4. The core reads the `on-demand` points (`on-demand` batches) and the fallback registers (`read-addr`).
5. M1: with `fallback.configure = true`, it writes the fallback registers (`write-addr`), reads them back and compares with the `safe` (`J0205`, `kind = fallback` of `J0401`).
6. The core resends the **applied value** of each output (`write-kind = resync`), in the order of the atomic groups (A8 of document 16: at the start of a generation, the applied value is the `safe`).
7. The link goes `up` and the scheduler starts the periodic batches and arms the `push` batches.

### 2.2 Reference to a driver and tiers

| Form in `[devices.<d>] driver` | Tier | Resolution |
|---|---|---|
| `"<name>"` | N | entry of the registry compiled into `jokerd` (§3.8); absent on the node: `J0850` |
| `"w:<name>"` | W | `[drivers.<name>]` with `tier = "w"` (ref/01 §5.7); component in `source`. Reserved 1.2: in 1.0, `J0290` |
| `"x:<name>"` | X | `[drivers.<name>]` with `tier = "x"`; executable in `exec` or `source`. Reserved 1.2: in 1.0, `J0290` |

`<name>` follows `[a-z][a-z0-9-]{0,31}` (ref/01 §3.8). The same name may exist in two tiers (a tier X `bacnet-ip` and, later, a native one): the prefix decides, and changing tier is an explicit change in the manifest.

### 2.3 Descriptor

Each driver publishes a static **descriptor** (`descriptor` in `spec/cddl/jdp.cddl`). In tier N it is compiled into the binary; in tier W it is the custom section `joker:driver-meta` of the component (deterministic CBOR, readable without instantiating); in tier X it goes in the `hello` and, for `jk check` without a node, in a file `<exec>.jdp-meta` next to the executable or in the `descriptor` layer of the OCI artifact (§5.9).

| Field (key) | Type | Req. | Validation | Use |
|---|---|---|---|---|
| `name` (0) | driver name | yes | equal to the name in `[drivers]` or in the registry (`J0851`) | |
| `version` (1) | SemVer | yes | equal to the lock's (`J0885`) | |
| `tier` (2) | `n` 0, `w` 1, `x` 2 | yes | equal to the prefix (`J0851`) | |
| `protocols` (3) | list of families | yes, ≥ 1 | families of ref/03 §2.2.2 (`modbus`, `i2c`…) or `x:<name>`/`w:<name>` | the profile's `driver` must be here (`J0808`) |
| `buses` (4) | list of `bus-kind` | yes, ≥ 1 | | the `kind` of the device's `bus` must be here (`J0171`); `none` = no transport |
| `instance` (5) | `device` 0, `bus` 1 | yes | | §2.1 |
| `capabilities` (6) | list of names (§2.13) | yes | unknown names ignored | `J0854` |
| `config` (7) | schema (§2.3.1) | yes (may be empty) | | specific keys of `[devices.<d>]` |
| `address` (8) | schema | yes | | address keys of the points |
| `quirks` (9) | schema | yes | | `quirks` of the profile |
| `native` (10) | schema | yes | | `native` table of the profile's actions |
| `resources` (11) | `{ memory, cpu, tasks }` | W and X | W: `memory` ≤ 64 MiB; X: no limit of its own (`J0874` above `[drivers]`) | §4.5, §5.7 |
| `sdk` (12) | string | no | | diagnostics |
| `interface` (13) | `joker:driver@x.y.z`, `jdp/1` or `joker-hal@x.y` | W, X | supported version (`J0859`, `J0861`) | |
| `micro` (14) | bool | no (`false`) | tier N only | compiles `no_std` for Joker Micro (§3.7) |
| `license` (15) | SPDX | no (yes to publish) | | Hub |
| `description` (16) | string ≤ 1 KiB | no | | |
| `discovered` (17) | schema | with `discover` | | driver fields in `[discovered.<d>]` of the lock (ref/02 §2.1.11) |

**The schemas of the catalog drivers** are not invented per driver: they are the tables of the other references, field by field. `config` = the device keys of appendix A of ref/01; `address` = the address keys of the family in ref/03 §2.2.5 (and, in drivers without a profile, the per-signal keys of appendix A, with the same names); `quirks` = ref/03 §2.2.6; `discovered` = the `[discovered]` column of appendix A of ref/01. A catalog descriptor that diverges from them is a driver bug, caught by `drv.meta.schema_roundtrip` (§6.4).

**Descriptor digest:** SHA-256 of the deterministic CBOR of the `descriptor`. The lock pins it (`[drivers.<n>] descriptor`, ref/02 §2.1.8), the plan cache uses it as the key (§5.3.3), and the generation blob of an MCU and the `HELLO` of Joker Link carry it (§3.7).

#### 2.3.1 Schema language

The schemas (`field` in the CDDL) describe TOML keys that the core validates **before** `plan`, with identical messages in every driver. This is what allows `jk check` to say "`unit_id` must be between 1 and 247" for a tier X driver whose executable does not run on the laptop.

| Type | Accepted TOML | Reaches the driver (CBOR) |
|---|---|---|
| `string` | string (≤ `max` B) | text |
| `int`, `uint`, `float` | number (with `range`) | int, uint, float64 |
| `bool` | bool | bool |
| `duration` | duration literal (ref/00 §3.1) | uint (ns) |
| `quantity` | quantity literal of the dimension of `unit` | float64 in the unit `unit` |
| `choice` | one of the strings of `values` | text |
| `list` | array of `element` (≤ `max`) | array |
| `table` | table with the fields of `fields` | map with text keys |
| `map` | table of free keys (`snake_case`) with `element` values | map |
| `address` | `"host:port"`, IP, or IP/CIDR | text |
| `path` | absolute path | text |
| `secret` | `"secret:<name>"` (ref/00 §9) | in `plan`: the name (text); in `open`: bstr with the value, never logged |
| `any` | any TOML value | equivalent CBOR; `plan` validates it |

Errors: unknown key `J0140` (ref/01), wrong type, outside `range`, outside `values`, required key missing: `J0852` (device), `J0853` (address), `J0818` (quirks, ref/03), `J0821` (`native` table, ref/03).

### 2.4 What the driver receives

The core compiles the manifest, the lock and the profile into a **plan request** (`plan-request`). The driver never reads TOML or configuration files.

#### 2.4.1 From the manifest

| Origin (ref/01) | Destination in the request | Who uses it |
|---|---|---|
| `[buses.<b>]` (all keys) | `bus` (`bus-spec`: name, `kind`, `params` in CBOR) | the host opens the transport; the driver only reads the parameters (e.g. `baud` for the cost computation) |
| `[devices.<d>]`: `driver`, `bus`, `profile` | resolution of the driver, the transport and the profile | core |
| `[devices.<d>]`: `poll`, `align`, `signals.*.poll` | `point-spec.poll`; scheduling | scheduler |
| `[devices.<d>]`: `timeout` | `device-spec.timeout-ns`; deadline of the calls (§2.14) | core and driver |
| `[devices.<d>]`: `retries` | retries done by the **core** (the driver makes one attempt per call) | core |
| `[devices.<d>]`: `discover` | `connect` returns `discovered`; `plan` receives the map pinned in the lock | core and driver |
| `[devices.<d>]`: `fallback` | registers read and written with `read-addr`/`write-addr` (§2.1, steps 4–5) | core |
| `[devices.<d>]`: `signals.*` (`alias`, `max_age`, `limits`, `stuck_detect`, `deadband`, `scale`, `offset`, `unit`, `record`) | do not reach the driver | acquisition stage |
| `[devices.<d>]`: `signals.*.debounce` | `point-spec.debounce-ns` with the `debounce` capability; without it, the acquisition stage does the debounce | driver or core |
| `[devices.<d>]`: `signals.*`, address keys (`line`, `topic`, `node`… in drivers without a profile) | `point-spec.addr` | driver |
| `[devices.<d>]`: all the other keys (`address`, `unit_id`, `node_id`, `position`, `network`, `priority`…) | `device-spec.config`, validated by the descriptor's `config` schema | driver |
| `[devices.<d>]`: `config = { … }` | validated by the profile's `[config]` (ref/03 §2.2.10); the values without `apply` go in `device-spec.profile-config` | driver (*chip handlers*) |
| `[devices.<d>]`: `actions.*` (`permit`, `role`, `timeout`) | do not reach the driver, only `action-spec.timeout-ns` | core |
| `[outputs."<s>"]`: `timed`, `schedule_depth` | `point-spec.timed`; `committed-write.at` | driver with `timed-write` |
| `[outputs."<s>"]`: `safe = "relinquish"` | `write-kind = relinquish` if the profile says `native = true`; otherwise, `[relinquish]` steps with `write-addr` | driver or core |
| `[atomic.<g>]` | `committed-write.atomic` and the order of the writes in a call | driver |
| `[drivers.<n>]` | startup (W: component and limits; X: process, user, sandbox, §5.2.4) | host |

#### 2.4.2 From the profile

| Origin (ref/03) | Destination | Who uses it |
|---|---|---|
| `[profile] driver` | must be in `descriptor.protocols` (`J0808`) | core |
| `[profile] quirks` | `device-spec.quirks`, validated by the `quirks` schema | driver (`plan`) |
| `[profile] firmware` | compared with `device-info.firmware` (`J0826`) | core |
| `[defaults]` + `[signals]`: address keys (§2.2.5) | `point-spec.addr` | driver |
| `type` | `point-spec.type-name`, `expect` and `width` (§2.7) | driver and core |
| `direction`, `poll`, `kind` | `point-spec.direction`, `poll` | driver and core |
| `word_order`, `byte_order`, `bit`, `invert`, `scale`, `offset`, `scale_factor`, `unit`, `quantity`, `enum`, `bits`, `invalid`, `limits`, `stuck_detect`, `deadband`, `monotonic`, `len`, `encoding` | do not reach the driver (except `len`, via `width`) | acquisition stage |
| `[coherent]` and `scale_factor` pairs | `point-spec.coherent` | driver (`plan`) |
| `[config]` | schema of the device's `config = { … }` | core; the driver receives the values |
| `[fallback]` | addresses read and written by the core with `read-addr`/`write-addr` | core |
| `[actions.<a>] steps` | run by the core with `read-addr`/`write-addr`/`sleep` (ref/03 §2.2.12) | core |
| `[actions.<a>] native` | `action-spec` and `invoke` | driver |
| `[connect] steps`, `[relinquish.<s>] steps` | run by the core | core |
| `[relinquish.<s>] native = true` | `write-kind = relinquish` | driver with `relinquish` |
| `[events]`, `[enums]`, `[implements]`, `[discovery] names` | do not reach the driver | core |
| `[discovery] kind`, `bases`, `models`, `max_models` | `device-spec.quirks` (key `discovery`) | driver with `discover` |

#### 2.4.3 TOML → CBOR conversion

The fields `config`, `profile-config`, `quirks`, `addr`, `native` and `params` are the **deterministic** CBOR (RFC 8949 §4.2.1) of the TOML table, after it has been validated and normalized by the schema: text keys; durations in ns (uint); quantities in float64 in the schema's unit; integers as int/uint; dates as RFC 3339 text; `any` values converted type by type. Two equal tables always give the same bytes, and that is what makes `plan` comparable between runs and between machines.

### 2.5 Points (`point-spec`)

| Field | Type | Meaning |
|---|---|---|
| `id` | u32 | dense index in the instance (0..n), in the order of the canonical path; stable within a generation |
| `device` | u16 | index of the device in the instance (0 in `instance = device`) |
| `path` | string | path relative to the device, for diagnostics only |
| `direction` | `input` \| `output` \| `param` | |
| `expect` | `raw-kind` | raw form the core expects (§2.7) |
| `width` | u16 | registers (`regs`), maximum bytes (`bytes`, `text`) or 0 |
| `type-name` | string | `type` of the profile, as is |
| `addr` | CBOR | address, in the driver's `address` schema |
| `poll` | `period(ns)` \| `on-demand` \| `push` | |
| `class` | `cycle` \| `telemetry` \| `diagnostic` | `cycle` if some cycle group reads the signal; `diagnostic` for `joker.*` and parameters; otherwise `telemetry` |
| `coherent` | optional u32 | coherence group (groups that share points already merged, ref/03 §2.2.9) |
| `debounce-ns` | u64 | §2.4.1 |
| `timed` | bool | output with timed writes |

Outputs are points too (`direction = output`) and **are not read cyclically**: the slot of an output holds the applied value (ref/03 §2.2.5). A batch never contains an output.

### 2.6 Plan (`read-plan`)

`plan` groups the points into **batches** (`batch-spec`). Each batch is one protocol operation (a Modbus request, an OPC UA `Read`, a subscription, a PDO).

| Field | Rule | Violation |
|---|---|---|
| `batches[].id` | 0..n−1, in the order of the list | `J0856` |
| `batches[].points` | increasing, no repeats, all from the same `device`, no `output` | `J0856` |
| coverage | each `input`/`param` point is in **exactly one** batch | `J0856` |
| `kind` | `poll` only with `period` points, all with the **same** period; `on-demand` only with `on-demand` points; `push` only with `push` points | `J0856` |
| `period-ns` | equal to the period of the points (`poll`), 0 in the others | `J0856` |
| coherence | the points with the same `coherent` are in the same batch | if they do not fit: `J0813` (ref/03); if the driver separates them: `J0856` |
| `cost-ns` | > 0 in `poll` and `on-demand` on buses with a scheduler | enters the `J0370` budget (05 §5) |
| `write-cost-ns` | cost of a write of one point | same |
| `private` | ≤ 4 KiB per batch, ≤ 1 MiB in the plan | `J0856` |
| `warnings` | warnings only | shown by `jk check` with the code |
| errors | `list<diagnostic>`, with a code from ref/03 §2.9 (`J0809`–`J0813`, `J0818`, `J0821`) or from this reference; another code or none: `J0855` | |

The plan is **deterministic**: the same request (the same CBOR bytes) gives the same plan (the same CBOR bytes). The core stores the plan in the generation and, on Joker Micro, in the blob (§3.7). The generation preparation on the node recomputes it and compares it with the one from `jk check`; different is `J0857`.

### 2.7 Raw values and encoding

The driver delivers and receives **raw values**. The core converts between raw and engineering value in a single place (01 §3.3; ref/03 §3 "Acquisition").

| `raw-kind` | Code | Payload | Used by |
|---|---|---|---|
| `none` | 0 | — | only in failures; with `status` `good` it is `J0869` |
| `boolean` | 1 | bool | coils, discrete inputs, GPIO |
| `int` | 2 | i64 | already typed protocols (OPC UA Int32, CANopen INTEGER) |
| `uint` | 3 | u64 | same, and CAN frame fields |
| `float` | 4 | f64 | OPC UA Float/Double, MQTT JSON |
| `regs` | 5 | `width` 16-bit words, in protocol order | Modbus (16-bit registers) |
| `bytes` | 6 | ≤ 256 B | I²C, SPI, M-Bus, binary protocols |
| `text` | 7 | UTF-8 ≤ 256 B | text protocols, MQTT |

**`expect` and `width` from the profile's `type`:**

| Family | `type` | `expect` | `width` |
|---|---|---|---|
| `modbus` (FC 1, 2, 5, 15) | `bool` | `boolean` | 0 |
| `modbus` (FC 3, 4, 6, 16) | `bool` (with `bit`), `u16`, `i16`, `bits16` | `regs` | 1 |
| | `u32`, `i32`, `f32`, `bits32` | `regs` | 2 |
| | `u64`, `i64`, `f64`, `bits64` | `regs` | 4 |
| | `string`, `bytes` | `regs` | `len` |
| `i2c`, `spi` | any | `bytes` | `len` (or the size of the type) |
| `gpio`, `revpi` (bits) | `bool` | `boolean` | 0 |
| `revpi` (8, 16 or 32 bits, ADR 0078) | `i64`, `u64` | `int`, `uint` (sign- or zero-extended) | 0 |
| other families and `x:`/`w:` | numeric | `int`, `uint` or `float`, whatever the protocol gives | 0 |
| | `string` | `text` | `len` |
| | `bytes` | `bytes` | `len` |

A Modbus `bool` output with `bit` is a `regs` point of `width` 1, but its writes carry the `boolean` after `invert`: the driver reads the register and writes it back with the bit set or cleared (ref/03 §2.2.5, ADR 0076). The descriptor does not declare this table: `plan` rejects a `type` the driver cannot deliver (`J0809`). A sample with a `raw-kind` different from `expect` is `J0869` and becomes `bad(config_error)`.

**Read (raw → value):** the order of ref/03 §3 (decoding with `word_order`/`byte_order`/`bit`/`invert` → `invalid` → scale → unit → NaN → `limits` → `enum` → `deadband` → `stuck_detect` → link). An already typed `int`/`uint`/`float` skips the decoding.

**Write (value → raw):** the inverse, done by the core after the envelope: signal unit → profile unit → `(v − offset) / scale` → rounding to the nearest integer, with ties to even (`round_ties_even`, the same on every architecture) → check of the range of the raw type → encoding (`word_order`, `byte_order`). A value outside the range of the raw type is never saturated silently: `jk check` rejects a non-representable `safe` (`J0858`, error) and warns of a non-representable `range` (`J0858`, warning); at runtime, a non-representable write is `Rejected(Range)` in the decision log and does not reach the driver. The Modbus drivers confirm a write by its echo (ADR 0076). In this runtime (ADR 0075) the plan gives an output without a `range` the interval its raw type can carry, with `on_range = "reject"`, which is where that `Rejected(Range)` comes from; a constant `range` the raw type cannot carry, or an expression `range` on an output whose raw type bounds it, is `J0858` as an error when the generation is planned.

### 2.8 Samples and quality

`raw-sample { point, value, status, ts-src, ts-hw }`. The `status` is the quality that **the equipment** reports. The qualities that depend on time, on limits, on the envelope or on ML belong to the core.

| Code | Reasons a driver may report | Example |
|---|---|---|
| `good` | `none`, `overridden`, `local_override` | OPC UA `Good_LocalOverride`; PLC with the value forced |
| `uncertain` | `stale`, `substituted`, `sensor_cal`, `initial`, `clock_unsynced` | OPC UA `Uncertain_LastUsableValue`, `Uncertain_SubstituteValue`, `Uncertain_InitialValue`; `ts-src` timestamp from an unsynchronized remote clock |
| `bad` | `comm_failure`, `device_fault`, `out_of_range`, `config_error`, `not_connected`, `stale` | point with its own error in a batch response (OPC UA `Bad_*` per node) |
| any | 240–255 (private, ref/00 §4.3) | the code counts; outside the node, the reason appears as 0 |

**Only the core** generates `not_confirmed`, `out_of_distribution`, `frozen`, `partial_write`, `nan`, `tripped` and `deadline`. A pair outside the table, a point outside the batch, a second value for the same point in the same read, or a `none` with `good`, is `J0869` (warning, counted in `joker_driver_invalid_samples_total{device}`), and the sample becomes `bad(config_error)`. A point of the batch without a sample becomes `bad(comm_failure)` in that read.

**Batch failures** (`batch-failure`): the core applies them to all points of the batch.

| Variant | JDP code | Quality of the points | Counts for the link |
|---|---|---|---|
| `timeout` | 0 | `bad(comm_failure)` | yes |
| `comm-failure(detail)` | 1 | `bad(comm_failure)` | yes |
| `device-fault(detail)` | 2 | `bad(device_fault)` | no (the equipment responded) |
| `not-connected` | 3 | `bad(comm_failure)` | puts the link `down` |
| `config-error(detail)` | 4 | `bad(config_error)` | no; warning `J0869` |

**Quality that comes from the link** (05 §12), applied by the core on top of the driver's: `connecting` → `uncertain(initial)` on the points without a value; `degraded` → `uncertain(stale)` on the affected batches; `down` → `bad(comm_failure)`; `stopped` (driver dead, trapped or in crashloop) → `bad(not_connected)`; `drift` → values read normally, outputs blocked in `prod`.

**Time:** `ts-src` is UTC in ns from the equipment; `ts-hw` is the node's monotonic clock (the same `CLOCK_MONOTONIC` for `jokerd` and for a tier X process on the same node). `ts_acq` is set by the core, and all samples of a `read` call (or of the same `push` batch delivered at once) receive the same `acq_id` (02 §6.2).

### 2.9 Writes

The driver only receives **applied writes** (ADR 0005). It never sees proposals.

| `write-kind` | Code | Origin | The driver |
|---|---|---|---|
| `set` | 0 | commit of a group (latest-wins box or ordered queue) | writes |
| `keepalive` | 1 | renewal of the equipment fallback (01 §8); never suppressed (E8) | writes, same value |
| `force-safe` | 2 | `force_safe` bit of the supervisor (01 §3.4, §20) | writes; on an output with an `edge` adapter, the core converts it into the steps of the `fall` action (17 §5.2) |
| `relinquish` | 3 | `safe = "relinquish"` with `[relinquish.<s>] native = true` | relinquishes (value `none`) |
| `resync` | 4 | resend of the applied value after each `connect` (§2.1, step 6) | writes |

`committed-write { seq, point, value, kind, at, atomic }`:

- **Order:** a `write` call carries one or more writes, in the order in which they must go onto the wire. The driver respects it. The writes of an atomic group (M20) always go **together in a single call**, in the declared `order` (`atomic.index` 0..count−1), and the driver does not interleave other writes of the same device with them. With the `atomic-request` capability, the driver puts them in the same request when the protocol allows it (contiguous FC 16).
- **Acks:** exactly one `write-ack` per write, in the same order, with the same `seq`. If a write of an atomic group fails, the following ones of the group carry `skipped` and the core marks the group `uncertain(partial_write)` (02 §7.3).
- **Timed:** `at` only appears with the `timed-write` capability and on `timed` points. The driver applies the write at `at` with its declared precision; if it cannot by `at` + tolerance (the descriptor's, 100 µs by default for GPIO), it returns `late`, and the core records `J0878`. Without the capability, the core applies the write at the first tick with `t ≥ at` and the driver receives it without `at` (02 §7.2).
- **Pulses** (`pulse`): the core generates two `set` writes (the value and, afterwards, the `safe`). The driver does not know what a pulse is. Outside a pulse, a `resync` or a `keepalive` of a pulse output carries `safe`, never the pulse value (ADR 0072).

| `write-status` | Code | Effect in the core |
|---|---|---|
| `ok` | 0 | write confirmed in the decision log |
| `timeout` | 1 | counts for the link; N consecutive timeouts → `down` (01 §20) |
| `comm-failure` | 2 | same |
| `device-rejected` | 3 | `J0877` in the decision log; the output stays `uncertain(not_confirmed)` until the readback |
| `not-connected` | 4 | link `down`; the latest-wins box keeps the value for the `resync` |
| `unsupported` | 5 | `J0877`; for `relinquish` writes without support, `jk check` should already have given `J0207` |
| `late` | 6 | `J0878` |
| `skipped` | 7 | partial atomic group |

### 2.10 Access by address, steps and actions

The step language of the profiles (ref/03 §2.2.12: `write`, `read`, `wait`, `until`, `fail_if`) is run **by the core**, and not by the driver. The driver only offers two primitives (capability `raw-access`):

- `read-addr { device, addr, expect, width, type-name } → raw-value | batch-failure`;
- `write-addr { device, addr, value, type-name } → () | batch-failure`.

The address comes already in CBOR in the driver's `address` schema, and the value already encoded (§2.7; a step with `signal = "<output>"` is scaled by the core). The value of a step is one scalar of the step's `type` (`boolean`, `int`, `uint` or `float`), in both directions: the driver does the protocol's encoding from the step's address (for Modbus, the register count of the type, `word_order`, `byte_order` and `bit`), and a type or value it cannot carry is `config-error` (ADR 0073). In 1.0, `modbus-rtu`, `modbus-tcp`, `sysfs` and `gpio` offer `raw-access`; `sysfs` and `gpio` reach only the LEDs, zones and lines the device's points and actions declare, and none of the four has native actions (ADR 0073). They go through the device's serialized queue and through the bus scheduler, with the priority of writes. The steps serve the profile actions, `[connect]`, `[relinquish]`, `[config] apply` and the fallback (M1). That way, a new driver has actions, fallback and relinquish **without writing code for it**. The exception is the `sim` driver: the steps of a model's `[actions]` change signals that only the driver has, so they are native actions of the driver (ref/03 §2.4.9, ADR 0054).

**Native actions** (`[actions.<a>] native = { … }`: an OPC UA method, a CANopen SDO, a vendor command) reach `invoke(action-call { call, action, args })`:

- `args` is the deterministic CBOR of the argument record, already validated by the core against the action's `args` (`J0822`); the result is the CBOR of the `result` record;
- the permissive, the role, the per-device serialization, the idempotency key and the `dedup_window` (E9) belong to the core: an invocation repeated within the window never reaches the driver;
- long actions (`long = true`) report progress (`progress(call, permille, text)`) and check for cancellation (`cancelled(call)`); the final result is that of `invoke`;
- `action-failure`: `timeout`, `device(error)`, `invalid(error)`, `not-connected`, `unsupported`, `cancelled`. The core projects them onto the apps' `action-error` (ref/04): `device` → `device`, `invalid` → `invalid`, `timeout` → `timeout`, the others → `device` with the code.

### 2.11 Discovery and scan

- **Discovery of a device** (`discover = true`, M2): it runs inside `connect`, and the map goes in `connect-info.discovered`. The core computes the `digest` (SHA-256 of the deterministic CBOR, after **re-encoding** it itself) and compares it with the lock. `plan` always receives the map **from the lock** (`device-spec.discovered`); in `prod` an unpinned map is not used (05 §7). For the Modbus drivers with SunSpec (ADR 0077), the map is `{ models, scales }` (ref/03 §2.2.15), and a chain whose digest differs from `device-spec.discovered` fails `connect` with `J0401 kind=map`, after reporting what was found: the plan's addresses would read other registers. The common fields of `[discovered.<d>]` are those of `device-info` (`vendor`, `model`, `serial`, `firmware`, `hw_revision`, `cert`), and the driver's fields, taken from `device-info.extra` and from the discovered map, follow the descriptor's `discovered` schema (ref/02 §2.1.11).
- **Network scan** (`jk discover <uri>`, capability `scan`): in an instance of its own, with a link created from the URI and the options. It returns `scan-hit { address, config, info, profile }`. The host limits the rate (by default 20 requests/s) and the deadline (`--timeout`, 60 s).
- **Reads only.** Neither discovery nor scan writes to the equipment. The host does not call `write`, `write-addr` or `invoke` in those phases; what the driver sends over the transport is its own responsibility, and conformance checks it with a double that records writes (`drv.discover.read_only`, §6). A detected write is `J0871`.

### 2.12 Link state and health

**Enumeration `link_state`** (ref/00 §6): `connecting` 0, `up` 1, `degraded` 2, `down` 3, `drift` 4, `stopped` 5. The driver only reports the first four (it is the `link-state` of the WIT and of the CDDL); `drift` (drift from the lock) and `stopped` (driver not running) belong to the core, and appear in the API and in the report to the Deck with the six values.

The **effective state** is the worst between what the driver reports (`report-link`) and what the core computes from the calls:

| Core state | Entered when | Left when |
|---|---|---|
| `connecting` | before the first successful `connect`; during the backoff | `connect` and steps 2–6 of §2.1 completed → `up` |
| `degraded` | ≥ 20% of the last 20 batch executions failed with `timeout` or `comm-failure` | < 5% in the last 20 → `up` |
| `down` | `connect` fails; 3 consecutive executions of **all** batches fail; `not-connected`; `write-status` `timeout` 3 times in a row | `disconnect` and a new `connect` with backoff 250 ms → 30 s, ×2, jitter ±20% |
| `stopped` | X process dead, W trap, crashloop; `jk device stop` (`POST /v1/devices/{d}/stop`, ref/09 §5.12: orderly close, no restart) | restart of the driver (§5.7); after a `stop`, only `jk device start`, the next startup of `jokerd` or a generation that touches the device |

The transitions raise the alarms `joker.device.<d>.degraded`, `.offline`, `.drift` (05 §12) and the event `joker.device.<d>.link` with the previous state, the new one and the detail.

**Health:** `health()` returns `{ link, detail, counters }`. It is synchronous, without I/O, and the core calls it every 5 s. The counters (≤ 64, `snake_case` names) are exported as `joker_driver_<name>{device}`. The core itself measures, for every tier, the latency of each call (p50/p99 per batch), timeouts, reconnections, bytes through the host's transport and the coalescing efficiency (05 §12).

**Crashloop per tier.** This table is the only source of the thresholds; §4.5 and §5.7 refer to it.

| Tier | Counts as a failure | Crashloop threshold | Recovery | Code and alarm |
|---|---|---|---|---|
| N | does not apply: runs inside `jokerd` | — | — | — |
| W | trap of the instance (`Store`), including resource limit (`J0868`) | **3 traps in 10 s** (sliding window) | recreation with backoff 1 s → 5 min (×2); in crashloop, at the maximum rate | `J0866`; `joker.driver.<n>.crashloop` (critical) |
| X | death of the process (§5.7, state `dead`) | **5 deaths in 10 min** | backoff 1 s × 2ⁿ (max. 5 min, jitter ±20%); 10 consecutive min in `ready` → counter reset to zero | `J0866`; `joker.driver.<n>.crashloop` (critical) |

**Why they differ.** Recreating a W instance is cheap (a new `Store`, no process, in milliseconds), and a trap repeats immediately if the cause is deterministic: three in 10 s already say it is a bug, and not a transient failure. An X process takes seconds to start (systemd unit, handshake, `open` and `connect` of the instances), and also dies from external causes (OOM, a vendor SDK that loses the connection): with the backoff, 10 s is not even enough for three deaths, and that is why the window is 10 min.

### 2.13 Capabilities

| Capability | The driver offers | Required by | Without it |
|---|---|---|---|
| `write` | `write` | `output` points in the profile or in the manifest | `J0854` |
| `push` | `push` batches | `poll = push` (subscriptions, GPIO by edge) | `J0854` |
| `raw-access` | `read-addr`, `write-addr` | `steps` actions, `[connect]`, `[relinquish]` with steps, `[config] apply`, `[fallback]` | `J0854` |
| `native-actions` | `invoke` | `native` actions | `J0854` |
| `discover` | discovered map in `connect` | `discover = true` | `J0177` (ref/01) |
| `scan` | `scan` | `jk discover` | `jk discover` says the driver does not scan |
| `coherent` | coherent read in one request (the same response) | `[coherent]` and `scale_factor` | `J0854` |
| `atomic-request` | atomic group in a single request when contiguous | — (improves coherence; without it, consecutive writes without interleaving) | — |
| `timed-write` | `committed-write.at` | `timed = true` | `J0854` |
| `relinquish` | `write-kind = relinquish` | `[relinquish.<s>] native = true` | `J0207` (ref/01) |
| `debounce` | debounce in hardware or in the kernel | — | the acquisition stage does the debounce |
| `hw-timestamps` | `ts-hw` in the samples | — (informative: `jk check` warns if an app uses `write_at` on the `ts_hw` of a signal without it) | — |
| `rt-cycle` | real-time part on the `rt/drv/<bus>` thread (§3.5) | buses with `cycle` (EtherCAT, piControl with sync) | `J0854` |
| `block-read` | blocks (M7) | `kind = "block"` | reserved 1.1 (`J0807`) |
| `streams` | streams (M4) | `kind = "stream"` | reserved 1.1 (`J0807`) |

Reading (`read`) is not a capability: every driver has it.

### 2.14 Concurrency and deadlines

- **One call at a time per instance.** The core never calls two operations of the same instance at the same time, with one exception: a pending `read` of a `push` batch is **canceled** to run a `write`, `read-addr`, `write-addr` or `invoke`, and is restored afterwards. That is why the read of a `push` batch must be *cancel-safe*: when canceled, it does not lose data (it stays in the driver's state or in the transport).
- **One bus at a time.** The bus scheduler (05 §5) serializes the instances that share a `serial`, `i2c`, `spi` or `ethernet` bus with `bus` declared: only one call is in progress on the bus. Priorities: writes and actions → `cycle` batches by period → `telemetry` → `diagnostic`, discovery and scan. TCP devices without `bus` and `can` buses do not serialize between devices (each one has its own connection or the frames are asynchronous).
- **Deadlines.** Each call has an absolute (monotonic) deadline that the driver receives and should use in the transport timeouts. If the driver does not return by the deadline plus a margin, the core cancels the call (N and W: the future is dropped; X: `CANCEL` on the ring and then death of the process if nothing comes, §5.7), counts `J0867` and treats it as `timeout`.

| Call | Default deadline | Origin |
|---|---|---|
| `plan` | W: 2 s of clock and 10⁹ units of *fuel*; X: 5 s; N: no deadline (it should be pure and fast) | this document |
| `open` | 1 s | |
| `connect` | `quirks.connect_timeout` (ref/03, 5 s) × no. of devices of the instance, up to 60 s | ref/03 §2.2.6 |
| `read` | the device's `timeout` | ref/01 |
| `write` | `timeout` × (1 + ⌊n/8⌋) for n writes | |
| `read-addr`, `write-addr` | `timeout` per step | |
| `invoke` | the action's `timeout` | ref/03, ref/01 |
| `scan` | `--timeout` (60 s) | |
| `health` | 100 ms (N, W); 2 s for the response (X) | |
| `disconnect` | 2 s | |
| cancellation margin | 50 ms (N, W); 500 ms (X) | |

Retries (`retries`) belong to the core, in the same poll period, if the remaining time allows: one attempt per call, always.

### 2.15 Limits

| Thing | v1 limit | Above |
|---|---|---|
| points per instance | 65,536 | `J0856` |
| batches per plan | 16,384 | `J0856` |
| devices per instance (`instance = bus`) | 1,024 | `J0856` |
| native actions per instance | 4,096 | `J0856` |
| raw value (`regs`, `bytes`, `text`) | 256 B (after decoding, the signals remain limited to 64 B, ref/00 §10) | `J0869` |
| `config`, `quirks`, `profile-config` (CBOR) | 64 KiB each | `J0852` |
| `addr` of a point | 1 KiB | `J0853` |
| `private` | 4 KiB per batch; 1 MiB per plan | `J0856` |
| whole plan (CBOR) | 16 MiB | `J0856` |
| action arguments and result | 16 KiB (strings ≤ 1 KiB, lists ≤ 64, ref/00 §10) | `J0822` / `invalid` |
| log message | 1 KiB, 16 fields; 100/s per instance, burst 1,000 | truncated, counted (`J0886`) |
| health counters | 64 | the excess ones ignored |
| results of a scan | 4,096 | truncated |
| samples per read | those of the batch | `J0869` |

---

## 3. Tier N: native drivers in Rust

### 3.1 Crates

| Crate | `no_std` | Contents |
|---|---|---|
| `joker-hal` | yes (features `alloc`, `std`, `plan`) | types of §2, traits `Planner`, `Driver`, `RtPart`, transports, `Cx`, registry, `MaybeSend`, deterministic CBOR (own subset codec, ADR 0047) |
| `joker-serial-sys` | no | the modem lines of a serial port (`TIOCMBIS`/`TIOCMBIC`) for the Linux serial transport, the only `unsafe` it needs (ADR 0047) |
| `joker-drivers/<name>` | depending on the driver (`micro = true` requires `no_std`) | one driver; exports a `DriverEntry` |
| `jokerd` (module `drivers`) | no | the registry generated from the features (§3.8), the bus scheduler, the acquisition stage, the W and X hosts |
| `joker-jdp` | no | JDP: protocol, rings, client (`jokerd` side) and server (Rust SDK of tier X, §5.10) |
| `joker-driver-sdk` | yes | SDK for tier W drivers in Rust: `export_driver!` over the same traits (§4.6) |
| `joker-drvtest` | no | conformance suite and equipment doubles (§6) |

**The same Rust code serves the three tiers.** A driver written against `joker-hal` compiles into `jokerd` (N), into a component with `joker-driver-sdk` (W), or into an executable with `joker-jdp` (X). What changes is the host of the transports and of the calls.

### 3.2 Types

```rust
// crate joker-hal — #![no_std]; `alloc` and `std` are features.
pub type PointId = u32;
pub type BatchId = u32;
pub type DeviceIx = u16;
pub type MonoNs = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum QualityCode { Good = 0, Uncertain = 1, Bad = 2 }

/// Numeric reason of ref/00 §4.3 (240–255 private to the driver).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reason(pub u8);
impl Reason {
    pub const NONE: Self = Self(0);
    pub const OVERRIDDEN: Self = Self(1);
    pub const LOCAL_OVERRIDE: Self = Self(2);
    pub const STALE: Self = Self(3);
    pub const SUBSTITUTED: Self = Self(5);
    pub const SENSOR_CAL: Self = Self(7);
    pub const CLOCK_UNSYNCED: Self = Self(8);
    pub const INITIAL: Self = Self(9);
    pub const COMM_FAILURE: Self = Self(12);
    pub const DEVICE_FAULT: Self = Self(13);
    pub const OUT_OF_RANGE: Self = Self(14);
    pub const CONFIG_ERROR: Self = Self(15);
    pub const NOT_CONNECTED: Self = Self(16);
    pub const fn private(n: u8) -> Self { Self(240 + (n % 16)) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawStatus { pub code: QualityCode, pub reason: Reason }
impl RawStatus {
    pub const OK: Self = Self { code: QualityCode::Good, reason: Reason::NONE };
    pub const fn bad(reason: Reason) -> Self { Self { code: QualityCode::Bad, reason } }
    pub const fn uncertain(reason: Reason) -> Self { Self { code: QualityCode::Uncertain, reason } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RawKind { None = 0, Bool = 1, Int = 2, Uint = 3, Float = 4, Regs = 5, Bytes = 6, Text = 7 }

/// Borrowed raw value; the sink copies it. No allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawValue<'a> {
    None,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Regs(&'a [u16]),
    Bytes(&'a [u8]),
    Text(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub struct RawSample<'a> {
    pub point: PointId,
    pub value: RawValue<'a>,
    pub status: RawStatus,
    pub ts_src: Option<i64>,
    pub ts_hw: Option<MonoNs>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState { Connecting, Up, Degraded, Down }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction { Input, Output, Param }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll { Period(u64), OnDemand, Push }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadClass { Cycle, Telemetry, Diagnostic }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchKind { Poll, OnDemand, Push }

/// Zero-copy views: on Linux they borrow the generation; on the Joker Micro, flash.
pub struct PointSpec<'a> {
    pub id: PointId,
    pub device: DeviceIx,
    pub path: &'a str,
    pub direction: Direction,
    pub expect: RawKind,
    pub width: u16,
    pub type_name: &'a str,
    pub addr: Cbor<'a>,
    pub poll: Poll,
    pub class: ReadClass,
    pub coherent: Option<u32>,
    pub debounce_ns: u64,
    pub timed: bool,
}

pub struct ActionSpec<'a> {
    pub id: u32,
    pub device: DeviceIx,
    pub name: &'a str,
    pub native: Cbor<'a>,
    pub timeout_ns: u64,
    pub long: bool,
}

pub struct DeviceSpec<'a> {
    pub index: DeviceIx,
    pub name: &'a str,
    pub config: Cbor<'a>,
    pub profile_config: Cbor<'a>,
    pub quirks: Cbor<'a>,
    pub discovered: Option<Cbor<'a>>,
    pub timeout_ns: u64,
}

pub struct BusSpec<'a> { pub name: &'a str, pub kind: BusKind, pub params: Cbor<'a> }

pub struct PlanRequest<'a> {
    pub bus: Option<BusSpec<'a>>,
    pub devices: &'a [DeviceSpec<'a>],
    pub points: &'a [PointSpec<'a>],
    pub actions: &'a [ActionSpec<'a>],
}

/// A batch at runtime, as handed to `read`.
pub struct ReadBatch<'a> {
    pub id: BatchId,
    pub device: DeviceIx,
    pub kind: BatchKind,
    pub points: &'a [PointId],
    pub private: &'a [u8],
    pub deadline: MonoNs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind { Set, Keepalive, ForceSafe, Relinquish, Resync }

#[derive(Clone, Copy, Debug)]
pub struct AtomicRef { pub group: u32, pub index: u16, pub count: u16 }

pub struct CommittedWrite<'a> {
    pub seq: u64,
    pub point: PointId,
    pub value: RawValue<'a>,
    pub kind: WriteKind,
    pub at: Option<MonoNs>,
    pub atomic: Option<AtomicRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WriteStatus {
    Ok = 0, Timeout = 1, CommFailure = 2, DeviceRejected = 3,
    NotConnected = 4, Unsupported = 5, Late = 6, Skipped = 7,
}

/// Batch-level failure (§2.8). `detail` is for humans, ≤ 40 bytes on the JDP wire.
#[derive(Clone, Copy, Debug)]
pub enum BatchFailure {
    Timeout,
    CommFailure(&'static str),
    DeviceFault(&'static str),
    NotConnected,
    ConfigError(&'static str),
}

/// Error with a registry code (ref/00 §8). No allocation: static strings, or a
/// fixed-capacity buffer with the `DetailBuf` helper.
#[derive(Clone, Debug)]
pub struct DriverError { pub code: Code, pub detail: Detail }

#[derive(Clone, Debug)]
pub enum ActionFailure {
    Timeout,
    Device(DriverError),
    Invalid(DriverError),
    NotConnected,
    Unsupported,
    Cancelled,
}

pub struct DeviceInfo<'a> {
    pub vendor: Option<&'a str>,
    pub model: Option<&'a str>,
    pub serial: Option<&'a str>,
    pub firmware: Option<&'a str>,
    pub hw_revision: Option<&'a str>,
    pub cert_sha256: Option<[u8; 32]>,
    pub extra: Cbor<'a>,
}

pub struct Diagnostic<'a> { pub code: Code, pub point: Option<PointId>, pub message: &'a str }
```

The names, the order of the fields and the numeric values are those of the WIT (§4) and of the CDDL. `Cbor<'a>` is a view over deterministic CBOR with `decode::<T>()` (through the own codec of the deterministic CBOR subset of §2.4.3, ADR 0047; reading never allocates when `T` only borrows). `Code` is a registry code validated at compile time (`code!("J0810")`).

### 3.3 Traits

```rust
/// Marker: `Send` on std hosts (tokio multi-thread), nothing on the Joker Micro (Embassy).
#[cfg(feature = "std")]
pub trait MaybeSend: Send {}
#[cfg(feature = "std")]
impl<T: Send> MaybeSend for T {}
#[cfg(not(feature = "std"))]
pub trait MaybeSend {}
#[cfg(not(feature = "std"))]
impl<T> MaybeSend for T {}

/// Planning half. Host-side only (jk, jokerd, jk build for MCUs): needs `alloc`.
#[cfg(feature = "plan")]
pub trait Planner {
    /// Static descriptor (§2.3).
    const DESCRIPTOR: &'static Descriptor;

    /// Pure: no I/O, no clock, no randomness, no global state. Same request → same plan.
    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>>;
}

/// Runtime half. `no_std`, no allocation required.
pub trait Driver: Sized + MaybeSend + 'static {
    /// Builds the instance state. No I/O. `args` gives the request, the plan and the link.
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError>;

    fn connect(
        &mut self,
        cx: &mut Cx<'_>,
        out: &mut ConnectOut<'_>,
    ) -> impl Future<Output = Result<(), DriverError>> + MaybeSend;

    /// One execution of `batch`; one `out.push` per point. For `Push` batches: return as soon
    /// as at least one sample was pushed, or `Ok(())` at the deadline. MUST be cancel-safe for
    /// `Push` batches.
    fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend;

    /// Capability `write`. One `acks.set(i, …)` per write; unset acks become `CommFailure`.
    fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) -> impl Future<Output = ()> + MaybeSend {
        async move {
            let _ = (cx, writes);
            acks.fill(WriteStatus::Unsupported);
        }
    }

    /// Capability `raw-access`.
    fn read_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrRead<'_>,
        out: &mut RawSink<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend {
        async move {
            let _ = (cx, req, out);
            Err(BatchFailure::ConfigError("raw-access not supported"))
        }
    }

    fn write_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrWrite<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend {
        async move {
            let _ = (cx, req);
            Err(BatchFailure::ConfigError("raw-access not supported"))
        }
    }

    /// Capability `native-actions`. The result record goes to `out` as CBOR.
    fn invoke(
        &mut self,
        cx: &mut Cx<'_>,
        call: &ActionCall<'_>,
        out: &mut ActionOut<'_>,
    ) -> impl Future<Output = Result<(), ActionFailure>> + MaybeSend {
        async move {
            let _ = (cx, call, out);
            Err(ActionFailure::Unsupported)
        }
    }

    /// Synchronous, cheap, no I/O.
    fn health(&self, out: &mut HealthOut<'_>) {
        let _ = out;
    }

    /// Must succeed.
    fn disconnect(&mut self, cx: &mut Cx<'_>) -> impl Future<Output = ()> + MaybeSend {
        async move {
            let _ = cx;
        }
    }
}

/// Capability `scan`: runs in its own instance for `jk discover`.
pub trait Scanner: MaybeSend + 'static {
    fn scan(
        cx: &mut Cx<'_>,
        req: &ScanRequest<'_>,
        link: Link,
        out: &mut ScanOut<'_>,
    ) -> impl Future<Output = Result<(), DriverError>> + MaybeSend;
}

/// Capability `rt-cycle`: the part of the driver that runs on `rt/drv/<bus>` (§3.5).
pub trait RtCapable: Driver {
    type Rt: RtPart;
    /// Called once after `connect`; the core moves the part to the RT thread.
    fn rt_part(&mut self) -> Result<Self::Rt, DriverError>;
}

pub trait RtPart: Send + 'static {
    /// One bus cycle. Synchronous, no allocation, no blocking, no await, bounded WCET.
    fn cycle(&mut self, io: &mut RtIo<'_>) -> RtStatus;
}
```

- The implementation writes `async fn read(…)`; the compiler checks that the future is `Send` on Linux (the trait method declares `impl Future + MaybeSend`).
- `OpenArgs` gives `request()`, `plan()` (`PlanView`: batches, `private` of each batch and of the plan) and `config::<T>(device)`, which decodes the CBOR of a device's configuration into a type of the driver.
- `RawSink::push(sample)` copies the sample to the device's SPSC ring; it returns `Err(Full)` if the ring is full, and the drop is counted (ADR 0013). It never blocks.
- `WriteAcks::set(index, status, ts_hw)`, `fill(status)`.
- `ConnectOut::device(index, info, discovered)`.
- `ActionOut::result(cbor)`; progress and cancellation through `cx.progress(permille, msg)` and `cx.cancelled()`.

### 3.4 `Cx` and `Link`

```rust
pub struct Cx<'a> { /* host services, borrowed for the duration of one call */ }

impl Cx<'_> {
    /// Runtime clock (trait `Clock`, ADR 0007): monotonic, UTC and clock quality.
    pub fn now(&self) -> Instant;
    /// Absolute deadline of the current call.
    pub fn deadline(&self) -> MonoNs;
    pub fn sleep_until(&self, at: MonoNs) -> impl Future<Output = ()> + MaybeSend + '_;
    pub fn report_link(&mut self, state: LinkState, detail: &str);
    /// Structured, rate-limited, never blocks (ring drained by `svc`).
    pub fn log(&self) -> &Logger;
    /// Only inside `invoke` of a long action.
    pub fn progress(&mut self, permille: u16, message: &str);
    pub fn cancelled(&self) -> bool;
    /// Device state fixed in joker.lock (identity, discovered map), for comparisons the driver
    /// needs itself (e.g. picking the SunSpec chain). The drift verdict is the core's.
    pub fn locked(&self, device: DeviceIx) -> Option<LockedDevice<'_>>;
    /// std only: runs a blocking closure on the blocking pool, bounded by the deadline.
    #[cfg(feature = "std")]
    pub fn spawn_blocking<F, R>(&self, f: F) -> impl Future<Output = Result<R, BatchFailure>> + Send
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static;
}

/// The transport given by the host; the driver never opens one itself.
pub enum Link {
    None,
    Serial(SerialPort),   // boxed SerialIo; embedded_io_async::{Read, Write} + read_frame(idle) + info()
    Tcp(TcpEndpoint),     // connect(deadline) -> TcpStream (embedded_io_async)
    Udp(UdpSocket),       // send, send_to (allowed networks), recv
    I2c(I2cBus),          // embedded_hal_async::i2c::I2c, restricted to the instance's addresses
    Spi(SpiDevice),       // embedded_hal_async::spi::SpiDevice
    Can(CanSocket),       // filters set by the host
    Gpio(GpioChip),       // tier N only: line requests (chardev v1 in 1.0, ADR 0093 / embedded-hal)
    Paths(DevicePaths),   // tier N only: device nodes granted by the manifest (piControl, V4L2, raw socket)
}
```

The transports implement the traits of `embedded-io-async` and `embedded-hal-async` 1.0, and that is what makes the same driver portable: on Linux they are adapters over `tokio`, `tokio-serial`, `i2cdev`; on Joker Micro, over the Embassy HALs; in conformance, over doubles (§6). **Rule:** a tier N driver does not open sockets, device files or serial ports by itself; it uses the `Link`. A driver that needs something outside the `Link` (specific ioctls) uses `Link::Paths` and declares it in the `config` schema (`path`), so that `jk doctor` checks permissions and `jk check` knows there is no generic double for it.

**`SerialIo`.** The `embedded-io-async` traits use `async fn` and are not object-safe, so a host cannot hide its port behind them. A serial line is therefore a `SerialPort` that boxes a `SerialIo`: a poll-based, object-safe trait that the host implements (tokio on Linux, a double in tests, Embassy later), and `SerialPort` implements `embedded_io_async::{Read, Write}` (0.7) on top of it, so drivers stay portable. The variant needs `alloc`. On Linux the port is opened in raw mode with `VMIN = 1, VTIME = 0`, and the modem lines (`dtr`, `rts`, ref/01 §5.6) go through `joker-serial-sys` (ADR 0047).

**M0 subset (ADR 0047).** `joker-hal` implements in M0 the records of §3.2, the `Driver` trait of §3.3 with every method and its default, `Planner` (feature `plan`), `RtPart`, `RtCapable` and a minimal `RtIo`, the plan of §2.6 with its validation (`J0856`), and `Cx` with `now`, `deadline`, `sleep_until`, `report_link`, `progress`, `cancelled` and `log`. In M0, `Cx::log(level, message)` replaces the `Logger` accessor, and `Cx` also offers `until(at, future)` and `within(future)` (bounded by the call's deadline), so that a driver turns a silent equipment into `timeout` without a runtime of its own. Still M0 gaps, with their surface reserved as written here: `Cx::locked`, `Cx::spawn_blocking`, `Scanner`, `DriverEntry` and the registry by features (§3.8), the schema language of §2.3.1 (until then each driver's `plan` validates its keys, with the same codes), `AdcRead`, `CanFrames`, `RtChannel`, and the `Tcp`, `Udp`, `I2c`, `Spi`, `Can` and `Gpio` links.

### 3.5 Thread contract

| Where | What | May | Must not |
|---|---|---|---|
| any thread, outside the node (`jk check`, `jk build`) | `Planner::plan` | allocate; bounded recursion | I/O, clock, randomness, global state, `unsafe` |
| `io` (tokio multi-thread, `SCHED_OTHER` nice −5, `io` CPUs) | `open`, `connect`, `read`, `write`, `read_addr`, `write_addr`, `invoke`, `health`, `disconnect`, `Scanner::scan` | `await` on I/O; allocate (Linux); `cx.spawn_blocking` | block the thread for more than 100 µs between `await`s (no `std::thread::sleep`, no synchronous I/O, no contended locks); create threads; touch the slots or the process images; call the cycle core |
| `rt/drv/<bus>` (`SCHED_FIFO`, `rt` CPU) | `RtPart::cycle` | read and write in `RtIo` (sample ring, latest-wins boxes of the bus's outputs, queue of timed writes, `force_safe`); non-blocking syscalls of the bus (sendto on a raw socket, piControl ioctl) | allocate, block, `await`, log outside the ring, exceed the declared WCET |
| Joker Micro (Embassy executor, bus priority) | the same methods of `Driver` | `await` | allocate (no `alloc`), block the executor |

- **Nobody calls two operations of the same instance at the same time** (`&mut self`); the exception is the cancellation of a `push` read (§2.14).
- **Cancellation:** any future may be dropped at an `await` (deadline, new generation, `push`). The driver stays consistent: for example, the next Modbus TCP transaction id must not collide with a late response, and the driver discards responses to canceled requests.
- **Detected blocking:** in debug builds and in conformance, the host measures the duration of each `poll` of the future; above 1 ms it is `J0883` (warning) with the name of the driver and the method. A vendor library with a blocking API does not go into tier N: it uses `spawn_blocking` with a deadline, or goes to tier X.
- **`rt-cycle`:** the RT part receives the bus's outputs directly from the latest-wins boxes (this is where `force_safe` is honored without going through the group, 01 §3.4) and publishes samples on the same SPSC ring as the `io` part. Communication between the RT part and the `io` part of the same driver is through lock-free structures with fixed capacity, provided by `joker-hal` (`RtChannel<T, N>`).

### 3.6 What the core does on top (acquisition and output stages)

The acquisition stage runs in the `io` domain, in the same task as the bus scheduler, right after each `read` (01 §3.3). It is core code, the same for the three tiers:

1. **Sample validation** (§2.8): point in the batch, only once, `raw-kind` = `expect`, allowed quality pair. Failure → `J0869`, `bad(config_error)`.
2. **Application of the batch failure** and of the link quality (§2.8, §2.12).
3. **Decoding and conversion**, in the order of ref/03 §3 (raw type, `word_order`, `byte_order`, `bit`, `invert`, `invalid`, `scale_factor`, `scale`, `offset`, canonical unit, NaN, `limits`, `enum`, `deadband`, `stuck_detect`).
4. **Time:** `ts_acq` (monotonic, of the reception), `acq_id` common to the samples of the call, `ts_src`/`ts_hw` from the driver.
5. **Debounce** of the `bool`s without the `debounce` capability: for those read by poll, rules D1–D5 of document 16, §3.4 (equal consecutive reads that cover at least the `debounce`). While it waits, the slot is not written (D3).
6. **Slot:** seqlock write (single writer: this stage, 01 §3.2), `changed` bitset per group, historian queue.

The output stage does the reverse path: it reads the latest-wins boxes and the ordered queues (01 §3.4), converts the applied value to raw (§2.7), groups the atomic writes, generates `keepalive`, `force-safe`, `resync` and the pulse writes, calls `write` with the deadline, and records each `write-ack` in the decision log and in the readback verification (`verify`, 01 §7).

### 3.7 `no_std` and Joker Micro

A driver that also serves Joker Micro declares `micro = true` and complies with:

- `#![no_std]` without `alloc` in the `Driver` part; the `Planner` stays behind the `plan` feature (host only);
- transports only through the `embedded-io-async`/`embedded-hal-async` traits (`Link::Serial`, `I2c`, `Spi`, `Can`, `Gpio`);
- capacities fixed at compile time (`heapless`, const generics), no recursion on the read path;
- `Config` decoded by borrowing from the CBOR in flash (`OpenArgs::config::<T<'_>>()`).

**The plan is computed on the host.** `jk build --target mcu:…` runs `Planner::plan` on the laptop (it is pure), records the plan (deterministic CBOR) and the `point-spec`s in the compiled generation (embedded or in the blob, 19 §4.1; format in ref/08), and the firmware only runs the `Driver` part. `jk check` adds the plan to the firmware's capacities (`J0710`). A driver without `micro = true` on an MCU target is `J0875`.

**The configuration of each device in the blob.** Each record of the blob's `DEVICES` section (ref/08 §2.13.3) points to a deterministic CBOR item in `BLOBS`, with the `micro-device-config` schema of `spec/cddl/jdp.cddl`:

| Key | Field | Contents |
|---|---|---|
| 0 | `descriptor` | digest of the descriptor of the driver with which the plan was made (§2.3) |
| 1 | `device` | the `device-spec` (§2.4): `config` (the descriptor's `config` schema, converted as in §2.4.3), `profile-config`, `quirks`, `discovered` and `timeout-ns` |
| 2 | `points` | the `point-spec`s, each with the `SignalId` of the signal in the MCU's generation |
| 3 | `plan` | the `read-plan` |
| 4 | `bus` | the `bus-spec`, if there is a bus |

The firmware only decodes what its driver reads (by borrowing, without `alloc`). **The descriptor digest travels with the generation:** it goes in the item above and in the driver's `REQUIRES` record (the 8 B field, with the first 8 B of the digest), and the `HELLO` of Joker Link announces the descriptors of the drivers compiled into the firmware (ref/07). A plan made with another descriptor is rejected at build (`J0773`) and, if it still reaches the MCU, at boot (`JK_E_GEN_FIRMWARE`, `J-E2516`); it is case D12 (§9).

Catalog drivers with `micro = true` in v1: `gpio`, `adc`, `pwm`, `i2c`, `spi`, `uart` (`serial-line`), `modbus-rtu` (client), `modbus-slave` (server), `can`, `onewire` (19 §3), and `esp32` (only on ESP32 boards: touch pads, Wi-Fi RSSI and uptime; ref/01 appendix A).

**The traits of each MCU driver (normative table).** `embedded-hal` 1.0 does not cover every driver: the ADC was dropped before 1.0, CAN only has a *blocking*/`nb` API, and 1-Wire and touch have no trait. Each MCU driver uses only the trait of this table, and the `chip-*` modules of `joker-micro` (ref/08 §2.11.2) implement it over the family's HAL. Document 19 §3 and ref/08 §2.11.2 refer here.

| MCU driver | Trait | Trait status | Notes |
|---|---|---|---|
| `gpio` | `embedded_hal::digital::{InputPin, OutputPin}` + `embedded_hal_async::digital::Wait` | stable (1.0) | direct |
| `i2c`, `spi` | `embedded_hal_async::{i2c::I2c, spi::SpiDevice}` | stable (1.0) | bus sharing with `embedded-hal-bus` |
| `uart`, `modbus-rtu`, `modbus-slave`, serial link | `embedded_io_async::{Read, Write}` | **0.x**, exact version in the workspace (13 §5.1) | the types do not appear in the C ABI or in the blob |
| `pwm` | `embedded_hal::pwm::SetDutyCycle` | stable, *duty* only | frequency and phase stay in the `chip-*` module, from the board |
| `adc` | **`joker_hal::AdcRead`** (own) | — (there is no ecosystem trait) | implemented in each `chip-*` |
| `can` | **`joker_hal::CanFrames`** (own) | — (`embedded-can` has no async) | implemented in each `chip-*` over the HAL's async API |
| `onewire` | none: an *open-drain* pin (`embedded_hal::digital` with `Flex`) and the HAL's timing, or a UART | — | the timing belongs to `chip-*` |
| `esp32` (touch, RSSI, uptime) | `esp-hal` API (touch is behind `unstable`) | unstable | only in the `chip-esp` module |

The two own traits have the shape of those of `embedded-hal-async`, so that they can be dropped when the ecosystem standardizes an equivalent (minor change with an adapter):

```rust
/// joker-hal: ADC channel read (no embedded-hal 1.0 trait exists).
pub trait AdcRead {
    type Error: core::fmt::Debug;
    /// One conversion; raw code in 0..=(1 << bits) - 1. Scaling is the core's (ref/03 §3).
    async fn read(&mut self) -> Result<u16, Self::Error>;
    /// Resolution in bits, as the board declares it (`[adc] bits`, ref/03 §2.3.3).
    fn bits(&self) -> u8;
}

/// joker-hal: async CAN frames (embedded-can only has blocking/nb).
pub trait CanFrames {
    type Error: core::fmt::Debug;
    type Frame: embedded_can::Frame;
    async fn transmit(&mut self, frame: &Self::Frame) -> Result<(), Self::Error>;
    async fn receive(&mut self) -> Result<Self::Frame, Self::Error>;
}
```

### 3.8 Registry by features

Each driver crate exports an entry:

```rust
// crates/joker-drivers/modbus/src/lib.rs
pub const MODBUS_TCP: joker_hal::DriverEntry = joker_hal::driver_entry! {
    planner: ModbusPlanner,
    driver: ModbusDriver,
    scanner: Some(ModbusScanner),
    rt: None,
};
```

`DriverEntry` holds the descriptor and type-erased function pointers (`plan` over CBOR, `open` that returns `Box<dyn ErasedDriver>` on Linux). `jokerd` has a registry module **generated by `build.rs`** from the Cargo features:

| Cargo feature | Name in `jk runtime build --features` | Drivers |
|---|---|---|
| `drv-sim` | `sim` | `sim` (always present in `dev`) |
| `drv-modbus` | `modbus` | `modbus-tcp`, `modbus-rtu`, `sunspec` |
| `drv-gpio`, `drv-i2c`, `drv-spi` | `gpio`, `i2c`, `spi` | `spi`: 1.0.x |
| `drv-sysfs` | `sysfs` | LEDs and temperatures from `/sys/class` (05 §10) |
| `drv-revpi` | `revpi` | 1.0.x (implemented, ADR 0078) |
| `drv-mqtt` | `mqtt` | |
| `drv-joker-link` | `joker-link` | Joker Link driver (ref/07) |
| `drv-opcua`, `drv-mbus`, `drv-onewire`, `drv-serial-line` | same | 1.0.x |
| `drv-can`, `drv-canopen` | same | 1.1 |
| `drv-ethercat` | `ethercat` | 1.2 |
| `drivers-wasm` | `wasm` | tier W host (§4); 1.2 |
| `drivers-jdp` | `jdp` | tier X host (§5); 1.2 |

- The versions in the last column are those of the 1.0 cut (document 26, §5; ADR 0035); a feature of a later version does not exist in a 1.0 build. The default build (`default`, the "full" one of 01 §21) includes `sim`, `sysfs`, `modbus`, `gpio`, `i2c`, `mqtt`, `joker-link` in 1.0, and adds `spi` and `revpi` when they ship (1.0.x) and `wasm` and `jdp` in 1.2. The `minimal` build (09 attack 12) only `sim`, `sysfs`, `modbus`, `mqtt`. Both have a binary size target, measured in CI (01 §21).
- `jk runtime build --features ethercat,bacnet-ip` accepts driver names and maps them to the features; an unknown name is a usage error (exit code 1).
- The binary embeds the **driver manifest** (name, version, protocols, capabilities, descriptor digest of each entry), exposed in `GET /v1/runtime/drivers` (ref/09) and reported to the Deck in the report (ref/10). The lock does not repeat it: N drivers are pinned by the runtime version (`runtime` of the lock), and on an MCU by the `features` of `[mcu.<d>]` (ref/02 §2.1.10). `jk check --node` and `jk apply` reject a device whose N driver is not in the node's runtime (`J0850`).
- On Joker Micro, `jk build --target mcu` generates a static `enum` with the requested drivers (dispatch without `dyn`), from the same entries.

---

## 4. Tier W: the `joker:driver` world

The complete world is in `wit/joker-driver.wit`. This section gives the rules that the WIT does not express.

**The host is 1.2.** The cut of document 26, §5 (ADR 0035) moves the tier W host to 1.2: a 1.0 `jokerd` does not load W components, and a `w:<name>` driver is `J0290` (ref/01 §5.7). The world and the rules of this section are already closed, so that the in-tree drivers can be written against them.

**Stability (normative rule).** Until 1.2, the `joker:driver` world and the tier W SDK (`joker-driver-sdk`) exist as **unstable**: the world carries `@unstable(feature = driver-world)`, and only the **in-tree** drivers (those of the project's catalog, built from `crates/joker-drivers/` and published with it) use it in `prod`. A W component from outside the tree is accepted in `dev` and rejected in `prod` (`J0860`, `error-in-prod`), like any unstable item. The world may change between minors until **1.2**, when it becomes stable (`@since(1.2.0)`) and open to third parties, with the Rust and C SDK, the mandatory conformance suite and publication on the Hub (21 §4). The version rule (§4.1) applies from now on; compatibility with the previous minor (§4.8) only applies from 1.2. The other documents refer here.

### 4.1 Version and corrections with respect to document 05, §8

| In 05 §8 | Problem | Here |
|---|---|---|
| `package joker:driver@0.1.0` with `import joker:runtime/log@0.3.0` | violates ref/00 §1 (`joker:driver` has the version of the `joker:runtime` it imports) | `joker:driver@0.1.0` imports `joker:runtime/log@0.1.0` and `joker:runtime/types@0.1.0`; in 1.0, both `@1.0.0` |
| errors `result<_, string>` | the runtime cannot decide anything about a string (ref/00 §8) | `io-error { kind, detail }`, `batch-failure`, `action-failure`, `write-status`: closed enums; `error { code, message }` with a J-code where it is meant for people |
| `plan(config: list<u8>, points: list<u8>) -> result<list<u8>, string>` | bytes without a schema | `plan-request` and `read-plan` in WIT; only what is **driver-specific** (config, address, quirks, `private`) is CBOR, with a schema declared in the descriptor (§2.3.1) |
| `read(batch) -> list<raw-sample>` with `ok: bool` | quality reduced to a boolean | `raw-status { code, reason }` (§2.8) and `batch-failure` |
| `write(writes: list<u8>) -> list<bool>` | untyped writes, acks without a cause | `committed-write` and `write-ack` with `write-status` |
| `raw-value` with `bools(list<bool>)` and without `uint`/`text`/`none` | a point is a value, not a list; types were missing | `raw-kind` of §2.7 |
| `use raw.{raw-sample}` in the world without importing `raw` | invalid WIT | types in `interface types`, used by the interfaces |
| transport resources with no way of obtaining them | the driver had no way of receiving the `bus` | `variant link` delivered in `open` and in `scan` |
| no actions, discovery, health, deadlines, link state, access by address | gaps | `invoke`, `connect-info.discovered`, `scan`, `health`, `host.deadline`, `host.report-link`, `read-addr`/`write-addr` |

### 4.2 Interfaces

| Interface | Direction | Contents |
|---|---|---|
| `joker:runtime/types@0.1.0` | imported (types) | `quality-code`, `instant`, `error` |
| `joker:runtime/log@0.1.0` | imported | `log(level, msg, fields)`, `enabled(level)` |
| `joker:driver/types` | types | all the records of §2 |
| `joker:driver/host` | imported | `now`, `sleep-until`, `deadline`, `report-link`, `progress`, `cancelled` |
| `joker:driver/transport` | imported | `serial-port`, `tcp-endpoint`, `tcp-stream`, `udp-socket`, `i2c-bus`, `spi-device`, `can-socket`, `variant link` |
| `joker:driver/guest` | exported | `plan`, `open`, `connect`, `read`, `write`, `read-addr`, `write-addr`, `invoke`, `scan`, `health`, `disconnect` |

A driver that does not have a capability exports the function anyway (the world is closed) and returns `unsupported`/`config-error`; the Rust SDK generates those functions from the default methods of the trait.

### 4.3 `joker:driver-meta` section

Custom section of the component with the `descriptor` (deterministic CBOR). It is required: a component without it, or with it invalid, is `J0851`. `jk inspect driver.wasm` shows it, and the Hub indexes it. The `interface` field must be `joker:driver@<package version>` of the world the component exports; inconsistent is `J0851`.

### 4.4 Instantiation modes

| Mode | When | Available imports | Exports called |
|---|---|---|---|
| **plan** | `jk check`, `jk build`, `jk lock`, generation preparation | only `log`; `host.*` and `transport.*` trap with `J0882` | `plan` |
| **device** | each active driver instance (one device, or one bus with `instance = bus`) | all; a `link` with the instance's transport | `open`, then the rest |
| **scan** | `jk discover` | all; `link` created from the URI | `scan` |

- **One `Store` per instance.** State is not shared between instances, and a trap affects a single instance.
- The component is precompiled (AOT, `.cwasm`) at installation, like the apps (01 §9), with the engine configured as for the async instances: without mandatory canonicalized NaNs (the driver does not enter the deterministic trace; its samples do).
- In plan mode it runs with *fuel* (deterministic): the same component gives the same plan on the laptop and on the node.

### 4.5 Resources and timings

| Resource | Default | Maximum | Declared in | Exceeded |
|---|---|---|---|---|
| linear memory | 16 MiB | 64 MiB | `descriptor.resources.memory`; `[drivers.<n>] memory` may lower it | `memory.grow` fails; normally trap → `J0868` |
| *fuel* in plan mode | 10⁹ | 10¹⁰ | `[drivers.<n>] plan_fuel` | trap → `J0855` with the detail |
| continuous CPU without `await` | the guest yields the thread every 1 ms (epoch with *async yield*) | — | — | — |
| CPU per call (sum) | 50 ms | 1 s | `[drivers.<n>] cpu_per_call` | trap → `J0868` |
| deadline per call | §2.14 | | | cancellation → `J0867` |
| tables, instances, resources | 10,000 table elements; 1,024 resource handles | | | trap |

A W driver runs on the `io` threads, shared with the N drivers: that is why it yields the thread every 1 ms of continuous execution, and a slow W driver delays its own batches, but not those of the other devices.

**Failures.** A trap discards the instance (`Store`), puts the points in `bad(not_connected)` and the link in `stopped`, and recreates it with backoff; the crashloop threshold (`J0866`) is that of the table in §2.12. The outputs remain covered by the device fallback (01 §8).

### 4.6 SDKs

- **Rust:** `joker-driver-sdk` with `joker_driver_sdk::export_driver!(ModbusPlanner, ModbusDriver, scanner = ModbusScanner)`. It generates the bindings, the `joker:driver-meta` section from the `DESCRIPTOR`, and adapts the WIT transports to the `embedded-io-async` traits. Target `wasm32-wasip3`.
- **C:** `joker_driver.h` generated with `wit-bindgen c`, plus `libjokerdrv` with the same API as `jdp.h` (§5.8) so that a C driver can be W or X with the same code: it comes in 1.2 (21 §4).
- Any other language that produces a component for the world will do (Go with TinyGo, Zig), as long as it adds the `joker:driver-meta` section.

### 4.7 Artifact

OCI media type `joker.driver.v1+wasm` (ref/00 §7.1; signed by the `drivers` role): the component, with the signature alongside and the annotation `org.joker.driver.interface = "joker:driver@<version>"`. The lock pins `digest`, `version`, `descriptor` and `wit` (ref/02 §2.1.8).

### 4.8 Compatibility

- From 1.2 (stable world, §4), the host supports the `joker:driver` world of its version and the **previous minor**, through a generated adapter, as in `joker:runtime` (M21). In 1.0 and 1.1, an in-tree driver is rebuilt with each runtime. A component for a world newer than the runtime is rejected before the deploy (`J0859`).
- In a minor, only additions are made: new functions in `guest` with a default behavior defined here (an old component does not export them, and the adapter returns `unsupported`), new cases at the **end** of enums and variants with a declared projection, new interfaces, new capabilities.
- The `i2c`, `spi` and `can` cases of `link` are part of the world already. The 1.0 host implements `serial`, `tcp` and `udp`; a W driver that declares `buses` with `i2c`, `spi` or `can` is rejected in `jk check` by a 1.0/1.1 node (`J0873`) and accepted from 1.2.

---

## 5. Tier X: Joker Driver Protocol (JDP)

**The host is 1.2.** The cut of document 26, §5 (ADR 0035) moves the tier X host to 1.2: a 1.0 `jokerd` does not start JDP drivers, and an `x:<name>` driver is `J0290` (ref/01 §5.7). The protocol, `libjdp` and `joker-jdp` of this section are closed now, and they become stable with the host. The shared structures of `spec/cddl/jdp.cddl` (`descriptor`, `read-plan`, the schema language) are not reserved: tier N drivers and the Joker Micro blob use them in 1.0.

A tier X driver is a `joker-drv-<name>` process that speaks JDP with `jokerd`. It serves vendor binary SDKs, C libraries (BACnet, IEC 104, GigE Vision) and code that is not trusted. The cost is IPC: +20–50 µs per batch compared with tier N (05 §1), with no copy for streams (1.1).

### 5.1 Overview

```
 jokerd (user joker)                                         joker-drv-<name> (own user)
 ┌──────────────────────────────┐   control: UDS SOCK_SEQPACKET, CBOR    ┌──────────────────────────┐
 │ host JDP (joker-jdp)          │◀────────────────────────────────────▶│ libjdp / joker-jdp        │
 │  supervision, deadline, health│   hello, plan, open, connect, …       │  worker per instance      │
 │                               │                                       │                           │
 │  memfd B (jokerd writes)    ──┼── down ring: READ, WRITE, CANCEL ────▶│ (mapped read-only)        │
 │  (mapped read-only)   ◀───────┼── up ring: SAMPLE, READ_DONE, ACK ────┼── memfd A (driver writes)
 │  eventfd up / eventfd down    │   notification                         │                           │
 └──────────────────────────────┘                                       └──────────────────────────┘
```

- **Control** (low rate): handshake, plan, lifecycle, access by address, actions, scan, health, logs.
- **Data** (bus rate): read requests and writes (`down`), samples, read completions, acks and link state (`up`), in two SPSC rings in shared memory.
- **Each side only writes to its own memory.** `jokerd` treats everything that comes from the driver as hostile (§5.5.6).

### 5.2 Startup, identity and security

#### 5.2.1 Startup modes

| `launch` | Who starts the process | Control channel | User | When |
|---|---|---|---|---|
| `systemd` (default where there is systemd) | `jokerd`, with `StartTransientUnit` over D-Bus, unit `joker-drv-<name>.service` (or `joker-drv-<name>@<device>.service` with `isolation = "per_device"`) | the driver connects to `/run/joker/drv/<name>.sock` | `DynamicUser=yes`, or `User=<user>` | production |
| `spawn` | `jokerd`, with `fork`+`exec` | `socketpair` inherited on the fd given by `JDP_FD` | that of `jokerd`, or `user` if `jokerd` has `CAP_SETUID`/`CAP_SETGID` | systems without systemd, `dev` |
| `external` | another entity (its own systemd unit, container) | the driver connects to `/run/joker/drv/<name>.sock` | whatever that entity gives, verified by `jokerd` | vendor integrations |

`jokerd` does not need root (01 §17). In `systemd`, a polkit rule installed with the package authorizes the `joker` user to manage `joker-drv-*` units and nothing else. In `prod`, an X driver with `launch = "spawn"` that runs with the `jokerd` user is `J0865` (`error-in-prod`): a vendor SDK with the same user as the control would nullify the isolation.

#### 5.2.2 Sandbox (`systemd` mode)

Properties of the transient unit, derived from `[drivers.<n>]` and from the devices that use it:

| Property | Value |
|---|---|
| `DynamicUser`, `User`, `SupplementaryGroups` | `user` (default: dynamic); `groups` |
| `MemoryMax`, `CPUQuota`, `TasksMax` | `memory` (256 MiB), `cpu` (100%), 64 |
| `OOMScoreAdjust` | +500 (01 §17) |
| `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `ProtectKernelTunables`, `ProtectKernelModules`, `LockPersonality`, `RestrictRealtime` | yes |
| `DevicePolicy=closed`, `DeviceAllow` | only the nodes of `link-spec` (serial port, `/dev/i2c-*`) and `devices` |
| `RestrictAddressFamilies` | `AF_UNIX` and, if some link is a network one, `AF_INET`, `AF_INET6`; `AF_CAN` for `can`; `AF_PACKET` only with `raw_net = true` |
| `IPAddressDeny=any`, `IPAddressAllow` | the addresses of the devices and the networks of `net` and of `udp` |
| `StateDirectory` | `joker-drv/<name>` (the only place where it may write, for SDK caches) |
| `SetCredential` | `jdp-token` (§5.2.3) |
| `Environment` | `JDP_SOCKET`, `JDP_TOKEN_FILE=$CREDENTIALS_DIRECTORY/jdp-token`, `JDP_DRIVER`, `JDP_NODE`, and `env` |
| `Restart` | `no`: it is `jokerd` that restarts (§5.7) |

#### 5.2.3 Authentication

Three checks, all required outside `spawn`:

1. **Socket permissions.** `/run/joker/drv/` is `0710 joker:joker-drv`; each `<name>.sock` is `0660 joker:joker-drv`, created by `jokerd` before starting the driver and deleted afterwards. The driver units have the `joker-drv` group as a supplementary group; no other user has access.
2. **Peer credentials.** `SO_PEERCRED` on the connection: the `pid` must belong to the cgroup of the unit that `jokerd` started (read from `/proc/<pid>/cgroup`), or, in `external`, the `uid` must be that of `user`. Failure: `J0862`, connection closed.
3. **Token.** 32 random bytes per startup, delivered by systemd credential (or, in `spawn`, in a `0400` file in a temporary directory, deleted when the `hello` arrives). The `hello` carries it; different, or reused, is `J0862`.

In `spawn` the connection is a `socketpair` and there is no socket in the file system; the token is still required.

#### 5.2.4 Keys of `[drivers.<name>]`

The normative list of the keys, with types, defaults and validation, is in **ref/01 §5.7** (`tier`, `source`, `digest`, `exec`, `args`, `launch`, `user`, `groups`, `memory`, `cpu`, `cpu_per_call`, `plan_fuel`, `isolation`, `net`, `devices`, `raw_net`, `env`, `restart`). Here is what each one does on the host:

```toml
[drivers.bacnet-ip]
tier      = "x"
exec      = "/usr/lib/joker/drivers/joker-drv-bacnet-ip"
launch    = "systemd"
memory    = "256 MiB"
cpu       = "50 %"
isolation = "process"
net       = ["10.20.0.0/16"]
env       = { BACNET_LOG = "warn" }

[drivers.dlms-cosem]
tier   = "w"
source = "oci://registry.example.com/joker/drivers/dlms-cosem:0.3.1"
memory = "8 MiB"
```

| Key | Tier | Effect |
|---|---|---|
| `launch` | x | startup mode (§5.2.1); `spawn` with the `jokerd` user in `prod` is `J0865` |
| `user` | x | `"dynamic"` (default with `launch = "systemd"`): `DynamicUser=yes`; with `launch = "spawn"`, the default is `joker-drv-<driver>`; in `external`, the `uid` that `SO_PEERCRED` must show (§5.2.3) |
| `groups` | x | `SupplementaryGroups` (§5.2.2) |
| `memory` | w, x | W: linear memory limit (§4.5); X: `MemoryMax` |
| `cpu` | x | `CPUQuota` |
| `cpu_per_call`, `plan_fuel` | w | §4.5 |
| `isolation` | x | `per_device`: one `joker-drv-<name>@<device>` unit per device |
| `net`, `devices`, `raw_net` | x | `IPAddressAllow`, `DeviceAllow`, `AF_PACKET` (§5.2.2) |
| `env` | x | `Environment`, in addition to the `JDP_*` variables |
| `restart` | w, x | minimum and maximum restart backoff (§4.5, §5.7) |

### 5.3 Control channel

#### 5.3.1 Transport and framing

- Unix socket `SOCK_SEQPACKET`: each datagram is **one** complete CBOR message, ≤ 64 KiB. There is no length prefix.
- The file descriptors (memfd, eventfd, streams) go in `SCM_RIGHTS` in the datagram of the message that declares them (`rings`, `stream-open`), and only in those; descriptors in any other message are closed and it is `J0863`.
- JDP is **Linux only** in v1 (memfd, eventfd, seqpacket). On development macOS and Windows, X drivers do not exist; `sim` is used, or the driver itself compiled as N.

#### 5.3.2 Messages

Each message is a CBOR map with integer keys (`spec/cddl/jdp.cddl`): `0` type, `1` request id, `2` instance, `3` status (responses only), `4` J-code, `5` text; the body uses keys ≥ 16. The shared structures (`plan-request`, `read-plan`, `connect-info`…) mirror the WIT records: the key of each field is its position in the record, from 0; an empty `option` is an absent key; a `variant` is `[case, payload]`; an `enum` is the index of the case.

| Type | Name | Direction | Response | Body |
|---|---|---|---|---|
| 1 | `hello` | D → J | 2 | JDP versions `[min, max]`, `descriptor`, token, pid, SDK |
| 2 | `welcome` | J → D | — | chosen version, node, generation, `limits` |
| 3 | `plan` | J → D | 4 | `plan-request` |
| 4 | `plan-res` | D → J | — | `read-plan` or list of `diagnostic` |
| 5 | `rings` | J → D | 6 | entry size, capacities, sizes of the memfds; 4 fds |
| 6 | `rings-res` | D → J | — | — |
| 7 | `open` | J → D | 8 | instance, `plan-request`, `read-plan`, `link-spec` |
| 8 | `open-res` | D → J | — | — |
| 9 | `connect` | J → D | 10 | deadline |
| 10 | `connect-res` | D → J | — | list of `connect-info` |
| 11 | `disconnect` | J → D | 12 | deadline |
| 12 | `disconnect-res` | D → J | — | — |
| 13 | `close` | J → D | 14 | — |
| 14 | `close-res` | D → J | — | — |
| 15 | `read-addr` | J → D | 16 | `addr-read`, deadline |
| 16 | `read-addr-res` | D → J | — | `raw-value` or `batch-failure` |
| 17 | `write-addr` | J → D | 18 | `addr-write`, deadline |
| 18 | `write-addr-res` | D → J | — | optional `batch-failure` |
| 19 | `invoke` | J → D | 20 | `action-call`, deadline |
| 20 | `invoke-res` | D → J | — | result (CBOR) or `action-failure` |
| 21 | `progress` | D → J | notification | call, per mille, text |
| 22 | `cancel` | J → D | notification | call |
| 23 | `scan` | J → D | 25 | `scan-request`, `link-spec`, deadline |
| 24 | `scan-hit` | D → J | notification | scan id, `scan-hit` |
| 25 | `scan-res` | D → J | — | — |
| 26 | `health` | J → D | 27 | — |
| 27 | `health-res` | D → J | — | `health-report` per instance |
| 28 | `log` | D → J | notification | level, text ≤ 1 KiB, ≤ 16 fields |
| 29 | `bye` | both | notification | reason (`shutdown`, `generation`, `restart`, `protocol`, `fatal`) |
| 30 | `error` | both | notification | J-code, text, offending id |
| 31–39 | — | — | — | reserved 1.x |
| 40–42 | `stream-open`, `stream-res`, `stream-close` | D ↔ J | | **1.1** (§5.6) |

**Status** (`status`): `ok` 0, `error` 1 (see `code`), `unsupported` 2, `invalid` 3, `timeout` 4, `busy` 5, `denied` 6, `version` 7.

**Rules:**

- Ids chosen by the requester, unique among its pending requests. A response to an unknown id raises `error` and is ignored.
- Tolerance (ref/00 §1): unknown keys ignored; a request of unknown type gets an `unsupported` response with the same id; a notification of unknown type is ignored.
- Per instance, at most **one** pending lifecycle request (`open`, `connect`, `disconnect`, `close`); `read-addr`, `write-addr` and `invoke` are serialized by `jokerd` with the writes of the same instance (§2.14). A second simultaneous request of the same class receives `busy` (it is a `jokerd` bug, and the driver only reports it).
- The deadlines in the body are `CLOCK_MONOTONIC` instants of the node, in ns.

#### 5.3.3 Sequence

```
driver                                            jokerd
  │ connect(/run/joker/drv/x.sock) or inherited fd  │
  │── hello{jdp:[1,1], descriptor, token, pid} ───▶│  checks peer, token, descriptor, lock
  │◀───────── welcome{st:ok, jdp:1, limits} ────────│
  │◀───────── rings{…} + SCM_RIGHTS(A,B,up,down) ───│
  │── rings-res{st:ok} ───────────────────────────▶│
  │◀───────── plan{plan-request} ───────────────────│  (only if the plan is not in the cache for
  │── plan-res{read-plan} ────────────────────────▶│   this descriptor digest and this request)
  │◀───────── open{inst:0, request, plan, link} ────│
  │── open-res{st:ok} ────────────────────────────▶│
  │◀───────── connect{inst:0, deadline} ────────────│
  │── connect-res{connect-info…} ─────────────────▶│  drift (J0401), [connect] steps, fallback
  │◀── down ring: READ(on-demand), WRITE(resync) ───│
  │── up ring: SAMPLE…, READ_DONE, WRITE_ACK ─────▶│  link up; periodic poll; push armed
  │              …                                   │
  │◀───────── health{} every 5 s ───────────────────│
  │── health-res{…} ──────────────────────────────▶│
  │◀───────── bye{generation} ──────────────────────│  new generation: disconnect, close
```

### 5.4 Handshake and versions

- The driver sends `hello` as the **first** message, within ≤ 5 s of starting; another message first, or nothing, is `J0863` and the process is terminated.
- `jokerd` chooses the highest common JDP version. Without a common version: `welcome{st: version}`, `J0861`, and the driver exits with code 3. In v1 only `jdp = 1` exists.
- `jokerd` checks, in order: peer and token (`J0862`); well-formed descriptor (`J0851`); `name` and `tier` equal to those of `[drivers]`; `version` and descriptor digest equal to those of the lock (`J0885`); digest of the executable equal to that of the lock (checked before starting; ref/02 §2.1.8).
- **Compatibility:** a new JDP version only adds message types, optional keys and entry types in the rings. The `jokerd` of a minor supports the current `jdp` and the previous one; a driver compiled with a more recent `libjdp` speaks the version that the `welcome` chooses (`libjdp` implements the previous ones).

### 5.5 Data plane

#### 5.5.1 Shared memories

`jokerd` creates two memfds (`MFD_CLOEXEC | MFD_ALLOW_SEALING`), sizes them, seals them with `F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_SEAL` and passes them in `rings`:

- **A** (written by the driver): `up` ring (entries and `head`) and `tail` of the `down` ring;
- **B** (written by `jokerd`): `down` ring (entries and `head`) and `tail` of the `up` ring. Before passing it, `jokerd` maps it with write access and applies `F_SEAL_FUTURE_WRITE`: the driver can only map it read-only.

That way, each index and each entry has **a single writer** (the slot principle, 01 §3.2, applied between processes), and a driver cannot corrupt `jokerd`'s requests or the indices that `jokerd` uses.

| Memfd A (driver writes) | Offset | Size |
|---|---|---|
| header (written by `jokerd` before passing it; `jokerd` never reads it again) | 0 | 64 |
| `up.head` (u64) | 64 | 64 (own line) |
| `down.tail` (u64) and `down.sleeping` (u32 at +8) | 128 | 64 |
| driver counters: `up.dropped` (u64), `up.published` (u64) | 192 | 64 |
| entries of the `up` ring | 256 | `up_cap` × 64 |

| Memfd B (`jokerd` writes) | Offset | Size |
|---|---|---|
| header | 0 | 64 |
| `down.head` (u64) | 64 | 64 |
| `up.tail` (u64) and `up.sleeping` (u32 at +8) | 128 | 64 |
| `jokerd` counters: `down.dropped`, `down.published` | 192 | 64 |
| entries of the `down` ring | 256 | `down_cap` × 64 |

**Header** (64 B, little-endian): `magic` `"JDPRINGA"` or `"JDPRINGB"` (8 B), `version` u16 = 1, `entry_size` u16 = 64, `up_cap` u32, `down_cap` u32, `flags` u32 = 0, reserved (40 B, zeros).

**Capacities:** powers of 2. Default: `up` 4,096 entries (256 KiB), `down` 1,024 (64 KiB). Maximum: 65,536 each. `jokerd` sizes `up` to fit at least 4 × the largest `poll` batch and 256 `push` entries per instance.

#### 5.5.2 Indices and memory ordering

- `head` and `tail` are monotonic u64s (they never wrap around in practice); the position of the entry is `index & (cap − 1)`.
- **Producer:** reads `tail` with *acquire*; if `head − tail == cap`, the ring is full (the sample is dropped and `dropped` incremented; a `WRITE` is never dropped: `jokerd` waits for space, because the writes are bounded by the latest-wins boxes); writes the entry (or entries); publishes `head` with *release*.
- **Consumer:** reads `head` with *acquire*; copies the entries `[tail, head)` to private memory; publishes `tail` with *release*.
- The atomics are lock-free `AtomicU64` (on armv7, `ldrexd`/`strexd`); `jokerd` rejects a target without lock-free 64-bit atomics (there is none among the platforms of 01 §22).

#### 5.5.3 Notification by `eventfd`

Two `eventfd`s (`EFD_NONBLOCK | EFD_CLOEXEC`, counter mode): `up` wakes `jokerd`, `down` wakes the driver.

- **Consumer**, before sleeping: `sleeping = 1` (*SeqCst*); rereads `head`; if there are entries, `sleeping = 0` and consumes; otherwise, waits on the `eventfd` (poll/epoll, or the tokio reactor), and on waking reads the counter and sets `sleeping = 0`.
- **Producer**, after publishing `head`: *fence SeqCst*; if the consumer's `sleeping == 1`, writes 1 to the `eventfd`.
- The `sleeping` of each ring is in the **consumer's** memory (A for `down`, B for `up`).

Without the `sleeping` clause, each batch would cost two syscalls; with it, a busy consumer pays none.

#### 5.5.4 Entries

All entries are 64 bytes, little-endian, with a common 16-byte header:

| Off. | Type | Field |
|---|---|---|
| 0 | u8 | `kind` |
| 1 | u8 | `flags`: bit 0 `more` (`CONT` entries follow), bit 1 `end` (last of the call), bit 2 `has_ts_src`, bit 3 `has_ts_hw`, bit 4 `has_at`, bits 5–7 at 0 |
| 2 | u16 | `inst` |
| 4 | u32 | `a`: point, batch or 0 |
| 8 | u64 | `tag`: id of the read or `seq` of the write |

**`up` ring (driver → `jokerd`):**

| `kind` | Name | `a` | `tag` | Body (16–63) |
|---|---|---|---|---|
| 1 | `SAMPLE` | point | id of the `READ` (or of the arming of the `push` batch) | 16 u8 `q_code`; 17 u8 `q_reason`; 18 u8 `raw_kind`; 19 u8 0; 20 u16 `raw_len` (bytes of the payload; 8 for scalars); 22 u16 0; 24 i64 `ts_src`; 32 u64 `ts_hw`; 40–63 payload (24 B) |
| 2 | `CONT` | index of the continuation (1..) | that of the previous entry | 16–63: 48 B of payload |
| 3 | `READ_DONE` | batch | id of the `READ` | 16 u8 result (`0xFF` ok, or the `batch-failure` case); 17 u8 0; 18 u16 samples sent; 20 u32 0; 24–63 UTF-8 detail (≤ 40 B, zero-padded) |
| 4 | `WRITE_ACK` | point | `seq` | 16 u8 `write-status`; 17–23 0; 24 u64 `ts_hw` (with `has_ts_hw`); 32 u64 id of the `WRITE` call; 40–63 detail (≤ 24 B) |
| 5 | `LINK` | 0 | 0 | 16 u8 `link-state`; 17–23 0; 24–63 detail (≤ 40 B) |

**`down` ring (`jokerd` → driver):**

| `kind` | Name | `a` | `tag` | Body |
|---|---|---|---|---|
| 16 | `READ` | batch | id of the read (≠ 0) | 16 u64 deadline; 24 u8 `batch-kind`; 25–63 0 |
| 17 | `WRITE` | point | `seq` | 16 u8 `write-kind`; 17 u8 `raw_kind`; 18 u16 `raw_len`; 20 u32 atomic group (0 = none, otherwise id + 1); 24 u16 index; 26 u16 total; 28 u32 id of the call; 32 u64 `at` (with `has_at`); 40 u64 deadline; 48–63 payload (16 B) |
| 18 | `CANCEL` | 0 | id of the read to cancel | 16–63 0 |
| 19 | `CONT` | index | that of the previous entry | 16–63: 48 B of payload |

**Payload:** `boolean` u64 0/1; `int` i64; `uint` u64; `float` bits of f64; `regs` u16 LE words in protocol order; `bytes` as is; `text` UTF-8. A value larger than the space of the first entry continues in consecutive `CONT` entries (flag `more` on all but the last); `raw_len` ≤ 256, so at most 5 continuations in `up` and 6 in `down`. A sequence of continuations is written and published at once (a single `head`).

**Calls:** a `write` call of the core is the `WRITE` entries with the same call id, with `end` on the last one; the driver only starts applying them when it sees the `end` (and so an atomic group arrives whole). A read is one `READ`; the responses are the `SAMPLE`s with its `tag` and one `READ_DONE`.

**`push` batches:** the `READ` of a `push` batch **arms it**. After that the driver emits `SAMPLE`s with the `tag` of the arming whenever there is data, without `READ_DONE`, until a `CANCEL` with that `tag` or a `disconnect`. In tier X there is no cancellation for writes: the process is concurrent by nature, and the SDK delivers the `push` data through `jdp_push()` from any thread.

#### 5.5.5 Rate and latency

- `jokerd` publishes a `READ` and waits for the `READ_DONE` until the deadline; the acquisition stage runs over the received `SAMPLE`s when the `READ_DONE` arrives (all with the same `acq_id`) or, in a `push` batch, at each drain.
- The target round-trip cost of a batch of 20 points, with the consumer awake, is ≤ 20 µs on a CM4, and ≤ 50 µs with both `eventfd` notifications (to be measured in Phase 1; 05 §1).

#### 5.5.6 Validation by `jokerd`

`jokerd` treats memfd A and all the driver's messages as hostile input:

1. `up.head` is read once per drain; `head < tail` or `head − tail > up_cap` is `J0863`.
2. Each entry is **copied** to private memory before being interpreted (the driver may change it in the meantime).
3. Known `kind`; `inst` open; `a` within the points or batches of the instance; `tag` of a pending read or of an armed `push`; `raw_len` ≤ 256 and consistent with `raw_kind`; continuations complete and in order; valid UTF-8 `text`; reserved bits of `flags` at zero.
4. A violation in 1 or an unknown `kind` terminates the driver (`J0863`: SIGKILL, the points to `bad(not_connected)`, restart with backoff). A violation in 3 within a well-formed entry discards the entry with `J0869` and counts it; 100 per minute is treated as 1.
5. The control messages are validated against the CDDL; the `log` and `progress` above the rate of `limits` are discarded and counted (`J0886`).

### 5.6 Streams (reserved for 1.1)

The streams (M4) of X drivers (GigE Vision cameras, camera SDKs) use descriptors passed by `SCM_RIGHTS`, without copying:

- `stream-open` (D → J), with the point of the stream, the number and size of the buffers, the format and the dimensions, and the fds: the n buffers (memfd or dmabuf), a memfd with two rings of frame descriptors and two `eventfd`s;
- `frames` ring (driver → `jokerd`): 64 B entries with the buffer index, `seq`, `ts_hw`, useful bytes, `stride`, flags;
- `release` ring (`jokerd` → driver): index of the buffer returned when the last reference to the frame is dropped (01 §11);
- `latest` or `queue(n)` semantics from the manifest, applied by `jokerd` (a driver that is slow to return buffers loses frames, counted).

In 1.0 these messages receive `unsupported`, and a profile with `kind = "stream"` is `J0807`. The exact layout of the frame rings is fixed in 1.1, in the same entry format as §5.5.4.

### 5.7 Supervision and restart

| Process state | Entered | Left |
|---|---|---|
| `starting` | `jokerd` starts the unit or does `spawn` | valid `hello` → `ready`; 5 s without `hello` → `dead` |
| `ready` | handshake, `rings`, `open` and `connect` of the instances | new generation → `draining`; death → `dead` |
| `draining` | `bye{generation}` or `bye{shutdown}` | `bye` from the driver or 2 s → SIGTERM; 3 s more → SIGKILL |
| `dead` | EOF on the socket, `pidfd`/`SIGCHLD` (`spawn`), unit `failed` (`systemd`), 3 unanswered `health` (15 s), `J0863`, cancellation deadline exceeded | → `backoff` |
| `backoff` | | after 1 s × 2ⁿ (max. 5 min, jitter ±20%) → `starting` |
| `crashloop` | the tier X threshold in the table of §2.12 (`J0866`, critical alarm `joker.driver.<n>.crashloop`) | 10 consecutive min in `ready` → counter reset to zero |

- **On death:** all points of all instances of the process immediately go to `bad(not_connected)` and the link to `stopped`; the pending writes receive `not-connected` in the decision log; the latest-wins boxes keep the last applied value, which is resent as `resync` after the next `connect` (§2.1); meanwhile, the device fallback (M1) covers the outputs, as with an N driver that is down (01 §20).
- **Hang:** an exceeded call deadline generates `CANCEL` (read) and, if the response does not arrive within another 500 ms, counts as a failed `health`; three in a row kill the process (`J0881`).
- **`process` vs `per_device` isolation:** with `process`, a crash affects all the devices of that driver; with `per_device`, only one, at the cost of one process per device.
- **New generation:** if nothing changed for the driver (same digest, same requests), the process and the instances continue; if only the request of one instance changed, `jokerd` does `disconnect`/`close`/`open`/`connect` of that instance; if the executable changed, `bye{generation}` and a new process.
- **Logs:** the `log` messages go to the structured log of `jokerd` with `driver=<n> inst=<i>`; the process's stdout and stderr go to the unit's journal (or are discarded in `spawn`, after 64 KiB per minute).

### 5.8 C SDK: `libjdp`

The normative header is `spec/c/jdp.h`. The model:

| Aspect | Rule |
|---|---|
| entry | `int jdp_main(argc, argv, &driver, user)`: reads the environment, does the handshake and serves until the `bye`; returns the exit code (0 clean `bye`, 2 environment, 3 version or authentication, 4 protocol) |
| callbacks | `jdp_driver_t`: `plan`, `open`, `connect`, `read`, `write`, `read_addr`, `write_addr`, `invoke`, `scan`, `health`, `disconnect`, `close`; the optional ones set to `NULL` respond `unsupported` |
| threads | one control thread, one data thread, and **one worker thread per instance**; the callbacks of an instance are called one at a time, on that thread, and **may block** until the deadline they receive (this is the use case of blocking vendor SDKs) |
| thread-safe | `jdp_push`, `jdp_report_link`, `jdp_log`, `jdp_progress`, `jdp_cancelled` |
| memory | the pointers passed to a callback are only valid during the callback; `libjdp` copies everything the driver gives it |
| `push` | `push` batches do not call `read`: the driver calls `jdp_push(inst, batch, samples, n)` when it has data; before the batch is armed, or after it is canceled, it returns `JDP_E_CLOSED` |
| CBOR | minimal reader by path (`jdp_cbor_get_uint(config, "unit_id", &u)`) for `config`, `addr`, `quirks` and arguments |
| ABI | `JDP_ABI_VERSION_MAJOR`/`MINOR`; structures with `size` in the first field grow at the end |

Skeleton of a driver (illustrative):

```c
#include "jdp.h"

static jdp_status_t plan(void *u, const jdp_plan_request_t *rq, jdp_plan_builder_t *b) {
    (void)u;
    for (size_t i = 0; i < rq->n_points; i++) {           /* one batch per point: the simplest */
        const jdp_point_t *p = &rq->points[i];
        if (p->direction == JDP_DIR_OUTPUT) continue;
        jdp_plan_add_batch(b, p->device, JDP_BATCH_POLL, &p->id, 1, p->period_ns, 2000000, NULL, 0);
    }
    return JDP_OK;
}
/* … open, connect, read, disconnect … */
int main(int argc, char **argv) {
    static const uint8_t meta[] = { /* descriptor in CBOR, generated by jk driver meta */ 0xA0 };
    jdp_driver_t d = { .size = sizeof d, .abi_major = JDP_ABI_VERSION_MAJOR,
                       .abi_minor = JDP_ABI_VERSION_MINOR,
                       .descriptor = { meta, sizeof meta }, .plan = plan /* , … */ };
    return jdp_main(argc, argv, &d, NULL);
}
```

`jk driver meta <driver>.driver.toml` generates the CBOR descriptor from a TOML file with the same fields, by the names of §2.3, at the root (for those who do not write Rust). The `.driver.toml` suffix follows ADR 0026; the file is recognized by the keys `name`, `tier` and `protocols` at the root, and other content is `J0803` (ref/03 §2.1.6). The skeleton above is not a valid driver (`open`, `connect`, `read`, `disconnect` and a non-empty descriptor are missing); it serves to show the shape.

### 5.9 Tier X artifact

OCI media type `joker.driver.v1+tar` (ref/00 §7.1; signed by the `drivers` role): a multi-platform index (`linux/arm64`, `linux/amd64`, `linux/arm/v7`), and in each platform a tar layer with `bin/joker-drv-<name>`, `lib/` (vendor libraries, loaded with `RPATH=$ORIGIN/../lib`) and `descriptor.cbor`. `jokerd` extracts it to `/var/lib/joker/drivers/<name>/<digest>/` (read-only for the driver's user). The lock pins the digest of the executable for the target's architecture (ref/02 §2.1.8); an executable that is not an ELF of the node's architecture is `J0872`.

### 5.10 Rust SDK: `joker-jdp`

```rust
fn main() -> std::process::ExitCode {
    // The same Planner + Driver as tier N; transports are real sockets/ports inside the sandbox.
    joker_jdp::serve::<ModbusPlanner, ModbusDriver>(joker_jdp::Options {
        scanner: Some(joker_jdp::scanner::<ModbusScanner>()),
        ..Default::default()
    })
}
```

- `serve` runs one tokio `current_thread` runtime per instance (the equivalent of the `libjdp` worker thread) and implements the `Link` with the resources of the `link-spec` (opens the serial port, connects to `host:port`), within the sandbox restrictions.
- `push` batches use the same cancel-safe `read` as tier N: `serve` keeps the read armed and sends the samples as they arrive.
- `cx.spawn_blocking` uses threads of the driver's process (there is no blocking limit in tier X, only the deadline).

---

## 6. Driver conformance suite

A suite, **the same for the three tiers**, that says whether a driver complies with this contract. Requested for 1.2 (21 §4: tier W SDK and publication on the Hub); the interface is fixed in v1, and the catalog drivers of 1.0 already go through it.

### 6.1 How it is run

| Form | What for |
|---|---|
| `jk driver test <ref> [--fixture <f>] [--case <glob>] [--json] [--report <out.cbor>]` | `<ref>` is `modbus-tcp` (N, from the local `jokerd`), `w:./target/dlms.wasm` or `x:./build/joker-drv-acme` |
| `joker_drvtest::run::<P, D>(fixture)` in `cargo test` | N driver, or the same code before it becomes W or X |
| catalog CI and the Hub's publication check | requires the signed report (`J0880`) |

The runner (`joker-drvtest`) is a complete host with the **same** acquisition stage, the same scheduler and the same W and X hosts as `jokerd`, and a virtual clock for tiers N and W (X runs in real time, with deadlines scaled × 4).

### 6.2 Fixture

The suite uses the **single conformance schema** of ref/03 §2.6 (D10: `[suite]`, `[[case]]`, `do`, `expect`, `at`, `equals`), with `kind = "driver"`. What differs from the core and the classes is the *fixture*: each driver brings a file that says how to test it, with the configuration of a device, a test profile and the **double** of the equipment (what is on the other side of the transport). It is called `<driver>.fixture.toml` (ADR 0026: the name of the driver from the `[fixture] driver` key, one file per driver, because the same package may have `modbus-tcp` and `modbus-rtu`), starts with the `[fixture]` table (without it, or with another suffix, it is `J0803`, ref/03 §2.1.6) and is not a suite: it is passed with `--fixture`, and the generic cases of `conformance/drivers/*.conformance.toml` run against it.

```toml
# drivers/modbus/conformance/modbus-tcp.fixture.toml
schema = 1

[fixture]
driver  = "modbus-tcp"
profile = "conformance/modbus-test@1"      # the fixture's own profile (ref/03)
config  = { address = "127.0.0.1:0", unit_id = 1 }
poll    = "100ms"
timeout = "200ms"

[double]
kind = "modbus-server"                     # double built into the suite
registers = { "hr.40100" = 0, "ir.30000" = [0x0001, 0x86A0], "ir.30010" = 2 }
exceptions = { "hr.40999" = 2 }            # illegal address → exception 2

[values]                                   # raw values the cases expect to read, per profile point
"p_ac"  = { raw = [1, 34464] }             # regs, in protocol order
"state" = { raw = [2] }
```

| Table | Keys | Meaning |
|---|---|---|
| `[fixture]` | `driver` (the form of §2.2), `profile` (ref/03), `config` (validated by the descriptor's `config` schema, §2.3.1), `bus` (table with the keys of `[buses.*]` of ref/01, if the driver needs one), `poll`, `timeout` | the device under test |
| `[double]` | `kind` and the keys of the double | the simulated equipment |
| `[values]` | `"<profile point>" = { raw }` | the raw value the double serves, in the `raw-kind` form of §2.7; it is what `equals` compares when a case uses `from_fixture` |

Built-in doubles: `modbus-server` (TCP and RTU), `serial-script`, `tcp-script` and `udp-script` (transcript: expected bytes → response bytes, silences, delays, close), `can-loop`, `i2c-regs` (register map per address). A driver whose protocol has no built-in double uses a transcript, or an external double: `double = { kind = "external", exec = "…", ready = "tcp:127.0.0.1:4840" }`.

### 6.3 Cases

The generic cases live in `conformance/drivers/*.conformance.toml`, one file per suite, written by hand from this reference (the Cravo rule, 16 §8), and are parameterized by the fixture.

```toml
# conformance/drivers/read.conformance.toml
schema = 1

[suite]
kind  = "driver"
id    = "drv.read"
spec  = ["ref/06 §2.8", "ref/06 §2.12", "ref/06 §2.14"]
tiers = ["n", "w", "x"]

[[case]]
id    = "timeout"
level = "basic"
do = [
  { at = "0s",    call = "connect", key = "c" },
  { at = "100ms", fault = "silence", for = "1s" },
  { at = "100ms", call = "read", batch = "first", key = "r1" },
  { at = "400ms", call = "read", batch = "all" },
  { at = "700ms", call = "read", batch = "all" },
  { at = "1s",    call = "read", batch = "all", key = "r4" },
]
expect = [
  { within = "0s .. 100ms",                     call = "c",  ok = true },
  { within = "100ms .. 100ms + timeout + 50ms", call = "r1", failure = "timeout", link = "up" },
  { within = "1s .. 1s + timeout + 50ms",       call = "r4", failure = "timeout", link = "down" },
]
```

**`[suite]` and `[[case]]`:** the keys of ref/03 §2.6.1, with `kind = "driver"`, plus two more: `tiers` in `[suite]` (list of `n`, `w`, `x`: where the suite runs; default: all three) and `level` in `[[case]]` (`basic` \| `full`, §6.4). `requires` are **driver capabilities** (§2.13), plus `micro` (field 14 of the descriptor) and `systemd` (the runner has `launch = "systemd"`): a case whose requirement the driver does not meet becomes `skip`. A case for a single tier (the "Requires" column of §6.4) lives in a suite with that `tiers`.

**Timeline.** The instants are those of ref/03 §2.1.3, counted from the `open` of the instance, in virtual time (N and W) or real time (X, with deadlines scaled × 4). **Declared exception to D10** (ref/03 §5): in the instants and intervals of a `driver` suite, and only there, the term `timeout` is the fixture's `timeout` (already scaled in X), so that a case serves any driver without rewriting the numbers (`"100ms + timeout + 50ms"`).

**Actions** in `do` (each with `at`; optional `key`, for the expectations to refer to them):

| Action | Form | Semantics |
|---|---|---|
| call | `{ at, call = "<operation>", key, batch \| writes \| addr \| action \| args }` | an operation of §2.1: `plan`, `plan_twice` (two plans of the same request, in different processes), `open`, `connect`, `read` (`batch = "first"`, `"all"` or the index), `write` (`writes` = list of `{ point, value, kind }`), `read_addr`, `write_addr` (`addr`, `value`), `invoke` (`action`, `args`), `scan`, `health`, `disconnect`, `cancel` (drops the call in progress) |
| fault | `{ at, fault = "<type>", for }` | in the double: `silence`, `delay`, `close`, `garbage`, `exception`, `reset`; and `kill` (X: kills the driver's process) |

**Expectations** in `expect` (when: `at`, `during`, `within`, `before`, as in ref/03 §2.5.4; combinators `any_of`, `all_of`):

| Subject | Predicates |
|---|---|
| `call = "<key>"` | `ok` (bool), `failure` (case of `batch-failure`), `samples` (`"exact"`: one sample per point of the batch), `acks` (list of `write-status`, in order), `diagnostic` (code), `plan_equal` (bool: the two plans of `plan_twice` with the same bytes), `link` (link state after the call, `link_state` of ref/00 §6) |
| `point = "<profile point>"` | `equals` (raw value, or `"from_fixture"` for the one in `[values]`), `quality` (the `raw-status`, with the literals of ref/03 §2.1.3) |
| `wire = "double"` | `no_writes` (bool: the double saw no writes) |
| `driver = "<tier>"` | `trap` (W), `killed` (X), `no_block` (N: no `poll` > 1 ms) |

A malformed fixture or case is `J0887`; a failed case is `J0879`.

### 6.4 Mandatory cases of v1

The id of each case in the reports is `<suite id>.<case id>` (`drv.read` + `timeout` = `drv.read.timeout`), as in the core and in the classes (ref/03 §2.6.1).

| Id | Level | Checks | Requires |
|---|---|---|---|
| `drv.meta.valid` | basic | well-formed descriptor, name, version, tier, schemas | — |
| `drv.meta.schema_roundtrip` | full | the fixture's keys validate against the schema and reach the driver with the right types | — |
| `drv.plan.pure` | basic | W: plan without imports other than `log`; N: no I/O (test transport that fails when used) | — |
| `drv.plan.deterministic` | basic | two plans of the same request, in different processes, with the same bytes | — |
| `drv.plan.coverage` | basic | rules of §2.6 | — |
| `drv.plan.errors` | basic | invalid address → `J0810`; overlap → `J0811`; `forbidden` → `J0812`; coherent group too large → `J0813` (those that apply to the family) | — |
| `drv.plan.coherent` | basic | coherent points in the same batch and read in a single operation of the double | `coherent` |
| `drv.connect.ok`, `drv.connect.refused` | basic | `connect-info` per device; refusal → error with a code, link `down` | — |
| `drv.read.exact_points` | basic | one sample per point of the batch, none extra, `raw-kind` = `expect`, values from the fixture | — |
| `drv.read.timeout` | basic | `timeout` before the deadline + margin; `degraded` and `down` by the rules of §2.12 | — |
| `drv.read.device_fault` | basic | exception from the double → `device-fault` | — |
| `drv.read.cancel_safe` | basic | read canceled halfway; the next one is correct (no late response mixed in) | — |
| `drv.quality.allowed` | basic | only pairs of §2.8 | — |
| `drv.reconnect` | basic | `close` of the double → `down` → `disconnect` → `connect` → `up`, with no old state | — |
| `drv.write.order_and_acks` | basic | order on the wire equal to that of the call; one ack per write, same `seq` | `write` |
| `drv.write.atomic` | basic | atomic group in order, without interleaving; failure halfway → `skipped` on the following ones | `write` |
| `drv.write.rejected` | basic | exception → `device-rejected` | `write` |
| `drv.addr.roundtrip` | basic | `write-addr` followed by `read-addr` on the same address | `raw-access` |
| `drv.push.delivery` | basic | `push` samples delivered with `ts` and without drops below the nominal rate | `push` |
| `drv.push.cancel_safe` | basic | `push` `read` canceled for a `write`; no message lost | `push`, `write` |
| `drv.action.native` | full | `invoke` returns the record; timeout; `cancelled` in a long action | `native-actions` |
| `drv.discover.read_only` | basic | discovery and scan: the double saw no writes (`J0871`) | `discover` or `scan` |
| `drv.discover.stable` | basic | two consecutive discoveries give the same map (after re-encoding) | `discover` |
| `drv.timed.precision` | full | `at` writes applied within the declared tolerance; `late` when impossible | `timed-write` |
| `drv.relinquish` | full | `relinquish` reaches the double as a release | `relinquish` |
| `drv.health` | basic | `health` without I/O, < 100 µs | — |
| `drv.n.no_block` | basic (N) | no `poll` > 1 ms (`J0883`) | tier N |
| `drv.n.no_std` | basic (N, `micro`) | compiles for `thumbv8m.main-none-eabihf` without `alloc` in the `Driver` part | `micro = true` |
| `drv.w.limits` | basic (W) | respects `memory` and `cpu_per_call`; trap → `stopped` and recreation | tier W |
| `drv.x.handshake` | basic (X) | `hello` in ≤ 5 s; wrong token → refused; version | tier X |
| `drv.x.restart` | basic (X) | process killed → `stopped` → restart → `resync` of the outputs | tier X |
| `drv.x.sandbox` | full (X) | the process does not open destinations outside `net` nor files outside the `StateDirectory` | tier X, `systemd` |

`basic` is required to enter the catalog and to publish on the Hub; `full` is shown on the Hub as a badge. A case whose capability the driver does not declare becomes `skip`, with a note, and does not count.

### 6.5 Report

Deterministic CBOR (and `--json`): driver, version, tier, artifact digest, suite version, fixture (digest), result per case (`pass`, `fail` with the step and the detail, `skip` with the missing capability), metrics (read latency p50/p99, bytes). The Hub requires the report signed by the publication key, with every `basic` at `pass` for the published version (`J0880`). A failed case is `J0879`.

---

## 7. Codes J0850–J0899

| Code | Severity | Message |
|---|---|---|
| `J0850` | error | tier N driver not compiled into the node's runtime |
| `J0851` | error | driver descriptor missing, malformed or inconsistent with `[drivers]` |
| `J0852` | error | device key outside the driver schema (type, range, values, required key missing) |
| `J0853` | error | point address outside the driver's address schema |
| `J0854` | error | the profile or the manifest requires a capability that the driver does not declare |
| `J0855` | error | the driver's `plan` failed without a valid code, or ran out of *fuel* |
| `J0856` | error | inconsistent plan (coverage, batches, periods, limits) |
| `J0857` | error | non-deterministic plan: the node computed a different plan from `jk check` |
| `J0858` | error | the `safe` of an output is not representable in the raw type |
| `J0888` | warning | a bound of the `range` of an output is not representable in the raw type (the value is saturated) |
| `J0859` | error | `joker:driver` world version not supported by this runtime (reserved 1.2) |
| `J0860` | error-in-prod | driver component uses unstable items of the world (in 1.0 and 1.1, any W driver from outside the tree, §4) (reserved 1.2) |
| `J0861` | error | no common JDP version (reserved 1.2) |
| `J0862` | error | JDP authentication refused (peer credentials or token) (reserved 1.2) |
| `J0863` | error | JDP protocol violation; driver terminated (reserved 1.2) |
| `J0864` | error | could not start the tier X driver (executable, user, unit) (reserved 1.2) |
| `J0865` | error-in-prod | tier X driver without its own user or with privileged groups (reserved 1.2) |
| `J0866` | error | driver in crashloop (thresholds per tier in §2.12) |
| `J0867` | warning | call to the driver exceeded the deadline |
| `J0868` | error | tier W driver: trap or resource limit exceeded (reserved 1.2) |
| `J0869` | warning | invalid sample from the driver (point, raw type, quality, size) |
| `J0870` | warning | ingest ring full: samples dropped |
| `J0871` | error | write to the equipment during discovery or scan |
| `J0872` | error | tier X driver executable invalid for the node's architecture (reserved 1.2) |
| `J0873` | error | transport not supported by this host for this tier (reserved 1.2) |
| `J0874` | error | resources declared by the driver above what is allowed (reserved 1.2) |
| `J0875` | error | driver without `no_std` support on a Joker Micro target |
| `J0876` | warning | connection to the equipment failed (`connect`) |
| `J0877` | warning | write rejected or not supported by the equipment |
| `J0878` | warning | timed write applied outside the tolerance (reserved 1.1) |
| `J0879` | error | driver conformance case failed |
| `J0880` | error | driver publication without a valid conformance report (reserved 1.2) |
| `J0881` | error | tier X driver hung (health unanswered); terminated (reserved 1.2) |
| `J0882` | error | tier W driver used a forbidden import in plan mode (reserved 1.2) |
| `J0883` | warning | tier N driver blocked the `io` thread for more than 1 ms |
| `J0884` | warning | atomic group with outputs on different driver instances: the order between them is not guaranteed on the wire |
| `J0885` | error | driver descriptor name, tier, version or digest differs from the lock, or plan made with another descriptor |
| `J0886` | warning | driver above the log or progress rate; messages discarded |
| `J0887` | error | malformed driver fixture or conformance case |
| `J0889`–`J0899` | — | reserved |

In `mode = "dev"`, the `error-in-prod` codes appear as `W0860` and `W0865` (16 §9).

---

## 8. Expected behavior

**Semantics.** A driver has no semantics of its own for quality, time, envelope or arbiter: all of that belongs to the core (document 16, §2–§3) and is the same in every tier. The driver only says what the equipment answered.

**Determinism.** `plan` is pure and deterministic (§2.6). A driver's samples are not deterministic (they come from the world), but they enter the trace through the process image (16 §7), and so the replay does not need the driver. The `sim` driver is deterministic (ref/03 §3).

**Order.** Per instance: the writes go out in the order of the calls and, within a call, in the order of the list; the atomic groups are not interleaved. Between instances of the same bus, the scheduler serves by priority (§2.14). Between instances on different buses there is no guaranteed order (`J0884` if an atomic group crosses them).

**Concurrency.** One call per instance at a time, with the exception of `push` (§2.14). Between instances, everything is concurrent in the `io` domain. The acquisition stage is the single writer of the slots of the device's signals (01 §3.2), for the three tiers.

**Failures.** An N driver that panics brings down `jokerd` (ADR 0016; mitigated by the supervisor, the watchdog and the device fallback). A trapped W driver and a dead X driver only affect their own instances: `stopped`, `bad(not_connected)`, restart with backoff, `resync` (§4.5, §5.7).

**Generations.** A new generation only restarts the instances whose plan request or artifact changed (01 §15, minimal diff). A new envelope does not touch the driver. The `PointId`s may change between generations; never within one.

**Compatibility.** The contract evolves like `joker:runtime` (M21): in a minor, only optional operations, fields at the end of the records (CBOR: new keys; WIT: a new world with an adapter), cases at the end of enums, capabilities and message types are added. A driver compiled for 1.y runs on any 1.x with x ≥ y (21 §1.1).

**Security.** A driver only receives the transport that the manifest gives it (W: by capability; X: by the sandbox; N: by rule and review). No tier receives proposals or may write without going through the envelope: drivers only see applied writes and authorized action steps (ADR 0005). Secrets (`secret`) arrive only in `open`, never in `plan`, logs or reports.

---

## 9. Stress

**D1 — A W driver calls the transport inside `plan`.** On the laptop, `jk check` runs `plan` in plan mode; the call traps with `J0882`, and the message says which import was used. *What broke:* in the first version, `plan` ran in the same instance as `connect`, and a driver could "validate" the address by making a request to the equipment, which made `jk check` depend on the network. **Fix incorporated:** plan mode in an instance of its own, with only `log` (§4.4), and the same contract checked in N by the case `drv.plan.pure`.

**D2 — Ten meters on an RS-485 and one hangs.** A meter stops responding. Each of its batches spends the whole `timeout`, and the scheduler serializes the bus. *What broke:* without a rule, the driver did its own retries inside `read`, and a dead meter with `retries = 3` took 4 × 300 ms per poll, stealing the bus from the other nine. **Fix incorporated:** one attempt per call, core retries only if they fit in the period (§2.14), and the device goes `down` after 3 total failures (§2.12), with the poll moving to the backoff rate. The `J0370` budget already counts the worst case of one timeout per cycle.

**D3 — An X driver dies in the middle of an atomic group.** The `WRITE` of `pcs.mode` went out, that of `pcs.p_set` did not. The acks never arrive. `jokerd` marks `not-connected` on both writes, the group becomes `uncertain(partial_write)` (02 §7.3), the points `bad(not_connected)`. Restart in 1 s; after the `connect`, the `resync` resends the whole group in order. **Holds.** The interval without control is covered by the equipment fallback, as with an N driver that is down.

**D4 — A vendor SDK with a buffer overflow writes garbage into the shared memory.** The process writes `up.head = 2⁶³` and random bytes into the entries. *What would break* with a single shared-write memfd: the driver could also rewrite the `down` requests or `jokerd`'s `tail`, and `jokerd` could read an entry that was changing while it interpreted it (TOCTOU). **Fix incorporated:** two memfds with a single writer each, `jokerd`'s sealed with `F_SEAL_FUTURE_WRITE` (§5.5.1), copy before validating and index checks (§5.5.6). The impossible `head` is `J0863`: SIGKILL and restart.

**D5 — Another X driver tries to talk on a neighbor's socket.** The `acme-bms` process connects to `/run/joker/drv/bacnet-ip.sock` (the `joker-drv` group gives it access to the directory). `SO_PEERCRED` shows a pid that does not belong to the `joker-drv-bacnet-ip.service` cgroup: `J0862`, connection closed. Even if it got through, it would lack the token. **Holds**, with two independent checks (§5.2.3).

**D6 — `jokerd` is not root and the driver must run as another user.** *What broke:* document 01 §17 says at the same time that `jokerd` does not need root and that tier X drivers run with their own user; a process without `CAP_SETUID` cannot start another one with another user. **Fix incorporated:** startup through transient systemd units with `DynamicUser` and a polkit rule limited to `joker-drv-*` (§5.2.1); `spawn` only with explicit `CAP_SETUID`, and `J0865` in `prod` without its own user.

**D7 — An MQTT (push) driver with a pending write.** The `push` read is waiting for messages; a write arrives. The core cancels the read, does the write, and arms the read again. *What broke:* a driver that had taken a message from the MQTT client, but had not yet delivered it, lost it on cancellation. **Fix incorporated:** `push` reads must be *cancel-safe* (§2.14), and the case `drv.push.cancel_safe` catches it. In tier X the problem does not exist (the process is concurrent; §5.5.4).

**D8 — An N driver calls a blocking C library.** A 40 ms call holds a tokio `io` thread, and the batches of other devices on that thread are delayed. The case `drv.n.no_block` measures each `poll` and fails above 1 ms (`J0883`). The way out is `cx.spawn_blocking` (with a deadline) or tier X. **Holds**, with the rule written and tested (§3.5).

**D9 — The same driver in three tiers.** An integrator writes an `acme-bms` driver in Rust, tests it as N in `cargo test`, publishes it as X (it has to link a vendor library) and, when the vendor opens the protocol, as W. The three conformance reports give the same results; only the latency changes. **Holds**, because the types, the operations and the codes are the same (§3.1), and there is a single suite (§6).

**D10 — Crossed versions.** A 1.3 node receives a W driver compiled for `joker:driver@1.2.0` (accepted, through the previous-minor adapter) and another for `@1.4.0` (rejected by the agent before the deploy, `J0859`). An X driver with `libjdp` 1.5 speaks JDP 1 with a 1.0 `jokerd`, because the `welcome` chooses the version. *What broke:* 05 §8 imported `joker:runtime/log@0.3.0` in a `joker:driver@0.1.0`, which made it impossible to say which runtime a driver is meant for. **Fix incorporated:** the two versions are always equal (§4.1).

**D11 — A W driver that is slow on the CPU.** A DLMS driver does AES in software and spends 30 ms per read. It yields the thread every 1 ms (epoch with *yield*), so it does not hold an `io` thread; it adds up to 30 ms, below the 50 ms of `cpu_per_call`. With 80 ms, it traps (`J0868`) and the integrator raises `cpu_per_call` in `[drivers]` or rewrites the driver. **Holds**, with the limit visible and adjustable.

**D12 — The Joker Micro driver with the plan on the laptop.** `jk build --target mcu:rp2350` runs the Modbus RTU `plan` on the laptop, and the plan goes into the blob. The firmware in the field has another version of the driver (a patch). *What would break:* a plan made with version X executed by version Y, with `private` in a different format. **Fix incorporated:** the blob carries the digest of the descriptor with which the plan was made, and the firmware rejects a plan from another descriptor (`J0885`; the `HELLO` of Joker Link reports it, ref/07). Changing the firmware's driver forces the generation to be recompiled.

---

## 10. Decisions and rejected alternatives

| Decision | Rejected alternatives | Why |
|---|---|---|
| **One contract, three hosts**; the same Rust trait serves N, W (SDK) and X (`joker-jdp`) | three APIs designed separately | a driver changes tier without being rewritten; a single suite |
| `plan` separate from the driver's runtime (`Planner` with `alloc`, `Driver` `no_std`) | a single trait with `plan` | the plan runs on the laptop and in the MCU build; the firmware does not need `alloc` |
| Action steps, `[connect]`, `[relinquish]` and fallback run **by the core** over `read-addr`/`write-addr` | each driver implements steps and M1 | the semantics of actions and fallback stays in one place; a new driver gets them for free |
| Raw ↔ engineering encoding in the core, including `word_order` and `round_ties_even` rounding | the driver returns the value already scaled | 05 §2 and ADR 0003: one implementation, two drivers do not diverge; determinism across architectures |
| Retries and backoff in the core; one attempt per call | retries inside the driver | predictable bus budget (D2) |
| Push as a long cancel-safe read | `subscribe` with the driver's own task | works in `no_std` (no dynamic spawn) and with `&mut self`; in X, natural push through `jdp_push` |
| Configuration and addresses in CBOR **with a declared schema**; the rest in WIT | everything in WIT (one type per driver) or everything in bytes | what is generic stays typed; what belongs to the protocol is validated by the core with identical messages in every driver |
| JDP over `SOCK_SEQPACKET` with CBOR, and data in SPSC rings of 64 B entries | gRPC, protobuf, stream with a length prefix, single ring | CBOR is the project's encoding (ref/00 §7); seqpacket preserves boundaries and carries fds; 64 B = one cache line |
| Two memfds with one writer each, one sealed `F_SEAL_FUTURE_WRITE` | one shared memfd | the driver cannot corrupt `jokerd` (D4) |
| Startup through transient systemd units with `DynamicUser` | `jokerd` with root or with `CAP_SETUID` by default | 01 §17 without contradiction (D6); sandbox with the OS's tools |
| One X process per driver by default (`per_device` optional) | always one process per device | vendor SDKs are often singletons; fine-grained isolation remains available |
| `stopped` state and `not_connected` reason for a dead driver; `comm_failure` for equipment that does not respond | a single reason | distinguishes "the software went down" from "the cable went down", which call for different reactions |
| WIT interfaces `guest`/`host`/`transport` (the name `driver` was kept for the world) | `driver` interface | WIT does not allow an interface and a world with the same name |

---

## 11. Version markers

| Item | Version |
|---|---|
| common contract (§2), capabilities except `block-read` and `streams` | **1.0** |
| tier N: `joker-hal`, registry by features, `rt-cycle`, `no_std` on Joker Micro | **1.0** |
| tier X: complete JDP 1 (control, rings, supervision, sandbox), `libjdp`, `joker-jdp` | **1.2** (the host; document 26, §5; ADR 0035). The shared structures of §2 stay 1.0 |
| tier W: `joker:driver` world (version pinned with `joker:runtime`) and `joker-driver-sdk` **unstable** (`@unstable(feature = driver-world)`, only in-tree drivers in `prod`, §4); host with `serial`, `tcp`, `udp` | **1.2** (the host; document 26, §5; ADR 0035). Until then the world is only a closed draft |
| tier W **stable and open to third parties** (`@since(1.2.0)`): world, Rust and C SDK, `i2c`, `spi`, `can` transports in the host (`w-i2c-spi-can`), mandatory conformance, publication on the Hub | **1.2** (21 §4) |
| conformance suite: interface, `basic` cases, report | **1.0** (used by the catalog); mandatory for publishing on the Hub in **1.2** |
| catalog drivers of 1.0 | **1.0**: `sim`, `modbus-tcp`, `modbus-rtu` (with `sunspec`), `gpio`, `sysfs`, `i2c`, `mqtt` (input), `joker-link`; on the MCU, `gpio`, `adc`, `pwm`, `i2c`, `modbus-rtu`, `esp32`, `extern` |
| catalog drivers of 1.0.x | `revpi` (implemented, ADR 0078), `spi`, `opcua` (client), `m-bus`, `onewire`, `serial-line`; on the MCU, `modbus-slave`, `spi`, `uart` and `onewire` |
| catalog drivers of 1.1 | `can`, `canopen` (on Linux and on the MCU) |
| streams of X drivers through `SCM_RIGHTS` (messages 40–42), `streams` capability | **1.1** (M4; already reserved in the CDDL, document 11, §"Ready for ML") |
| block signals, `block-read` capability | **1.1** (M7; reserved) |
| Wasm on Joker Micro (W drivers with `wasmi`) | **1.4** (21 §6); the world is the same |
| building and energy drivers over this contract (`bacnet-ip` X, `dlms-cosem` W, `iec-104` X) | **1.2** in the catalog (BACnet in any tier, 21 §4); afterwards, new drivers in any minor (ADR 0022, rule 3) |
