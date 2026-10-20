# 12 — Limits

What Joker does **not** do, and why. This document is an **analysis** document (not normative; precedence is in `docs/README.md`): the rules live in the normative documents and in the references. Even so, promising a customer something that is listed here as "no" is a sales mistake, not an engineering one.

In the v1 scope review, each limit was reassessed with one question: **does it affect us?** The result splits them into four types:

| Type | Meaning |
|---|---|
| **Ground truth** | not a limitation of Joker, but a property of the problem; no product solves it |
| **Out of niche** | not what Joker wants to be; another product solves it better and Joker integrates with it |
| **Resolved in v1** | it was a limit and no longer is (mostly thanks to Joker Micro, document 19) |
| **To achieve** | it affects the target audience; there is a path, with a phase assigned |

---

## Summary

| # | Limit | Type | Answer |
|---|---|---|---|
| 1 | Functional safety (SIL/PL) | to achieve, long term | Joker Micro + qualified toolchain; product decision |
| 2 | Hard real time | **resolved in v1** | Joker Micro (cycles up to ~10 kHz) |
| 3 | Sampling above ~1 kHz | **resolved in v1**; waveforms in 1.1 | Joker Micro with DMA; block signals in 1.1 |
| 4 | The envelope does not guarantee good decisions | ground truth | shadow, KPIs, replay (tools, not guarantees) |
| 5 | Equipment reachable without going through Joker | to achieve, in part | gateway mode, secure protocols; the rest is network physics |
| 6 | Compiling for vendor NPUs | out of niche | with ML in 1.1: transport and execute, not compile |
| 7 | Fleet server | **to achieve, in v1** | Joker Deck (document 20) |
| 8 | Microcontrollers | **resolved in v1** | Joker Micro (document 19) |
| 9 | Certified meter | out of niche | transport signed data intact |
| 10 | Legal framework for data | ground truth | mechanisms, not compliance |
| 11 | OS update | out of niche; maybe later | integration with RAUC/Mender; an OS image is a future hypothesis |
| 12 | SCADA and HMI | out of niche; commissioning to achieve | Studio (`jk ui`) in v1.x |
| 13 | Plant historian | out of niche | uplink to existing historians |
| 14 | Third-party history and sessions | out of niche | actions when needed |
| 15 | Classes do not make equipment interchangeable | ground truth | conformance reduces the risk, does not eliminate it |
| 16 | Node redundancy (high availability) | **to achieve**, v2 | active/standby through leases |
| 17 | IEC 61131-3 languages | **to achieve**, v1.x | Structured Text compiled to Wasm and native |
| 18 | ML | deferred to **1.1** | v1 is built ready for it |

---

## 1. Functional safety (SIL/PL)

**The limit.** Joker is not certified to IEC 61508 (SIL), ISO 13849 (PL) or IEC 62061. Emergency stops, light curtains, STO, safe speed monitoring and electrical protections live in certified safety hardware and safety PLCs. The arbiter's `safety` level is for **software** protections.

**Does it affect us?** Today, little: in the target niches (energy, buildings, machines), functional safety already belongs to a relay or a safety PLC, and Joker **reads** its state. In the long run, it may be interesting: a certifiable battery controller would be a different product.

**Path, if it is ever wanted.** It is not certifying `jokerd` (Linux is not certifiable at useful SIL levels), it is certifying a **subset of Joker Micro**: the core (`joker-core`, without Wasm or network) on an MCU with safety support, compiled with a qualified toolchain. Ferrocene is qualified by TÜV SÜD for IEC 61508 SIL 3, and has a subset of `core` certified at SIL 2. The deterministic core, with no allocation and with formal conformance, is exactly the kind of code that gets certified. **Cost:** years and a certification budget. **Decision:** a product one, not for v1. What is done already is not closing the door: `joker-core` stays small, `no_std`, without `unsafe` and with the conformance suite.

## 2. Hard real time — resolved in v1

**Before:** the cycle domain on Linux is *soft real-time* (~100 µs of jitter at 1 kHz with PREEMPT_RT).
**Now:** **Joker Micro** runs the cycle groups in interrupt executors with hardware priority, with microsecond jitter and cycles up to ~10 kHz (to be measured). The recommended pattern for fast loops is **Linux node + coprocessor MCU**: Linux decides, the MCU executes and holds the outputs.
**Still out:** loops above ~10 kHz (motor FOC, power electronics), which belong to the firmware of the drive or the converter.

## 3. High-rate sampling — resolved in v1

**Before:** a Linux thread cannot sample at 25 kHz with useful timing.
**Now:** a Joker Micro with a DMA-driven ADC samples at the hardware rate and sends blocks or features to the Linux node (document 19, U3). **Block signals** (M7) arrive in 1.1, with ML, because the main use case (vibration) is an ML one. In v1, the MCU can already compute and send scalar features (RMS, peaks).

## 4. The envelope limits actions, it does not guarantee good decisions — ground truth

Range, rate, permissives, leases and actuation budgets limit **what** an app or a model can do, and none of this guarantees that it does it **well**. No system solves this: it is the difference between safety and quality. Joker provides the tools (shadow mode, KPIs, replay, causal tracing), and quality belongs to whoever writes and validates the logic.

## 5. Equipment reachable without going through Joker — to achieve, in part

**The limit.** Modbus TCP/RTU, BACnet and many OPC UA installations have no authentication: whoever is on the fieldbus writes to the equipment without going through Joker. Writes from atomic groups are not transactional on the equipment.

**Does it affect us?** Yes. In energy, with the NIS2 directive and the requirements of grid operators, fieldbus security is a question every customer asks.

**What Joker can do (v1.x):**
- **gateway mode:** the node with two interfaces, the field one with no route out, and `jk doctor --network` checking the configuration (firewall, no forwarding, exposed services);
- **single-client rule** (document 09, attack 11) with the bridges to SCADA;
- support for the secure variants of the protocols when the equipment has them: Modbus/TCP Security (TLS), OPC UA with `SignAndEncrypt`, BACnet/SC;
- with **Joker Micro** as a coprocessor, critical I/O is no longer on a network: it is on a serial cable inside the cabinet.

**What stays out:** protecting an unauthenticated piece of equipment on a network that the customer does not segment. That is network physics, and it is written down in the installation documentation.

**What is already closed on Joker's side:** the local API only accepts TCP with mTLS, with roles and anti-replay (ref/09 §2, §8.4; ref/00 §9); the agent and a compromised Deck are in the threat model of ref/10 (§2.4, §4); the keys, the signing roles and the per-recipient encrypted secrets are in ref/02 (§2.4, §2.5) and in ADRs 0024 and 0025.

## 6. Compiling models for NPUs — out of niche

With ML deferred to 1.1 (limit 18), the position holds for that time: Joker transports, verifies and executes vendor payloads (HEF, TensorRT), but does not compile them. The compilers are proprietary and that is where they should stay.

## 7. Fleet server — to achieve, in v1

**Before:** "it is another product".
**Now:** it is still another product, but **from the same family**, and the MVP enters v1: **Joker Deck** (document 20), with inventory, desired state from Git, rollouts in waves with KPI stop, and fleet health. The node keeps working without it (local-first). The contract between the two is the `agent/1` protocol (ref/10).

## 8. Microcontrollers — resolved in v1

**Joker Micro** (document 19): the same `no_std` core, the same Rust apps compiled natively, three delivery modes (firmware, C library, Rust crate) and Joker Link to talk to the Linux nodes and to the Deck. The C ABI and the artifacts are in ref/08, and Joker Link at the wire level in ref/07.

## 9. Certified meter — out of niche

Joker does not certify energy measurements for billing (MID, *Eichrecht*). **What it can do, and it is cheap:** transport the records **signed by the meter** (formats such as OCMF or SML with signature) intact, from the meter to the destination, without altering them, so that the chain of evidence remains that of the certified meter.

## 10. Legal framework for data — ground truth

Images with people, household consumption and vehicle location are personal data. Joker provides mechanisms (no recording by default, retention by policy, encryption, control over what leaves through the uplink), and the legal basis belongs to the integrator.

**Anonymization and sharing with third parties — out of niche.** Pseudonymizing sites, anonymizing across sites (k-anonymity, differential privacy) and serving several consumers with access control and consent belongs to the data platform: the edge sees only one site, and real anonymization requires seeing the population. Joker keeps what prevents leaks at the node's exit (signal classification, output denied by default, encryption of what leaves, redaction in logs, retention): document 26, §8.

## 11. OS update — out of niche (for now)

Kernel, bootloader, secure boot and system partitions stay with RAUC, SWUpdate, Mender or the board's distribution. The Deck integrates with them for coordinated updates. **Hypothesis for after v1:** a minimal Linux image ("Joker OS", Yocto or Buildroot) with PREEMPT_RT, A/B and `jokerd` preinstalled, for the reference boards. It would solve the RT kernel, `isolcpus` and updates in one go. It is only justified with large fleets asking for it.

## 12. SCADA and HMI — out of niche; commissioning to achieve

Mimic diagrams, operator screens and multi-site alarm management belong to a SCADA or an HMI, which connect through the bridges or the API. **But** a **commissioning** interface (signal dashboard, manual writes, trace, replay, instance state) is DX, and it is needed: **Joker Studio** (`jk ui`, served by the node or by the Deck), in v1.x, as a client of the local API (ref/09) and of the TypeScript library in the browser (ref/05, 1.3).

## 13. Plant historian — out of niche

The local historian keeps days to months on a node. A plant's years of history live in a central historian (TimescaleDB, InfluxDB, PI…), and Joker sends to it.

## 14. Third-party history and sessions — out of niche

The equipment's internal history is fetched through an action when needed. Sessions with per-operator identity do not propagate to the equipment: Joker connects with a service identity, and the operator's identity (the principal of ref/09 §2.2) stays in the decision log.

## 15. Classes do not make equipment interchangeable — ground truth

Two instances of the same class accept the same methods and do not respond in the same way (ramps, timings, faults). Class conformance guarantees a minimum behavior. Replacing a piece of equipment still requires reviewing the envelope and running the scenarios.

## 16. Node redundancy — to achieve, v2

**The limit (new).** A critical site with a single node has a single point of failure. Today, the answer is the equipment fallback and, with Joker Micro, the `safe` on the coprocessor.

**Path.** The design already has the pieces for **active/standby**: the arbiter with leases, the immutable generations and the trace. Two nodes with the same generation; the active one writes, and the standby reads everything and keeps the process image warm; the switchover happens when the active one's lease expires, with a quorum arbiter (a light third node, or an MCU) preventing *split-brain*. It is v2 work, and it changes nothing in the v1 core. The references already reserve what v2 needs: the first `@` segment in paths (ref/00 §2), `/v2` in the API (ref/09 §13) and `agent/2` with the site keys (ref/10 §3.14).

## 17. IEC 61131-3 languages — to achieve, v1.x

**The limit (new).** Automation engineers write in Structured Text (ST), and many will not learn Rust.

**Path.** ST is, by design, cyclic and deterministic, which fits the cycle domain exactly. There are open ST compilers on top of LLVM (for example, RuSTy, written in Rust). An ST frontend with the Joker SDK would compile the same app to Wasm (Linux) and to native (Joker Micro), as a tier 2 language. Classes appear to ST as function blocks. Ladder and FBD stay out.

## 18. Machine learning — deferred to 1.1

v1 is the runtime that interacts with the devices. ML (document 06) moves to **1.1**, and v1 is built ready for it (document 11, §"Ready for ML"): the WIT package reserves the `ml` and `frames` interfaces as `@unstable` (ref/04 §2.9), the resource classes include `ml`, the trace reserves the model outputs, and the agent and the lock reserve the `.jkm` artifact type.
