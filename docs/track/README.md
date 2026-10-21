# Learning track: from the bench to the fleet

A track in four modules and eleven parts. It starts in an empty directory on the laptop and ends with two Linux nodes managed by a Deck, an ESP32 running Joker Micro, a class of your own and a bad version of an app stopped at the canary. All with the bench hardware (document 23): a Raspberry Pi 3 B+, a Raspberry Pi Zero W, an ESP32 and the laptop.

The track is three things at once:

1. **the course**: whoever finishes it understands the whole project, from the core tick to the rollout of a fleet;
2. **the demonstration of the v1.0 API**: every command, file, key and output is what the references in `../ref/` specify, and the references prevail when they diverge;
3. **the DX acceptance test and the test plan on the real bench** (document 11, exit gate 4; document 23, §5).

> Joker is in the design phase. The outputs shown are the specified ones; when Phase 1 exists, each part is run on the bench and the real outputs replace these.

---

## The rhythm

The track always moves from the simulated to the real, and climbs layer by layer:

```
             signals (A)                     classes (B)      Joker Micro (C)     fleet (D)
 laptop      1 ─ 3a ─ 4a ─ 5a                7a ─ 8a
                  │     │     │               │     │
 hardware    2 ─ 3b ─ 4b ─ 5b ─ 6            7b ─ 8b ──────▶  9 ─ 10  ─────────▶  11
             Pi 3   Pi 3+ESP32   Pi Zero      Pi 3             Pi 3 + ESP32        Deck + Pi 3 + Pi Zero
```

- **Sim → real.** Every new behavior is first written and tested in the simulator (with a virtual clock, in milliseconds), and only then goes to the board. The move to hardware touches only manifests.
- **Signals → classes → MCU → fleet.** First you learn what is underneath (signals, quality, envelope, arbiter); then you climb to classes, which take work away from the app; then control goes down to the microcontroller; and finally you manage everything as a fleet.

---

## The route

### Module A — Signals and the Linux node

| Part | What you build | Hardware | Joker concepts |
|---|---|---|---|
| [01](01-hello-bench.md) | a project from scratch; the app `light` 0.1 blinks the **simulated** `ACT` LED, with tests and a scenario | laptop | project, manifest and overlays from the start, `sim` driver, signal, port and binding, output with `safe`, cycle group, hot-reload, `jk read`/`jk trace`, `joker-test` with a virtual clock, scenarios |
| [02](02-the-pi-as-a-node.md) | the same LED, now the **real** `ACT` of the Pi 3, with the app unchanged | Pi 3 B+ | installing `jokerd`, CA and enrollment, `jk doctor --rt`, board profile, `[resources]`, `sysfs` driver, lock per manifest, `jk apply --confirm`, generations and rollback, manual write and arbiter, three-layer safe state |
| [03](03-temperature-sensor.md) | the CPU temperature as a physical process: the LED blinks faster the hotter the Pi is; alarm, historian, API (app 0.2) | Pi 3 B+ | inputs and quality, `calc` with windows, alarms, historian, HTTP API and the Python client, `stress-ng` as load |
| [04](04-esp32-modbus.md) | the ESP32 as Modbus RTU equipment: the profile written from the register map, the `BOOT` button turns the system on and off (app 0.3) | Pi 3 + ESP32 over USB | buses (`dtr`/`rts`), `jk profile import csv`, `jk profile test`, profile `identity`, edges and `debounce`, `readback`, equipment fallback |
| [05](05-touch-and-faults.md) | the touch sensor as a rotary knob; the system tolerates the loss of the ESP32 (app 0.4); a replay proves the fix (app 0.4.1) | Pi 3 + ESP32 | noisy analog signals, `deadband`, `get_or` and substituted quality, simulated faults, `jk device stop`, real M1 fallback, `jk record`/`jk replay` from the recorded state |
| [06](06-switching-boards.md) | the same project on the Pi Zero W, with the ESP32 over Wi-Fi; the profile of a board that does not exist yet (app 0.4.1, unchanged) | Pi Zero W + ESP32 over Wi-Fi | overlays by refactoring (`joker.rpi.toml`), `modbus-tcp`, `jk board probe`/`check`/`new`, `dev` level, degradation without RT |

### Module B — Classes

| Part | What you build | Hardware | Joker concepts |
|---|---|---|---|
| [07](07-classes.md) | the same app on top of `Led`, `Button`, `TempSensor`, `AnalogIn`; the scenario from part 5 passes unchanged (app 0.5) | sim → Pi 3 (and Zero) | classes and instances (implicit and composite), `implements` in the profile, `jk class show`, `jk explain <instance>`, `jk status --instances`, `jk class verify` on the hardware |
| [08](08-your-own-class.md) | a class of your own, `status_light@1`, and its implementation; the business app shrinks to ~20 lines (app 0.6) | sim → Pi 3 | `jk class new`, `.jkc`, virtual instances, `#[joker::implements]`, conformance, the four people of document 14; optional extra: a composite class, `local/bench_io@1` |

### Module C — Joker Micro

| Part | What you build | Hardware | Joker concepts |
|---|---|---|---|
| [09](09-joker-micro.md) | the ESP32 now runs Joker Micro; the same signals arrive over Joker Link, and the LED's `safe` lives on the MCU (app 0.6, unchanged) | Pi 3 + ESP32 (Joker firmware) | `jk build --target mcu:esp32-devkit`, `jk flash`, `jk node enroll --link`, `joker.lab-link.toml`, envelope on the MCU, killing `jokerd` with the LED safe |
| [10](10-logic-on-the-mcu.md) | a native app on the MCU (`reflex`) and the link over Wi-Fi | ESP32 (+ Pi 3) | native apps on the MCU, `esp32` driver, `[net.wifi]`, encrypted UDP link, standalone node |

### Module D — Fleet

| Part | What you build | Hardware | Joker concepts |
|---|---|---|---|
| [11](11-production-and-fleet.md) | production (signatures, secrets), the Deck on the laptop with two nodes, a rollout in waves that stops at the canary (apps 0.7 and 0.7.1) | laptop (Deck) + Pi 3 + Pi Zero W | `mode = "prod"`, keys and trust, `secrets.toml.enc`, `joker-deck`, `fleet.toml`, cohorts, waves and `halt_if` |

---

## Materials

**Only the bench, and nothing is bought.** No part asks for a breadboard, an LED, a resistor, a jumper wire, an ADC or an RS-485 adapter.

| Item | What it is | Where it comes in |
|---|---|---|
| laptop | macOS, Linux or Windows with WSL2; stable Rust; `jk`; SSH | all |
| **Raspberry Pi 3 Model B+** | 4× Cortex-A53, 1 GB, aarch64, Wi-Fi; the `lab` node | 2–11 |
| **Raspberry Pi Zero W v1** | 1× ARM1176 (ARMv6), 512 MB, Wi-Fi only; the `zero` node | 6, 7, 11 |
| **original ESP32** (development board) | Xtensa LX6, CP2102 or CH340 USB-serial bridge, LED on GPIO2, `BOOT` button on GPIO0, T0 touch sensor on GPIO4, Wi-Fi | 4–11 |
| cables and cards | the boards' power supplies, a micro-USB cable (ESP32 → Pi 3: power and serial port), two microSD cards | — |
| network | a local Wi-Fi network with the laptop and the three boards on it | 2–11 |

The real signals all come from the boards themselves (document 23, §3): the `ACT` LED and the CPU temperature of the two Pis, and the LED, the button, the touch, the RSSI and the uptime of the ESP32. **The Pi's CPU temperature is the "physical process" of the track**: it heats up under load (`stress-ng --cpu 4 --timeout 5m`) and cools down by itself, with a time constant of one minute. That is what gives real dynamics without components.

The ESP32 connects to the Pi 3 with the micro-USB cable. The Pi Zero W has no way to connect it over USB (it only has a micro-USB OTG port, and there is no adapter), so it talks to it over Wi-Fi, in Modbus TCP (part 6).

---

## The directory, at the end of the track

```
joker-bench/
  joker.toml                                  common manifest: outputs and envelopes, cycles, calc, alarms, apps, instances
  joker.sim.toml                              overlay: extends joker.toml → everything simulated (laptop)
  joker.rpi.toml                              overlay: extends joker.toml → the `pi` device (sysfs), common to both Raspberry Pis (part 6)
  joker.lab.toml                              overlay: extends joker.rpi.toml → lab node (Pi 3): board, RT resources, ESP32 over Modbus RTU
  joker.zero.toml                             overlay: extends joker.rpi.toml → zero node (Pi Zero W): board, ESP32 over Modbus TCP
  joker.lab-link.toml                         overlay: extends joker.lab.toml → the ESP32 as Joker Micro over Joker Link (part 9)
  joker.sim.lock  joker.lab.lock  joker.zero.lock  joker.lab-link.lock
                                              one lock per input manifest (joker.rpi.toml only comes in through extends)
  map-esp32.csv                              part 4 (the register map the profile came from)
  boards/rpi-zero-w.board.toml                board profile (part 6)
  profiles/local/bench-esp32@1.profile.toml   equipment profile (part 4; implements in part 7)
  classes/local/status_light.jkc              class (part 8)
  classes/local/status_light.conformance.toml conformance suite of the class (part 8)
  micro/esp/joker.toml  micro/esp/joker.lock  Joker Micro manifest (parts 9–10) and its lock ([wcet] of the chip, part 10)
  micro/esp/apps/reflex/                     native MCU app (part 10)
  sim/pi.sim.toml  sim/esp.sim.toml           simulation models
  sim/scenarios/*.scenario.toml               scenarios: blink, heat_up, button, esp_fault
  apps/light/                                   the business app, 0.1 → 0.7.1
  apps/status_light_ctl/                      part 8
  secrets.toml.enc                            parts 9–11 (recipient node:esp, wifi_bench, mqtt_bench)
  keys/*.pub                                  part 11
  fleet.toml                                  part 11 (what the Deck reads from the repository)
```

The optional extra of part 8 (`classes/local/bench_io.jkc` and its `.conformance.toml`, the instance `esp_io` and the app `apps/bench_diag/`) is done on a separate branch, `extra-bench-io`, and does not stay in this directory.

Every file states its type in its name (ADR 0026): `joker.<variant>.toml` is an overlay, and `jk … --env <variant>` selects it; `.board.toml`, `.profile.toml`, `.sim.toml`, `.scenario.toml` and `.conformance.toml` are the other types.

Up to part 5, `joker.lab.toml` extends `joker.toml` directly and holds `[devices.pi]`. In part 6, when the second Raspberry board appears, `[devices.pi]` moves out to `joker.rpi.toml`, and `joker.lab.toml` and `joker.zero.toml` extend it instead. It is a lesson in overlays: you refactor when the second case appears, and not before.

## The two rules

1. **The app changes only when the behavior or the layer changes.** The behavior changes in parts 3, 4, 5 and 11; the layer changes in parts 7 (classes) and 8 (your own class). Moving from the simulator to hardware (part 2), from one board to another (part 6) or from Modbus to Joker Link (part 9) touches only manifests.
2. **Changing layer or board does not change the behavior.** The scenario `sim/scenarios/esp_fault.scenario.toml`, written in part 5, passes unchanged on versions 0.4 (and the 0.4.1 fix), 0.5 and 0.6 of the app, and the same common manifest runs on the Pi 3 and on the Pi Zero W.

| Version of `light` | Part | What changed |
|---|---|---|
| 0.1 | 01 | blinks the `led` with `half_period` |
| 0.2 | 03 | blinks faster the hotter the CPU is; `Err` without a `good` temperature → `safe` |
| 0.3 | 04 | the ESP32 button turns the system on and off, which starts on; the ESP32 LED shows the state |
| 0.4 | 05 | the touch adjusts the sensitivity; tolerant of the loss of the ESP32 |
| 0.4.1 | 05 | fix seen in the replay: without the ESP32, it does not propose its LED (stays until part 6) |
| 0.5 | 07 | the same, on top of class handles |
| 0.6 | 08 | the same, on top of the instance `indicator` (`status_light@1`) |
| 0.7 | 11 | `alert` mode with the alarm `cpu_hot` |
| 0.7.1 | 11 | a deliberately bad "improvement": overruns on the Pi Zero W, stopped at the canary |

## Conventions

- **Prompts.** `$` is a command on the laptop; `lab$` on the Pi 3 (over SSH, `ssh pi@lab.local`); `zero$` on the Pi Zero W (`ssh pi@zero.local`). Almost everything runs on the laptop: `jk` talks to the nodes through the API.
- **Nodes.** `lab` (Pi 3 B+, `lab.local`) and `zero` (Pi Zero W, `zero.local`). The local CA lives in `~/org-ca` (part 2).
- **Devices**, with the same names in the simulator and on the hardware, so that the paths never change:
  - `pi`: the Raspberry Pi. `pi.act_led` (`bool`, output) and, from part 3 on, `pi.cpu_temp` (`f64`, `°C`). `sim` driver on the laptop, `sysfs` on the boards.
  - `esp`: the ESP32. `esp.led` (output), `esp.led_rb`, `esp.button`, `esp.touch`, `esp.rssi`, `esp.uptime`, `esp.fw`. `sim` driver on the laptop, `modbus-rtu` on the Pi 3 (parts 4–5), `modbus-tcp` on the Zero (part 6), `joker-link` from part 9 on.
- **Cycle group:** `main`, 20 ms.
- **Outputs:** blocks without a language, short, with the codes from `crates/joker-manifest/codes.toml`. In `mode = "dev"`, codes of severity `error-in-prod` appear as `W` warnings with the same number (`W0205`); pure warnings keep the `J` (`J0352`).
- **Versions.** Each part ends with a commit (`git commit`; with jj, `jj describe && jj new`) and states what it leaves behind: the app version, the new files and the generation of each node.
- **Example names:** the user `dl`, the organization `bench` (`jk ca init ~/org-ca --org bench`), `example.com` domains.

## Estimated time

| Part | 01 | 02 | 03 | 04 | 05 | 06 | 07 | 08 | 09 | 10 | 11 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Time | 45 min | 1 h 30 | 1 h 30 | 2 h | 1 h 30 | 1 h 30 | 1 h | 1 h 30 | 2 h | 1 h 30 | 2 h 30 |

About 17 hours in total. Part 2 includes preparing the SD card; part 9 includes installing the Xtensa toolchain (`espup`), which takes a while.

## What is left out, and arrives in 1.x

The track uses only v1.0 features (document 21, §2). Left out, on purpose:

| Subject | Version |
|---|---|
| ML (Burn, ORT, accelerators), streams and pipelines, block signals, scheduled writes, intents (proposal-only writers) | 1.1 |
| Structured Text, tier W driver SDK, BACnet and EtherCAT; apps in Python | 1.2 |
| Joker Studio, gateway mode, remote sessions through the Deck | 1.3 |
| Wasm and ML on microcontrollers, low-power nodes | 1.4 |
| public Hub with certified evidence, fleet audit at scale | 1.5 |
| active/standby redundancy, node mesh | 2.0 |

The bench does not prove everything either (document 23, §8): half-duplex RS-485, CAN, a real ARM MCU and TPM are declared as not validated.

---

## At the end of the track you know…

Each part links to the documents and references that explain it. Whoever finishes the track and reads the right-hand column understands the project.

| Part | You know… | To go deeper |
|---|---|---|
| 01 | how a project is organized (common manifest and overlays), what a signal, a port and a binding are, why every output has `safe`, what happens in a tick, and why a test with a virtual clock is exact | documents 00, 01 §3–§4, 02, 03, 04, 15; ref/01 §4–§5, §7; ref/03 §2.4–§2.5; ref/04 §4, §6; ADR 0003, 0006, 0007 |
| 02 | how a node gains an identity without opening ports to the network, what a board profile promises, how CPUs are reserved for the cycle, what the lock pins, why you apply with confirmation, how the arbiter decides between a human and an app (and how you ask who controls an output), and what each layer of the safe state covers and does not cover | documents 01 §2, §6, §8, §15, §17, 16 §1, 23; ref/02 §2.1–§2.3, §3.2–§3.3; ref/03 §2.3; ref/09 §2.1–§2.3; ADR 0011, 0015, 0025 |
| 03 | how quality travels from a driver to the app and to the expressions, how windows and alarms are written, what the historian keeps, and how an external program reads and writes through the API | documents 02 §10–§11, 16 §3; ref/01 §5.14–§5.17; ref/05 (part B); ref/09; ADR 0013 |
| 04 | how a piece of real equipment is described in a profile from its register map, how that profile is tested before use, how a bus budget is counted, and what the M1 fallback is | documents 05, 23 §4; ref/03 §2.2; ref/06; ref/01 appendix A; ADR 0008 |
| 05 | how an app tolerates faults without lying about them, how faults are provoked in the simulator and on the bench, and how a recorded trace, with the state of the apps, reproduces an incident and proves a fix | documents 01 §20, 04 §7, 16 §3; ref/03 §2.4.8, §2.5; ref/04 §6; ADR 0005 |
| 06 | how the same project changes board with overlays only, how the profile of a board that does not exist is written, and how `jokerd` degrades on a weak board | documents 01 §22, 23 §5.2, §7; ref/01 §7; ref/03 §2.3 |
| 07 | what a class gives an app, how a profile declares that it implements a class, why the fault scenario passes the same, and how a class is verified on the real equipment | documents 14, 17; ref/04 §4.8; ADR 0017, 0018 |
| 08 | how an abstraction with a contract, conformance and an implementation is created, and who owns each layer | documents 14 §6–§7, 17; ref/03 §2.6; ref/04 §4.8.4 |
| 09 | how the core runs on an MCU with the same bytes, what Joker Link guarantees, and why the `safe` of an MCU output survives the death of `jokerd` | documents 19, 16 A12; ref/07; ref/08; ADR 0020 |
| 10 | how logic that runs on the MCU itself is written, and what changes on an IP link | documents 19 §4–§6; ref/08 §2.11; ref/07 §3 |
| 11 | what changes in `prod`, how you sign and distribute secrets without exposing them, and how a fleet is updated in waves with an automatic stop | documents 20; ref/02 §2.4–§2.5; ref/10; ADR 0021, 0022, 0024, 0025 |
