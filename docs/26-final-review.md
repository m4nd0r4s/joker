# 26 — Final review and the decision to start

The last round before writing code. It brought together four views: a **coherence audit** of the whole corpus, the **lessons from the Rust embedded ecosystem** (document 25), a **stress test through the eyes of those who have to build** this, and a list of **candidate features**. It ends with a verdict and with the decisions left to the project owner.

It is analysis and planning (precedence of `docs/README.md`). The cut proposal of §5 came into force with the acceptance of ADR 0028 (2026-10-21).

---

## 1. Where we are

| What | Status |
|---|---|
| Documents | 00–26, plus the track (11 parts), the examples and the proposals |
| Normative references | 11 (`docs/ref/00`–`10`), with `wit/` (3 packages), `spec/cddl/` (6), `spec/openapi/` and `spec/c/` |
| ADRs | 0001–0028 accepted (0028 on 2026-10-21) |
| Code registry | `spec/codes.toml` (the count lives there; it is not repeated in the prose, B8) |
| Validation | all the TOML blocks and `codes.toml` with `tomli`; the OpenAPI with `openapi-spec-validator`; the C headers with `gcc -pedantic`; the WIT files with the official parser (`jco`); the CDDL files with a syntax parser |

What has **not** been validated is still task 0a of Phase 0: the Rust signatures and the TypeScript of the references were never compiled, and the CDDL files (except that of `joker:meta`) were not validated against instances.

## 2. Coherence audit

The audit checked about 4,300 cross-references by script and reviewed the content by sampling. It fixed about 30 mechanical faults (cross-references, "free" code ranges that already had codes, counts, indexes) and 17 unequivocal contradictions. That left **13 contradictions that called for a decision**, and they were all decided and applied:

| # | Contradiction | Decision |
|---|---|---|
| A1 | a failing `requires_good` had no result | `Rejected(Quality)` and the output's `on_interlock` (16 E4) |
| A2 | `Rejected(ClockUnsynced)` outside the envelope order | step 0 of the envelope; it renews the lease (01 §7, 16 E1) |
| A3 | leases and manual levels on MCU outputs | lease = `min(TTL, the link's max_lease)`; the MCU does not persist remote levels (16 A12/A14) |
| A4 | `joker:driver` 1.0 or 1.2 | unstable in 1.0, for in-tree drivers only; stable for third parties in 1.2 |
| A5 | 1.1 APIs in the tier 2 SDKs | the same behavior as in Rust |
| A6 | `max_age` equal to `poll` | rejected (`J0816`) |
| A7 | three app state enums | a single `app_state` (ref/00 §6) |
| A8 | rotating a secret on an MCU | a new blob with the same Linux generation (exception to ADR 0024 §3, written down) |
| A9 | divergent phases of drivers and classes | `v4l2`/`spi-adc` 1.1; BACnet and EtherCAT 1.2; initial `std/` per 14 §10 |
| A10 | two crashloop thresholds with the same code | one table per tier in ref/06 |
| A11 | three lists of `no_std` crates | the one in document 13 §1 |
| A12 | old ADRs without "amended by" | a note on ADR 0019 in the affected ones |
| A13 | minor ones (12 normative?, `.jkm`, "it is not the fleet server") | aligned |

## 3. Lessons from the Rust embedded ecosystem

The detail and the sources are in **document 25**. The five lessons that weigh the most:

1. **Inherit from the HAL down, do not redo it.** The Joker Micro drivers sit on `embedded-hal` 1.0 and `embedded-hal-async`, which already have implementations for every MCU family that matters. Where `embedded-hal` does not reach (ADC, CAN), Joker defines two small traits of its own.
2. **The generic firmware collides with compile-time pin ownership.** The HALs hand over each pin once, with the type decided at build time; the generation in flash only says at runtime what each pin is. The way out is a pin table built at boot, without `steal` or `unsafe`.
3. **`critical-section` and `portable-atomic` become core invariants**, for the 64-bit counters on 32-bit MCUs.
4. **Chips are identified by their exact name**, and the ecosystem crates are chosen by *features* per family (`chip-rp`, `chip-stm32`, `chip-esp`), with the `[board] chip` key in the board profiles.
5. **The risk is below the core:** Embassy is at 0.x versions, `esp-hal` only has the basic peripherals stable, the ESP32's Wi-Fi is still in beta, and Xtensa remains outside official LLVM. The rule: the core only depends on 1.x crates, and what is 0.x stays in the per-chip modules, with the toolchain pinned per family.

**Not adopted:** *typestate* in the app API, a firmware compiled per site, Kconfig, direct register access. **What the ecosystem does not have, and is Joker's value proposition on top of it:** quality, units, envelope, arbiter, replay and fleet.

The high-priority proposals (E1–E6) are already applied in the specifications; E7–E17 stay in document 25.

## 4. Final stress: twelve new cases

| # | Case | Result | Fix |
|---|---|---|---|
| E1 | the first external person contributes a driver | **broke** (process) | `CONTRIBUTING.md` with DCO, `drivers/contrib/` with its own level, `drvtest` required, an owner per driver (depends on the license) |
| E2 | a security bug in wasmtime | holds, with work | follow the wasmtime long-support versions; AOT cache indexed by version; `.cwasm` generated in CI; `SECURITY.md`; seccomp and Landlock in `jokerd`; the `safe` on the MCU as the last layer |
| E3 | updating the runtime in a fleet with old MCUs | **broke** | **applied:** OTA receiver mandatory in the 1.0 firmware (`J0795`); core version of each MCU in the lock, and `jk check` rejects features it does not know (`J0796`) |
| E4 | a customer without Rust | broke the DX | `jk new site --template no-apps` template; `jk` only requires `cargo` if there are Rust apps; precompiled catalog apps (F14) |
| E5 | compiling `jokerd` on a Pi 3 | **broke** | **applied:** nothing is ever compiled on the node; cross build on the laptop and in CI |
| E6 | binary size | holds if measured | **applied:** targets for the `minimal` build and the full one, `cargo bloat` with a limit in CI; 01 §21 no longer mentions ML in 1.0 |
| E7 | the documentation diverges from the code | **was already happening** before the first line | **applied:** the counts refer to the source and are checked by `joker-spec-tests`; proposal: changes at three levels (erratum, reference change, ADR), and in Phases 0–1 a divergence opens a triage *issue* within 48 h |
| E8 | dependencies with a single maintainer | holds with a policy | the core without dependencies; the link's own CBOR codec; `cargo vet` and `cargo deny` from day one; a table of critical dependencies with a *fork* plan |
| E9 | a dependency changes license | holds if there is a list beforehand | license *allowlist* in `cargo deny`, narrower for the MCU; tier X is the boundary for copyleft libraries; decided after Joker's own license |
| E10 | DX with many error codes | broke slowly | `jk explain` with an example per code; each active code with a test that produces it, otherwise it becomes `reserved`; merge near-duplicates before 1.0 |
| E11 | the decision log on an SD card | **broke** (50 fsync/s per output) | **applied:** the log stores result changes with a repetition count; immediate `fsync` only for trips, manual writes, rejections and level changes; `storage` on the board and warning `J0381` |
| E12 | the Xtensa toolchain lagging behind | broke from time to time | **applied:** the Xtensa job does not block the CI; core MSRV ≤ the Xtensa toolchain; the ESP32-C6 (RISC-V) is the preferred MCU when one is bought |

## 5. The scope against the team

**What v1.0 promises today**, counted: 11 references (~234,000 words of normative text), hundreds of codes, ~150 operations in the API, 16 Linux drivers and 5 MCU drivers, 25 Joker Link messages with 6 transports, ~95 `jk` commands, 28 crates, three delivery modes on the MCU, plus the Deck and the Hub.

**The estimate** (rough, with experienced people, without pilots): **~350–435 person-weeks**. The capacity of document 11 is ~155 (3.5 people × 44 weeks). **It falls short by 2.2 to 2.8 times.** With the planned team, v1.0 as written ships in 2.5–3 years.

**The biggest risk is not the design, it is the breadth.** The core is small and well specified; what kills is the periphery. And the golden rule (ADR 0022) makes this worse: everything that ships stable in 1.0 is frozen for the whole of series 1, including hundreds of things specified but never implemented.

**The proposal: ADR 0028, "stable = implemented".** In 1.0, only what was implemented and passed the exit gate is stable; the rest is **reserved**, as ML already is (`@unstable` in the WIT, reserved key in the manifest, `x-joker-since` in the OpenAPI, reserved message id, `status = "reserved"` in the code registry). Adding later is a *minor*, and the golden rule stays intact. What must be right from 1.0, because it stays in the field without a software update: the trace format, layers L1–L3 of Joker Link with negotiation, the OTA receiver on the MCU, the minimal `agent/1.0` core, the stable subset of the WIT, the lock format, the code namespace and the media types.

**The proposed cut:**

| Version | What |
|---|---|
| **1.0 (must ship)** | complete `no_std` core and ≥ 150 conformance cases; manifest, lock, overlays, RT admission; `jokerd`, `joker-host`, Rust SDK, `joker-test`; drivers `sim`, `modbus-tcp`, `modbus-rtu`, `sunspec`, `gpio`, `sysfs`, `i2c`, `mqtt` (input), `joker-link`; classes (`jkc`, minimal `std/`, implicit, composite and virtual instances); historian and MQTT/Sparkplug B uplink; the API and the `jk` that the track uses; Rust and Python client; Joker Micro in firmware and crate mode, ESP32 and RP2350, generation in flash, OTA receiver; Joker Link over UART, UDP and TCP; `agent/1.0` with GitOps; secrets, signatures, local CA |
| **1.0.x** (drivers and extensions, ADR 0022 §3) | `revpi`, `spi`, `opcua` (client), `m-bus`, `onewire`, `serial-line`, `modbus-slave` on the MCU, ESP32-C6 and STM32, LSP, class simulators |
| **1.1** (with ML) | shadow mode for apps, uplink budgets, OTLP, orchestrated MCU OTA, CAN, CANopen, RS-485 with turn-taking on the link, TypeScript client |
| **1.2** | TypeScript SDK (with Python), tier W and X hosts, OPC UA and Modbus server bridges, Zenoh and ROS 2, Joker Micro C library |
| **Deck and Hub** | leave the critical path of the runtime: "Deck 0.x" with its own versions, speaking `agent/1.0` |

With the cut, the work drops to ~190–240 person-weeks. **It still does not fit** in 155: it is necessary to choose between ~60–65 weeks with 4 people, or keeping the 44 weeks with 5 people (one on Micro from Phase 0). The pilot gates also change: the 30-day BESS pilot becomes 30 days on the bench (with a real pilot when there is a customer), and the 50 + 50 node pilot becomes 10 + 10 for 30 days.

## 6. The first vertical slice (M0)

Before the rest of Phase 0: **6 weeks, 1–2 people, on the bench of document 23**, to prove the design end to end and measure the first real figures.

| Layer | In M0 |
|---|---|
| core | registry, slots, image per group, arbiter (`safety`, `manual:4`, app), envelope (range, rate, `safe`, lease), trips, trace format; already `no_std` and without allocation in the tick |
| manifest | `[project]`, `[target]`, `[buses]`, `[devices]`, `[outputs]`, `[cycles]`, `[apps]`, `extends`, ~40 codes, minimal lock |
| drivers | `sim`, `sysfs` (`ACT` LED and temperature), `modbus-rtu` with the `local/bench-esp32@1` profile |
| bench firmware | the one of document 23 §4, RTU only |
| host and SDK | the cycle subset of `joker:runtime`, cached AOT, `#[joker::app]` and `#[joker::cycle]` |
| `jokerd` and `jk` | one cycle thread, drivers in tokio, generations with `apply --confirm`, API on the Unix socket (`jk` reaches it over SSH); `new`, `dev`, `check`, `apply`, `confirm`, `read`, `write`, `record`, `replay` |
| app | the `light` of parts 1–5 of the track, without classes |

**Exit gates:** (1) the same `light.wasm` runs in the simulator and on the Pi 3, and only the manifest changes; (2) with the ESP32 cable unplugged, the samples become `bad(comm_failure)`, the LED turns off through the firmware and the app sees `Err`; (3) a manual write with a TTL beats the app and expires by itself; (4) a generation that breaks the bus is reverted if it is not confirmed; (5) **10 minutes recorded on the Pi 3 are replayed on the laptop with the same bytes, 100 times**; (6) 30 conformance cases, written by someone who does not read the core code, pass on x86_64 and aarch64, and the core compiles for `thumbv8m` and `xtensa-esp32`; (7) the unit error of `jk check` comes out as in document 00; (8) the first real figures (jitter on the Pi 3, size, RSS, startup, cost of the Wasm call, build time); (9) a **friction report**, in which each divergence between the specification and reality becomes an erratum or an ADR.

At the end of M0 the cut of §5 is reassessed with real figures: it is the decision point of document 11, brought forward from ~20 weeks to 6.

## 7. Candidate features

Outside the proposals P1–P8 of document 24. **The five recommended:**

| # | Feature | Why | Version |
|---|---|---|---|
| **F1** | CI integration: `jk check`, `jk test`, `jk lock --check` and regression replay in an action, with SARIF diagnostics annotated on the PRs | the fleet model is GitOps; it is where the integrator lives; cheap | 1.0 |
| **F2** | `jk why <output>`: why the output has this value (winning level, lease, *clamps*, interlocks, latest decisions and the inputs read), on a single screen | the field technician and debugging; the data already exists | 1.0 |
| **F3** | `/metrics` in Prometheus format and official Grafana dashboards | it is what operations teams already use; the `joker.*` signals already exist | 1.0 |
| **F4** | Rehearsal of a generation against a recorded trace: "what would have happened yesterday with this configuration?" | an offline shadow, without risk; it makes replay sellable. It is only valid in open loop, and `jk` must say so in every result | 1.1 |
| **F5** | Site dossier generated from the manifest (signals, envelopes, safe states, interlocks, alarms, fallbacks, versions and hashes) | it is the *as-built* document the integrator delivers to each customer | 1.0.x |

**Others evaluated:** signed commissioning report (1.1), *fuzzing* of a site's envelopes (1.1; internal *fuzzing* of the formats is an exit criterion of Phase 1, not a feature), a second signature for changes that widen envelopes or change `safe` (1.1), TUI in `jk` (1.1), LSP for `.jkc` (1.1), simulation model fitted from the historian (1.2), in-browser *playground* with the core in Wasm (1.x), app linter (1.1), site templates and catalog apps in the Hub (1.1), conformance seal (1.2), *tag* export for SCADA (1.1), automatic maintenance counters per output (1.0.x).

## 8. Data protection: the boundary

Conclusion of the previous round: **anonymization and pseudonymization belong to the data platform**, not to Joker. Joker sees one site; anonymizing properly requires seeing the whole population, and sharing with third parties (access, consent, contracts) is another product.

What stays on Joker's side is **not letting it leak**. The proposed minimum:

| # | Minimum | Version |
|---|---|---|
| D1 | `data_class` per signal (`operational`, `personal`, `sensitive`), inherited by derived signals | 1.0 (key already reserved in 1.0) |
| D2 | egress denied by default: uplinks, bridges and exports declare the classes they accept | 1.0 |
| D3 | traces, exports and diagnostic packages encrypted per recipient (HPKE, like secrets) | 1.0 |
| D4 | logs and error messages never carry values of `personal` or `sensitive` signals | 1.0 |
| D5 | retention per class and deletion with audit | 1.0.x |
| D6 | encryption at rest, with the key in the TPM when one exists; `jk doctor` checks the OS disk encryption | 1.3 |

The tension that stays declared: replay needs the exact values, so a trace with personal signals is personal data. It goes out encrypted, has its own retention, and `jk replay` says so.

## 9. Verdict

**Yes, it is ready to start**, but with M0 (§6) and not "with the v1.0 of document 11". The specification is mature; the risk is no longer the design, it is the scope and the schedule. And the volume of the specification (1.45 MB normative) is itself a maintenance cost, so the rule of Phases 0–1 is that a divergence opens a triage *issue*, and not that "the code is wrong" by decree.

**They block the first commit:**

1. **The team:** how many people, and who owns which front (document 13). M0 needs 1–2 people and an independent conformance author, who can be an agent with access only to `docs/`.
2. **The license:** before the first dependency (the *allowlist* derives from it) and before any external contribution. Recommendation: that of document 18 (Apache-2.0 OR MIT).
3. **The cut and the new baseline of the roadmap:** accept ADR 0028 or not, and choose between ~60–65 weeks with 4 people and 44 weeks with 5, before starting. Without this, "late" means nothing.

**Decided on 2026-10-21:**

| Decision | Result |
|---|---|
| team | the project owner with several agents in parallel, one jj workspace per front (`AGENTS.md` §4); conformance is written by an agent that only reads `docs/` |
| license | Apache-2.0 OR MIT (`LICENSE-APACHE`, `LICENSE-MIT`) |
| cut | ADR 0028 accepted; the cut of §5 is the baseline, to be reassessed at the end of M0. The new schedule baseline of document 11 is left for the end of M0, with real figures |

**They have a deadline, but do not block:**

| Decision | By when |
|---|---|
| name and brand, `@joker` organization on npm | before the first public repository or crate |
| business model of the family | before publishing (it blocks the OEM channel) |
| CI hardware (CM4/CM5, RP2350 with a probe, ESP32-C6) | end of Phase 1: it is the first purchase, and it is cheap |
| day-1 catalog and which of P1–P8 (document 24) go in | end of Phase 1 |
| D1–D4 (§8) | during Phase 0, so that the key is reserved in the schema |

**During Phase 0 (after M0):** the open questions of document 16 §10; the three-level specification change process; the dependency, toolchain and wasmtime version policies; and task 0a (compiling the references).
