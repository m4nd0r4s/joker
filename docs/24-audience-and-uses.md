# 24 — Audience and uses

This document says **who Joker is for**, what each person does with it, and what it still lacks for each one. It is analysis and planning, like documents 11 and 21: the proposals of §5 only become normative when they enter a reference, with an ADR.

---

## 1. The thesis: one core, two audiences

Joker has **a professional audience**, which pays and which defines the design, and **a general audience**, which learns, experiments and builds community. Both are served with the **same core**, for these reasons, in this order:

1. **What protects a 1 MWh battery protects a relay in the garage.** Typed signals with unit and quality, envelope, `safe`, leases, tests with a virtual clock and replay are not "industrial things": they are what hobbyist automation platforms lack.
2. **High-level abstractions are the bridge.** When the industrial drivers and classes exist, whoever only wants to "turn on the light when the button is pressed" uses `std/led@1` and `std/button@1` without knowing what a Modbus register is. Layer 3 (document 14, §1) is, by construction, the layer of a wider audience.
3. **One audience must not weaken the other.** What the general audience needs comes in as **defaults** in `mode = "dev"`, **shortcuts** in `jk` and new **classes**; never as exceptions to the guarantees of the core. An output without `safe` is still `J0201` for everyone.

The professional audience comes first in the roadmap because it is the one that pays and because a design made for it serves the other; the reverse is not true.

## 2. The people

### 2.1 The main buyer: the energy and automation integrator

Installs and operates BESS, PV, heat pumps and chargers at many sites, with equipment from several manufacturers and grid operator requirements. It is towards them that the hard decisions lean. What they buy:

- a site is **a template with overlays**, not a new project (`templates/joker.bess-v2.toml`, ADR 0026);
- a profile written once serves **every site** with that equipment (Hub);
- the **envelope** protects the equipment from a bug in the app;
- the **Deck** does rollouts with a canary, KPI stop and rollback (document 20).

### 2.2 Who works with Joker every day

| Person | What they do | Where they work | What matters most to them |
|---|---|---|---|
| **Control programmer** | writes the logic (peak shaving, SG-Ready, load management) | `apps/`, classes, `jk dev`, `jk test`, scenarios, `jk replay` | testing without hardware; the app does not change when the equipment changes |
| **Hardware integrator** | gets new equipment talking | `*.profile.toml`, CSV → `jk profile import csv`, `jk profile test`, `*.board.toml`, `jk board probe` | a register map turns into a profile in an afternoon; the Hub already having the equipment |
| **Field technician and operator** | commissions, diagnoses, forces outputs, acknowledges alarms | `jk status`, `jk read`, `jk write --ttl`, `jk alarms`, `jk doctor` | understanding what is going on without reading code; nothing dangerous by accident |
| **Fleet owner (OT/DevOps)** | manages nodes, keys, versions and secrets | Deck, `fleet.toml`, `jk ca`, `jk key`, `jk secret`, `jk publish` | rollouts that stop by themselves; a Deck that neither reads secrets nor writes signals |

In a small team, one person does all four things; the learning track was designed for that person.

### 2.3 The channel: who makes the ecosystem grow

| Person | How they use it | When they are well served |
|---|---|---|
| **Equipment manufacturer (OEM)** | puts Joker Micro inside the product (C library, crate or firmware) and publishes the profile and class of the equipment in the Hub; the product speaks Joker Link with any Joker gateway | technically 1.0 (C ABI, Joker Link, `implements`); **blocked by the license** (document 18, §6) |
| **Classic automation engineer (PLC)** | wants IEC 61131, not Rust | **1.2** (Structured Text); until then, the worst served audience |
| **Driver and class author** | writes tier W drivers and class packages for the Hub | 1.2 (stable tier W SDK, Hub with conformance verification) |

### 2.4 The general audience

| Person | How they use it | When they are well served |
|---|---|---|
| **Maker and curious programmer** | has a Raspberry Pi at home and wants to automate things in Rust or Python, with cheap boards (ESP32, RP2040/RP2350) as extensions | partially in 1.0 (Rust, track, simulator); well served with the proposals of §5 |
| **Learner** (student, technical training) | does the track with the bench of document 23 | 1.0 (track, bench without purchases) |
| **Data engineer** | reads the historian for analysis; later trains models | 1.0 (Python client, `jk history`, export); ML in 1.1 |

### 2.5 Who is not the audience

Certified safety functions (SIL), motion control below 1 ms, control room SCADA and HMI, and **consumer home automation with hundreds of brand integrations** (that is Home Assistant's ground; Joker does not compete on integrations, and connects to it, §5 P8). Document 12 has the reasons.

## 3. How each person uses Joker: the first hour and the first week

| Person | First hour | First week |
|---|---|---|
| Integrator (new site) | `jk new site --template bess`, `jk check`, `jk dev --env sim` | profiles from the Hub, site overlay, `jk lock --node`, `jk apply --confirm`, Deck with the canary |
| Control programmer | `jk app new`, scenario, `jk test` with a virtual clock | the app with classes, replay of a real incident (`jk replay --diff`) |
| Hardware integrator | `jk profile import csv mapa.csv`, `jk profile check` | `jk profile test --uri …` on the equipment, `implements`, publication in the Hub with a conformance report |
| Field technician | `jk status`, `jk read 'pcs.**' --watch` | `jk write … --ttl --reason`, `jk alarm ack`, `jk doctor`; today **only through the CLI** (§4) |
| Fleet | `jk ca init`, `jk node enroll … --deck` | `fleet.toml`, waves, `halt_if`, secret rotation |
| OEM | `jk build --target mcu:<board> --lib`, `joker_node.h` | the product exporting its class over Joker Link; profile and class in the Hub |
| Maker | install `jokerd` on the Pi, `jk init`, blink the `ACT` LED (track 1–2) | the whole board as a device, an ESP32 as an extension, a Python script (§5 P1–P3) |

## 4. What this reading reveals

1. **The field technician has no graphical interface in v1.** The Studio arrives in 1.3. A field technician rarely lives in a CLI, and the main buyer feels this at the first commissioning. → P4.
2. **The day-1 catalog decides adoption.** Without ready profiles for the most common equipment, each customer's first project starts by writing profiles. → P5.
3. **Vehicle chargers speak OCPP**, and OCPP is not in any plan (documents 11 and 21). For an energy integrator it is a first-line protocol. → P6.
4. **The OEM depends on the license**, and that channel does not start without it.
5. **The maker wants Python in 1.0**, and the Python app SDK is in 1.2 (document 21, §4). The Python client exists in 1.0, but through the API one can only write as a `manual` writer, with a TTL. → P3.
6. **The maker wants "the whole board"** without declaring it line by line, and wants to use an Arduino or an ESP32 as an extension without writing firmware. → P1 and P2.

## 5. Proposals

| # | Proposal | Serves | Proposed version |
|---|---|---|---|
| P1 | The whole board as a device | maker, integrator | 1.0 |
| P2 | Expander firmware for MCUs | maker, integrator (U1 coprocessor), OEM | 1.0 (Phase 2) |
| P3 | External apps (any language, through the API, as declared writers) | maker, data engineer | 1.0 or 1.1 |
| P4 | Read-only web dashboard served by `jokerd` | field technician, maker | 1.0 |
| P5 | Day-1 catalog (prioritized list of profiles and classes) | integrator | 1.0 |
| P6 | OCPP (1.6J and 2.0.1) | energy integrator | 1.2 or 1.3 |
| P7 | `std/` classes for the general audience | maker, learner | 1.x |
| P8 | Bridge to Home Assistant (MQTT *discovery*) | maker | 1.x |

### P1 — The whole board as a device

Today the board profile describes the fixed hardware (`[io.<name>]` → `board.<name>`, ref/03 §2.3.2), and the lines of the GPIO header are declared one by one in a `gpio` device. The proposal: **a device that is the whole board**.

```toml
[target]
board = "rpi3b-plus@1"

[devices.placa]
driver = "board"                       # proposal: exposes what the board profile describes
lines  = { gpio17 = "output", gpio27 = { direction = "input", bias = "pull-up" } }
```

- The `board` driver exposes the fixed IO (LEDs, temperature, board buttons) and the header lines that the manifest asks for, with the names printed on the board (`placa.gpio17`, `placa.act_led`, `placa.cpu_temp`), using the drivers that already exist (`gpio`, `sysfs`) underneath.
- The board profile gains `implements` for the fixed IO: the `ACT` LED fulfills `std/led@1`, and the app writes `io.placa.act_led.on()`.
- `jk init --board rpi3b-plus` creates this device, and `jk board show` says which lines are free.
- It is not an `rpi3@1` class: a board is a **device** (ADR 0027, "a role, not a chip"); the roles (LED, button, sensor) are what the classes are.

### P2 — Expander firmware for MCUs

It is the Firmata idea, with Joker's guarantees. A **generic Joker Micro firmware** per board, which exports over Joker Link **all the pins** of the board profile, without apps or local logic:

- it is flashed once (`jk flash --expander --board esp32-devkit`);
- the Linux node declares in **its** manifest what each pin is (`esp.gpio2 = "output"`, `esp.gpio0 = { direction = "input", bias = "pull-up" }`, `esp.t0 = "touch"`); `jk apply` compiles and sends the generation to the MCU, as in part 9 of the track;
- the `safe` and the lease of the outputs live on the MCU: if the Pi dies, the pins go to `safe`. That is what Firmata does not do.

It serves the maker ("I use an ESP32 as an extension of the Pi"), the integrator (it is the U1 coprocessor of document 19, without per-project firmware) and the OEM (the development board before the product). **Boards:** those of the Joker Micro targets (ESP32, RP2040/RP2350, STM32) and the Arduino boards based on them (Nano ESP32, Nano RP2040). Classic AVR boards (Uno with 2 KB of RAM) are **not** enough for the core.

### P3 — External apps

An app that runs **outside** `jokerd`, in any language, and talks through the API with the client (`joker_client`, ref/05 part B), but **declared in the manifest as a writer**, with a level and a lease:

```toml
[apps.rega]
kind   = "api"                         # proposal (not to be confused with the Joker Micro `kind = "extern"`, ref/08)
writer = { level = "control:12", lease = "5s" }
writes = ["jardim.valvula"]
reads  = ["jardim.**", "meteo.**"]
```

- It writes through the arbiter and the envelope like any writer; if the process dies, the lease expires and the output goes to `safe`.
- It **does not have** what Wasm apps have: sandbox, determinism and exact replay. `jk check` and `jk status` say so, and in `prod` a justification in the lock is required.
- It gives Python (and any other language) in 1.0 without waiting for the tier 2 SDK, and remains useful after it: an integration script does not need to be Wasm.

### P4 — Read-only web dashboard

`jokerd` serves at `/ui` a static page (without third-party JavaScript) with signals and quality, alarms, the state of devices and links, generations and the health of instances, **read-only**, through the same API and the same roles (`viewer`). Writes remain in `jk` until the 1.3 Studio. It covers most of what the field technician needs at commissioning, with little code.

### P5 — Day-1 catalog

A prioritized list, to publish in the Hub with a conformance report before 1.0: the SunSpec profile (Fronius, SMA and other inverters that comply with it), meters (Janitza UMG, Eastron SDM630/SDM120, Carlo Gavazzi EM), two or three common BESS, heat pumps with SG-Ready and Modbus, generic Modbus I/O modules, and the boards `rpi4`, `rpi5`, `rpi3b-plus`, `revpi-connect-5`, `esp32-devkit`, `rp2350-pico2`. The final list is a commercial decision; the criterion is "what appears in 80% of the sites of the first customers".

### P6 — OCPP

Chargers communicate with a central system through OCPP (WebSocket + JSON). Joker can be the **local central system** of a site (the charger connects to the node), which gives local load management without a cloud. It is a tier N driver with a WebSocket server, and the `std/evse@1` class (already in the initial `std/`, document 14 §10). Proposal: 1.2, with the "industry and ecosystem" theme.

### P7 — `std/` classes for the general audience

`std/relay@1`, `std/pwm_out@1`, `std/servo@1`, `std/rgb_led@1`, `std/humidity_sensor@1`, `std/presence@1`. They are small, they describe roles and they also serve industry (a relay is a relay).

### P8 — Bridge to Home Assistant

A bridge (M11) that publishes chosen instances and signals over MQTT with Home Assistant *discovery*, and accepts commands as a declared writer. Joker does not compete on consumer integrations; it enters the home as the place where the serious logic runs.

## 6. Risks of serving both audiences

- **Dispersion:** each proposal of §5 is small, but together they are a front. The rule of ADR 0022 still applies: one theme per version, at most three headline features.
- **Support:** a general audience brings many questions. The track, the simulator and error messages with a code and help are the first line.
- **Security by default:** a maker exposes ports without thinking. The API still listens only on the Unix socket by default, and TCP always requires mTLS.
- **Image:** "Joker is for hobbyists" drives away the industrial buyer. The communication separates the two, and the product is the same.

## 7. What remains to be decided

1. Which of the proposals P1–P8 go in, and in which version (and those that go into 1.0 need a reference and an ADR).
2. The license (it blocks the OEM and conditions the community).
3. The final list of the day-1 catalog.
