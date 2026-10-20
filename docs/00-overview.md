# 00 — Overview

**Joker** is the wildcard runtime: it runs on any edge device, from a Raspberry Pi, a RevPi or an industrial PC with Linux (`jokerd`) to a microcontroller that costs a few euros (Joker Micro, document 19), and it controls everything the device touches. It reads sensors, drives actuators, speaks the industrial protocols and collects telemetry; in 1.1, it also runs ML models. It is a family: the runtime, Joker Micro, the registry (Hub) and the fleet control plane (Deck, document 20). To whoever programs it, it offers a single abstraction: **typed signals, with unit, quality and time**.

The name comes from the card that stands in for any other. It does not literally serve for *everything*: document 12 says exactly where it does not go.

---

## The thesis

Today there are three families of edge software, and each one solves a third of the problem.

1. **Container orchestrators** (balena, AWS Greengrass, Azure IoT Edge). They handle the deploy well and the device badly. They do not know what a Modbus register is, they have no notion of a safe state, and every container reinvents drivers, buffers and telemetry.
2. **Protocol gateways** (Eclipse Kura, EdgeX Foundry, Node-RED). They speak many protocols, but ML comes in as an appendix and the robustness is that of a web application. EdgeX is several microservices in containers, which on a CM4 with 2 GB is expensive. Kura is Java/OSGi. Node-RED is great for prototyping and weak at guaranteeing.
3. **PLC runtimes** (CODESYS, TwinCAT, Siemens Industrial Edge). They are deterministic and safe, but closed and expensive, and the DX is that of classic automation. ML, modern telemetry and modern software stay outside.

**Joker brings the three thirds together in a single process, with a rule that all three violate:**

> The application never touches the world. It declares which signals it reads and which signals it wants to write. The runtime owns the I/O, the time, the safety and the telemetry. Every write is a *proposal*, which a native safety envelope accepts, clamps or rejects before it reaches the hardware.

From this rule come the properties that matter:

- apps are **portable between sites**, because they speak of ports and not of addresses;
- apps are **hot-swappable**, because the safe state does not depend on them;
- they are **testable without hardware**, because the world is a process image that can be simulated and replayed;
- ML can **actuate without becoming dangerous**: the envelope limits what the model can do, although it does not guarantee that it does it well.

---

## The seven bets

| # | Bet | Accepted cost |
|---|---|---|
| 1 | A single abstraction: the **signal** (value + quality + time + unit) | protocols with rich semantics lose detail; actions (M19) cover the rest |
| 2 | The app **does not write to the hardware**: it proposes, and the envelope decides | one more layer on every write (~µs) |
| 3 | **Two execution domains**: the deterministic cycle and the async one | whoever programs must choose where each thing runs |
| 4 | Apps in **WebAssembly** (Component Model, WASI 0.3) | boundary overhead; the native Rust SDK remains as an escape hatch |
| 5 | **The clock is an input** to the core, and the world is reproducible | strong internal discipline; ML on GPU requires recording the outputs (M22) |
| 6 | **The same core on Linux and on microcontrollers** (`no_std` from day one); ML comes in 1.1 as a runtime resource | discipline of fixed capacities in the core; ML waits for 1.1 |
| 7 | **Telemetry never blocks control** | drops are possible, but always counted and never silent |

---

## The seven mechanisms that set Joker apart

**1. Process image with snapshot.** At the start of each cycle the inputs are frozen into a consistent snapshot, and the outputs written during the cycle are only applied at the end, all at once. There are no torn reads and no half-done writes. It is the model of a PLC, offered to modern code.

**2. Per-output safety envelope.** Each actuator declares its range, its maximum rate of change, its interlocks, its safe state and its lease. The envelope runs in native code, after arbitration and before the driver, and **there is no path that bypasses it**, not even a manual command from the CLI.

**3. Mandatory safe state.** An output without a declared `safe` is a validation error: there is no default. When the writer stops renewing the lease (because the app died, the model hung or the network went down), the output goes to the safe state, and the runtime checks that the *device's own fallback* is configured coherently (M1).

**4. Ports and bindings.** An app declares typed ports with a unit in its code (`grid_power: kW`). The site manifest binds each port to a concrete signal (`meter.p_total`). An incompatible unit is a compilation error, and units of the same dimension (W → kW) are converted by the runtime. The same app, without recompiling, runs on a hundred sites with a hundred different register maps.

**5. Arbitration with priorities and leases.** Each output has an arbiter with priority levels, in the manner of the BACnet *priority array*: `safety > manual > supervisor > optimizer`. The active lease with the highest priority wins, and a manual override expires on its own. It solves at the root the conflict "the operator changed it, the model undid it" (M15).

**6. Shadow mode.** A new version of an app or of a model runs in parallel with the current one, with the same inputs, but its writes are recorded instead of applied. `jk shadow diff` compares the two, and promotion is conditional on declared criteria. It is the only path to put into production something that actuates (M16).

**7. Deterministic replay.** Because the clock is an input and all I/O goes through the process image, a recording (`jk record`) is enough to replay exactly what an app did (`jk replay`), on a laptop and without hardware. Field bugs become regression tests.

---

## DX in three screens

**First contact: five minutes, no hardware.**

```sh
jk new tank --template sim-tank     # simulated plant: tank, pump, level sensor
cd tank
jk dev                                # builds, starts the local runtime, hot-reload
```

**A complete app in Rust:**

```rust
use joker::prelude::*;

#[joker::app]
struct Level {
    #[input(unit = "m")]    level: Input<f64>,
    #[output(unit = "%")]   pump:  Output<f64>,
    #[param(unit = "m", default = 1.5)] target: Param<f64>,
}

#[joker::cycle(period = "100ms")]
fn control(io: &mut CycleIo<Level>, _st: &mut ()) -> Result<()> {
    let err = io.level.get()? - io.target.get();     // `?`: bad quality is an error, never a number
    io.pump.set((err * 80.0).clamp(0.0, 100.0));     // the pump empties the tank
    Ok(())
}
```

**An error, as `jk check` shows it:**

```
error[J0107]: incompatible unit (dimension)
  --> joker.toml:42:18
   |
42 | bind.soc = "pcs.p_ac"
   |            ^^^^^^^^^^ `pcs.p_ac` is a power (kW); port `soc` expects a fraction (%)
   |
   = help: signals with unit % on this site: bms.soc, bms.soh, inv1.derate
```

The same `.wasm` artifact that runs on the laptop against the simulated plant runs on the RevPi against the real hardware. Only the manifest changes.

---

## What exists today, and where Joker comes in

| Project | What it does well | What it lacks compared to Joker |
|---|---|---|
| Eclipse Kura | many protocols (Modbus, OPC UA, S7) and MQTT | Java/OSGi, heavy; no safety envelope; weak ML |
| EdgeX Foundry | device normalization, ecosystem | microservices in containers; latency between services; no deterministic cycle |
| Node-RED | thousands of protocol nodes, prototyping | robustness, types, tests, performance |
| AWS Greengrass / Azure IoT Edge | deploy and ML coupled to the cloud | do not model the device; lock-in |
| balena | OTA and container fleets | knows nothing about I/O |
| CODESYS / TwinCAT | determinism, IEC 61131-3 | closed, expensive, classic DX, external ML |
| WasmEdge / wasmCloud | Wasm at the edge | generic: no signals, no drivers, no envelope |
| ForestHub edge-agents | offline AI agents on RPi/Jetson | focus on agents; industrial protocols still on the roadmap |

Joker **does not compete** with the OS or with SCADA, and it integrates with both (document 12). Fleet management belongs to **Joker Deck**, the family's control plane (document 20, ADR 0021), and Joker talks to it (or to another manager) through the agent (ref/10).

---

## How to read this documentation

| Document | For whom | When |
|---|---|---|
| **00** this one | everyone | first |
| **01 Architecture** | whoever implements | normative reference |
| **02 Signal model** | everyone | before writing an app |
| **03 Manifest** | whoever configures sites | daily reference |
| **04 SDK** | whoever writes apps | daily reference |
| **05 Drivers** | whoever integrates hardware | reference |
| **06 ML** | whoever puts models to run (includes the analysis of Burn) | reference |
| **07 CLI** | everyone | daily reference |
| **08 Stress tests** | whoever decides | before accepting the design |
| **09 Architecture under stress** | whoever implements | before writing the core |
| **10 Design changes** | whoever followed v1 | after 08 and 09 |
| **11 Roadmap** | whoever plans | — |
| **12 Limits** | **everyone** | **before promising anything at all** |
| **13 Implementation plan** | whoever starts Phase 0 | — |
| **14 Classes and instances** | whoever writes business logic or abstractions | after 04 |
| **15 File map** | everyone | when you get lost among files |
| **16 Core semantics** | whoever implements Phase 0 | normative reference |
| **17 IDL `.jkc`** | whoever writes classes | reference |
| **18 v1 readiness** | whoever decides | before the first commit |
| **19 Joker Micro** | whoever integrates MCUs | normative reference |
| **20 Family and Deck** | whoever plans the product | — |
| **21 From v1 to v2** | whoever plans the product | after 11 |
| **22 API round** | whoever implements or integrates | before opening a reference: the map of the interfaces and decisions D1–D23 |
| **23 Bench** | whoever develops and tests now | before connecting hardware: what each board does and what is validated with it |
| **24 Audience and uses** | whoever decides the product | before prioritizing 1.x |
| **25 Rust embedded ecosystem** | whoever implements Joker Micro and the drivers | before Phase 2 |
| **26 Final review** | whoever decides to start | **before the first commit** |
| **ref/** References | whoever implements an interface | normative reference for its surface, above the per-area documents; starts at `ref/00-conventions.md` |
| **track/** | whoever is learning | first, if you have never used Joker |
| **adr/** | whoever questions a decision | when questioning it |
| **examples/** | whoever is learning | after 02 |

---

## Glossary

| Term | Meaning |
|---|---|
| **Signal** | named and typed quantity, with a unit; each reading is a **sample** (value, quality, time, seq) |
| **Quality** | `Good`, `Uncertain(reason)` or `Bad(reason)`; inherited from OPC UA |
| **Path** | stable name of a signal: `device.group.signal`, e.g. `inv1.ac.p` |
| **Port** | input, output, parameter or action declared by an app, with type and unit |
| **Binding** | association, in the manifest, between a port and a concrete signal |
| **Process image** | dense vector with the last value of each scalar signal, frozen per cycle |
| **Cycle group** | set of periodic tasks that share period, priority and snapshot |
| **Cycle domain** | real-time threads where the cycle groups run |
| **Async domain** | tokio runtime where event-driven tasks, ML, network and historian run |
| **Envelope** | range, rate, interlocks, safe state and lease of an output |
| **Lease** | temporal validity of a write; once expired, the arbiter moves to the next level or to the safe state |
| **Arbiter** | decides, by priority, which of the competing writes on an output is the effective one |
| **Trip** | state of an output forced to the safe state until there is an explicit reset |
| **Device fallback** | what the equipment itself does when it loses communication; declared and verified |
| **Stream** | bulk data signal (images, point clouds) with shared buffers |
| **Block signal** | waveform: blocks of N samples with t0 and sampling period |
| **Pipeline** | native chain stream → preprocessing → model → postprocessing → signal |
| **Profile** | reusable map of a piece of equipment: registers, scales, units, fallback |
| **joker.lock** | pinned resolution of profiles, discovered maps, firmware versions and hashes |
| **Shadow mode** | parallel execution without effect, with the writes recorded for comparison |
| **Trace** | recording of the input snapshot and of the decisions, sufficient for replay |
| **Resource class** | set of CPUs, memory and accelerators assigned to a workload (`rt`, `ml`, `bg`) |
| **Action** | command-type port (reset, start calibration, OPC UA method), with a result |
| **Class** | versioned contract of an equipment type for a program (`led@1`, `pump@1`): members, capabilities, methods, queries |
| **Instance** | a concrete piece of equipment that fulfills a class: implicit (profile), composite (manifest) or virtual (app) |
| **Active implementation** | an app that implements a class with its own behavior, on top of lower-level signals |
