# 21 — From v1 to v2

Document 11 takes the project up to **v1.0**. This document continues from there: what goes into each **1.x** version, why in that order, how it is decided what goes in, and what makes a version a **v2**. The versioning and support policy is that of ADR 0022.

The timelines are estimates relative to the start of Phase 0 (month 0), for the team of document 11. They serve to order, not to promise.

---

## 1. The rules of the game

### 1.1 What a 1.x is and what a 2.0 is

| | 1.x (minor) | 2.0 (major) |
|---|---|---|
| apps compiled for `joker:runtime@1.y` | run on any 1.x with x ≥ y | run on 2.0 through an adapter, for the whole of series 2 |
| 1.y manifests and locks | accepted; `jk migrate` is optional | `jk migrate` required; 2.0 reads the last 1.x |
| `.jkc` classes | only minor changes (document 17, §9) | there may be majors of `std/` classes |
| Joker Link and agent protocol | compatible in both directions within 1.x | new protocol version, with negotiation and support for the previous one |
| core semantics (document 16) | **never changes** an existing conformance result; only adds cases | may change, with each difference in an ADR and new cases |

The golden rule: **a 1.0 installation that works keeps working, the same way, in every 1.x.** Everything that would break it waits for v2.

### 1.2 How it is decided what goes into a 1.x

1. **One theme per version**, with at most three headline features. The rest waits.
2. **Demand from pilots decides the order** within what is compatible with the golden rule. A driver requested by a customer may go into an earlier version, because drivers are extension points (tier N, W or X) and do not touch the core.
3. **Each version has exit gates**, like the v1 phases. Without the gates, the version does not ship, and the features move to the next one.
4. **Nothing goes in without a document.** Each headline feature has an ADR, and the changes to the normative documents go into the same set of commits.

### 1.3 Cadence and support (ADR 0022)

- One minor every **~3–4 months**, after 1.0.
- Each minor receives fixes until **six months after the next minor**.
- **1.0 is supported for 12 months**, because it is the one the first industrial customers put in the field.
- **The last minor of series 1 is LTS:** 3 years of security fixes and critical defect fixes, without features. It is the version a customer can leave in a substation without touching it.
- The **Deck and the Hub** have their own versions; compatibility with nodes is through the agent protocol version, and a Deck fully supports nodes of the **last three minors and of the LTS**. An older 1.x node receives only the **minimal core** of the protocol (`hello`, `poll` and `desired`, `report`, `events`, `renew`, and the commands `confirm`, `reject` and `report_full`, in the `agent/1.0` form, which never changes in series 1), and that is enough to send it a new runtime: a node is never left without a path to be updated (ref/10 §3.14).

---

## 2. Overview

```
 month: 0          11        15         19          23          27         31          ~36
        ├── v1 ────┤ 1.1 ────┤ 1.2 ─────┤ 1.3 ──────┤ 1.4 ──────┤ 1.5 ──────┤ 2.0 ───────┤
        core,       ML        industry    security     Micro+      fleet at    the site as
        Linux,                and eco-    and opera-               scale       a system
        Micro,                system      tion                      (LTS)
        Deck MVP
```

| Version | Theme | Headline |
|---|---|---|
| **1.0** | the wildcard runtime | `no_std` core, Linux, Joker Micro, classes, Deck MVP |
| **1.1** | intelligence | ML (Burn, ORT, Hailo), streams and pipelines, block signals |
| **1.2** | industry and ecosystem | Structured Text, tier W driver SDK, BACnet and EtherCAT |
| **1.3** | security and operation | gateway mode and secure protocols, Joker Studio, remote operation in the Deck |
| **1.4** | Micro+ | Wasm on MCUs, ML on MCUs, low-power nodes |
| **1.5** (LTS) | fleet at scale | public Hub, fleet observability and audit, standalone MCUs at scale |
| **2.0** | the site as a system | active/standby redundancy, node mesh, WIT world 2 |

---

## 3. v1.1 — Intelligence (≈ months 11–15)

The ML postponed from v1 (ADR 0019), already defined in document 11 and detailed in document 06.

| Area | Contents | Document |
|---|---|---|
| backends | Burn (Flex, CubeCL CPU, Vulkan, CUDA), ORT, HailoRT | 06 §4, §12 |
| artifacts | `.jkm`, lineage (M10), model OTA through the Deck | 06 §2, §11 |
| bulk data | streams (M4), native pipelines (M5), scheduled writes (M6) | 01 §11, 06 §5, 02 §7.2 |
| waveforms | block signals (M7), with high-rate acquisition on Joker Micro | 01 §11, 19 U3 |
| trust | shadow and promotion of models (M16), input guard and fallback controller (M17), non-deterministic outputs in the trace (M22) | 06 §7, §8, §10 |
| LLMs | proposal-only writers and intents with approval (M18), tier X model servers | 01 §6.1, 06 §13 |
| learning | on-device training (Burn) | 06 §9 |
| drivers | `v4l2`/`libcamera`, `spi-adc` (Linux) | 05 §10 |

**Exit gates:** the vision and vibration benches (document 08, rounds 2 and 3); the six Burn validations (document 06, §12.6); a model promoted in the field only through shadow.

**Plan B:** if the Burn validation fails on CPU, 1.1 ships with ORT as the default CPU backend and Burn for GPU and training (ADR 0009).

---

## 4. v1.2 — Industry and ecosystem (≈ months 15–19)

The goal is that an automation integrator can use Joker **without learning Rust**, and that third parties can write drivers **without touching `jokerd`**.

| Area | Contents | Why now |
|---|---|---|
| **Structured Text** | IEC 61131-3 (ST) frontend as a tier 2 language, compiled to Wasm (Linux) and native (Joker Micro); classes appear as function blocks | it is the language of the automation audience (document 12, §17) |
| **tier W driver SDK** | stable `joker:driver` world (`@since(1.2.0)`; in 1.0 and in 1.1 it is `@unstable`, for in-tree drivers only, ref/06 §4), open to third parties, Rust and C SDK, driver conformance tests, publication in the Hub | opens the catalog to the ecosystem (document 05, §8) |
| **Python SDK** | tier 2, for slow logic and integrations | a common request from data teams |
| building protocols | **BACnet/IP** (tier X and native; no BACnet tier is in 1.0) and BACnet/SC | round 6 of document 08 |
| machine protocols | **EtherCAT** (`ethercrab`), GigE Vision (tier X) | round 4; vision of 1.1 |
| energy protocols | **IEC 60870-5-104**, **IEC 61850 MMS** (tier X), **DLMS/COSEM** (tier W), **EEBus** | substations, meters, controllable loads |
| classes | `std/` extended beyond the initial 1.0 list (document 14, §10): `meter.dc`, `battery.bms` and those a new protocol asks for, each one with an ADR | aligned with the standards of document 14, §10 |

**Exit gates:** an automation engineer without Rust does the learning track in ST (parts 1–7); three tier W drivers written by people outside the core team, published in the Hub; a real building site with BACnet in a pilot.

---

## 5. v1.3 — Security and operation (≈ months 19–23)

The question from every energy and infrastructure customer (NIS2 directive, grid operator requirements): "how is this secure, and how do I operate it?"

| Area | Contents | Document |
|---|---|---|
| **gateway mode** | node with two interfaces; `jk doctor --network` checks firewall, forwarding, exposed services; hardening profiles per board | 12 §5 |
| **secure protocols** | hardening: Modbus/TCP Security (TLS) and BACnet/SC, new; OPC UA `SignAndEncrypt` is already the default in 1.0 (ref/01 §5.18), and 1.3 only hardens it (check in `jk doctor --network`, hardening profiles) | 12 §5 |
| **supply chain** | SBOM per artifact, verifiable reproducible builds, build attestations | 09 attack 9 |
| **signed data** | intact transport of records signed by meters (OCMF, SML) | 12 §9 |
| **Joker Studio** (`jk ui`) | local commissioning: signal and instance browser, dashboard, manual writes with TTL, alarms, trace and replay | 20 §1, 12 §12 |
| **Deck: remote operation** | remote sessions (`jk` and Studio through a tunnel), approval of intents, audit export | 20 §4.1 |
| **Deck: multi-tenant** | one integrator, many customers | 20 D4 |

**Exit gates:** an external penetration test of a node in gateway mode with no critical findings; commissioning of a new site done only through the Studio, without the CLI; an integrator managing three customers in a multi-tenant Deck.

---

## 6. v1.4 — Micro+ (≈ months 23–27)

The v1 Joker Micro is the core with native apps. 1.4 takes it where Linux does not reach.

| Area | Contents | Why |
|---|---|---|
| **Wasm on M2 MCUs** | `wasmi` (`no_std`, deterministic, with *fuel*): swap apps on an ESP32-S3 or STM32H7 without reflashing | MCU fleets with logic that changes (document 19, §5) |
| **ML on MCUs** | Burn Flex `no_std` in production for small models (anomaly, classification), building on the 1.1 experience | inference at the sensor, without Linux |
| **low power** | battery nodes: cycle groups woken by events or by a timer, the MCU sleeps between ticks, energy budget in `jk check` | remote sensors, meters |
| **long-range uplinks** | LoRaWAN and NB-IoT for standalone nodes (telemetry and alarms; writes only with a long lease) | sites without a network (**only if there is demand** from pilots) |
| telemetry on the MCU | complete Sparkplug B; Zenoh-pico | direct integration without a Linux node |
| targets | nRF52/nRF54 (with BLE), STM32U5 (low power) | more families |

**Exit gates:** an app swapped over OTA on 100 M2 MCUs without reflashing; an anomaly model running on an MCU with < 100 KB of RAM; a battery node with a measured autonomy of > 1 year on a 1 min cycle.

---

## 7. v1.5 — Fleet at scale (LTS) (≈ months 27–31)

The version that closes series 1 and stays supported for 3 years. That is why it is one of **consolidation**, and not of core novelties.

| Area | Contents |
|---|---|
| **public Hub** | open registry of community profiles, boards, classes and drivers, with automatic verification (class conformance, `jk profile test` against simulators) |
| **Deck: observability** | aggregated runtime metrics, fleet alarms, dashboards per site and per equipment model |
| **Deck: audit** | aggregated and searchable decision logs, with retention by policy |
| **standalone MCUs at scale** | direct management by the Deck of thousands of networked Joker Micro devices |
| classes | `jk class extract` (from Rust crates to `.jkc`), if demand justifies it |
| consolidation | removal of *deprecation* warnings (without removing features), operations documentation for 3 years, assisted migration from 1.0 |

**Exit gates:** 1,000 nodes and 5,000 MCUs in a Deck, with 99.9% of the nodes in the desired state within 24 h after a rollout; the 1.0 → 1.5 migration guide tested on three real sites.

---

## 8. v2.0 — The site as a system (≈ months 31–36)

Up to v1.5, a node is the unit: each `jokerd` has its own signals, and the MCUs are its devices. v2 changes the unit to the **site**: several nodes and MCUs that behave as **one system**, with redundancy.

### 8.1 What goes in

| Area | Contents | Document |
|---|---|---|
| **active/standby redundancy** | two nodes with the same generation; the standby keeps the process image hot; switchover when the active node's lease expires; a quorum arbiter (a third lightweight node or a Joker Micro) prevents *split-brain* | 12 §16 |
| **node mesh** | a site signal space shared between nodes (over Zenoh), with per-node qualified paths (`@caldeira/pump.run_cmd`); instances that span nodes; alarms and arbiters per site | new |
| **WIT world 2** | if the *shared-everything threads* are stable in the Component Model, one instance per app instead of two (document 09, attack 6); removal of the world 0.x adapters | 04, 09 |
| **site manifest** | one manifest for several nodes, with the placement of the apps and instances decided by `jk check` or declared | 03 |
| **Joker OS** (if there is demand) | reference image (Yocto/Buildroot) with PREEMPT_RT, system A/B and `jokerd` preinstalled | 12 §11 |

### 8.2 Why it is a major

- **Paths** gain a node qualifier, which changes the grammar of document 02, §9.
- The **arbiter** can now be on a different node from the outputs it arbitrates, which changes rules of document 16.
- The **WIT world** changes structure.
- The **agent protocol** now describes sites, not only nodes.

All these changes have a migration (`jk migrate`), and 2.0 runs world 1 apps through an adapter.

### 8.3 What stays as a separate track

| Theme | Why separately |
|---|---|
| **Joker Micro Safety** (IEC 61508 certifiable subset, with Ferrocene) | it is a product with its own certification process, taking years (document 12, §1); it may start in parallel from 1.5 on, if there is a product decision |
| federated learning | depends on fleets with ML in production; it is v2.x |

---

## 9. Stress of the plan

**T1 — 1.1 is delayed because Burn fails the validation.** Plan B is written: ORT as the default CPU backend. 1.1 does not depend on Burn winning.

**T2 — A pilot needs BACnet in 2026, and BACnet is in 1.2.** Drivers do not touch the core. JDP (tier X) is stable since 1.0, and an integrator can write their own BACnet driver on top of a C library for the pilot; the catalog driver, in any tier, is in 1.2 (document 05, §10) and can be brought forward to an earlier minor without touching the theme. Rule 1.2.2 exists for this.

**T3 — The 1.x versions bloat.** The limit of three headline features per version, and the exit gates, are the containment. What does not fit moves to the next one, and the waiting list is public.

**T4 — A security flaw forces an incompatible change in a 1.x.** It is the only exception to the golden rule: a security ADR may break compatibility, with automatic migration and a notice to every Deck.

**T5 — v2 is too big.** The node mesh and active/standby can be separated: 2.0 with active/standby (which reuses almost everything from v1), and the mesh in a 2.1. The decision is taken at the end of 1.5, with the customers' requests in front.

**T6 — The Deck competes for the same people as the core.** The Deck has its own versions and team from 1.3 on, and compatibility is through the agent protocol. A delay in the Deck does not delay the runtime.

**T7 — An industrial customer never wants to update.** 1.0 has 12 months and 1.5 LTS has 3 years. After that, an extended support contract is a commercial decision, not a technical one.

**T8 — Structured Text turns out to be hard to integrate with classes and Wasm.** 1.2 can ship with ST for the cycle domain only (the typical use of a PLC), leaving the async tasks to Rust. It is the most important case, and the rest waits.

---

## 10. How this document evolves

- At the end of each version, this document is updated: the version that shipped becomes history, with what was left out and why, and the next one gains detail.
- The sections of each 1.x version become, at the time, a normative document of their own (just as 16, 17 and 19 were for v1).
- A feature only changes version with a note here and, if it touches a contract, with an ADR.
