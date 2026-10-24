# ref/00 — Cross-cutting interface conventions

This document fixes the rules that **all** Joker interfaces share: TOML files, `.jkc`, WIT, SDKs (Rust, Python, TypeScript), drivers (tier N, W, X), Joker Link, C ABI, HTTP API and agent protocol. Each reference in `docs/ref/` applies these rules and does not repeat them. A reference that contradicts them is wrong.

It is **normative**, with the same precedence as document 16 (ADR 0023). Everything that is semantics (arbiter, envelope, quality, expressions) stays in 16. Only the **representation** lives here.

---

## 1. The surfaces and their versions

| Surface | Version identifier | Where the reference is |
|---|---|---|
| runtime (`jokerd`, `jk`) | product SemVer (`1.4.2`) | — |
| manifest, overlays | `[project] joker = "1.4"` (minimum runtime version) | ref/01 |
| lock, `node.toml`, client configuration, `secrets.toml.enc` | `version = 1` | ref/02 |
| profiles, boards, simulation, scenarios, conformance | `schema = 1` as the **first key/value pair** (only comments and blank lines before it) | ref/03 |
| `.jkc` classes | class version (`pump@1`) + implicit `jkc = 1` | 17 |
| app world | WIT package `joker:runtime@0.x.y` until 1.0; **`joker:runtime@1.0.0` in 1.0** | ref/04 |
| tier W drivers | WIT package `joker:driver@…`, with the **same version** as the `joker:runtime` it imports; `@unstable` in 1.0 and 1.1 (in-tree drivers only), stable in 1.2 | ref/06 §4 |
| tier X drivers (JDP) | integer `jdp` in `hello` and in `welcome` (1 in v1); `jokerd` supports the current one and the previous one | ref/06 |
| Rust, Python, TypeScript SDKs | package SemVer; the SDK declares the WIT world it generates | ref/04, ref/05 |
| Joker Link | `link_version` (u8 major, u8 minor) in `HELLO` | ref/07 |
| Joker Micro C ABI | `JK_ABI_VERSION_MAJOR` / `JK_ABI_VERSION_MINOR` | ref/08 |
| generation blob (`.jkgen`) and firmware descriptor (`.jkfw`) | `format` and `expr_isa` in the header | ref/08 |
| local HTTP API | prefix `/v1`; `info.version` in the OpenAPI | ref/09 |
| agent protocol (node ↔ Deck) | `agent/1` | ref/10 |
| OCI artifacts | media type with `.v1` (table in §7.1) | §7.1 |

**Compatibility rule (ADR 0022) applied to the representation:**

1. A minor only **adds**: optional fields, new messages, new variants at the end of an enum, new endpoints.
2. **Readers tolerate the unknown** in machine-to-machine interfaces (JSON, CBOR, WIT via adapter): unknown fields are ignored; an unknown enum variant is mapped to the `unknown` variant or to the category (an unknown quality reason counts by its `quality-code`). This applies to whoever **reads responses and messages**. A server is strict with the **requests** it receives: an unknown field in an HTTP request is `J-E1011`. Each new case of a WIT enum declares its projection onto the previous worlds (ref/04 §2.8).
3. **Files written by people are strict:** an unknown key in a TOML is an error (`J0140`), because it is almost always a typo. Extension keys start with `x_` and are ignored.
4. A field number, an integer CBOR key or a code is **never reused** after being removed.

---

## 2. Names

| Where | Style | Example |
|---|---|---|
| TOML keys, JSON keys, enum values in JSON | `snake_case` | `max_lease`, `"comm_failure"` |
| signal paths, names of devices, apps, instances, groups | `snake_case`, grammar of document 02, §9 | `pump1.run_cmd` |
| WIT | `kebab-case` (WIT rule) | `quality-reason`, `write-at` |
| Rust | Rust conventions (`snake_case`, `CamelCase`) | `last_outcome()`, `WriteOutcome` |
| Python | PEP 8 | `last_outcome()`, `WriteOutcome` |
| TypeScript | `camelCase` for the SDK API, `PascalCase` for types | `lastOutcome()`, `WriteOutcome` |
| C | prefix `jk_` for functions and types, `JK_` for macros and constants | `jk_tick`, `JK_GOOD` |
| errors and warnings | codes of document 16, §9 | `J0107`, `J-E0901` |

**Names coming from the user are not translated.** Ports, parameters, class members, signals and enum variants declared by the user stay `snake_case` in every language, including TypeScript (`pump.run_cmd`, `inputs.grid_p`). Only the SDK API follows the language's style. This way a name is found with a `grep` across all files of a project.

**Reserved:**

- paths: the first segment `joker` (the runtime) and `board` (fixed IO of the boards, `board.<name>`); the first segment `@` (node-qualified paths in v2, document 21, §8);
- app names `joker*`; the node name `local`; TOML keys starting with `_`;
- app port names, in every language: `now`, `dt`, `kv`, `secrets`, `snapshot`, `subscribe`, `sleep`, `sleep_until`, `log`, `cmd`, `state`, `http`, `random`, `meta`;
- class member names: `name`, `index`, `health`, `read`, `raw`, `has` (document 17, §3.2).

**Enum values in TOML** are written in `snake_case` (`"clamped_rate"`, `"bad(comm_failure)"`). The CLI display forms (`Clamped(Rate)`, `Bad(CommFailure)`) are accepted as exact synonyms in scenarios and in conformance cases, and `jk fmt` normalizes them.

**File names** (ADR 0026). The type of each file is in its name, in the form `<name>.<type>.toml`; the content must match the suffix (`J0803`, ref/03 §2.1.6):

| Type | Name | Example | Reference |
|---|---|---|---|
| manifest | `joker.toml` | `joker.toml`, `micro/esp/joker.toml` | ref/01 |
| overlay | `joker.<variant>.toml`, in any directory | `joker.lab.toml`, `templates/joker.bess-v2.toml` | ref/01 §7.7 |
| lock | `joker.lock`; `joker.<variant>.lock` for each input overlay | `joker.lab.lock` | ref/02 §2.1.1 |
| equipment profile | `<model>@<major>.profile.toml` | `profiles/local/bench-esp32@1.profile.toml` | ref/03 §2.2 |
| board profile | `<board>.board.toml` | `boards/rpi-zero-w.board.toml` | ref/03 §2.3 |
| simulation model | `<name>.sim.toml` | `sim/esp.sim.toml`, `classes/std/valve.sim.toml` | ref/03 §2.4 |
| scenario | `<name>.scenario.toml` | `sim/scenarios/esp_fault.scenario.toml` | ref/03 §2.5 |
| conformance suite | `<name>.conformance.toml` | `conformance/envelope/rate.conformance.toml` | ref/03 §2.6 |
| driver fixture | `<driver>.fixture.toml` | `drivers/modbus/conformance/modbus-tcp.fixture.toml` | ref/06 §6.2 |
| driver descriptor (TOML source) | `<driver>.driver.toml` | `joker-drv-acme.driver.toml` | ref/06 §5.8 |
| class package | `<package>.package.toml` | `classes/acme/water/water.package.toml` | ref/03 §2.7 |
| class | `<name>.jkc` (`<name>@<major>.jkc` with two majors) | `classes/std/pump.jkc` | 17 |
| node, client, Deck, fleet, secrets, code registry | fixed names | `node.toml`, `config.toml`, `deck.toml`, `fleet.toml`, `secrets.toml.enc`, `codes.toml` | ref/02, ref/10 |

`<variant>` follows the grammar of board names (`[a-z0-9]+ ("-" [a-z0-9]+)*`, ≤ 32 characters). References are written without the suffix when they are names (`profile = "local/bench-esp32@1"`, `board = "rpi-zero-w"`: resolution adds it) and with it when they are paths (`model = "sim/esp.sim.toml"`).

---

## 3. Time

| Quantity | Binary (WIT, trace, JDP) | Microcontroller (C ABI, Joker Link) | Text (TOML, JSON, CLI) |
|---|---|---|---|
| monotonic instant | `u64` ns | `u64` µs, epoch at MCU boot | decimal string (`"mono_ns": "81234567890"`) |
| UTC instant | `s64` ns since 1970-01-01T00:00:00Z | `s64` µs (only after `TIME`) | RFC 3339, `Z`, up to 9 digits (`"2026-09-24T10:00:00.123456789Z"`) |
| duration | `u64` ns | `u32` µs (up to ~71 min) or `u32` ms (`_ms` fields) | duration literal (§3.1) |
| clock quality | enum `clock-quality` | same, numeric (§6) | `"synced"`, `"holdover"`, `"unsynced"` |

- Leap seconds do not exist: UTC is counted the way POSIX counts it.
- A monotonic instant **only has meaning on the node that produced it** and in the clock generation (document 01, §18). It never leaves the node without the UTC pair beside it.
- `jokerd` converts µs ↔ ns at the Joker Link boundary.
- **Joker Link exception:** an MCU's messages carry only the monotonic time in µs. The UTC pair is established by `TIME` and kept on the Linux node, which does the translation (ref/07 §6.23).
- In TypeScript, `i64`, `u64` and instants in ns are always `bigint` (the monotonic time in ns exceeds 2⁵³ after ~104 days).

### 3.1 Duration literals

```
duration := number space? unit
unit     := "ns" | "us" | "µs" | "ms" | "s" | "m" | "min" | "h" | "d"
```

- `m` and `min` are minutes **only where a duration is expected** (duration keys, window arguments, `--ttl`). In a physical quantity, `m` is the meter. There is never any ambiguity, because the type of the position is known.
- There are no compounds (`1h30m`): write `"90m"`.
- Decimals are allowed (`"1.5s"`); the value is rounded to ns.
- In the **instants** of scenarios and conformance cases (`at`), and only there, a sum `duration + duration` is accepted (`"2m+1.2s"`); in driver suites, also the term `timeout` (`"timeout+10ms"`, ref/03 §5).
- **Canonical output form** (generated files, API responses, `jk fmt`): integer in the largest exact unit, with `m` for minutes (`"90m"`, `"1500ms"`, `"2h"`).

### 3.2 Quantity literals

`"125 kW"`, `"-20 kW/s"`, `"49.8 Hz"`, `"5 %"`. The space between number and unit is optional. **In the manifest and in catalog files**, a quantity with a unit always requires the literal with its unit (`J0151`); only machine interfaces (API JSON) accept a plain number, interpreted in the signal's unit. The units are those of the catalog in document 16, §5. Memory and disk sizes use IEC prefixes: `"512 MiB"`, `"1 GiB"`.

---

## 4. Values, samples and quality

### 4.1 Values

| Signal type | Binary (`u64` of the slot, the trace and the Link) | JSON |
|---|---|---|
| `bool` | 0 or 1 | `true` / `false` |
| `i64` | two's complement | number if \|v\| < 2⁵³, otherwise decimal string |
| `u64` | as is | number if v < 2⁵³, otherwise decimal string |
| `f64`, `f32` | IEEE 754 bits of `f64` (`f32` promoted) | number; `NaN` and infinities do not exist in JSON and only appear with quality `bad(nan)` and `"value": null` |
| `enum<N>` | numeric value of the variant | variant name (`"run"`) |
| `bits<N>` | the word | number, and `"bits": ["alarm_overtemp", …]` with the active bits named |
| `string`, `bytes` | the wide slot (≤ 64 B, does not fit in `u64`; ADR 0062); in the trace v1, the length in bytes | UTF-8 string; `bytes` in base64 with the standard alphabet and padding (RFC 4648 §4), the only form accepted on input |
| record (events, arguments and results of actions and methods, `Internal<T>`, mailbox messages) | **deterministic CBOR** (RFC 8949, §4.2.1), map with **integer keys = position of the field in the schema** (`joker:meta` or `.jkc` contract); within a major, fields are only added at the end | JSON object with the field **names** |

**Unit:** a numeric value is always expressed in the **declared unit of the signal**. Text interfaces show the unit alongside (`"unit": "kW"`) and accept, on input, a number (in the signal's unit) or a quantity literal (`"50 kW"`, converted; a wrong dimension is `J0107`).

### 4.2 Sample in JSON

```json
{
  "path": "meter.p_total",
  "value": 412.5,
  "unit": "kW",
  "quality": "good",
  "reason": "none",
  "ts_src": "2026-09-24T10:00:00.120000000Z",
  "ts_acq": "2026-09-24T10:00:00.123456789Z",
  "seq": 88123
}
```

`ts_src` and `ts_hw` only appear when they exist. The `ts_acq` in JSON is already translated to UTC; the monotonic time is available by requesting `?mono=true` (ref/09). Optional fields, requested by parameter: `age`, `acq_id`, `bits`, `ts_acq_mono_ns`, `ts_hw_mono_ns` (ref/09 §3.3).

### 4.3 Numeric quality codes

Used in the slot (document 01, §3.2: code in bits 0–7, reason in bits 8–15, flags in bits 16–31), in the trace, in Joker Link and in the C ABI.

| Code | Value | | Reason | Value | | Reason | Value |
|---|---|---|---|---|---|---|---|
| `good` | 0 | | `none` | 0 | | `comm_failure` | 12 |
| `uncertain` | 1 | | `overridden` | 1 | | `device_fault` | 13 |
| `bad` | 2 | | `local_override` | 2 | | `out_of_range` | 14 |
| | | | `stale` | 3 | | `config_error` | 15 |
| | | | `not_confirmed` | 4 | | `not_connected` | 16 |
| | | | `substituted` | 5 | | `nan` | 17 |
| | | | `out_of_distribution` | 6 | | `tripped` | 18 |
| | | | `sensor_cal` | 7 | | `deadline` | 19 |
| | | | `clock_unsynced` | 8 | | | |
| | | | `initial` | 9 | | | |
| | | | `frozen` | 10 | | | |
| | | | `partial_write` | 11 | | | |

The order is that of the WIT enum `quality-reason`. Values 20 to 239 are reserved for future reasons; 240 to 255, for private use by drivers (they appear as reason 0 outside the node).

### 4.4 Write results

| Result | Value | Reason (for `rejected`) | Value |
|---|---|---|---|
| `none` (no proposal in this tick) | 0 | `wrong_type` | 1 |
| `accepted` | 1 | `interlock` | 2 |
| `clamped_range` | 2 | `quality` | 3 |
| `clamped_rate` | 3 | `range` | 4 |
| `suppressed` | 4 | `atomic_group` | 5 |
| `overridden` | 5 | `not_owner` | 6 |
| `deferred` | 6 | `clock_unsynced` | 7 |
| `shadowed` | 7 | `budget` | 8 |
| `rejected` | 8 | | |
| `tripped` | 9 | | |

Document 02, §7.1, writes `Clamped(Range|Rate)`: these are values 2 and 3. WIT distinguishes them with `clamped(clamp)` (ref/04). In JSON: `{"outcome": "clamped_rate", "applied": 1.0}`, `{"outcome": "rejected", "reason": "interlock"}`, `{"outcome": "overridden", "level": "manual:3"}`.

---

## 5. Identifiers

| Thing | Text form | Binary form | Rule |
|---|---|---|---|
| signal | path (`pump1.run_cmd`) | `SignalId` `u32` | the `SignalId` is **dense and valid only within one generation**, in the byte order of the canonical paths; it never leaves the node without the registry hash beside it (ordering and hash encoding: ADR 0031) |
| signal, stable across generations | — | *path-hash*: first 8 B of the SHA-256 of the canonical path | binds the external interface of an MCU to each generation (ref/08) |
| public key | key: `x25519:<base64>`, `ed25519:<base64>`; key id: `ed25519:<32 hex>` (the first 16 B of the SHA-256 of the public key) | 32 B; id 16 B | ref/02 §2.5 |
| arbiter level | `"<band>:<level>"` (`"optimizer:13"`); the level is absolute (1–16) and must be inside the band | `u8` 1–16 | bands of document 01, §6 |
| writer | `app:<name>`, `user:<principal>`, `bridge:<name>`, `impl:<instance>`, `intent:<id>`, `link:<device>` (remote writer on an MCU, 16 A12) | `u32` per generation; in the trace, the remote writers of an MCU are ≥ `0x8000_0000` | A1 of document 16 |
| principal | `unix:<user>`, `cert:<CN>`, `deck:<deck_id>/user:<id>`, `deck:<deck_id>/group:<g>` | — | ref/09 §2.2, ref/10 |
| generation | integer `gen` assigned at creation, and `sha256:<hex>` of the content | 32 B | the hash identifies, the integer orders creation; a rollback **reactivates** the previous generation (same hash, same `gen`) and the history records the activation with `origin` |
| artifact digest | `sha256:<hex>` (OCI) | 32 B | |
| node | `[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?` (DNS label) | string | unique within an organization |
| site | same grammar | string | |
| class | `<package>/<name>@<major>` (`std/pump@1`; `std/` may be omitted) | contract hash, 32 B | document 17, §9 |
| instance, member | path (`pump1`, `pump1.start`) | `u32` instance, `u16` member | |
| idempotency key | printable ASCII string, 1–64 B | same | `dedup_window` window (E9) |
| intent, request, session | ULID (`01J8…`, 26 characters) | 16 B | sortable by time |

---

## 6. Shared numeric enumerations

| Enum | Values |
|---|---|
| `clock_quality` | `synced` 0, `holdover` 1, `unsynced` 2 |
| `alarm_state` | `normal` 0, `active_unacked` 1, `active_acked` 2, `rtn_unacked` 3, `unknown` 4 |
| `severity` | `low` 0, `medium` 1, `high` 2, `critical` 3 |
| `role` | `viewer` 0, `operator` 1, `admin` 2 |
| `writer_kind` | `cycle` 0, `async` 1, `manual` 2, `bridge` 3, `impl` 4, `proposal` 5 |
| `app_state` | `starting` 0 (loading, compiling or in `init`), `running` 1, `crashloop` 2, `stopped` 3 (stopped by a person or by the generation), `failed` 4 (`init` failed or the app does not load), `shadow` 5 (shadow mode, M16: proposals recorded, not applied), `updating` 6 (hot swap in progress, 01 §9); it is the only enum of an app's states, in the API (ref/09 §5.14), in the OpenAPI (`AppState`) and in the agent (`app-state`, ref/10 §2.2) |
| `generation_state` | `pending` 0, `confirming` 1, `confirmed` 2, `rolled_back` 3, `failed` 4, `discarded` 5 |
| `mode` | `dev` 0, `prod` 1 |
| `link_state` (devices) | `connecting` 0, `up` 1, `degraded` 2, `down` 3, `drift` 4, `stopped` 5 |
| `session_mode` (Joker Link) | `full` 0, `observe` 1, `maintenance` 2, `refused` 3 |
| `ota_kind` | `generation` 1, `firmware` 2, `model` 3 (1.1 on Linux; model OTA for MCUs in 1.4), `app` 4 (1.4); 0 is never used |
| signal type | `bool` 0, `i64` 1, `u64` 2, `f64` 3, `f32` 4, `enum` 5, `bits` 6, `string` 7, `bytes` 8, `record` 9 (`Internal<T>`) |

---

## 7. Encodings

| Where | Encoding | Why |
|---|---|---|
| files written by people | TOML 1.0, UTF-8 (ADR 0018) | readable, reviewable in a PR |
| HTTP API, WebSocket, agent protocol over HTTPS | JSON (RFC 8259), UTF-8; the agent may negotiate CBOR | debuggable, universal tooling |
| Joker Link, agent protocol over MQTT/CoAP, JDP (control), `joker:meta`, arguments and events in WIT | CBOR (RFC 8949) with **integer keys** in the protocol messages | compact, `no_std`, standardized |
| everything that is signed or hashed | **deterministic** CBOR (RFC 8949, §4.2.1) | the same content always gives the same bytes |
| signed documents (desired state, trust bundle, generation blob, OTA manifest, signed reports) | COSE_Sign1 (RFC 9052), **always in CBOR**, also when they travel over HTTPS; algorithms `EdDSA` (Ed25519, organization keys) and `ES256` (P-256, node identity keys in a TPM or secure element) | a single signature family, also on MCUs (ADR 0025) |
| node TLS identity | **P-256** X.509 certificates issued by the organization's CA | supported by TPM 2.0 and secure elements |
| OCI artifacts | signatures compatible with cosign/notation, alongside the artifact | existing registry tooling |
| trace | binary of document 16, §7 | |

The schemas of CBOR messages are written in **CDDL** (RFC 8610), in `spec/cddl/`. The JSON schemas of the HTTP API are in OpenAPI 3.1 (`spec/openapi/joker-api.yaml`). WIT is in `wit/`. **CRC:** wherever there is a CRC (Joker Link, generation blob, persistence records on the MCU), it is always **CRC-32C** (Castagnoli), in little-endian (ref/07 §3.2).

### 7.1 Media types

The short form (`joker.app.v1+wasm`) is the one used in the documents and in the API; in the OCI registry, the full `mediaType`/`artifactType` is `application/vnd.joker.<short form without "joker.">` (`application/vnd.joker.app.v1+wasm`).

| Short form | Content | Signing role | Reference |
|---|---|---|---|
| `joker.runtime.v1` | `jokerd` binary (multi-platform index) | `runtime` | ref/09, ref/10 |
| `joker.config.v1+cbor` | descriptor of a generation of a Linux node (effective manifest, lock, contracts, artifacts by digest) | `config` | ref/10 §2.6 |
| `joker.manifest.v1+toml`, `joker.lock.v1+toml`, `joker.provenance.v1+json` | the parts of a generation sent to the API | (inside the generation) | ref/09 §5.13 |
| `joker.app.v1+wasm`, `joker.app.v1+cwasm` | component of an app; AOT for one architecture | `apps` | ref/04 |
| `joker.driver.v1+wasm`, `joker.driver.v1+tar` | tier W driver; tier X driver (multi-platform index) | `drivers` | ref/06 §4.7, §5.9 |
| `joker.profile.v1+toml`, `joker.board.v1+toml`, `joker.sim.v1+toml` | profile, board, simulation model | `profiles`, `boards` | ref/03 §2.8 |
| `joker.class.v1+jkc`, `joker.class.contract.v1+json`, `joker.class.conformance.v1+toml` | class and its layers | `classes` | ref/03 §2.8 |
| `joker.class.package.v1+toml` | class package file | `classes` | ref/03 §2.7 |
| `joker.conformance-report.v1+json` | conformance report (*referrer*) | whoever produced it | ref/03, ref/06 §6.5 |
| `joker.micro-firmware.v1`, `joker.generation.v1` | Joker Micro firmware; `.jkgen` generation blob | `firmware`; `config` | ref/08 |
| `joker.secrets.v1` | sealed set of secrets for a node | `secrets` | ref/02 §2.4 |
| `joker.key.v1+pem` | public key | — | ref/09 |
| `joker.trace.v1` | `.jktrace` trace | — | 16 §7 |
| `joker.model.v1+jkm` | ML model | `models` | **reserved 1.1** |

---

## 8. Errors

There is **a single vocabulary** of errors: the codes of document 16, §9, in the `codes.toml` registry. Each surface wraps it in its own way:

| Surface | Form |
|---|---|
| `jk` | rustc style (code, location, help) and exit codes of document 07, §8 |
| HTTP API | `application/problem+json` (RFC 9457) with `code` (`"J0213"`), `title`, `detail`, `status` and, when there is a file, `spans` |
| WIT | `result<_, error>` with `record error { code: string, message: string }` wherever there are errors the app should distinguish; `string` only where the app decides nothing with it |
| Joker Link, JDP, agent protocol | a small numeric `status` of the message itself, plus an optional `code` (registry string) |
| C ABI | `jk_status_t` (`JK_OK` = 0, negative errors) |
| SDKs | the language's error types, which carry the code (`err.code == "J-E0901"`) |

An error seen by a person **always** carries the code, so that `jk explain <code>` works.

**New ranges** (added to the table of document 16, §9):

| Range | Area |
|---|---|
| `J014x`–`J019x`, `J023x`–`J029x` | manifest and overlays (ref/01) |
| `J041x`–`J049x` | lock (ref/02) |
| `J073x`–`J076x` | Joker Link, validation (ref/07) |
| `J077x`–`J079x` | Joker Micro, validation (ref/08) |
| `J0800`–`J0849` | profiles, boards, simulation, scenarios, conformance, packages (ref/03) |
| `J0850`–`J0899` | tier N, W, X drivers (ref/06) |
| `J0900`–`J0929` | node, client, trust, secrets (ref/02) |
| `J0930`–`J0999` | enrollment, agent protocol, sessions (ref/10) |
| `J-E0001`–`J-E0799`, `J-E0900`–`J-E0999` | runtime errors seen by apps and of the Rust SDK (ref/04) |
| `J-E0800`–`J-E0899` | tier 2 SDKs and client libraries (ref/05) |
| `J-E1000`–`J-E1999` | HTTP API, WebSocket, SSE and sessions (ref/09) |
| `J-E2000`–`J-E2499` | Joker Link at runtime (ref/07) |
| `J-E2500`–`J-E2999` | C ABI and Joker Micro crate (ref/08) |

In the C ABI, `jk_status_t` is 0 on success, negative on error and positive for informational results (`JK_EMPTY`, `JK_PENDING`, `JK_RELINQUISHED`).

---

## 9. Common security

- **Roles:** `viewer`, `operator`, `admin` (document 01, §14). Each endpoint, command and message declares the minimum role.
- **Writes coming from outside the node** through the API over TCP, through the tunnel of a remote session and through the agent protocol carry a `nonce` (ULID) and `ts` (UTC), and are rejected with a repeated nonce or, with the clock `synced`, outside ±30 s (document 01, §17). With the clock `holdover` or `unsynced`, only the nonce and the binding to the target are checked, and this is marked in the audit. The nonce memory never forgets: if it fills up, requests are rejected (`J-E1029`).
- **Bridges** (OPC UA server, Modbus server, ROS 2) authenticate through their own protocol and write as declared writers, with level and lease (document 01, §14); they do not carry a nonce.
- **Manual writes** always carry a TTL (A10).
- **No interface exposes secrets:** a secret is referred to by name (`secret:mqtt_password`), never by value.

---

## 10. Common limits

| Thing | v1 limit |
|---|---|
| path segment | 32 characters; 8 segments |
| `string` in a signal | 64 B UTF-8 |
| `bytes` in a signal | 64 B |
| string in an event or argument | 1 KiB |
| event (complete record, CBOR) | 8 KiB |
| arguments or result of a method or action | 4 KiB |
| mailbox message between cycle and async | 256 B |
| `kv` of an app | key 128 B; value 64 KiB |
| proposal buffer of an `on-cycle` | 16 KiB by default (ref/04 §3.3) |
| list in an event or argument | 64 elements |
| internal app record (`Internal<T>`) | 64 B |
| arbiter levels | 16 |
| idempotency key | 64 B |
| Joker Link frame | `max_frame` 512–4 096 B (default 1 024) |
| Joker Link message | `max_msg` 2 048–65 535 B (default 4 096) |
| HTTP request body | 16 MiB (artifacts go by chunked upload, ref/09) |
