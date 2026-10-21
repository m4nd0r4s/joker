# ADR 0035 — Reconcile the references with the 1.0 cut

**Status:** accepted (v1, 2026-10-21; the planning decisions of §2 were taken by the planner while the owner was away, and review is welcome) · **Date:** 2026-10

## Context

ADR 0028 decided that in 1.0 only what is implemented and passed its exit gate is stable, and that the rest is reserved with the mechanisms that ML already uses. Its initial cut is the table of document 26, §5. ADR 0028 left two things for later: the mechanical marking of the references, and a decision wherever an older document says something else.

The older documents say something else in many places. Document 11 still plans the whole of the v1 specification as written: the C library of Joker Micro, CAN, CANopen, the OPC UA and Modbus server bridges, the TypeScript SDK and client, app shadow, uplink budgets, OTLP and Zenoh, and orchestrated MCU OTA, all before 1.0. The references followed it: ref/01 annex A gives version 1.0 to every catalog driver, ref/05 §1.1 and §30 ship the TypeScript SDK and the TypeScript client in 1.0, ref/06 §11 ships the tier X host and JDP in 1.0, ref/07 §18 ships all six Joker Link transports, ref/10 §6 ships the standalone MCU profile and CoAP, document 07 §6 and ref/09 §10.4 show app shadow on a 1.0 node, and ref/09 §13 declares app shadow and bridges as 1.0 operations.

The cut table also leaves some questions open: what "orchestrated MCU OTA" covers when `jk apply` already carries MCU generations, where the standalone MCU profile goes, which client libraries ship, how the LSP of §5 relates to the 1.1 of §7, what "minimal `std/`" means, and how the code registry marks a reserved code (ADR 0028 names `status = "reserved"`, which the registry did not have).

## Decision

### 1. The cut wins

Document 26, §5 wins everywhere an older document contradicts it. A surface that the cut does not keep in 1.0 becomes **reserved**: it keeps its specified form, exists in the 1.0 schemas, and answers with the "reserved for a future version" mechanism of its surface. AGENTS.md §1 already states the rule; this ADR applies it.

"The API and the `jk` that the track uses" (the 1.0 row of the cut) is read by feature: the operations and commands of the features of the 1.0 row stay 1.0, and those of the features that the cut moves are reserved. The evidence-based inventory that ADR 0028 asks for (a stable surface needs a test that exercises it) runs when there is code to inventory, at the reassessment at the end of M0 (ADR 0028, decision 5), and can only reserve more.

### 2. Planning decisions

1. **MCU OTA.** MCU generations carried by `jk apply` over Joker Link (with their confirmation chained to `jk confirm`) and the manual `POST /v1/micros/{device}/ota` stay in 1.0, and so does the OTA receiver on the MCU (ADR 0028, decision 4). "Orchestrated MCU OTA" is node- or Deck-driven firmware rollouts across MCUs: firmware carried by the activation of a generation, the `mcu` phase of the agent and the `mcus` key of the desired state. It is 1.1.
2. **Standalone MCUs.** The autonomous MCU agent profile (the micro profile of ref/10 §2.15) and CoAP are 1.1, not 1.0. The `agent/1.0` core does not change for them.
3. **Versions of the rest.** App shadow is 1.1. The tier X (JDP) host is 1.2, and so is the tier W host. The TypeScript client is 1.1; the TypeScript and Python SDKs are 1.2. The Rust and Python API clients stay 1.0.
4. **LSP.** The LSP for `.jkc` (and the TOML files) is 1.0.x: §5 of document 26 wins over the 1.1 of its §7.
5. **`std/`.** The normative 1.0 list of the standard class library is document 14, §10, split in two issues. "Minimal `std/`" in document 26, §5 means "no more than document 14, §10".
6. **Per-recipient secrets** (ADR 0024) move to Phase 2, because part 10 of the learning track needs them.
7. **Phase gates and 1.0 targets.** The gates of document 11 (p99.9 ≤ 200 µs, RSS ≤ 90 MB) are phase exit thresholds; the values of document 01, §21 (100 µs, 60 MB) are the 1.0 targets. They do not contradict each other: both are kept, and each is labelled for what it is.
8. **Hardware.** Without CM4, CM5 or RP2350 hardware, the gates run on the bench of document 23 (Pi 3, ESP32) and in QEMU, and their results are reported as provisional until the hardware exists.
9. **The Deck** lives in its own repository, with its own versions (Deck 0.x, ADR 0022 decision 5), speaking `agent/1.0`. Its `halt_if` expressions reuse `joker-expr`.
10. **OCPP (P6) and the Home Assistant bridge (P8)** stay unplanned until the catalog decision at the end of Phase 1.

Three readings of the cut table, needed to mark the references:

11. **Joker Link transports.** 1.0 ships UART, UDP and TCP. RS-485 without turn-taking is a UART (same framing and addressing) and stays 1.0; RS-485 half-duplex with turn-taking and CAN/CAN-FD are 1.1, as the cut says. USB CDC is not in the cut; it is an extension of the transport layer and goes to 1.0.x. L1–L3 and the negotiation in `HELLO` stay 1.0 for every transport (ADR 0028, decision 4).
12. **"ESP32"** in the 1.0 row is the Xtensa ESP32 family that `esp-hal` supports: the original ESP32 (the bench) and the ESP32-S3. The ESP32-C6 and the STM32 targets are 1.0.x.
13. **Uplinks.** The 1.0 uplinks are MQTT and Sparkplug B. OTLP and the HTTP webhook are 1.1, with uplink budgets; Zenoh is 1.2, with ROS 2.

### 3. The marks

Each surface uses the mechanism of ADR 0028, decision 2:

| Surface | Mark |
|---|---|
| WIT | `@unstable(feature = …)`. A case of an enum or variant cannot be gated: it keeps its number and its docstring says that a 1.0 host never produces it |
| manifest | the key stays in the 1.0 schema with the `since` of its version; using it is `J0290` (ref/01 §6.6). The files of ref/03 use their own code, `J0807` |
| OpenAPI | `x-joker-since` > 1.0 and a 501 response with `J-E1017`; the schemas stay complete. Fields of 1.0 schemas that only later versions fill carry a `description` saying so |
| Joker Link, agent | the section, message, key or enum value is marked "reserved" with its version, in the reference and as a `RESERVED` comment in the CDDL |
| C headers | a header paragraph that says the ABI is reserved |
| code registry | `status = "reserved"` with `since` |

A version of **1.0.x** is a patch release that adds drivers and extensions (ADR 0022, decision 3). Until it ships, a 1.0.x surface is reserved like the others; its `since` becomes the number of the patch release that implements it.

### 4. `status` in the code registry

A `[[code]]` entry gains an optional `status`: `"active"` (the default, omitted) or `"reserved"`. A reserved code only exists for a reserved surface, and always has `since` with the version that brings it. Every code with `since` later than 1.0 is reserved, so the codes that already had `since = "1.1"` (ML, intents, scheduled writes) or `"1.3"` (remote sessions) get `status = "reserved"` as well. A reserved code becomes active, with no other change, when its surface is implemented. Document 26, §4 E10 then reads: every active code has a test.

### 5. Where the older documents contradict the cut

The references, `wit/` and `spec/` are corrected in the same set of commits as this ADR (ADR 0023). Each line says what contradicted the cut.

| Document | Section | Contradiction (now reserved) |
|---|---|---|
| ref/01 | §2.1, §15 | the Phase column only knew 1.0 and 1.1; bridges, app shadow and uplink budgets were 1.0 |
| ref/01 | §5.6 | buses `spi` (1.0.x), `can` (1.1), `ethercat` (1.2); `duplex = "half"` with Joker Link (1.1) |
| ref/01 | §5.7 | `[drivers]` of the tier W and X hosts (1.2) |
| ref/01 | §5.12.3, §5.13, §5.17.3, §5.18, §5.22 | app shadow (1.1), class simulators `instances.*.sim` (1.0.x), `budget` and the `otlp`, `http` (1.1) and `zenoh` (1.2) uplinks, bridges (1.2), `[link]` over CAN and `peer = "deck"` (1.1) |
| ref/01 | annex A | `revpi`, `spi`, `opcua`, `m-bus`, `onewire`, `serial-line`, MCU `modbus-slave` (1.0.x); `can`, `canopen` (1.1); `w:`/`x:` (1.2); `joker-link` over CAN, RS-485 turn-taking (1.1) and USB (1.0.x) |
| ref/03 | §2.2.2, §2.2.5 | the families of the drivers above, at 1.0 |
| ref/03 | §2.4.11, §6 | class simulators at 1.0 (1.0.x); no version for the ESP32-C6 and STM32 boards (1.0.x) |
| ref/04 | §2.2, §3, §12 | `run-mode.shadow`, the `SHADOW` flag and `shadowed` produced in 1.0 (1.1) |
| ref/05 | §1.1, §19, §21, §30 | TypeScript SDK in 1.0 (1.2), TypeScript client in 1.0 (1.1); codes `J-E0802`–`J-E0864` (1.2) |
| ref/06 | §2.2, §3.8, §4, §5, §7, §11 | tier X host, JDP, `libjdp` and the tier W host in 1.0 (1.2); the catalog drivers above; codes of the W and X hosts (1.2) and of timed writes (1.1) |
| ref/07 | §1, §3.1, §3.3, §3.4, §3.6, §14, §15, §18 | CAN, RS-485 half-duplex (1.1) and USB CDC (1.0.x) in 1.0; the micro profile framing in 1.0 (1.1); `J0737`, `J-E2004` (1.1) |
| ref/08 | §1, §2.1, §2.11.2, §2.15.1, §6 | the C ABI (library mode) in 1.0 (1.2); ESP32-C6 and STM32 targets (1.0.x); MCU drivers `spi`, `uart`, `modbus-rtu-slave`, `onewire` (1.0.x), `can` (1.1); link transports; `J-E2503` (1.2) |
| ref/09 | §5.1, §5.13, §5.14, §5.18, §5.20, §10.4, §13 | the `shadow.apps` feature and the app shadow operations in 1.0 (1.1); `/v1/bridges` in 1.0 (1.2); uplink budgets (1.1); firmware in `Plan.micros` (1.1); `J-E1510`–`J-E1512` (1.1) |
| ref/10 | §1.1, §2.2, §2.4.3, §2.6, §2.6.6, §2.7, §2.14, §2.15, §2.17, §3.1.3, §3.4.3, §3.6, §3.13, §6 | the micro profile, CoAP and the `mcu` phase in 1.0 (1.1); `J0963`–`J0965`, `J0990`, `J0991` (1.1) |
| document 07 | §2, §2.2, §3, §5, §6, §9 | app shadow in 1.0 (1.1); TypeScript apps in 1.0 (1.2); `jk build --lib` without a version (1.2); firmware through `jk confirm` (1.1); `jk lsp` without a version (1.0.x) |
| document 11 | all the phases | the plan of the whole v1 specification before 1.0. It gets a scope note now; its rebaseline is a separate task |
| document 26 | §7 | LSP for `.jkc` in 1.1 (decision 2.4: 1.0.x) |

These older documents also contradict the cut, and are left to their own rebaseline; until then, the cut wins (§1):

| Document | Section | Contradiction |
|---|---|---|
| document 04 | §4.3 | tier 2 figures "when each SDK exists (TypeScript in Phase 3…)": the TypeScript SDK is 1.2 |
| document 13 | §1 | crate `joker-jdp` (1.2) and Zenoh and OTLP in `joker-link` (1.2, 1.1) without a version |
| document 19 | §3, §3.1, §4.2, §6, §7 | MCU drivers SPI, UART, Modbus server, CAN, 1-Wire, the ESP32-C6 and STM32 targets, library mode, the six transports, and the U2 (standalone ESP32-C6) and U5 (C ABI in ESP-IDF) cases as v1 |
| document 20 | §1 | Joker Micro on ESP32-C6 and STM32 in v1 |
| document 21 | §9, T2 | JDP "stable since 1.0": the tier X host is 1.2 |

## Consequences

- The references, `wit/` and `spec/` mark every surface of §5 in the same set of commits, and `docs/11` points to the cut. The full rebaseline of document 11 (schedule, pilot gates at bench scale, the Deck and the Hub off the critical path) is a separate task.
- `docs/16` §9 documents the `status` field. `joker-manifest` reads it when the registry moves to the crate (document 13, task 0b), and `jk explain` shows the version of a reserved code.
- `joker-spec-tests` is to gain a rule: a code with `since` later than 1.0 must be `reserved`, and an OpenAPI operation with `x-joker-since` later than 1.0 must answer 501. The evidence rule of ADR 0028 (a stable surface without a test fails the CI) comes with the inventory at the end of M0.
- Moving a surface back into 1.0 (or into an earlier minor) needs a new ADR, with the implementation and its gate.
