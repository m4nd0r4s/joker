# 23 — The development bench

This document says **with which hardware Joker is developed and tested now**, and how each phase of the roadmap (document 11) is validated with it.

**The rule of this bench: nothing is bought.** Everything here works only with the three boards that exist, the cables they already use (power, micro-USB, SD cards), the laptop and the Wi-Fi network. What would need one more part is marked as **future purchase (optional)** and never blocks a phase: either it is covered some other way (simulation, QEMU), or it is declared as not validated on the bench.

---

## 1. Inventory and roles

| Board | What it is | Role |
|---|---|---|
| **Raspberry Pi 3 Model B+** (2017) | 4× Cortex-A53 at 1.4 GHz, 1 GB, aarch64, Ethernet (over USB 2.0), Wi-Fi, 40 pins | **Main Linux node** (`lab`). `jokerd` with the full wasmtime (Cranelift on aarch64), API, historian, drivers. Where jitter is measured with PREEMPT_RT. Powers the ESP32 over USB. |
| **Raspberry Pi Zero W v1** | 1× ARM1176 (ARMv6) at 1 GHz, 512 MB, 32-bit, Wi-Fi only | **Weak platform and second node** (`zero`). Proves that `jokerd` degrades with warnings and does not break; Modbus TCP client of the ESP32 over Wi-Fi; second node for the Deck (Phase 3). |
| **ESP32** (original, development board) | 2× Xtensa LX6 at 240 MHz, 520 KB of SRAM, single-precision FPU, Wi-Fi, Bluetooth, USB-serial bridge on the board (CP2102 or CH340), LED on GPIO2, `BOOT` button on GPIO0 | **First Joker Micro.** In Phase 1, a "real" Modbus device, over RTU and over TCP at the same time (§4); in Phase 2, coprocessor of the Pi 3 over Joker Link and a node over Wi-Fi. |
| laptop | — | development, `jk`, local CI, QEMU, and the Deck in Phase 3 |

The ESP32's **Bluetooth** is not used: no v1 interface provides for it.

## 2. Topology

```
                          Wi-Fi / local network
      ┌───────────────┬──────────────────────┬─────────────────────────┐
      │               │                      │                         │
 ┌────┴─────┐   ┌─────┴──────────────┐   ┌───┴─────────────┐    ┌──────┴────────────┐
 │ laptop   │   │ Raspberry Pi 3 B+  │   │ Pi Zero W       │    │ ESP32             │
 │ jk, CI,  │──▶│ jokerd (node "lab")│   │ jokerd (node    │    │ Phase 1: Modbus   │
 │ QEMU,    │   │ mosquitto (F2/F3)  │   │ "zero"): Modbus │───▶│ TCP server :502   │
 │ Deck(F3) │   │                    │   │ TCP client      │    │ Phase 2: UDP link │
 └──────────┘   │   USB ─────────────┼───┼─────────────────┼───▶│ UART0 via the USB │
                └────────────────────┘   └─────────────────┘    │ bridge (RTU/link) │
                                                                └───────────────────┘
       The ESP32 micro-USB cable connected to the Pi 3 provides power and a serial port
       (/dev/serial/by-id/usb-Silicon_Labs_CP2102_…): Modbus RTU in Phase 1, Joker Link
       in Phase 2. The Pi Zero W only reaches the ESP32 over Wi-Fi. No jumper wires.
```

**The trick that avoids buying parts:** the USB-serial bridge of the ESP32 board (CP2102 or CH340, depending on the manufacturer) is a real serial port for the Pi 3. It serves for Modbus RTU and for Joker Link without an RS-485 transceiver and without wires. In Phase 2, the ESP32's `UART0` stops being the console and becomes the link; the boot ROM messages at 115,200 baud arrive before the firmware and are garbage that COBS discards up to the first `0x00` (ref/07 §3.2).

**The Pi Zero W cannot use USB.** It has a single micro-USB OTG port and there is no adapter on the bench: it talks to the ESP32 over Wi-Fi, through Modbus TCP in Phase 1 (that is what the bench firmware serves, §4). The ESP32 remains powered by the USB of the Pi 3 (or of the laptop).

## 3. Real signals without components

The learning track (`docs/track/`) asks for an output, a discrete input, a noisy analog input and a piece of remote equipment. Without buying anything, the equivalents are:

| Board | Signal | How | Type |
|---|---|---|---|
| Pi 3, Pi Zero | the board's **`ACT` LED** | `sysfs` driver, `/sys/class/leds/ACT/brightness` (§7) | `bool` output |
| Pi 3, Pi Zero | **CPU temperature** | `sysfs` driver, `/sys/class/thermal/thermal_zone0/temp` (m°C) | `f64` input, `°C`. It is the "physical process" of the track: it heats up under load (`stress-ng --cpu 4 --timeout 5m`) and cools down on its own |
| ESP32 | **board LED** (GPIO2 on most boards; some have none) | GPIO | `bool` output |
| ESP32 | **`BOOT` button** (GPIO0, on all boards) | GPIO with pull-up, active low | `bool` input |
| ESP32 | **touch sensor** (T0 on GPIO4; touching the pin with a finger is enough) | *touch* peripheral | raw `u16`, signal `u64`: it is the bench "potentiometer", a real and noisy analog value, which **drops** when touched |
| ESP32 | **Wi-Fi RSSI** | radio | raw `i16`, signal `i64` in dBm (no unit in the catalog, because it is logarithmic: `unit = "1"`, with the description) |
| ESP32 | **uptime** | timer | raw `u32` in `s`, signal `f64` in `s`, `monotonic` |

The signal types are those that acquisition gives to the raw types of the profile (ref/03 §2.2.7: a raw integer with a `unit` other than `1` becomes `f64`), and they are the same as those the `esp32` driver of Joker Micro gives in Phase 2 (ref/01 appendix A): changing the firmware does not change the types.

That is enough to cover what the track wants to teach: an output with an envelope and `safe`, a discrete input with edges, an analog input with noise and `deadband`, a sensor that changes slowly and can be heated, a piece of remote equipment with quality, loss of communication and equipment fallback.

**What there is not:** real voltages and currents, relays, a genuine industrial sensor. The ESP32's ADC pins are floating and only give noise.

## 4. The ESP32 bench firmware (Phase 1)

In Phase 1, Joker Micro does not exist yet, but `jokerd` needs a real device to test the `modbus-rtu` and `modbus-tcp` drivers, loss of communication and the M1 fallback. That is why the ESP32 starts as **a simple Modbus device**, with a small firmware in Rust (`esp-hal`, `esp-wifi` and a `no_std` Modbus server, for example `rmodbus`), in `tools/bench/esp32-modbus/` of the Joker repository, flashed with `espflash`.

**Two transports, one table.** The firmware serves **at the same time**:

- **Modbus RTU on `UART0`**, through the USB bridge: 115,200 baud, 8N1, unit 1;
- **Modbus TCP over Wi-Fi**, on port 502, unit 1.

Both read and write the **same** register table, and any valid request, wherever it comes from, counts for the fallback timer.

| Register | FC | Type | Signal |
|---|---|---|---|
| coil 0 | 1 (read), 5 (write) | `bool` | board LED |
| discrete input 0 | 2 | `bool` | `BOOT` button (already inverted: 1 = pressed) |
| input register 0 | 4 | `u16` | touch sensor reading |
| input register 1 | 4 | `i16` | RSSI in dBm (0 without Wi-Fi) |
| input registers 2–3 | 4 | `u32` | seconds since boot |
| holding register 0 | 3 | `u16` | bench firmware version |

**Wi-Fi credentials.** They enter the build through environment variables, and never Git:

```sh
cd tools/bench/esp32-modbus
BENCH_WIFI_SSID="bench" BENCH_WIFI_PASSWORD="…" cargo build --release
espflash flash --monitor target/xtensa-esp32-none-elf/release/esp32-modbus
```

Without the two variables, the firmware compiles with RTU only (and the RSSI stays at 0). At boot, before `UART0` starts serving Modbus, the firmware writes a line to the serial console with the version, the SSID and the **IP** obtained by DHCP (`bench-esp32 v1 · wifi bench · ip 192.168.1.60 · modbus-tcp :502 · modbus-rtu uart0 115200 8N1`), and repeats it on each Wi-Fi connection. `espflash --monitor` shows it right after flashing; later, it is read on the Pi 3 with `picocom -b 115200` on the `by-id` port, **with `jokerd` stopped** (it is the same port). The address in `joker.zero.toml` (§6.4) is this IP: it is pinned with a DHCP reservation on the router.

**The equipment's own fallback (M1):** if it receives no Modbus request, over either transport, for 2 s, the firmware turns the LED off. It is the behavior that the profile's `[fallback]` declares, and it is what `jk doctor` checks.

**The profile** belongs to the project, not to the registry: `profiles/local/bench-esp32@1.profile.toml` (it is what the track writes in part 4, from the map above in CSV, with `jk profile import csv`; ref/03 §2.2.19). After the `# REVIEW` markers are resolved:

```toml
# profiles/local/bench-esp32@1.profile.toml
schema = 1

[profile]
name        = "local/bench-esp32"
version     = "1.0.0"
driver      = "modbus"
description = "ESP32 bench firmware (document 23, §4): LED, button, touch, RSSI, uptime"

[defaults]
poll = "100ms"

[signals]
"led"     = { at = 0, fc = 5, type = "bool", direction = "output", readback = "led_rb", safe_hint = false }
"led_rb"  = { at = 0, fc = 1, type = "bool" }
"button"  = { at = 0, fc = 2, type = "bool", label = "BOOT" }
"touch"   = { at = 0, fc = 4, type = "u16", deadband = 5, description = "raw value of the T0 touch sensor (GPIO4); drops when touched" }
"rssi"    = { at = 1, fc = 4, type = "i16", description = "Wi-Fi RSSI in dBm; 0 without a connection" }
"uptime"  = { at = 2, fc = 4, type = "u32", unit = "s", monotonic = true }
"fw"      = { at = 0, fc = 3, type = "u16", direction = "param" }

[fallback]
timeout    = "2s"
value      = false
applies_to = ["led"]
```

The same profile serves both transports: the `modbus` family accepts `modbus-rtu` and `modbus-tcp` (ref/03 §2.2.2). It is tested without a manifest with `jk profile test` and the URIs of ref/03 §2.2.20:

```sh
jk profile test local/bench-esp32@1 --node lab \
   --uri "modbus-rtu:/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0?baud=115200&unit=1"
jk profile test local/bench-esp32@1 --node zero --uri "modbus-tcp://192.168.1.60:502?unit=1"
```

**One device, one client at a time.** The firmware accepts requests over both transports, but **two nodes should not talk to the ESP32 at the same time**. Both would write the LED (coil 0) without a common arbiter, because the arbiter belongs to each node (document 16, A1), and the requests of one would keep the fallback timer alive when the other dies, which hides exactly what part 5 of the track wants to show. Before `zero` talks to the ESP32 (§6.4), `lab` stops doing so (`lab$ sudo systemctl stop jokerd`, or a generation without the `esp` device); and when the ESP32 becomes a Joker Micro (Phase 2), the `esp` of `zero` becomes `bad(comm_failure)`, because the Joker firmware no longer serves Modbus. Joker cannot prevent it: a Modbus device does not know who its clients are, and each node only sees its own requests. It is a rule of the bench, and it is also the rule of a real site.

In Phase 2, the **same ESP32** receives the Joker Micro firmware, and the same signals start arriving over Joker Link, with the same names and the same types (§3). The Pi 3 manifest only changes the device (§6.5), and the apps do not change. It is the clearest before-and-after of ADR 0020 one can have.

## 5. What is validated in each phase

### 5.1 Phase 0 — Deterministic and portable core

| Exit gate (document 11) | On the bench | Notes |
|---|---|---|
| `no_std` core, conformance on Linux | laptop and Pi 3 (aarch64) | |
| **the same bytes on an MCU** | **ESP32** (Xtensa, `xtensa-esp32-none-elf`), over serial | Rust for Xtensa uses the Espressif toolchain (`espup`): it is not in the official LLVM, and the CI must install it. That is why the Xtensa CI job **does not block** (document 13, §5), and the MSRV of the core stays ≤ the Rust version of the most recent Xtensa toolchain (13 §5.1) |
| the same conformance on ARM Cortex-M and RISC-V | **QEMU on the laptop**: `mps2-an505` (Cortex-M33, the RP2350 architecture) and riscv32 `virt` (that of the ESP32-C6) | covers the architectures of the reference targets without the boards; it proves the bytes, not the timing. These are the jobs that **block** the CI (13 §5) |
| WCET on the chip | ESP32, with the cycle counter (`CCOUNT`), reported over serial | no probe: the original ESP32 only has JTAG with an external adapter (future purchase) |

### 5.2 Phase 1 — MVP on Linux

| Exit gate | On the bench | Notes |
|---|---|---|
| `jokerd` on a real node, API, remote `jk dev`, replay | **Pi 3** | |
| `modbus-rtu`, `modbus-tcp`, `sysfs` drivers | ESP32 (§4): RTU through the Pi 3's USB bridge, TCP over Wi-Fi from the Pi Zero W; `sysfs` on both Pis | **half-duplex** RS-485 (turn-taking, `turnaround`) is not tested: the USB bridge is full-duplex. Declared |
| equipment fallback (M1) and loss of communication | unplug the ESP32 cable: the LED turns off within 2 s through the firmware itself, signals `bad(comm_failure)` on the Pi 3 | |
| jitter measured with PREEMPT_RT | Pi 3, kernel with `PREEMPT_RT` (in the mainline kernel since 6.12), `isolcpus=3` | **recorded, without a target**: the targets of document 01 §21 (≤ 100 µs p99.9) are for CM4/CM5. The Pi 3's Ethernet goes through USB and generates interrupts: measure with and without network. External measurement on a pin requires 2 wires and the ESP32 as an analyzer (future purchase: wires) |
| degradation without RT | Pi Zero: 1 core, `standard` kernel | `J0352` is expected on all groups, and `jokerd` must work with it |
| track, parts 01–07 (the DX exit gate of document 11) | Pi 3, Pi Zero W and ESP32, with the signals of §3 | §6.6; `jk class verify` on the hardware (part 7, §9) only in Phase 2 |

**Pi Zero W and wasmtime.** Document 01 §22 provides for Pulley on armv7. The Zero is ARMv6, one step below. **To be confirmed in Phase 1:** if wasmtime with Pulley compiles for `arm-unknown-linux-gnueabihf`, the Zero runs Wasm apps (slowly); if not, it runs native apps only (document 04, §5) and remains useful as a second node and as the ESP32's Modbus TCP client. In both cases it stays at the `dev` level (§7).

**Nothing is ever compiled on the node** (document 07, §1, principle 8). `jokerd`, the apps and the AOT are compiled on the laptop (or in the CI), by cross build: `aarch64-unknown-linux-gnu` for the Pi 3 and `arm-unknown-linux-gnueabihf` for the Pi Zero W. `jk runtime build` and `jk dev --node` compile on the laptop and send the result; the Raspberry Pis have neither `cargo` nor a toolchain. With 1 GiB (Pi 3) and 512 MiB (Pi Zero W) of RAM and an SD card (§7), compiling `jokerd` on the board would take hours and wear out the card.

### 5.3 Phase 2 — Joker Micro and interoperability

| Exit gate | On the bench | Notes |
|---|---|---|
| Joker Link over serial | ESP32 ↔ Pi 3 through the USB bridge (up to ~921,600 baud, depending on the bridge) | case U1 of document 19 at another scale |
| Joker Link over IP | ESP32 over Wi-Fi, UDP to the Pi 3 | with mandatory encryption (`J0179`) |
| kill `jokerd` → `safe` through the MCU | `systemctl kill jokerd` on the Pi 3; the ESP32 LED follows `safe` within the lease | the most important exit gate of ADR 0020, which can be validated already |
| OTA of the generation blob and of the firmware | over the link, with resume after unplugging the cable halfway | A/B with the Espressif bootloader (§7) |
| standalone node (U2) | ESP32 with MQTT over Wi-Fi to a broker on the Pi 3 | |
| RP2350, ESP32-C6, STM32 targets | **there are no boards**: conformance in QEMU (§5.1); the complete firmware only on the original ESP32 | the "three reference targets" gate becomes "one real target and two architectures in QEMU" until there is hardware |
| CAN, RS-485 | **not tested** | declared |

### 5.4 Phase 3 — Fleet, trust and v1.0

| Exit gate | On the bench | Notes |
|---|---|---|
| Deck MVP with real nodes | Deck on the laptop; Pi 3 and Pi Zero nodes; ESP32 as an MCU behind the Pi 3 **or** as a standalone MCU | two Linux nodes are enough for rollouts, cohorts and rollback |
| node offline for a long time | turn off the Pi Zero's Wi-Fi for days on end | |
| scale (hundreds of nodes) | **simulated agents** on the laptop (the agent protocol is the same, ref/10) | scale does not need hardware |
| TPM | **there is none** (the Pi 3 does not have one): identity in a file, and `jk doctor` says so (`J0933`, warning) | declared |

## 6. Bench manifests

They are the files of the track project, `joker-bench/` (`[project] name = "bench"`), in the state they are in after part 6: a common manifest and one overlay per node (ref/01 §7). The following parts add instances, classes and apps to `joker.toml`, and `joker.lab-link.toml` (part 9); the node files do not change. Each input manifest has its own lock (`joker.sim.lock`, `joker.lab.lock`, `joker.zero.lock`, `joker.lab-link.lock`; document 07, §1); `joker.rpi.toml` only comes in through `extends` and has no lock. The overlays are called `joker.<variant>.toml` and are chosen with `--env <variant>` (`jk apply --env zero` is `joker.zero.toml`; ADR 0026).

```
joker.toml ◀── joker.sim.toml                                          (laptop, everything simulated)
     ▲
     └──── joker.rpi.toml ◀── joker.lab.toml ◀── joker.lab-link.toml    (Pi 3; ESP32 over RTU, then over the link)
                  ▲
                  └──── joker.zero.toml                                 (Pi Zero W; ESP32 over Modbus TCP)
```

### 6.1 `joker.toml`: what is the same everywhere

```toml
# joker-bench/joker.toml — common: outputs and envelopes, cycles, calc, alarms, apps
[project]
name  = "bench"
joker = "1.0"
mode  = "dev"

[outputs."pi.act_led"]
safe = false

[outputs."esp.led"]
safe = false

[cycles.main]
period = "20ms"

[calc]
temp_avg  = "avg(pi.cpu_temp, 10s)"
touch_level = "clamp((60 - esp.touch) / 45, 0, 1)"

[alarms.cpu_hot]
when     = "calc.temp_avg > 70 °C"
delay_on = "10s"
severity = "medium"
message  = "CPU at {calc.temp_avg}"

[alarms.esp_offline]
when     = "!good(esp.uptime)"
delay_on = "5s"
severity = "high"
message  = "ESP32 not communicating"

[apps.light]
path   = "apps/light"
bind   = { led = "pi.act_led", temp = "calc.temp_avg", button = "esp.button", led_status = "esp.led", adjust = "calc.touch_level" }
params = { half_period = "500 ms", t_cold = "45 °C", t_hot = "70 °C" }
cycles = { blink = "main" }
```

It has no `[target]` nor `[devices]`: the `pi.*` and `esp.*` signals come from the overlays, and that is why the same `joker.toml` runs in the simulator (`joker.sim.toml`, with `driver = "sim"` on both devices) and on the two boards.

### 6.2 `joker.rpi.toml`: what the two Raspberry Pis have in common

```toml
# joker-bench/joker.rpi.toml — the "pi" device of any Raspberry Pi (sysfs)
extends = ["joker.toml"]

[devices.pi]
driver = "sysfs"
poll   = "1s"

[devices.pi.signals]
act_led  = { led = "ACT", direction = "output" }
cpu_temp = { thermal = "thermal_zone0" }
```

Up to part 5, this `[devices.pi]` lives in `joker.lab.toml`; part 6 moves it here when the second board appears (a refactoring of overlays: the effective manifest of `lab` does not change, and `jk diff` proves it).

### 6.3 `joker.lab.toml`: the Pi 3, with the ESP32 over Modbus RTU

```toml
# joker-bench/joker.lab.toml — node "lab" (Raspberry Pi 3 B+)
extends = ["joker.rpi.toml"]

[target]
board = "rpi3b-plus@1"

[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
bg = { cpus = [0, 1] }

[buses.esp32]
kind = "serial"
port = "/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0"
baud = 115200

[devices.esp]
driver  = "modbus-rtu"
bus     = "esp32"
unit_id = 1
profile = "local/bench-esp32@1"
```

The name of the serial port depends on the bridge of the ESP32 board (CP2102 or CH340): always use the stable path from `/dev/serial/by-id/`, which `jk doctor` and `GET /v1/node/hardware` show (ref/09 §5.2).

### 6.4 `joker.zero.toml`: the Pi Zero W, with the ESP32 over Modbus TCP

```toml
# joker-bench/joker.zero.toml — node "zero" (Raspberry Pi Zero W)
extends = ["joker.rpi.toml"]

[target]
board = "rpi-zero-w"

[devices.esp]
driver  = "modbus-tcp"
address = "192.168.1.60:502"
unit_id = 1
profile = "local/bench-esp32@1"
```

- **The board belongs to the project:** `boards/rpi-zero-w.board.toml`, written in part 6 with `jk board probe --node zero` and validated with `jk board check --node zero` (ref/03 §2.3.7; the result is in §7).
- **No `[resources]`:** with a single core there is no isolation, and the `main` group has no `rt` class. With `kernel = "standard"`, `jk check` gives `J0352` (warning) on the `main` group, and `jokerd` must work with it (§5.2).
- **The same profile, another transport:** the `modbus` family accepts `modbus-tcp` (ref/03 §2.2.2), and the `address` is the IP that the bench firmware shows on the console (§4). The paths (`esp.led`, `esp.touch`) and the app do not change.
- Only one node talks to the ESP32 at a time (§4).

### 6.5 `joker.lab-link.toml`: the same ESP32 over Joker Link (Phase 2)

```toml
# joker-bench/joker.lab-link.toml — the ESP32 becomes a Joker Micro
extends = ["joker.lab.toml"]

[buses.esp32]
baud = 921600

[devices.esp]
driver  = "joker-link"
unit_id = { unset = true }
profile = { unset = true }
auth    = "required"
report  = "20ms"
refresh = "1s"

[outputs]
"esp.led" = { unset = true }      # the envelope of this output now lives on the MCU
```

The device keeps the name `esp`, and the Joker Micro firmware exports the signals with the same names and types (`led`, `button`, `touch`, `rssi`, `uptime`; the LED and the button through the MCU's `gpio` driver, `line = "gpio2"` and `line = "gpio0"`; touch, RSSI and uptime through the `esp32` driver, ref/01 appendix A): the paths (`esp.led`) do not change, and neither do the apps. The profile is generated from the `HELLO` catalog (ref/03 §2.2.16). The envelope and the `safe` of the LED now live **on the MCU**: the Joker Micro manifest (`micro/esp/joker.toml`, compiled with `jk build --target mcu:esp32-devkit`, ref/08) declares them, and the Linux node does not arbitrate that output (ref/07 §8.1). The link key comes from the enrollment of the MCU (`jk node enroll <mcu> --link esp`). For the link over Wi-Fi (part 10), the MCU manifest declares `[net.wifi]` with the password as `secret:` (ref/01 §5.22), and the enrollment takes `--ca` so that the MCU can receive secrets (ref/08 §2.17).

### 6.6 The learning track on this bench

The track (`docs/track/`, parts 01–11) is the test plan of this bench: each part runs in a known state and validates a part of what Phases 1–3 promise. The detail, with commands and outputs, is in each part; here is what each one proves about the hardware.

| Part | Where | What it validates of the bench |
|---|---|---|
| 01 | laptop | no hardware: `jk dev`, the dashboard and the scenarios against the simulator (Phase 0/1) |
| 02 | Pi 3 | installation and enrollment of a real node; the `rpi3b-plus` board; the `sysfs` driver on the `ACT` LED. Without hardware fallback, the LED stays as it was when `jokerd` stops: that is `J0205` in `prod`, the lesson of the part |
| 03 | sim → Pi 3 | the CPU temperature through `sysfs`, heated with `stress-ng`; `calc`, alarms, historian and the HTTP API on a real node |
| 04 | sim → Pi 3 + ESP32 | the `modbus-rtu` driver through the USB bridge; the `local/bench-esp32@1` profile from the CSV (`jk profile import csv`, `jk profile test`); button edges and the LED's `readback` |
| 05 | sim → Pi 3 + ESP32 | real loss of communication (unplug the cable): `bad(comm_failure)` on the Pi 3 and the ESP32 LED turned off within 2 s by the firmware itself (M1 fallback); quality, `get_or` and replay |
| 06 | Pi Zero W + ESP32 over Wi-Fi | the `modbus-tcp` driver; an ARMv6 board of `dev` level written with `jk board probe` and `jk board check`; degradation without RT (`J0352`) |
| 07 | sim → Pi 3 (and Zero) | the `std/*` classes on top of real signals, with the same scenario as part 5 |
| 08 | sim → Pi 3 | a virtual instance (`status_light@1`) implemented by an app, on the hardware |
| 09 | Pi 3 + ESP32 (Joker firmware) | Joker Link over serial through the USB bridge; `jk flash` through `espflash`; the LED's `safe` on the MCU when `jokerd` is killed (the most important exit gate of ADR 0020) |
| 10 | ESP32 (+ Pi 3) | a native app on the MCU; the `esp32` driver; `[net.wifi]`; the link over UDP with mandatory encryption (`J0179`) |
| 11 | laptop (Deck) + Pi 3 + Pi Zero W | signatures and secrets in `prod`; the Deck MVP with two real nodes, cohorts and a rollout that stops at the canary (overruns on the Pi Zero W) |

## 7. Board profiles

The three files follow ref/03 §2.3. `rpi3b-plus` and `esp32-devkit` are in the registry; `rpi-zero-w` is not, because it is of `dev` level: it is what the track writes in part 6, with `jk board probe` and the `# REVIEW` markers resolved (ref/03 §2.3.7), and it stays in the project's `boards/`. The Pi 3 stays at level `2` (aarch64, without CI with hardware); the Pi Zero W and the original ESP32 stay at the `dev` level: they serve for development and are never production targets (01 §22). The two Raspberry Pis declare `storage = "sd"`: they boot from an SD card, and `jk check` and `jk doctor` warn (`J0381`) when the projected write load exceeds what a card can withstand (01 §12 and §19).

```toml
# boards/rpi3b-plus.board.toml
schema = 1

[board]
name         = "rpi3b-plus"
version      = "1.0.0"
arch         = "aarch64"
tier         = "2"
vendor       = "Raspberry Pi"
model        = "Raspberry Pi 3 Model B+"
cpus         = 4
memory       = "1 GiB"
clock        = "1.4 GHz"
storage      = "sd"
kernel       = "preempt_rt"
recommended  = { rt = [3], io = [2], bg = [0, 1] }
boot_cmdline = ["isolcpus=3", "nohz_full=3", "rcu_nocbs=3"]

[watchdog]
device  = "/dev/watchdog0"
timeout = "15s"

[gpio]
chip = "pinctrl-bcm2835"

[io.act_led]
driver    = "sysfs"
led       = "ACT"
direction = "output"

[io.cpu_temp]
driver  = "sysfs"
thermal = "thermal_zone0"

[i2c]
"i2c1" = "/dev/i2c-1"

[spi]
"spi0" = "/dev/spidev0.0"

[serial]
"uart0" = "/dev/serial0"

[net]
"eth"  = "eth0"
"wifi" = "wlan0"
```

```toml
# boards/rpi-zero-w.board.toml
schema = 1

[board]
name        = "rpi-zero-w"
version     = "1.0.0"
arch        = "armv6"
tier        = "dev"
vendor      = "Raspberry Pi"
model       = "Raspberry Pi Zero W (v1.1)"
cpus        = 1
memory      = "512 MiB"
clock       = "1 GHz"
storage     = "sd"
kernel      = "standard"
recommended = { rt = [0], io = [0], bg = [0] }

[watchdog]
device  = "/dev/watchdog0"
timeout = "15s"

[gpio]
chip = "pinctrl-bcm2835"

[io.act_led]
driver    = "sysfs"
led       = "ACT"
direction = "output"

[io.cpu_temp]
driver  = "sysfs"
thermal = "thermal_zone0"

[net]
"wifi" = "wlan0"
```

```toml
# boards/esp32-devkit.board.toml
schema = 1

[board]
name       = "esp32-devkit"
version    = "1.0.0"
os         = "none"
arch       = "xtensa-esp32-none-elf"
mcu_class  = "m2"
chip       = "esp32"
tier       = "dev"
vendor     = "Espressif"
model      = "ESP32 (Xtensa LX6), generic development board"
memory     = "520 KiB"
flash      = "4 MiB"
fpu        = "sp"
clock      = "240 MHz"
reset_safe = false
stack      = "16 KiB"

[capacities]
signals     = 256
outputs     = 64
groups      = 4
instances   = 32
apps        = 4
alarms      = 32
programs    = 64
trace_bytes = "16 KiB"
kv_bytes    = "16 KiB"

[irq]
priorities = 3

[pins.gpio0]
functions = ["gpio"]
note      = "BOOT button (boot pin: do not use as an output)"
[pins.gpio2]
functions = ["gpio", "pwm"]
note      = "board LED, on most boards (boot pin, pulled low)"
[pins.gpio4]
functions     = ["gpio", "adc", "touch"]
touch_channel = 0
note          = "T0 touch sensor"
[pins.gpio1]
functions = ["gpio", "uart"]
[pins.gpio3]
functions = ["gpio", "uart"]

[uart.uart0]
tx       = "gpio1"
rx       = "gpio3"
max_baud = 921600

[adc]
bits = 12
vref = "3.3 V"

[watchdog]
kind    = "esp32-rwdt"
timeout = "1s"

[link]
transports = ["uart", "udp", "tcp"]
default    = { transport = "uart", port = "uart0", baud = 115200 }

[boot]
bootloader = "esp-idf"
slots      = 2

[areas]
bootloader = { offset = 0x001000, size = 0x007000 }
boot_state = { offset = 0x009000, size = 0x002000 }
firmware   = { offset = 0x010000, size = 0x180000 }
fw_staging = { offset = 0x190000, size = 0x180000 }
gen_a      = { offset = 0x310000, size = 0x010000 }
gen_b      = { offset = 0x320000, size = 0x010000 }
persist    = { offset = 0x330000, size = 0x020000 }
identity   = { offset = 0x350000, size = 0x002000 }

[probe]
tool     = "espflash"
protocol = "serial"

[net]
wifi     = true
ethernet = false
```

Notes:

- **`reset_safe = false`** on the ESP32: on reset, the GPIOs stay as inputs until the firmware configures them; only the boot pins have an internal pull (GPIO2 has a pull-down, and that is why the LED stays off). On the bench there is no load connected, and `mode = "dev"` accepts it; in `prod` it is `J0786`, which part 11 of the track accepts with `accept_unsafe_reset` and the justification (ref/01 §5.22).
- The **partitions** follow the table of the Espressif bootloader (the partition table at `0x8000` stays outside the Joker areas); the A/B swap of the firmware uses its OTA partitions.
- **Flashing over serial** (`espflash`, through the USB bridge), without a probe: `jk flash` with `tool = "espflash"`.
- The original ESP32 is not in the v1 target table (19 §3.1), but it is of the same family as the ESP32-S3 (Xtensa, single FPU) and has M2-class memory: the capacities are those of the S3, reduced.

## 8. What this bench does not prove

| Gap | Consequence | How it is closed (when available) |
|---|---|---|
| no real ARM Cortex-M MCU | the M1 reference target (RP2350) only in QEMU; `probe-rs` and `jk bench --target mcu` through a probe are not tested | future purchase (optional): Raspberry Pi Pico 2 and Debug Probe |
| no ESP32-C6 (RISC-V, without FPU) | the `J0780` case (software floating point) is not measured | future purchase: an ESP32-C6 |
| half-duplex RS-485 and CAN | Joker Link turn-taking and Modbus RTU on a shared bus, and the link over CAN, remain without a real test | future purchase: USB-RS485 and CAN adapters |
| hardware of the RT target class (CM4/CM5) | the figures of document 01 §21 are not confirmed; those of the Pi 3 are recorded as a pessimistic reference | CI hardware (pending, document 18) |
| external measurement of jitter | only internal measurement (trace, metrics, `cyclictest`) | 2 jumper wires, with the ESP32 as a logic analyzer (edge capture with the timer) |
| TPM | identity only in a file | a board with a TPM or a secure element |
| real industrial equipment | no PLCs, inverters, meters | pilots |

None of these gaps blocks a phase: each affected exit gate is marked **"validated on the bench"** or **"validated in QEMU/simulation"**, and the rest waits for hardware.

## 9. What this changed in other documents

- **ref/01, appendix A, and document 05 §10:** new **`sysfs`** driver (tier N, 1.0), for the LED and the temperature of any Linux board (`/sys/class/leds`, `/sys/class/thermal`).
- **ref/03 §2.3:** `arch` accepts `armv6` (Linux, `dev` level only) and the `xtensa-esp32-none-elf` triple; `[boot] bootloader` accepts `esp-idf`; `[probe]` gains `tool` (`probe-rs` \| `espflash`) and the `serial` protocol.
- **ref/08 §2.15.1:** a row for the original ESP32 as a development target.
- **01 §22:** the Raspberry Pi 3 at level 2; the Pi Zero W (ARMv6) and the original ESP32 at the `dev` level.
- **07:** `jk flash` flashes through a probe (`probe-rs`) or over serial (`espflash`).
- **11:** the exit gates of Phases 0–2 say where they are validated while there is only this bench.
- **ref/03 §2.2.19–§2.2.20 and §2.3.7, document 07 §7:** `jk profile import csv`, the grammar of the equipment URIs of `jk profile test`, and `jk board probe`, `jk board check`, `jk board new` and `jk board publish`, to write the profile of the bench firmware and that of the `rpi-zero-w` board; `[pins] touch_channel` on MCU boards.
- **ref/09 §5.2 and OpenAPI:** `GET /v1/node/hardware` (`NodeHardware`), the read-only inventory that `jk board probe` uses.
- **ref/01 appendix A and §5.22:** the Joker Micro `esp32` driver (touch, RSSI, uptime) and the `[net.wifi]` section (`J0240`–`J0242`); **ref/08 §2.13.3 and §2.17:** the `NET` section of the blob and the path of the Wi-Fi password in `SECRETS`; **ref/02 §2.4.8:** the exception for the secrets of a coprocessor MCU with Wi-Fi.
