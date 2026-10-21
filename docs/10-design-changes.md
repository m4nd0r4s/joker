# 10 — Design changes

The 26 primitives that came out of the stress rounds (document 08) and of the attacks on the architecture (document 09), and the refinements that did not become primitives. All of them are already incorporated in documents 01–07, and the **Reference** column says where each one is specified in detail in `docs/ref/` (ADR 0023). This document is the index and the short justification of each one.

Rule used to decide between a primitive and "it stays in the app": **a primitive goes into the core if, without it, the guarantee depends on the discipline of whoever writes the app.** If it is only a convenience, it stays in a library or in the app. That is why power allocation across inverters (08, round 1d) was left out.

---

## 1. Primitives

| # | Primitive | Origin | What it solves | Where it is | Phase | Reference |
|---|---|---|---|---|---|---|
| **M1** | Declared and verified device fallback | 08·R1a | the safe state of the equipment when Joker is not there | 01 §8, 05 §6 | 1 | ref/01 §5.8, ref/02 §2.1.12, ref/06 §2.10 |
| **M2** | Discovery with its result in `joker.lock` and drift detection | 08·R1b | register maps that change with the firmware | 03 §14, 05 §7 | 1 | ref/02 §2.1.11, §3.1.3–§3.1.5; ref/06 §2.11 |
| **M3** | Causal tracing of writes | 08·R1c | proving that a decision resulted from certain inputs | 01 §4, 07 §4 | 1 | ref/09 §5.7 |
| **M4** | Streams with shared buffers | 08·R2a | bulk data (images, point clouds) | 01 §11 | 1.1 | ref/01 §6.2, ref/06 §5.6 |
| **M5** | Native pipelines | 08·R2b | getting bulk data to the accelerator without going through Wasm | 06 §5 | 1.1 | ref/01 §6.3 |
| **M6** | Scheduled writes (`write_at`, `on_late`) | 08·R2c | precise actuation from an asynchronous decision | 02 §7.2 | 1.1 | ref/04 §2.3 (`write-at`, `@unstable`) |
| **M7** | Block signals and triggered recording | 08·R3a,c | high-rate waveforms | 01 §11, 03 §11 | 1.1 | ref/01 §6 |
| **M8** | Resource classes | 08·R3d | training and ML competing with control for CPU | 01 §2, 03 §3 | 1 | ref/01 §5.4 |
| **M9** | First-class alarms | 08·R3f | alarms that flap and end up disabled | 02 §11 | 1 | ref/01 §5.15, ref/09 §5.8 |
| **M10** | Model lineage | 08·R3e | "which model, trained on what?" | 06 §11 | 1.1 | ref/02 §2.1.9 (reserved) |
| **M11** | Declarative bridges (ROS 2, OPC UA/Modbus server) | 08·R4b | interoperability without shortcuts around the envelope | 01 §14, 03 §12 | 2 | ref/01 §5.18 |
| **M12** | Uplink budgets | 08·R5b | LTE with metered data | 01 §13 | 3 | ref/01 §5.17, ref/10 §3.11 |
| **M13** | Manifest overlays (`extends`) | 08·R5c | thousands of sites without configuration drift | 03 §2 | 3 | ref/01 §7 |
| **M14** | Clock quality | 08·R5d | booting in 1970 without an RTC | 01 §18 | 1 | ref/00 §3, ref/01 §5.5 |
| **M15** | Arbiter with 16 levels, leases and manual TTL | 08·R6a | "the operator changed it and the model undid it" | 01 §6 | 1 | ref/00 §5, ref/09 §5.6 |
| **M16** | Shadow mode and promotion | 08·R6b | going into production without blind faith | 06 §10, 03 §8 | 3 (apps) · 1.1 (models) | ref/09 §5.14 |
| **M17** | Input guard and fallback controller | 08·R6c | out-of-distribution models | 06 §7 | 1.1 | ref/01 §6.1 (reserved) |
| **M18** | Proposal-only writers (intents with approval) | 08·R6d | LLMs and untrusted systems | 01 §6.1 | 1.1 | ref/09 §5.9, ref/04 §2.4 (`propose`) |
| **M19** | Actions (commands with a result) | 09·A1 | not everything is setting a value | 02 §8 | 1 | ref/04 §4.9, ref/07 §6.15, ref/09 §5.10 |
| **M20** | Atomic groups and write order | 09·A1, 08·R4a | coherent multi-register writes; wheels in the same frame | 02 §7.3 | 1 | ref/01 §5.16 |
| **M21** | Versioning policy (WIT, manifest, lock, `.jkm`) | 09·A8 | fleets with mixed versions | 09, attack 8 | 1 | ref/00 §1, ref/04 §2.8, ref/10 §3.14 |
| **M22** | Non-deterministic outputs recorded; async replay | 09·A10 | exact replay with ML on a GPU | 06 §8 | 1.1 | ref/04 §2.7 |
| **M23** | Real-time admission by WCET | 09·A2,A5 | impossible groups discovered in the field | 01 §4 | 1 | ref/01 §5.11, ref/02 §2.1.13 |
| **M25** | Portable `no_std` core and Joker Micro (firmware, C library, crate) | scope review | the same core on Linux and on MCUs; hard real time; hardware sampling | 19 | 0 (core) · 2 (MCU) | ref/08 |
| **M26** | Joker Link and profiles generated from `HELLO` | scope review | MCUs as devices of a Linux node, with `safe` guaranteed on the MCU | 19 §6 | 2 | ref/07 |
| **M24** | Equipment classes and instances (implicit, composite, virtual), written in `.jkc` | 14, 17 | business logic in domain terms; large apps organized by equipment | 14, 17 | 1–2 | ref/03 §2.6–§2.7, ref/09 §5.11 |

---

## 2. Refinements (no new primitive)

| # | Refinement | Origin | Where it is |
|---|---|---|---|
| R1 | `on-cycle` with a packed image (one call per task) | 09·A2 | 01 §9, 04 §2, ref/04 §3 |
| R2 | wasmtime configuration for 32 bits (no 4 GiB reservations) | 09·A3 | 01 §9 |
| R3 | Differentiated `oom_score_adj` | 09·A3 | 01 §17 |
| R4 | Latest-wins boxes for the drivers + supervisor `force_safe` bit | 09·A4 | 01 §3.4 |
| R5 | `stuck_detect` and `Frozen` quality | 09·A4 | 02 §4 |
| R6 | `J0351`: < 10 ms requires PREEMPT_RT; `jk doctor --rt` with `cyclictest`/`hwlatdetect` | 09·A5 | 07 §3 |
| R7 | No-alloc guard on the tick path, in CI | 09·A5 | AGENTS §2 |
| R8 | Actuation budget (`max_actuations`) | 09·A9 | 01 §7 |
| R9 | Relaxed SIMD disabled and NaNs canonicalized in the cycle | 09·A10 | 01 §9 |
| R10 | Single-client rule + bridges for SCADA | 09·A11 | 12 §5 |
| R11 | `safe = "relinquish"` | 08·R6 | 03 §6.1 |
| R12 | `on_late` required and with no default | 08·R2c | 02 §7.2 |
| R13 | `correlate` in pipelines (trigger ↔ frame) | 08·R2d | 08 R2 |
| R14 | Proportional rate in atomic groups | 08·R4a | 02 §7.3 |
| R15 | `param` with `poll = "on_demand"` | 09·A1 | 05 §3 |
| R16 | `J0380`: estimated site memory | 08·R5g | 03 §15 |

---

## 3. Rejected decisions (and why)

| Proposal | Why it was left out |
|---|---|
| Grouped outputs with allocation (distribute 30% across N inverters) | it is policy, not mechanism; it belongs in the app |
| A *relinquish default* value separate from `safe` | it would be a second truth about what is safe |
| One Wasm instance per app with a mutex | priority inversion on the RT path (09·A6) |
| Dynamic native Rust plugins for drivers | Rust has no stable ABI; third parties use W or X (09·A7) |
| User functions in expressions | the closed language is what makes cost and analysis predictable |
| `struct` as a device signal type | it complicates slots and the historian; prefixes + acquisition coherence are enough |
| Fleet server inside Joker | it is another product; Joker is the agent (08·R5f) |
| Compiling for vendor NPUs | proprietary and offline compilers (08·R2e) |
| Software sampling above ~1 kHz | Linux does not guarantee it; it requires an ADC with its own clock (08·R3b) |
