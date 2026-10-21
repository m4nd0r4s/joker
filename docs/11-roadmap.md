# 11 — Roadmap

> **Scope note.** The scope of 1.0 is the cut of [document 26, §5](26-final-review.md#5-the-scope-against-the-team) (ADR 0028, stable = implemented), not the plan below. Where this document puts in a phase before 1.0 something that the cut moves (the C library of Joker Micro, the ESP32-C6 and STM32 targets, CAN and CANopen, the 1.0.x drivers, Joker Link over CAN, USB or RS-485 with turn-taking, the bridges, class simulators, app shadow, uplink budgets, OTLP and Zenoh, orchestrated MCU OTA, the standalone MCU profile, the tier W and X hosts, the TypeScript SDK and client), the cut wins and the surface is reserved in the references. [ADR 0035](adr/0035-reconcile-the-references-with-the-1-0-cut.md) records the decisions that reconcile the two and lists the contradictions. The new baseline of this plan (schedule, pilot gates at bench scale, the Deck and the Hub off the critical path) is still to be written.

**v1 is the wildcard runtime that talks to devices, on Linux and on microcontrollers, with a manageable fleet.** Machine learning is **1.1**, and v1 is built ready for it (ADR 0019). Microcontrollers are in v1 (ADR 0020), and with them the core is `no_std` from day one.

Four phases up to v1.0, with measurable exit gates and a decision point after Phase 1. The durations are estimates for a team of 3–4 people with experience in Rust and automation, plus one person on the Deck from Phase 3. The ordering rule still holds: **first what makes the system safe, then what makes it useful, and finally what makes it scalable.**

```
 Phase 0 ──▶ Phase 1 ──▶ [decision] ──▶ Phase 2 ──▶ Phase 3 ──▶ v1.0 ──▶ v1.1 (ML) ──▶ v1.x / v2
 core        Linux MVP                  Joker Micro fleet and
 no_std                                 and interop trust
```

---

## Phase 0 — Deterministic and portable core (≈ 8 weeks)

**Goal:** the complete `joker-core`, **`no_std`**, with no I/O and no allocation in the tick, tested by conformance and properties. The reference specification is document 16, with the representation of ref/00.

**Before any feature**, the three tasks that close the references in code (document 22, §8):

- **0a — compile the references:** the `joker-spec-tests` crate compiles the Rust blocks of the references as doctests, runs `tsc` on the TypeScript blocks, validates the CDDL against the examples, generates the manifest JSON schema from the types and validates every TOML block against it, and validates the OpenAPI. CI fails if a reference and the code diverge.
- **0b — the code registry:** every code of the references in `crates/joker-manifest/codes.toml` (16 §9), with no collisions.
- **0c — the test vectors:** those of Joker Link (ref/07 §4.8), those of the C ABI (ref/08) and the core conformance cases become files in `conformance/`, run on Linux and on an MCU.

**Hardware:** until there is other hardware, Phases 0–2 are validated on the bench of document 23 (Raspberry Pi 3 B+, Pi Zero W, original ESP32), with QEMU for the missing architectures; each exit gate states where it was validated.

Then:

- `joker-units`, `joker-expr`, `joker-core` (registry, slots, image, arbiter, envelope, trips, alarms, expansion of class methods and queries, trace), all `no_std`, with fixed capacities and the `alloc` feature only for Linux.
- `joker-manifest` (TOML → validated model, J-codes, `extends`, lock v1, RT admission).
- `joker-drivers/sim` and `joker-test` (harness with virtual clock).
- `conformance/` with ≥ 150 cases, one file per suite (`conformance/<área>/<suite>.conformance.toml`, schema of ref/03 §2.6).
- CI: the core compiles for `x86_64`, `aarch64`, **`thumbv8m.main-none-eabihf` and `riscv32imac-unknown-none-elf`** (with conformance in QEMU, blocking) and for Xtensa (non-blocking); the conformance battery runs on the bench's real ESP32 and, when a board is available, on an RP2350 (document 13, §5).

**Exit gates:**

1. The 150+ cases pass, and the proptest of the arbiter and the envelope runs 10⁶ cases without violating invariants.
2. **The same cases give the same bytes on x86_64, on a real ESP32 (bench) and on Cortex-M33 and RISC-V in QEMU**, until an RP2350 board is available (document 23, §5.1).
3. The tick path does not allocate (no-alloc guard in CI; on the MCU, with no allocator linked).
4. Envelope + arbiter ≤ 300 ns per output on x86; the cost per output on a Cortex-M33 at 150 MHz is measured and published.
5. The same trace gives the same bytes in 1,000 runs.

## Phase 1 — Linux MVP (≈ 12 weeks)

**Goal:** the round 1 case (BESS + PV) on a bench, end to end.

- `jokerd`: threads, resource classes (M8), PREEMPT_RT, supervisor, hardware watchdog, generations and `apply --confirm`.
- `joker-host`: wasmtime, AOT, pooling, epochs, two instances per app, proposal buffer with method records.
- `joker-sdk` (Rust) with the Wasm and native backends (the latter used by Joker Micro in Phase 2), `joker:runtime@0.1`.
- Drivers: `modbus-tcp`, `modbus-rtu`, `sunspec` (discovery, M2), `gpio`, `i2c`, `sysfs` (LEDs and board temperature, document 23), `revpi`, `mqtt` (input). Device fallback (M1).
- Actions (M19), atomic groups (M20), versions (M21), clock quality (M14), alarms (M9), arbiter (M15), causal tracing (M3).
- Classes (M24), declarative part: `jkc` (document 17), registry, Rust codegen, implicit and composite instances, and the initial `std/`, which is the list of document 14, §10.
- `joker-store` v1, `joker-link` v1 (MQTT, Sparkplug B).
- `joker-api` (ref/09), `joker-client` (Rust, ref/05) and `jk` (the commands of parts 1–7 of the track, including `jk profile import`/`test` and `jk board probe`/`check`/`new`).
- Boards: `rpi4`, `rpi5`, `cm4`, `cm5`, `revpi-connect-4`, `revpi-connect-5`, `x86-generic`; `rpi3b-plus`, the bench board (document 23, §7).

**Exit gates:**

1. BESS bench pilot for 30 days without human intervention, with the fault matrix (document 09, attack 4) injected.
2. Jitter on CM4 and CM5 with PREEMPT_RT: p99.9 ≤ 200 µs at 1 ms.
3. RSS ≤ 90 MB on a site of 1,000 signals and 3 apps.
4. **DX:** a new person does parts 1–7 of the track (modules A and B: signals and classes, from the simulator to the Pi 3 and the Pi Zero W of the bench of document 23) with only the documentation, with the first app in simulation in < 30 min. Two sections of those parts use what only arrives later, and are left out of this measurement: the Python client of part 3, §9 (the Python and TypeScript client libraries are Phase 3; the API it uses is this phase) and `jk class verify` on hardware in part 7, §9 (the class conformance tests are Phase 2).

### Decision point (end of Phase 1)

| Criterion | Threshold |
|---|---|
| BESS pilot | 30 clean days |
| RT | ≤ 2× the target |
| DX | the measurements above, with 3 people |
| Profile cost | SunSpec + 2 PCS profiled and tested in < 2 weeks |

If RT fails, the promise on Linux drops to groups ≥ 10 ms, and the fast loops move explicitly to Joker Micro. If DX fails, the SDK is redesigned before Phase 2. If the profile cost fails, the importers get the investment first.

## Phase 2 — Joker Micro and interoperability (≈ 12 weeks)

**Goal:** the same core on microcontrollers, the three delivery modes, and the Linux node talking to the industrial world around it.

- `joker-micro` on Embassy, with reference targets **RP2350** (M1), **ESP32-C6** (M2, Wi-Fi) and **STM32G4/H5** (M1/M2).
- Delivery modes: **firmware**, **C library** (`libjoker_node.a` + `joker_node.h`) and **Rust crate** (document 19, §4).
- Embedded generation and **generation in flash** (generic firmware + signed blob).
- **Joker Link** (UART/RS-485, CAN, USB, UDP and TCP; ref/07) and the `joker-link` driver in `jokerd`, with profiles generated from the `HELLO`.
- MCU drivers: GPIO, ADC, PWM, I²C, SPI, UART, Modbus RTU client and **server**, CAN.
- MCU OTA (A/B, signed) over Joker Link.
- On Linux: `opcua` (client), `can`, `canopen`, `m-bus`; bridges (M11) OPC UA server and Modbus server.
- Classes, active part: virtual instances, class conformance tests, class simulators (part 8 of the track, and §9 of part 7, start to work).

**Exit gates:**

1. **Coprocessor on a bench** (document 19, U1): RevPi or Pi + RP2350 for 30 days, with `jokerd` killed 100 times and the outputs always at `safe` in ≤ lease.
2. A standalone ESP32-C6 node (U2) controlling a simulated heat pump, with generation in flash updated remotely 10 times.
3. An example of integration into existing ESP-IDF firmware through the C ABI (U5).
4. Budget: core + Link + Modbus RTU + 1 app + 64 signals ≤ 160 KB of flash and ≤ 48 KB of RAM on an RP2350.
5. Cycle jitter on the RP2350 at 1 kHz: p99.9 ≤ 10 µs.
6. **DX:** parts 8–10 of the track (your own class; the ESP32 as Joker Micro, over Joker Link over serial and over Wi-Fi) run on the bench with only the documentation.

## Phase 3 — Fleet, trust and v1.0 (≈ 12 weeks)

**Goal:** manage hundreds of nodes and MCUs without site visits.

- Linux runtime OTA (A/B); MCU OTA orchestrated by the node.
- **Agent protocol** `agent/1` (ref/10), published; crates `joker-agent` and `joker-agent-proto` (`no_std`, for the standalone MCUs).
- **Joker Deck MVP**: inventory, desired state from Git, rollouts in waves with KPI stop, fleet health. **Minimal Joker Hub**: OCI registry + publication checks.
- Uplink budgets (M12), complete overlays (M13), shadow mode **for apps** (M16).
- Zenoh/ROS 2 bridge; Zenoh and OTLP uplink.
- **TypeScript SDK** (`@joker/sdk`, `@joker/test`, `@joker/classes-<package>`) and Python and TypeScript client libraries of the API (ref/05). The Python SDK has its API closed in ref/05 and ships in 1.2.

**Exit gates:**

1. Field pilot with 50 Linux nodes and 50 MCUs on 3 hardware types for 60 days, with 2 runtime updates, 4 app updates and 2 MCU firmware updates, without visits.
2. A bad rollout stopped automatically by the Deck (document 20, D2) in a controlled trial.
3. Budget of 50 MB/month met on LTE, with alarms delivered in < 60 s.
4. **DX:** part 11 of the track (production, secrets, the Deck on the laptop with two nodes and a rollout stopped at the canary) runs on the bench with only the documentation, and §9 of part 3 with the Python client.

**At the end of Phase 3: v1.0 release.**

---

## v1.1 — Machine learning

- `joker-ml`, `.jkm`, Burn backends (Flex, CubeCL CPU, Vulkan, CUDA), ORT and HailoRT (document 06).
- Streams (M4), pipelines (M5), scheduled writes (M6), **block signals (M7)**, with high-rate acquisition by Joker Micro.
- Lineage (M10), shadow and promotion of models (M16), input guard and fallback controller (M17), proposal-only writers (M18), non-deterministic outputs in the trace (M22).
- On-device training.
- Experiment: Burn Flex `no_std` on M2 MCUs (small anomaly models).
- Exit gates: the vision and vibration benches (document 08, rounds 2 and 3) and the six Burn validations (document 06, §12.6).

### Ready for ML (what v1 already includes)

The v1 does not implement ML, but it does not force any change to receive it:

| In v1 | What for in 1.1 |
|---|---|
| the WIT package declares the `ml` and `frames` interfaces as `@unstable`, outside the stable world: rejected in `prod` (`J-E0003`), stubs in `dev` (ref/04 §2.9) | the shape can still change before ML exists; in 1.1 they become `@since(1.1.0)`, and adding functions is minor (M21). Scheduled writes (M6) and intents (M18) get the same treatment |
| the `joker:driver` world and the tier W SDK exist as `@unstable`, only for in-tree drivers (`J0860` outside the tree in `prod`; ref/06 §4) | stable and open to third parties in **1.2**, not in 1.1 |
| the `ml` resource class exists in the manifest and in the cpusets | models come in without touching scheduling |
| the trace format reserves the model outputs section (empty) | replay with ML (M22) does not change the format |
| the lock, the agent and the Hub reserve the artifact type `joker.model.v1+jkm` | model OTA is just one more artifact |
| the signal registry reserves the block and stream types | M4 and M7 do not change the `SignalId` or the slot |
| the envelope and the arbiter do not distinguish apps from models | a model that actuates is a writer like the others |

## v1.x and v2

The plan after 1.0 is in **document 21** (and the versioning policy in ADR 0022). In summary:

| Version | Theme | Headline |
|---|---|---|
| 1.1 | intelligence | ML (Burn, ORT, Hailo), streams, pipelines, block signals |
| 1.2 | industry and ecosystem | Structured Text, stable tier W driver SDK open to third parties (unstable in 1.0, ref/06 §4), Python SDK, BACnet, EtherCAT, IEC 104/61850, DLMS, EEBus |
| 1.3 | security and operation | gateway mode, secure protocols, SBOM, Joker Studio, remote and multi-tenant operation in the Deck |
| 1.4 | Micro+ | Wasm on M2 MCUs (`wasmi`), ML on MCUs, low power, LoRaWAN if there is demand |
| 1.5 (LTS) | fleet at scale | public Hub, fleet observability and audit, standalone MCUs at scale |
| 2.0 | the site as a system | active/standby, node mesh, WIT world 2, site manifest, Joker OS if there is demand |
| separate track | — | certifiable subset (SIL) of Joker Micro; federated learning (v2.x) |

## What is deliberately left for later

| Theme | Why wait |
|---|---|
| shared Wasm threads | waiting for stabilization in the Component Model (candidate for WIT world 2, document 21, §8) |
| federated learning | needs a real fleet and concrete cases (v2.x) |
| operation SCADA/HMI | another product (document 12, §12) |
