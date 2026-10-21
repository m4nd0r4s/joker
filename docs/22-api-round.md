# 22 — API round: the interfaces closed

This round took **all the interfaces** of Joker and wrote them down as an exhaustive reference, so that the implementation works on specifications only. The result is in `docs/ref/` (eleven documents) and in the machine-readable files in `wit/` and `spec/`. This document tells what was decided, where the references contradicted each other and how that was resolved, and what was attacked end to end.

---

## 1. What exists now

| Reference | Surface | Machine-readable |
|---|---|---|
| `ref/00-conventions.md` | common representation: versions, names, time, values, numeric codes, identifiers, encodings, errors, security, limits | — |
| `ref/01-manifest.md` | `joker.toml` and overlays: all the sections and keys, grammar of the values, effective manifest | (JSON schema generated from the types, 16 §10) |
| `ref/02-lock-node-and-client.md` | `joker.lock`, `/etc/joker/node.toml`, `/var/lib/joker/`, the `jk` client, secrets, trust | `spec/cddl/secrets.cddl` |
| `ref/03-profiles-boards-sim.md` | profiles, boards, simulation, scenarios, conformance (core and classes), packages and Hub | `wit/joker-sim.wit` |
| `ref/04-wit-and-rust-sdk.md` | `joker:runtime` world, cycle ABI, `joker:meta`, Rust SDK, `joker-test`, `J-E0xxx` errors | `wit/joker-runtime.wit`, `spec/cddl/joker-meta.cddl` |
| `ref/05-python-and-typescript-sdks.md` | tier 2 SDKs (Python, TypeScript) and client libraries (Rust, Python, TypeScript) | — |
| `ref/06-drivers.md` | tier N (traits), tier W (`joker:driver`), tier X (JDP), driver conformance suite | `wit/joker-driver.wit`, `spec/cddl/jdp.cddl`, `spec/c/jdp.h` |
| `ref/07-joker-link.md` | Joker Link at the wire level: framing, security, session, the messages (list in ref/07 §6.1), OTA, budgets | `spec/cddl/joker-link.cddl` |
| `ref/08-joker-micro-abi.md` | C ABI, `joker-micro` crate, firmware mode, `.jkgen` blob, `.jkfw` descriptor, targets | `spec/c/joker_node.h`, `spec/c/example/`, `spec/cddl/joker-micro.cddl` |
| `ref/09-http-api.md` | local HTTP API, WebSocket and SSE (the operations are those of the OpenAPI) | `spec/openapi/joker-api.yaml` |
| `ref/10-agent-protocol.md` | node ↔ Deck: enrollment, desired state, reports, events, sessions, HTTPS and MQTT, micro profile | `spec/cddl/agent.cddl` |

**Validation done:** all the TOML, JSON and YAML blocks pass a parser; the OpenAPI passes `openapi-spec-validator`; the three WIT packages (`joker:runtime`, `joker:driver`, `joker:sim`) pass the official parser (via `jco`), and `joker:driver` resolves against `joker:runtime`; the C headers compile with `-std=c11 -Wall -Wextra -pedantic` and as C++17; the `joker:meta` CDDL was validated with `zcbor` against instances, and the others pass a CDDL parser; the Joker Link test vectors (CRC, COBS, ChaCha20-Poly1305, COSE) were generated and verified by code.

**What has not been compiled yet:** the Rust signatures and the TypeScript blocks were reviewed by hand, and the CDDL files, except that of `joker:meta`, only passed a syntax parser. This becomes task 0 of Phase 0 (§8).

---

## 2. The map of the interfaces

```
            people                                           machines
 ┌──────────────────────────┐                   ┌────────────────────────────────┐
 │ joker.toml, overlays      │ ref/01            │ Joker Deck                     │
 │ profiles, boards, sim,    │ ref/03            │   ▲ agent/1 (JSON/HTTPS,       │
 │ scenarios, conformance    │                   │   │  CBOR/MQTT) ref/10         │
 │ classes .jkc              │ 17                └───┼────────────────────────────┘
 └────────────┬─────────────┘                        │
              │ jk check / build / lock (ref/02)     │
              ▼                                      │
 ┌──────────────────────────────────────────────────┴───────────────────────────┐
 │ jokerd                                                                        │
 │  HTTP/WS API ref/09 ◀── jk, Studio, Rust/Python/TS clients (ref/05)           │
 │  Wasm apps ── joker:runtime (ref/04; Rust SDK ref/04, Python/TS ref/05)       │
 │  drivers ── tier N traits · tier W joker:driver · tier X JDP (ref/06)         │
 │  joker-link device ───────────────────┐                                       │
 └───────────────────────────────────────┼───────────────────────────────────────┘
                                         │ Joker Link ref/07 (COBS+CRC-32C, CBOR)
                                         ▼
                        ┌───────────────────────────────────────┐
                        │ Joker Micro: firmware · C lib · crate │ ref/08
                        │   .jkgen blob, joker_node.h            │
                        └───────────────────────────────────────┘
            all with the same representation: ref/00 · the same semantics: 16
```

---

## 3. A value end to end

The power of an inverter behind an RP2350, read by the MCU at 10:00:00.1238 UTC with the quality `Uncertain(Stale)`. The same value in each interface:

| Where | Form | Reference |
|---|---|---|
| Modbus register on the MCU | raw `u16` `0x101D`, scale 0.1 | ref/03 §2.2, ref/06 §2.7 |
| slot on the MCU | `value` = bits of `412.5_f64`; `quality` = `0x0000_0301` (code 1, reason 3) | 01 §3.2, ref/00 §4.3 |
| Joker Link, `SAMPLES` | `[40, 5120, 412.5, 1800, 769]`: `sid`, 16-bit `seq`, value, `dt` in µs, quality | ref/07 §6.9 |
| slot in `jokerd` | `ts_acq` = monotonic instant (ns) of reception on the Linux node; `ts_hw` = the MCU's `t0 + dt`, converted to the node's monotonic clock by the offset and the rate that `TIME` estimates, with the "link" domain flag; `seq` of the node's slot (the MCU's 16-bit one only serves to discard stale samples) | ref/07 §8.3, §8.4, 01 §3.2 |
| Rust app, cycle | `inv.p.get()` → `Err(QualityError { port, quality: Quality::uncertain(Stale) })`; `get_uncertain()` → `Ok(412.5)` (in the port's unit, kW) | ref/04 §4.6, §4.7.1, 16 §3.1 |
| TypeScript app | `inv.p.get()` throws `QualityError`; `inv.p.tryGet()` → `{ ok: false, … }` | ref/05 §6.1 |
| trace | 16 B: `seq` u32, quality u32, value u64 | 16 §7 |
| HTTP API | `{"path":"inv.p","value":412.5,"unit":"kW","quality":"uncertain","reason":"stale","ts_hw":"2026-09-24T10:00:00.123800000Z","ts_acq":"2026-09-24T10:00:00.126100000Z","seq":88123}` (`ts_hw` is the MCU's instant, `ts_acq` that of reception; `seq` is that of the node's slot) | ref/00 §4.2, ref/09 §3.3 |
| Deck | does not receive the sample: it receives the telemetry (Sparkplug B) and, in the report, the health of the device | ref/10 §2.10, 20 §3.3 |

At no point is the quality lost, does the unit change without explicit conversion, or does a monotonic instant leave the node without its UTC pair.

## 4. A write end to end

An operator forces the pump of an MCU to stop, from the Studio on the laptop, over TCP with mTLS:

1. `POST /v1/outputs/pump.run_cmd/manual` with `value: false`, `ttl: "2h"`, `reason`, `level` by default `manual:4`, and the `Joker-Nonce` and `Joker-Timestamp` headers (ref/09 §5.6, §2.5).
2. `jokerd` checks the `operator` role, the anti-replay and the occupation of the level (16 A13). Because the output belongs to an MCU, the Linux node does **not** arbitrate it nor apply an envelope to it (`J0234`, ref/07 §8.1): it keeps the TTL and the level and treats the write as a remote writer `user:<principal>` (16 A10, A12).
3. The **MCU**'s arbiter is the one that decides: `jokerd` sends `PROPOSE` at the level delegated to the link, with `lease = min(remaining TTL, max_lease)` (default 5 s; ref/07 §6.12, §8.1, 16 A12). The MCU applies its envelope and returns `OUTCOMES` with `accepted` (value 1, ref/00 §4.4).
4. The HTTP response is `200` with the `WriteTicket` and the result, which resolves on the first `OUTCOMES` that covers the `PROPOSE` (ref/07 §8.2). A `clamped_rate` or `rejected(interlock)` is also `200`, because it is a result of the envelope and not a request error (ref/09 §3.4).
5. `jokerd` renews the lease on the MCU, in chunks of `max_lease`, for as long as the TTL lasts. If the cable drops, the lease expires **on the MCU** within ≤ `max_lease`, and the MCU moves to the next active level or to `safe` (ref/07 §7.4).
6. If `jokerd` restarts, the manual level is restored from disk with the deadline in UTC (16 A14), and is sent to the MCU again: the stop for maintenance does not disappear with a restart. Without a `synced` clock at startup, the remaining TTL is restored and the alarm `joker.output.<s>.manual_restored` is raised.
7. The Deck sees all this only in the report (`overrides`, the active manual levels, ref/10 §2.10.1) and in the `decision` events of the manual writes (ref/10 §2.11); the API audit stays on the node. **The agent protocol has no message for writing to signals** (ref/10 D9).

---

## 5. Cross-cutting decisions

Where the references, written in parallel, diverged or left a choice, it was decided as follows. The references and the existing documents were aligned with these decisions (§7).

| # | Decision | Where it applies |
|---|---|---|
| **D1** | **Precedence:** `ref/00` has the level of 16; `ref/01`–`ref/10` sit **above** the per-area documents (01–05, 14, 19, 20), which become the explanation and refer to the reference for the detail. ADR 0023. | docs/README, AGENTS |
| **D2** | **Records in CBOR** (events, arguments and results of actions and methods, `Internal<T>`): a map with **integer keys = position of the field** in the schema (`joker:meta` or `.jkc` contract). Fields are only added at the end within a major. JSON uses the names. | ref/00 §4.1, ref/04 §4.10, ref/07 §6.2, 17 §9 |
| **D3** | **Expressions:** short-circuit with quality is **symmetric** (`false && Bad` and `Bad && false` both give `false (Good)`), because the operands have no effects. `any`/`all` generalize the short-circuit; `count`/`sum`/`maxs`/`mins` give the worst quality. Integer arithmetic in `f64`, exact comparisons between integers. `own(x)` with no previous proposal returns the applied value. | 02 §4, 16 §3.2, §4, 17 §4.1 |
| **D4** | **New core rules:** A12 (remote writers: ownership by delegated level, lease counted at the consumer), A13 (two principals at the same manual level: the second is refused without `takeover`), A14 (manual levels persist across restarts, with the deadline in UTC), E10 (a NaN or infinite proposal, or one with a nonexistent enum, is `Rejected(WrongType)`). | 16 §1, §2 |
| **D5** | **Scheduled writes (M6) and intents (M18) are 1.1** in every surface: reserved in the manifest (`J0290`), `@unstable` in the WIT, endpoints that respond empty or `J-E1017`, reserved on the MCU (`J0783`). | 01, 02, 03, 04, 07, ref/* |
| **D6** | **Secrets encrypted per recipient (HPKE)**, with a signature per entry: `secrets.toml.enc` **may** go into Git, and that is how the Deck distributes them without being able to read them. In apps: `[apps.<app>.secrets] <local_name> = "secret:<name>"`. ADR 0024. | 01 §17, 03 §1, 15 §2.4, ref/01, ref/02, track |
| **D7** | **Identity and signatures:** the node identity is **P-256** (TPMs and secure elements support it); the organization keys that sign artifacts and documents are **Ed25519**; COSE_Sign1 accepts `EdDSA` and `ES256`. Signed documents always go in CBOR/COSE, also over HTTPS. Trust comes from a bundle signed by an offline root; `[security.trust]` in the manifest only restricts. ADR 0025. | ref/00 §7, 01 §17, 03 §13, 20 |
| **D8** | **One lock per input manifest:** `joker.rpi.toml` gives `joker.rpi.lock`. | 15, old track 05 |
| **D9** | **Rollback reactivates the previous generation** with the same hash and the same number; the history is of activations, each with an `origin`. `generation_state` gains `discarded`. | 01 §15, ref/09, ref/10 |
| **D10** | **A single conformance schema** for the core and for classes (`[suite]`, `[[case]]`, `do`, `expect`, `at`, `equals`), with the canonical literals in lowercase (`clamped_rate`, `bad(comm_failure)`) and the CLI forms as exact synonyms. | 16 §8, 14 §7.3, ref/03 §2.6 |
| **D11** | **`schema = 1`** is the first key/value pair of the catalog and test files (comments before it are allowed). | ref/00 §1, examples, track |
| **D12** | **CRC-32C** (Castagnoli) everywhere: Joker Link, generation blob, persistence records on the MCU. | 19 §6, ref/07, ref/08 |
| **D13** | **Media types in a single table** (ref/00 §7.1), with the full form `application/vnd.joker.<type>.v1+<suffix>`, including `runtime`, `config`, `driver` (wasm and tar), `secrets`, `manifest`, `lock`, `provenance`, `cwasm`, `conformance`. | 20 §3.2 |
| **D14** | **Rust SDK:** `Many<C, MIN, MAX>` (the `1..=8` form is not valid Rust); `.mins()` (avoids `Ord::min`); allocating in the cycle is trap `J-E0903` (the bump allocator left dangling pointers); `#[joker::shared]` goes away; `by_name()`; `clamped(clamp)` and `record error`. | 04, 14, track 06–08 |
| **D15** | **Tier 2 in the cycle too**, with a minimum period of 100 ms in TypeScript and 500 ms in Python, and admission by the measured **maximum** (GC pauses make p99.9 misleading). Plan B if WASI 0.3 async is not ready in the toolchains: a blocking variant on an `app` thread. | 04 §4, ref/02, ref/05 |
| **D16** | **Anti-replay** (nonce and `ts`) in the API over TCP, in the tunnel and in the agent. Bridges authenticate through their own protocol, with a level and a lease. The ±30 s window only applies with a `synced` clock; without it, the nonce and the binding to the target apply, and that goes into the audit. | ref/00 §9, 01 §17 |
| **D17** | **Identifiers stable across generations:** the *path-hash* (first 8 B of the SHA-256 of the canonical path) binds the external interface of an MCU to each generation. The `SignalId` remains dense and valid only within one generation. | ref/00 §5, ref/08 |
| **D18** | **Package names:** Python `joker_sdk`, `joker_client`, `joker_testing`, `joker_classes` (`import joker` would lead to installing the wrong PyPI package); npm `@joker/*` (**confirm the organization**); new crates `joker-client`, `joker-agent`, `joker-agent-proto` (`no_std`), `joker-hal`, `joker-driver-sdk`, `joker-jdp`. | 13 |
| **D19** | **Manual write:** default level `manual:4`; `--reason` required; `--to` (and not `--until`) in intervals. | 07 |
| **D20** | **Drivers:** the core executes the profile steps (`connect`, `relinquish`, M1 fallback) on top of two driver primitives (`read-addr`, `write-addr`); the raw ↔ engineering conversion belongs entirely to the core; tier X drivers start as transient systemd units with `DynamicUser`. | 05, 01 §17 |
| **D21** | **Joker Link:** security is configured on both sides, without negotiation (there is no *downgrade*); three delivery classes (ephemeral, state, reliable); session modes `full`, `observe`, `maintenance`, `refused`; `SAMPLES` retransmits the current value, never the old one. | 19 §6 |
| **D22** | **C ABI:** two headers (`joker_node.h` fixed, `joker_project.h` generated); the C code writes like a `kind = "extern"` app; an app *panic* on an MCU is fatal (`jk_fatal`), because there is no sandbox; the trip is recorded in flash until `service_deadline` after it happens. | 19 §4 |
| **D23** | **Agent:** the desired state is complete and idempotent, with `seq`; no queue of orders; phases in a fixed order (runtime → MCU → generation); a generation only confirms itself after a `report-ack` from the Deck (a generation that cut the network does not confirm itself); local protections against a compromised Deck (maintenance window, `max_applies_per_day`, `local_changes = "hold"`). | 20 |

---

## 6. Cross-cutting stress

The cases of each surface are in its reference (8 to 12 per document). Here are the ones that cross several.

**X1 — Misaligned versions everywhere.** Deck 1.5, node 1.3, MCU with firmware 1.0, app compiled for `joker:runtime@1.1`, `jk` 1.4 on the laptop. The agent negotiates `agent/1` and the Deck limits itself to what a 1.3 understands (ref/10 §3.14); the 1.1 app runs on a 1.3 (the world only grows); the MCU announces `link_version` 1.0 in `HELLO` and `jokerd` speaks 1.0 with it (ref/07); `jk` 1.4 reads `GET /v1/version` and the `features` of `GET /v1/info` and refuses, with a clear message, the options the node does not have (ref/09 §9.3, ref/05 §21). **Holds**, because each surface has its own version and the rule "readers tolerate the unknown, requests are strict" (ref/00 §1).

**X2 — A signal changes name between generations.** `pump.speed` becomes `pump.speed_ref`. The `SignalId` changes (it is dense per generation); apps receive new handles in the generation's `init`; the external interface of a C library on an MCU is bound by name (with the *path-hash* of the `PATHS` section) at each generation, and a generation that lacks a required entry is rejected by `jk build --generation` (`J0790`) and, if it reaches the MCU, by `jk_init` (`JK_E_GEN_FIRMWARE`, `J-E2516`), instead of binding the wrong variable (ref/08 §2.8.2). **Holds.**

**X3 — `unsynced` clock from start to finish.** A node starts without an RTC or a network. Control runs on the monotonic clock and does not change (01 §18); UTC instants go out marked with the `unsynced` clock, and apps with `requires_clock = "synced"` have their outputs blocked (`Rejected(ClockUnsynced)`); the API accepts writes over TCP with the nonce only and records it in the audit (D16); `TIME` carries the `unsynced` quality to the MCU, and the MCU stays `unsynced` (ref/07 §8.4); the agent does not check `issued` or `expires`, accepts the desired state by `seq`, and the phases with windows wait (`wait = clock`; ref/10 §3.12). **Holds**, with the rule "without a clock, nothing that depends on it decides silently".

**X4 — A secret tries to leave through a side door.** Through the generation's manifest (`GET /v1/generations/{gen}/manifest`, which only has `secret:<name>` references), through the trace (secrets are never signals), through `joker:meta` (names only), through the report to the Deck (names and `rev`, never values), through an app log (the `secrets` interface only exists in async, and the SDK marks the values as non-printable). **Holds**; the risk that remains is the app itself sending the secret over the network, which it can only do to the destinations in `net.allow` (ref/01).

**X5 — A slow WebSocket client, a slow Deck and a slow MCU at the same time.** Three independent queues, none on the control path: the WebSocket samples the slots (a slow client just receives less often) and closes with 4008 after 30 s of full queue; the agent has store-and-forward with marked drops (`gap`); Joker Link coalesces samples (`coalesced` in `SAMPLES`). **Holds**; what is lost is always counted.

**X6 — The same app in four languages.** The peak shaver in Rust, TypeScript and Python (ref/05 §20 shows it side by side) and, on an MCU, the same logic in C as an `extern` app (the pattern of `spec/c/example/`, ref/08 §2.17). The round found two bugs in the original example of document 04 (a 0 kW limit with the plan not yet computed; port `grid` against `grid_power` in the `bind`), which were fixed. **Broke, fixed.**

**X7 — A typo in a file.** `max_leas = "2s"`. It is `J0140` with a suggestion (`max_lease`), because files written by people are strict; in an API JSON, an unknown field in a request is `J-E1011`; in a CBOR received from a newer MCU, it is ignored. **Holds**: strict where a person makes mistakes, tolerant where a machine evolves.

**X8 — A compromised Deck.** It can send a desired state signed by the `deck` key, but the artifacts need signatures from the pipeline, which do not live in the Deck; it cannot write to signals (there is no message for that); it cannot widen envelopes nor change the configuration (a new generation must come signed by the `config` key, which does not live in the Deck, and goes through `jk check` on the node); it cannot read secrets (HPKE to the nodes). At most, it can go back to old generations and runtimes already signed, above the floors, limited by `max_applies_per_day` and by the local maintenance window. **Holds**, with the residual risk declared in ref/10 §4 (S1).

**X9 — An integrator writes a driver and a class and publishes to the Hub.** Tier W driver validated by the driver conformance suite (ref/06 §6), class validated by the class conformance suite (ref/03 §2.6), reports signed as OCI referrers, profile with a real hardware report (ref/03 §2.8). The Hub rejects publications without a report (`J0849`, `J0880`). **Holds.**

**X10 — Replay of an incident with an MCU in the middle.** The MCU's trace arrives through `TRACE` (in ns, without ML outputs), remote writers appear from `0x8000_0000` (16 §7), and the replay on the laptop reproduces the MCU's results byte for byte, because the core is the same (ADR 0020). **Holds.**

---

## 7. Findings in the existing documents

The eleven references recorded more than a hundred findings (the lists that remained after integration are in ref/01, appendix C, in ref/03, in ref/04 §9 and in ref/05 §33): contradictions between documents, examples that would not compile, keys used without being defined, arithmetic errors and two logic bugs. Each reference has its own list, with the exact change. **They were all accepted and applied**, with the resolutions of §5 where there was a conflict. The most important:

| Document | What changed |
|---|---|
| 16 | A12–A14, E10; symmetric short-circuit; aggregations; integers; `own()`; trace with remote writers and MCU in ns; §8 now refers to the single conformance schema |
| 02 | symmetric short-circuit; `Clamped` with both values; M6 marked 1.1 |
| 03 | `secrets` as a table; `trust` only restricts; `mtls` goes away (TCP is always mTLS); 1.1 keys marked; `"**alarm.**"` → `"alarm.**"`; `default` group aligned with A11 |
| 04 | WIT replaced by a cross-reference to `wit/`; 14 B proposals; no allocation in the cycle; `Many<C, MIN, MAX>`; `.mins()`; peak shaver fixed; tier 2 with minimums |
| 05 | new trait, `joker:driver` and JDP by cross-reference to ref/06 |
| 07 | new commands from the references; `--to`; `manual:4`; `--reason`; intents marked 1.1 |
| 14, 17 | `Many`; reserved member names; methods against members; idempotency only without `own()`; the pump rotation bug in §9.1 |
| 19 | CRC-32C, TCP, bidirectional `TIME`, C example with `joker_project.h`, fatal *panic*, U1 budget |
| 20 | media types; principle 2 rewritten; rollback by `reject` |
| 01, 13, 15 | secrets, P-256 identity, X drivers through systemd, new crates, layout of `/var/lib/joker` |
| examples, track (old) | `schema = 1`; `joker.rpi.lock`; `.gitignore` without `secrets.toml.enc`; 1.1 keys marked; SDK syntax updated |

---

## 8. What this asks of Phase 0

The round closes the specs **on paper**. Phase 0 closes them **in code**, before any feature:

1. **Task 0a — compile the references.** A `joker-spec-tests` crate that (1) compiles the Rust blocks of the references as doctests, (2) runs `tsc` on the TypeScript blocks, (3) validates the CDDL files with a complete validator against the examples, (4) generates the JSON schema of the manifest from the types and validates all the TOML blocks of the documents against it, (5) validates the OpenAPI, (6) checks the counts that the prose still repeats (codes, operations, messages) against the sources (`spec/codes.toml`, OpenAPI, CDDL; document 13, §5.1). It fails the CI if a reference and the code diverge.
2. **Task 0b — the code registry.** The seed already exists: `spec/codes.toml`, with the entries extracted from the tables (active and retired; the file is the source of the counts), one severity per code and no collisions. The task moves it to `crates/joker-manifest/codes.toml`, writes the product messages and wires up the CI test (16 §9). The round created most of the codes (those that are in the tables of the references and were not in those of the per-area documents; the counts are those of `spec/codes.toml`, checked by `joker-spec-tests`), in ranges separated by reference (ref/00 §8), without collisions: the two codes that appear in two references (`J0109`, `J0720`) have the same meaning in both.
3. **Task 0c — the test vectors.** Those of Joker Link (ref/07 §4.8), those of the C ABI (ref/08) and the core conformance cases become files in `conformance/`, run on Linux and on an MCU.

The questions of document 16, §10 (exact RT admission, `dt()` after `skip`, transitions from `Unknown`, bus budget beyond RTU) are still to be decided in Phase 0. The base of `extends` is no longer open: ref/01 closes it (the `extends` paths are relative to the file that declares them, `J0160`), and the JSON schema of the manifest is generated from the types.

**Still open, and not technical:** the license, the name and the brand, the `@joker` organization on npm, the team per front, the CI hardware and the business model (document 18, §6).
