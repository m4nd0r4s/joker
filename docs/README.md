# Joker — design documentation

The wildcard runtime for edge devices: the same core on embedded Linux and on microcontrollers, a single abstraction (the signal), a safety envelope between each app and the hardware.

Status: **design document.** None of this is implemented. The figures attributed to existing projects come from cited sources; the proposed targets are estimates to validate.

---

## Index

| File | Contents |
|---|---|
| `00-overview.md` | The thesis, the seven bets, the seven mechanisms, DX, landscape, glossary |
| `01-architecture.md` | Layers, processes and threads, process image, scheduling, envelope, arbiter, Wasm host, ML, historian, uplink, API, OTA, security, observability, failures, figures |
| `02-signal-model.md` | Types, units, quality, time, paths, scalar signals, block signals and streams, read and write semantics, actions, expressions |
| `03-manifest.md` | Complete reference of `joker.toml` and `joker.lock` |
| `04-sdk.md` | The WIT world, the Rust, TypeScript and Python SDK, tests, simulation |
| `05-drivers.md` | The three driver tiers, the trait, read plans, profiles, discovery, catalog |
| `06-ml.md` | The ML subsystem, the `.jkm` artifact, pipelines, on-device training, **analysis of Burn** |
| `07-cli.md` | The complete `jk` CLI |
| `08-stress-tests.md` | Six stress rounds on real use cases and what broke |
| `09-architecture-under-stress.md` | Twelve attacks on the architecture itself: budgets, failures, abstraction leaks, versions, security |
| `10-design-changes.md` | The 26 primitives that came out of documents 08 and 09 |
| `11-roadmap.md` | Phases up to v1.0, exit gates, decision point |
| `12-limits.md` | **What Joker does not do, and why** |
| `13-implementation-plan.md` | Crates, fronts and the first tasks of Phase 0 |
| `14-classes-and-instances.md` | Equipment classes (`Led`, `Pump`, `Inverter`), implicit, composite and virtual instances, `Many<T>`, active implementations, 14 attacks on the design |
| `15-files.md` | Map of all the files (manifest, overlays, lock, profiles, boards, classes, simulation, scenarios, apps, artifacts), the `<name>.<type>.toml` naming rule (ADR 0026) and the board/device/profile/instance/class vocabulary |
| `16-core-semantics.md` | **Normative for Phase 0**: exact rules of the arbiter, the envelope, quality, expressions; unit catalog; aliases; trace format; schema of the conformance cases; code registry |
| `17-jkc.md` | The `.jkc` class IDL: declarations, `method` vs `action`, grammar, execution, adapters, generation, versions |
| `18-v1-readiness.md` | The readiness round: end-to-end attacks, what changed, the verdict on closing v1 and starting to implement |
| `19-joker-micro.md` | **Joker on microcontrollers**: `no_std` core, firmware, C library and crate modes, Joker Link, coprocessor, 10 stress cases |
| `20-family-and-deck.md` | The family (Joker, Micro, Hub, Deck, Studio), the local-first principles and Joker Deck |
| `21-from-v1-to-v2.md` | **After 1.0**: versions 1.1 to 1.5 (LTS) by theme, v2 (the site as a system), the compatibility rules and the stress of the plan |
| `22-api-round.md` | **The API round**: the map of the interfaces, a value and a write end to end, the 23 cross-cutting decisions, stress across surfaces, what this asks of Phase 0 |
| `23-development-bench.md` | **The current bench**: Raspberry Pi 3 B+, Pi Zero W and ESP32, without buying anything; roles, wiring, bench firmware, manifests, board profiles, what is validated in each phase and what remains to be proven |
| `24-audience-and-uses.md` | **Who Joker is for**: the buyer, whoever works with it every day, the channel, the general audience, the first hour and the first week of each, and eight proposals (whole board as a device, expander firmware, external apps, read-only dashboard, day-1 catalog, OCPP, `std/` classes, bridge to Home Assistant) |
| `25-lessons-from-the-rust-embedded-ecosystem.md` | What to learn from PAC, HAL, BSP, `embedded-hal`, Embassy, `esp-hal`, `probe-rs`, `defmt`, Tock, Hubris and Zephyr: what to adopt, what not to, and the dependency risks |
| `26-final-review.md` | **The final review**: audit, lessons from the ecosystem, 12 stress cases, the scope against the team (ADR 0028), the first vertical slice (M0), candidate features, the data protection boundary and the verdict on starting |
| `ref/` | **Normative references of all the interfaces** (ADR 0023): `00` conventions · `01` manifest · `02` lock, node, client, secrets · `03` profiles, boards, simulation, scenarios, conformance · `04` WIT and Rust SDK · `05` Python and TypeScript SDKs, clients · `06` N/W/X drivers · `07` Joker Link · `08` Joker Micro C ABI · `09` HTTP API · `10` agent protocol |
| `proposals/` | Proposals and their reasoning; **not normative** (the IDL one was incorporated into 17) |
| `adr/` | Architecture decision records |
| `GLOSSARY.md` | English term for every domain word, translation style, the file-name map (ADR 0029) and the identifier map (ADR 0036) |
| `errata/` | Errata found while translating the specification, with their status; **not normative** |
| `examples/` | Guided walkthrough and three complete projects |
| `track/` | **Learning track** in 11 parts and 4 modules, on the bench of document 23 (laptop, Pi 3 B+, Pi Zero W, ESP32; nothing is bought): signals and the Linux node (the `ACT` LED, the CPU temperature, the ESP32 over Modbus RTU and TCP, faults and replay, switching boards), classes (those of `std/` and your own), Joker Micro on the ESP32 (Joker Link over serial and over Wi-Fi, an app on the MCU) and the fleet (`prod`, secrets, the Deck and a rollout in waves) |

---

## Precedence (v1)

When two documents diverge, the following prevail, in this order: **16** and **ref/00** (core semantics and representation) → **17** (`.jkc`) → **ref/01–ref/10** (interface references, with the files of `wit/` and `spec/`) → **01–05, 14, 19** (normative per area, which explain) → **20** (principles and contracts of the family) → **06–07, 15** → ADRs → **08–13, 18, 21, 22, 24–26** (analysis and planning) → **examples/**, **track/** and **proposals/** (illustrative). An example that contradicts a normative document is a bug in the example.

## Where to start

**If you have 10 minutes:** `00-overview.md` and then `12-limits.md`.

**If you are going to decide whether this gets built:** `08-stress-tests.md` and `09-architecture-under-stress.md`. That is where the design is attacked instead of defended.

**If you are going to implement:** `01` → `02` → `09` → `04` to understand, then `22` and the reference of your front in `ref/`, which is what gets implemented. The ADRs when you disagree with something.

**If you are learning:** `track/README.md`, part by part, with the bench of document 23: a Raspberry Pi 3 B+, a Pi Zero W, an ESP32 and the laptop, with no breadboard or components. Part 1 runs on the laptop only, in the simulator.

**If you are going to write apps:** `examples/00-first-project.md`, then `02` and `04`.

**If you are going to have ML actuate:** `06-ml.md`, round 6 of `08` and ADR 0012.

---

## In three sentences

The app never touches the world: it reads a consistent snapshot of signals and proposes writes, which a native envelope (range, rate, interlocks, lease, safe state) accepts, clamps or rejects before they reach the driver.

Control runs in a real-time cycle domain, without allocation or blocking. ML, networking and the historian run in a separate async domain, which can never delay it, and telemetry drops counted samples instead of stalling control.

Because the clock is an input and all I/O goes through the process image, any execution recorded in the field can be replayed bit for bit on a laptop, and any new version can run first in shadow mode.
