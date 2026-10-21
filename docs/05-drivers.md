# 05 — Drivers, profiles and boards

Drivers are the competitive moat and also the largest cost. This document explains how they are written, how equipment is described (profiles), how a bus is shared and where the catalog starts. The normative detail is in the references (ADR 0023): the three tiers in ref/06, profiles, boards and simulation in ref/03.

---

## 1. Three tiers

| Tier | Where it runs | Language | What for | Cost |
|---|---|---|---|---|
| **N** (native) | inside `jokerd`, `io`/`rt/drv` threads | Rust | base protocols: Modbus, OPC UA, GPIO, I²C, SPI, CAN, CANopen, EtherCAT, piControl, M-Bus, MQTT, V4L2 | a memory bug brings down the process (mitigated by Rust without `unsafe` outside `*-sys`) |
| **W** (Wasm) | inside `jokerd`, sandboxed | Rust, C, Go, … → Wasm | third-party and vendor protocols over simple transports (serial, TCP, UDP, I²C, SPI) | ~2–5× slower; no direct access to ioctls |
| **X** (external) | `joker-drv-<name>` process | any | vendor binary SDKs (GigE cameras, NPUs, proprietary libraries), untrusted code | shared-memory IPC; +20–50 µs per batch |

Rule: **N** for what is universal and critical, **W** for the ecosystem and **X** for what cannot be controlled.

---

## 2. The common contract

There is **a single contract** for the three tiers: the same operations, the same records and the same codes; only the call transport changes (Rust call, Wasm call, JDP). The reference is ref/06: §2 for the contract, §3 for the Rust traits of the `joker-hal` crate, which serve the three tiers (`Planner`, with `alloc`, for the plan; `Driver`, `no_std`, for the runtime).

| Operation | What it does | Required |
|---|---|---|
| `plan` | validates the configuration and the addresses and groups the points into batches; **pure** | yes |
| `open`, `connect`, `disconnect` | instance lifecycle; `connect` returns the identity and, with `discover = true`, the discovered map | yes |
| `read` | one execution of a batch: raw values and the status of each point | yes |
| `write` | **applied** writes, in the given order | with the `write` capability |
| `read-addr`, `write-addr` | access by address, for the steps the core executes | with `raw-access` |
| `invoke` | `native` actions (an OPC UA method, an SDO) | with `native-actions` |
| `scan`, `health` | network discovery (`jk discover`) and health | `health` yes; `scan` with `scan` |

- `plan` is **pure** (no I/O, clock or randomness) and runs in `jk check`, in `jk build` and when the generation is prepared: profile errors (invalid address, overlap, coherent group that does not fit in one request) show up before the deploy. The same request gives the same plan, byte for byte.
- The driver delivers and receives **raw values** (registers, bytes, bools, already typed integers). Scale, unit, byte and word order, rounding on write and derived quality belong to the core's acquisition and output stage (document 01, §3.3), based on the profile. A new driver need not reimplement this, and two drivers do not implement it in different ways.
- **The core drives the connection and executes the profile steps** (`[connect]`, `[relinquish]`, `[config] apply`, actions with `steps` and the M1 fallback) over the two primitives `read-addr` and `write-addr`. A new driver gains actions, fallback and relinquish without writing code for it. Retries and backoff also belong to the core: one attempt per call.
- Each driver publishes a **descriptor** (name, version, protocols, buses, capabilities and the schemas of the `config` keys, of the address, of the `quirks` and of the discovered map). The core validates the manifest and the profile against it **before** `plan`, with the same messages in every driver, also for a tier X whose executable does not run on the laptop.

---

## 3. Read plans

The plan groups points into **batches**, and each batch is one request in the protocol:

- **Modbus:** coalescing of contiguous registers within `max_regs` (125 by default, or whatever the profile says) and with gaps up to `max_gap` (8 registers by default, because reading garbage is cheaper than another request). The defaults of all `quirks` are in ref/03 §2.2.6. It respects `forbidden` (addresses that the equipment rejects or that have side effects on read, which really do exist).
- **OPC UA:** subscriptions with `MonitoredItems` and `samplingInterval` per poll group, and batched `Read` for the rest.
- **CANopen:** mapped PDOs (push) and SDO for the rest (slow, only for parameters).
- **SunSpec:** one batch per discovered model, with the scale model (`*_SF`) read in the same request as the values. The detail matters: if the scale factor and the value come from different requests, the new value can be read with the old scale.

Each batch has a **period** (the device's `poll` or that of individual signals) and a **priority**.

**On-demand parameters.** Object dictionaries with thousands of entries (CANopen OD, drive parameters) are not read in a cycle. Signals with direction `param` and `poll = "on_demand"` are read on `connect`, after each write and when an app or the CLI asks for them (`jk read --refresh`). They stay in the registry like any other signal, with their real age in view.

---

## 4. Acquisition coherence

All the points of a batch receive the same `acq_id` (document 02, §6.2). Profiles may force points into the same batch with the `[coherent]` table, one group per key (`ac = ["ac.p", "ac.q", "ac.pf"]`, ref/03 §2.2.9), and `plan` fails if they do not fit in one request (`J0813`). Value/`scale_factor` pairs are implicit groups. The driver must have the `coherent` capability.

---

## 5. Bus scheduler

Shared buses (RS-485, I²C, SPI, a PLC with a single TCP connection declared as `bus`) have **one scheduler** with queues by priority:

1. applied writes (commit) and actions (including the steps by address);
2. batches of the signals used by cycle groups, in order of period;
3. telemetry batches;
4. discovery and diagnostics.

Only one call is in progress on the bus. TCP devices without `bus` and CAN buses are not serialized across devices: each has its own connection, or the frames are asynchronous (ref/06 §2.14).

**Bus budget (checked in `jk check`, `J0370`).** For each bus, `jk check` estimates the occupancy:

```
Modbus RTU, 19200 baud, 8E1 (11 bits/char):
  request FC03 = 8 bytes, response with n registers = 5 + 2n bytes
  t(n) = (8 + 5 + 2n) · 11 / 19200 s + turnaround + server latency (profile: 10 ms)
  t(20) = 53 · 11 / 19200 s ≈ 30 ms  + 5 ms + 10 ms ≈ 45 ms

  3 meters × 2 batches × 45 ms = 270 ms of work every 200 ms
  → occupancy 135% → J0370: bus overloaded; with the 80% occupancy target,
    the minimum viable poll is ~340 ms (or merge each meter's two batches into one, if the
    profile allows it: 3 × (53+40)·11/19200 + … ≈ 3 × 68 ms = 204 ms → still 102%)
```

This catches the most common mistake in Modbus RTU projects: asking 10 meters on one RS-485 for what only fits 3. `J0370` is raised above 100%; the 80% target gives the minimum poll of the help. The model for the other buses (I²C, SPI, CAN, shared Ethernet) is in ADR 0049.

---

## 6. Profiles

A profile describes an **equipment model**, not an instance. It lives in the profile registry (Git + OCI) or in the project's `profiles/`.

```toml
# profiles/eastron/sdm630@1.profile.toml
schema = 1

[profile]
name     = "eastron/sdm630"
version  = "1.2.0"
driver   = "modbus"                  # modbus-rtu or modbus-tcp both work
vendor   = "Eastron"
docs     = "https://…"
quirks   = { max_regs = 40, max_gap = 0, min_interval = "20ms" }

[defaults]
fc        = 4                        # input registers
type      = "f32"
word_order = "big"                   # big | little
byte_order = "big"

[signals]
"l1.v"   = { at = 0,   unit = "V" }
"l2.v"   = { at = 2,   unit = "V" }
"l3.v"   = { at = 4,   unit = "V" }
"l1.i"   = { at = 6,   unit = "A" }
"p_total" = { at = 52, unit = "W", quantity = "active_power" }
"q_total" = { at = 60, unit = "var", quantity = "reactive_power" }
"f"      = { at = 70,  unit = "Hz", limits = ["45 Hz", "65 Hz"] }
"e_imp"  = { at = 72,  unit = "kWh", quantity = "energy", monotonic = true }

[coherent]
power = ["p_total", "q_total"]
```

With outputs, actions and fallback:

```toml
# excerpt from profiles/acme/pcs-125@3.profile.toml
[signals]
"p_set"    = { at = 40100, fc = 16, type = "i16", scale = 0.1, unit = "kW", direction = "output" }
"p_set_rb" = { at = 40100, fc = 3,  type = "i16", scale = 0.1, unit = "kW" }
"mode"     = { at = 40101, fc = 6,  type = "u16", enum = "PcsMode", direction = "output" }
"state"    = { at = 30010, fc = 4,  type = "u16", enum = "PcsState" }
"alarms"   = { at = 30011, fc = 4,  type = "bits16", bits = { 0 = "overtemp", 1 = "dc_fault", 5 = "grid_fault" } }

[enums]
PcsMode  = { off = 0, standby = 1, p_control = 2 }
PcsState = { off = 0, standby = 1, running = 2, fault = 3 }

[fallback]                           # M1
timeout_register = { at = 40200, fc = 6, type = "u16", unit = "s" }
value_register   = { at = 40201, fc = 6, type = "i16", scale = 0.1, unit = "kW" }
applies_to       = ["p_set"]

[actions.reset_fault]
steps = [{ write = { at = 40300, fc = 6, value = 1 } }, { wait = "500ms" }, { write = { at = 40300, fc = 6, value = 0 } }]
```

**Versions:** `profile@major`. A change of address or scale is major, and adding a signal is minor (full table in ref/03 §2.2.17). The lock pins the exact version. In the manifest, `profile = "sunspec"` is the short form of `profile = "joker/sunspec@1"`, the discovery profile built into the runtime (ref/03 §2.2.15).

**Implemented classes:** a profile may declare that the equipment fulfills classes (document 14), with the mapping of the members to its signals. Each device with that profile becomes an implicit instance of those classes.

```toml
[implements."inverter.pv@2"]
map = { p_ac = "ac.p", p_limit_pct = "w_max_lim_pct", state = "state" }
capabilities = ["reactive"]
```

A profile only enters the public registry if its implementations pass the class conformance tests (`jk class verify`).

**Importers:** `jk profile import` from SunSpec models (official JSON), OPC UA NodeSets (XML), CANopen EDS/DCF, EtherCAT ESI (XML) and register sheets in CSV. The result is a TOML profile reviewed by a person: the importer speeds things up, but does not replace that review.

---

## 7. Discovery (M2)

- `jk discover modbus-tcp://192.168.10.0/24 --unit 1..10`: read functions only, with a rate limit, and it never writes.
- `jk discover sunspec://192.168.10.30`: walks the model chain starting at 40000.
- `jk discover opcua://plc:4840`: browse with a filter by type.
- `jk discover can0`: CANopen LSS/heartbeat.

With `discover = true` in the manifest, the driver does the discovery on `connect`, compares with `joker.lock` and **does not use unpinned maps in `prod`** (document 03, §14).

---

## 8. Tier W: drivers in Wasm

The world is `joker:driver`, in `wit/joker-driver.wit` (ref/06 §4). In 1.0 it is **unstable** and only serves the in-tree drivers; it becomes stable and open to third parties in 1.2 (ref/06 §4, stability rule). It has **the same version** as the `joker:runtime` it imports: `joker:driver@0.1.0` imports `joker:runtime/types@0.1.0` and `joker:runtime/log@0.1.0`, and in 1.0 both move to `@1.0.0`. The interfaces are `types` (the contract records), `host` (clock, call deadline, link state, action progress), `transport` (serial port, TCP, UDP, I²C, SPI, CAN, delivered as a `link` in `open`) and `guest`, the exported one, with the operations of §2. Errors are closed enums (`batch-failure`, `write-status`, `action-failure`) and `error { code, message }` where it is meant for people; only the configuration, the addresses and the `quirks`, which are specific to each protocol, travel in CBOR, with the schema declared in the descriptor (custom section `joker:driver-meta`).

`plan` runs in an instance of its own, with `log` only (the transport traps with `J0882`), with deterministic *fuel*: the same component gives the same plan on the laptop and on the node. At runtime, each instance has its own `Store`, yields the `io` thread after every 1 ms of continuous CPU and has a CPU limit per call. A trap puts the points in `bad(not_connected)` and the link in `stopped`, and the instance is recreated with backoff. The 1.0 host provides serial, TCP and UDP; I²C, SPI and CAN come in 1.2.

The driver receives **only** the transport that the manifest gave it (`bus = "rs485"`), and does not open sockets of its own choosing. It is the right place for protocols such as DLMS/COSEM, IEC 62056-21, proprietary inverter protocols and EEBus over a provided transport. The Rust SDK (`joker-driver-sdk`) generates the component from the same traits as tier N.

---

## 9. Tier X: Joker Driver Protocol (JDP)

An external `joker-drv-<name>` process speaks JDP with `jokerd` (reference: ref/06 §5; `spec/cddl/jdp.cddl`, `spec/c/jdp.h`):

- **control:** Unix socket `SOCK_SEQPACKET` at `/run/joker/drv/<name>.sock`, CBOR messages (`hello`/`welcome` with the `jdp` version, plan, lifecycle, access by address, actions, scan, health, logs);
- **data:** two SPSC rings in shared memory, each in a memfd with **a single writer** (the `jokerd` one sealed), with notification through `eventfd`: read requests and writes in one direction, samples and acks in the other;
- **streams** (1.1): dmabuf/memfd descriptors passed through `SCM_RIGHTS`, zero-copy.

**Startup and isolation.** In production, `jokerd` (which is not root) starts each driver as a **transient systemd unit** with `DynamicUser`, sandbox (`ProtectSystem=strict`, `DevicePolicy=closed`, only the devices and addresses of the manifest) and memory and CPU limits; a polkit rule limits it to `joker-drv-*` units. The connection is authenticated by the peer's cgroup (`SO_PEERCRED`) and by a startup token. Without systemd, `launch = "spawn"`. `jokerd` treats everything that comes from the driver as hostile and validates each input before using it.

`jokerd` supervises the process: if it dies, the signals go to `bad(not_connected)`, the link to `stopped`, and the process is restarted with backoff; the outputs are covered by the device fallback. An SDK in C (`libjdp`) and in Rust (`joker-jdp`) turns a vendor SDK into a driver with ~200 lines.

---

## 10. Initial catalog

The column gives the phase up to 1.0 (document 11) or, after it, the version (document 21).

| Driver | Tier | Base | Phase or version |
|---|---|---|---|
| `sim` | N | — | 0 |
| `modbus-tcp`, `modbus-rtu` | N | `tokio-modbus` | 1 |
| `gpio` (chardev v2), `i2c`, `spi` | N | `gpio-cdev`/`linux-embedded-hal` | 1 |
| `sysfs` (LEDs and temperatures of any Linux board: `/sys/class/leds`, `/sys/class/thermal`) | N | — | 1 |
| `revpi` (piControl: DIO, AIO, MIO, gateways) | N | ioctl on `/dev/piControl0` | 1 |
| `esp32` (Joker Micro only: touch pads, Wi-Fi RSSI, uptime; ref/01 appendix A) | N | `esp-hal` | 2 |
| `mqtt` (input) | N | `rumqttc` | 1 |
| `sunspec` (over modbus) | N | profiles + discovery | 1 |
| `opcua` (client) | N | `async-opcua` | 2 |
| `v4l2` / `libcamera` | N / X | `v4l` / libcamera | 1.1 |
| `can`, `canopen` | N | `socketcan` | 2 |
| `m-bus` (energy, heat, water meters) | N | to be written | 2 |
| `onewire`, `serial-line` (text protocols) | N | — | 2 |
| `spi-adc` (ADCs with their own clock, blocks) | N | `spidev` + DMA | 1.1 |
| `ethercat` | N | `ethercrab` | 1.2 |
| `bacnet-ip` | X → N | `bacnet-stack` (C) in X, then native | 1.2 (both tiers) |
| `gige-vision` | X | Aravis | 1.2 |
| `iec-104`, `iec-61850 (MMS)` | X | existing libraries | 1.2 |
| `dlms-cosem` | W | to be written | 1.2 |
| `eebus` | W/X | — | 1.2 |

**`sysfs` on LEDs.** When opening an LED as an output, the driver sets `trigger` to `none` (otherwise the kernel keeps driving it, with `mmc0` or `heartbeat`) and restores the previous `trigger` on close. The `jokerd` package installs a udev rule that gives the `joker` group write access to `brightness` and `trigger` of `/sys/class/leds/*`, so that `jokerd` does not need to run as `root`. The detail is in ref/01, appendix A.

---

## 11. Board profiles

A board profile (`board = "…"`) describes the fixed hardware:

```toml
# boards/revpi-connect-5.board.toml
schema = 1

[board]
name    = "revpi-connect-5"
version = "1.0.0"
arch    = "aarch64"
cpus    = 4
memory  = "4 GiB"
recommended = { rt = [3], io = [2], ml = [1, 2], bg = [0] }

[watchdog]
device  = "/dev/watchdog0"
timeout = "60s"

[io.led_a1]
driver    = "gpio"
line      = "…"
direction = "output"

[io.relay]
driver    = "gpio"
line      = "…"
direction = "output"
fallback  = false                # what the hardware imposes when jokerd or the board stops (M1)

[serial]
"rs485-0" = "/dev/ttyRS485-0"

# [accel.<name>]: only what the board guarantees; the rest is detected (reserved for 1.1)
```

Fixed IO appears as `board.<name>` (`board.relay`). The `fallback` key states the state that the **hardware** imposes, and counts as device fallback (M1) in the check against `safe`; `safe` still belongs to the manifest (ADR 0006). The full format, with the microcontroller boards, is in ref/03 §2.3.

Boards planned in Phase 1: `rpi4`, `rpi5`, `cm4`, `cm5`, `revpi-connect-4`, `revpi-connect-5`, `x86-generic`. In Phase 2: `jetson-orin-nano`, `jetson-orin-nx`.

---

## 12. Connection, reconnection and health

The link state of each device is the `link_state` enum of ref/00 §6. The effective one is the worse of what the driver reports and what the core computes from the calls (ref/06 §2.12).

| Link state | Signal quality | Behavior |
|---|---|---|
| `connecting` | `uncertain(initial)` on points without a value | backoff 250 ms → 30 s, ×2, with jitter ±20% |
| `up` | that of the value | — |
| `degraded` (≥ 20% of the last 20 batch executions with timeout or communication failure) | `uncertain(stale)` on the affected batches | alarm `joker.device.<d>.degraded` |
| `down` | `bad(comm_failure)` | alarm `joker.device.<d>.offline`; outputs: device fallback |
| `drift` (M2) | values read normally, outputs blocked in `prod` | `J0401` with the `kind` (identity, firmware, map, fallback); alarm `joker.device.<d>.drift` |
| `stopped` | `bad(not_connected)` | the driver is not running (X process dead, W trap, crashloop, or stopped by hand with `jk device stop`, document 07 §4); tells "the software went down" apart from "the cable went down" |

The driver only reports the first four; `drift` and `stopped` belong to the core.

**`joker-link` devices** (a Joker Micro, document 19 §6). The session mode decided in `HELLO` (`session_mode`, ref/00 §6) gives the link state (ref/07 §7.2):

| Joker Link session | Link state | Quality | Node writes |
|---|---|---|---|
| no session, or handshake in progress | `connecting` | `uncertain(initial)` until the first sample | no |
| `full` | `up` (or `degraded`, with drops ≥ 20% in 10 s) | the one that comes in `SAMPLES` | yes |
| `observe` (the MCU does not match the lock or the generation) | `drift` | the one that comes | not in `prod` (`J-E2319`) |
| `maintenance` (MCU without a valid generation, or invalid catalog) | `down` | `bad(config_error)` | no; only OTA, trace, time, restart, catalog and key rotation |
| `refused` (no common major, wrong identity) | `down` | `bad(config_error)` | no |
| session lost (`link_timeout`, `BYE`, `retransmit` exhausted) | `down` | `bad(comm_failure)`; alarm `joker.device.<d>.offline` | no |

In every case, **the MCU is in charge of the MCU's outputs**: if the session drops, the remote leases expire there and `safe` is applied there (ref/07 §7.4).

Per-device metrics, the same in the three tiers and measured by the core: p50/p99 latency per batch, timeouts, reconnections, bytes, coalescing efficiency (useful registers / read registers) and the driver's own counters.
