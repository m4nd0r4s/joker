# Part 6 — Switching boards: the same project on the Pi Zero W, and the board that does not exist yet

**Goal:** get the project `bancada` running on a Raspberry Pi Zero W without changing a line of the app `luz`: separate in the overlays what belongs to Raspberry Pis from what belongs to the Pi 3, write the profile of a board the registry does not know, and talk to the same ESP32 over Wi-Fi instead of USB.

**You will learn:** refactoring overlays without changing the effective manifest · the effective manifest hash · what a board profile is (and what it is not) · `jk board probe`, `jk board check`, `jk board new` · support levels and when a board is published · resources on a single-core board · Modbus RTU and Modbus TCP with the same profile · two clients on the same equipment · one lock per manifest and WCET per board · `jk diff --against`.

**Before you start:** part 5 done (app `luz` 0.4.1 on lab). The Pi Zero W with an SD card and a micro-USB power supply. The ESP32 stays connected to the Pi 3 (that is where its power comes from) and is on the same 2.4 GHz Wi-Fi network. You need the ESP32's IP: the serial console showed it when you flashed the bench firmware in part 4 (or look it up in the router's DHCP table, and reserve it there).

---

## 1. The rule: switching boards only touches manifests

The Zero W is another machine: 32-bit ARMv6, **one** CPU core, 512 MB, Wi-Fi only and no free USB for the ESP32. But the logic is the same: blink the board's LED according to the CPU temperature, with the ESP32's button, touch and LED. The app does not know which board it runs on; what changes is what the manifest says about the board and about the path to the ESP32. At the end of the part, `jk diff` will show exactly that.

## 2. First, tidy up: what belongs to Raspberry Pis and what belongs to the Pi 3

`joker.lab.toml` mixes two things: the device `pi` (the `ACT` LED and the temperature through `sysfs`), which exists on any Raspberry Pi, and what belongs only to the Pi 3 (the board, the reserved CPU cores, the ESP32 on the USB port). The common part moves to a new overlay:

```toml
# joker.rpi.toml — what is common to the Raspberry Pi boards
extends = ["joker.toml"]

[devices.pi]
driver = "sysfs"

[devices.pi.signals]
act_led  = { led = "ACT", direction = "output" }
cpu_temp = { thermal = "thermal_zone0" }
```

```toml
# joker.lab.toml — lab node (Raspberry Pi 3 B+)
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
dtr  = "low"
rts  = "low"

[devices.esp]
driver  = "modbus-rtu"
bus     = "esp32"
unit_id = 1
profile = "local/bench-esp32@1"

[security.api]
listen = ["tcp:[::]:7443"]
```

Why does `[devices.pi]` not go into `joker.toml`? Because `joker.toml` is common to the simulator and the hardware, and in `joker.sim.toml` `pi` is `driver = "sim"`. The chain becomes `joker.lab.toml → joker.rpi.toml → joker.toml`, and the linearization is `joker.toml, joker.rpi.toml, joker.lab.toml` (ref/01 §7.1).

A refactoring has to be proven. The effective manifest hash is computed after the merge, with the defaults materialized; splitting a file into two overlays **does not change it** (ref/01 §8.3):

```sh
$ jk diff --env lab --node lab
```

```
effective manifest  joker.lab.toml → sha256:3f9a41c0…
active generation   lab · generation 6 · sha256:3f9a41c0…
no differences
```

And `jk explain devices.pi.driver --env lab` says which layer each key comes from (here, `joker.rpi.toml:5`). `joker.lab.lock` does not change either (`jk lock --check --env lab` exits with 0): the lock belongs to the input manifest, and the input manifest is the same (ref/02 §2.1.1). `joker.rpi.toml`, which only comes in through `extends`, has no lock.

## 3. The Zero, first attempt

The most natural way to start is to copy the node that already works and change the board:

```sh
$ cp joker.lab.toml joker.zero.toml       # and, in joker.zero.toml, board = "rpi-zero-w@1" instead of "rpi3b-plus@1"
```

`joker.zero.toml` ends up identical to the `joker.lab.toml` of §2, with one different line at line 5. On purpose: `jk check` will tell you, one by one, the things that belonged to the Pi 3.

```sh
$ jk check --env zero
```

```
error[J0172]: profile or board not found, or profile without a major: board `rpi-zero-w`
  --> joker.zero.toml:5:9
   |
 5 | board = "rpi-zero-w@1"
   |         ^^^^^^^^^^^^^^ searched in boards/, in oci://hub.example.com/joker and among the built-in ones
   = note: similar known boards: rpi3b-plus
   = help: if it is a new board, write the profile from the node itself:
            jk board probe --node zero --name rpi-zero-w --out boards/rpi-zero-w.board.toml
note: the checks that depend on the board (resources, memory, RT admission) are left undone
```

`jk check` stops here on purpose: the next steps of the effective manifest need the board (ref/01 §8.1, step 4). `jk build --target rpi-zero-w` gives the equivalent error on the build side, `J0827`.

The board `rpi-zero-w` is not in the registry, and not by oversight: the Zero W is ARMv6, which Joker only supports at the `dev` level (document 01, §22; document 23, §7). You will write it yourself. For that, the Zero has to be a node.

## 4. Prepare the Zero

1. **System:** Raspberry Pi OS Lite **32-bit**, written with Raspberry Pi Imager, with SSH on, Wi-Fi configured and the name `zero`.
2. **`jokerd` for ARMv6.** The package is called `armhf`, but beware: Debian's `armhf` is ARMv7, and an ARMv7 binary dies on the Zero with `Illegal instruction`. The Joker package for 32-bit Raspberry Pi OS is built for `arm-unknown-linux-gnueabihf` (ARMv6 with VFP2). Install it as in part 2:

```sh
zero$ uname -m && dpkg --print-architecture
armv6l
armhf
zero$ sudo apt install ./jokerd_1.0.0_armhf.deb
zero$ sudo jokerd enroll-code
```

3. **Enroll**, with the CA from part 2:

```sh
$ jk node enroll ssh://pi@zero.local --ca ~/org-ca --name zero --allow-dev --code 7KQ2M4XD9TPB3HVA
```

```
→ zero · ssh://pi@zero.local
identity    P-256 in a file (no TPM: J0933, warning) · KEM X25519
certificate CN=zero · issued by ~/org-ca · policy allow_dev = true
client      [nodes.zero] address = "ssh://pi@zero.local"
```

The node is alive, without a generation, waiting, and the `zero` context uses the bootstrap address, like `lab` in part 2. That is what the probe needs.

## 5. Write the board that does not exist

### 5.1 What a board profile is

A board profile describes **the computer**, and only the computer: the architecture, the CPU cores, the memory, the kernel it is supported with, the watchdog, the fixed IO soldered onto the board (the `ACT` LED, the SoC temperature sensor) and what the hardware does by itself when `jokerd` stops, the ports and interfaces with stable names, and a suggestion for `[resources]` (ref/03 §2.3).

It does **not** describe the equipment: the ESP32 does not appear there, nor whatever is connected to the pins. Equipment items are devices of the manifest, with device profiles (part 4). And it has no site decisions: no `safe` (ADR 0006), no `poll`, no envelopes. A board answers "what is this machine?", and that is why the same profile serves any project that runs on a Zero W.

Who uses it: `jk check` (CPU cores, memory for `J0380`, referenced peripherals, `recommended`), `jk build` (architecture and Wasm engine), `jk doctor` (kernel, watchdog) and the lock, which pins its version and digest.

### 5.2 The draft

`jk board probe` asks the node for its hardware inventory (`GET /v1/node/hardware`, reads only) and writes a draft. Whatever it cannot know by reading the node, it marks with `# REVIEW`:

```sh
$ jk board probe --node zero --name rpi-zero-w --out boards/rpi-zero-w.board.toml
```

```
→ zero · inventory read (reads only)
cpu     1 × ARM1176JZF-S (armv6l) · cpufreq max 1 GHz
memory  revision 9000c1 → 512 MiB (the kernel sees 427 MiB)
kernel  6.6.51+rpt-rpi-v6 · no PREEMPT_RT
sysfs   leds: ACT · thermal: thermal_zone0 (cpu-thermal) · watchdog0 (bcm2835-wdt, max 15 s)
gpio    gpiochip0 "pinctrl-bcm2835" (54 lines)
tty     serial0 → ttyS0
net     wlan0
draft   boards/rpi-zero-w.board.toml · 5 fields to review
```

The name follows the rule from part 4: `<board>.board.toml`, with the board schema in the editor and `J0803` if the content does not start with `[board]`. The manifest writes only `board = "rpi-zero-w@1"`, and resolution adds the suffix and looks in `boards/` (ADR 0026).

The draft has the same tables as the final file of §5.3, plus a `[serial]`, and five lines with `# REVIEW`:

```toml
# boards/rpi-zero-w.board.toml — draft from `jk board probe --node zero` (the lines to review)
[board]
model       = "Raspberry Pi Zero W Rev 1.1"          # read from /proc/device-tree/model
tier        = "dev"                                  # REVIEW: support level; armv6 is only accepted in "dev"
kernel      = "standard"                             # REVIEW: target kernel; the node runs a kernel without PREEMPT_RT
recommended = { rt = [0], io = [0], bg = [0] }       # REVIEW: with 1 CPU there is nowhere to isolate rt

[io.act_led]
driver    = "sysfs"
led       = "ACT"
direction = "output"
# REVIEW: fallback. What does this LED do when jokerd stops? The probe does not know.

[serial]
"uart0" = "/dev/serial0"                             # REVIEW: on the Zero W it is the mini UART (ttyS0)
```

### 5.3 Review, field by field

| Field | Decision | Why |
|---|---|---|
| `tier = "dev"` | stays | ARMv6 only exists at the `dev` level (ref/03 §2.3.1): it is for development and is never a production target. There is no CI with this hardware |
| `arch = "armv6"` | stays | it is what decides the Wasm engine and the binary; with another `tier` it would be `J0829` |
| `cpus = 1` | stays | it is the basis of `J0144` in the manifest's `[resources]` |
| `memory = "512 MiB"` | stays | the board has 512 MiB; the 427 MiB visible depend on the system's `gpu_mem`, which `jk doctor` compares with the node |
| `model` | `"Raspberry Pi Zero W (v1.1)"` | it is text, for people |
| `kernel = "standard"` | stays | there is no maintained PREEMPT_RT kernel for this board, and with one CPU core there would be nowhere to isolate it. No `boot_cmdline` |
| `recommended` | stays, everything on CPU 0 | there is no alternative; §6 comes back to this |
| `[watchdog]` | stays | confirm that no other service opens it (`jokerd` must be its only owner) |
| `[io.act_led]` without `fallback` | stays without | try it: with the LED on, `sudo systemctl stop jokerd` on the Zero, and the LED stays on. There is no hardware fallback. And even if there were, it would only count through the path `board.act_led`, and not through the `pi.act_led` of `joker.rpi.toml` (ref/03 §2.3.2): that is why `J0205` appears in `prod`, as on the Pi 3 (part 2) |
| `[serial]` | goes | the Zero W's mini UART has its clock tied to the core clock and the good UART belongs to Bluetooth; nothing in the project uses it, and a board only promises what it guarantees |

Remove the `# REVIEW` comments and change the `model`. The final file is the one in document 23 §7:

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

### 5.4 Validate against the node

```sh
$ jk board check boards/rpi-zero-w.board.toml --node zero
```

```
✓ schema       schema 1 · rpi-zero-w 1.0.0 · linux · armv6 · tier dev
✓ consistency  1 CPU; recommended in 0..0; armv6 with tier dev
✓ on node zero armv6l · 1 CPU · 512 MiB · kernel standard
               /dev/watchdog0 · pinctrl-bcm2835 · leds/ACT · thermal_zone0 · wlan0
```

**What `jk board check` says** when you start with the wrong shortcut. `jk board new` copies an existing board, and the Pi 3 looks like a good starting point:

```sh
$ jk board new rpi-zero-w --from rpi3b-plus
$ jk board check boards/rpi-zero-w.board.toml --node zero
```

```
error[J0829]: inconsistent board profile (architecture, system, CPUs, pins): the board says `arch = "aarch64"` and `cpus = 4`; node zero is armv6l with 1 CPU
  --> boards/rpi-zero-w.board.toml:6:1
error[J0828]: resource nonexistent on the board (line, port, bus, accelerator): on node zero, /dev/i2c-1 ([i2c] "i2c1"), /dev/spidev0.0 ([spi] "spi0"), eth0 ([net] "eth")
  --> boards/rpi-zero-w.board.toml:37:1
   = help: for a board that does not exist yet, start from what the node says: jk board probe --node zero
```

`jk board new` is for variants of a known board (another revision, another memory size). For a different machine, the probe is more honest: it starts from what exists, and only asks you for what it cannot know.

### 5.5 When a board is published

This one stays in `boards/`, and the lock pins it by the file's digest (`source = "path:…"`, ref/02 §2.1.4). A board is published when other projects will use it and it describes a supported target, level `1` or `2`, with the versions of ref/03 §2.3.6. **A `dev`-level board is never published** (`J0848`): in a registry, any manifest would resolve it as if it were a real target.

## 6. Now, the real problems

With the board in place, `jk check` gets past step 4 and shows what was copied from the Pi 3 without belonging to Raspberry Pis:

```sh
$ jk check --env zero --node zero
```

```
error[J0144]: value outside the allowed set: CPU 3 does not exist on the board rpi-zero-w (1 CPU: 0)
  --> joker.zero.toml:8:15
   |
 8 | rt = { cpus = [3] }
   |               ^^^ also io.cpus = [2] (line 9) and bg.cpus = [0, 1] (line 10)
   = help: remove [resources]: the default is the board's `recommended`
error[J0828]: resource nonexistent on the board (line, port, bus, accelerator): on node zero, port /dev/serial/by-id/usb-Silicon_Labs_CP2102_… ([buses.esp32])
  --> joker.zero.toml:14:8
   = note: the ESP32 is connected to the Pi 3, and not to this node
warning[J0352]: group ≥ 10 ms without PREEMPT_RT: jitter not guaranteed: group `main` (20 ms)
  = note: rpi-zero-w is `kernel = "standard"`; it is what is expected on this board (document 23, §5.2)
note: Wasm engine on armv6: Pulley (interpreter), to be confirmed in Phase 1 (document 23, §5.2)
```

One by one:

- **`[resources]`** belonged to the Pi 3, and that is why it lives in `joker.lab.toml` and not in `joker.rpi.toml`: the Pi 3 has four CPU cores and reserves core 3 for `rt`. On the Zero, you remove the section. Without it, the resources are the board's `recommended`: everything on CPU 0. On a single-core board, `rt` shares the CPU with `io` and `bg`; there is no isolation to do, and `jk check` says so in a note.
- **`J0352`** is a warning, and it stays. A 20 ms group without PREEMPT_RT works, but the jitter has no guarantee. That is what the Zero is on the bench for: to prove that `jokerd` degrades with warnings and does not break.
- **The Wasm engine.** On the Pi 3, apps are compiled by Cranelift. On ARMv6 there is no Cranelift: the plan is Pulley, the wasmtime interpreter, if it builds for `arm-unknown-linux-gnueabihf`. It is marked **to be confirmed** (document 23, §5.2). If it does not build, `jk check` gives the warning `J0243` (the board has no Wasm engine for the architecture: native apps only, document 04, §5), and this part stays half done. The track assumes it builds.
- **`[buses.esp32]`**: the ESP32 is not connected to the Zero. But the bench firmware serves the same register table over **Modbus TCP**, on Wi-Fi, port 502 (document 23, §4).

The profile is **the same**. `driver = "modbus"` in the profile is the family, and the `modbus` family serves both transports (ref/03 §2.2.2); the `at`/`fc` addresses do not change. Only the device changes, and the serial bus disappears:

```toml
# joker.zero.toml — zero node (Raspberry Pi Zero W): the ESP32 over Wi-Fi
extends = ["joker.rpi.toml"]

[target]
board = "rpi-zero-w@1"

[devices.esp]
driver  = "modbus-tcp"
address = "192.168.1.57:502"
unit_id = 1
profile = "local/bench-esp32@1"

[security.api]
listen = ["tcp:[::]:7443"]
```

Before applying, confirm that the ESP32 answers over Wi-Fi, from the Zero itself:

```sh
$ jk profile test local/bench-esp32@1 --uri "modbus-tcp://192.168.1.57:502?unit=1" --node zero
```

```
signal   register  raw    value    quality     latency
led_rb   coil 0    1      true     good        38 ms
button   di 0      0      false    good        (same request)
touch    in 0      61     61       good        41 ms
rssi     in 1      -58    -58      good        (same request)
uptime   in 2–3    5214   5214 s   good        (same request)
fw       hr 0      1      1        good        35 ms
3 requests · 7 signals
```

Modbus TCP has no authentication: any machine on the network can write to coil 0. On the bench that is acceptable; on a site, Modbus TCP lives on a separate fieldbus network.

## 7. Two clients on the same equipment

The `led_rb` at `true` in the table above is lab: the Pi 3 keeps talking to the ESP32 over RTU, and the firmware answers both transports at the same time. **Do not connect both nodes to the same LED.**

- **Each node arbitrates its own outputs, and only its own.** The arbiter, the envelope and the lease of `esp.led` on lab do not know that zero exists. Both would write their own `ligado` to coil 0, and the LED would follow the last request to arrive. Each node would see, in the readback, a value it did not write.
- **The equipment fallback stops protecting.** The firmware turns the LED off after 2 s *without requests from anyone*. With two clients, if zero goes down, lab's requests keep the ESP32 "company", and the LED stays at the last value zero wrote.
- In v1 there is no arbiter between nodes: a signal belongs to one node. A site signal space shared by several nodes is the v2 mesh (document 21, §8).

So, before applying on zero, take the ESP32 away from lab. The clean way is to go back to a lab generation that did not have `esp`: `jk generations --node lab` shows it, number 3, from part 3 (`luz` 0.2.0).

```sh
$ jk rollback 3 --node lab
```

Lab keeps blinking with the temperature and does not talk to the ESP32, which stays powered by the Pi 3's USB. Part 7 puts it back.

## 8. Lock, WCET and apply

```sh
$ jk build --target rpi-zero-w
$ jk lock --env zero --node zero
$ jk bench --env zero --node zero
```

One lock per input manifest (D8): `joker.zero.toml` gives `joker.zero.lock`, next to it, with the board, the profile, the app, the ESP32 fallback read over Wi-Fi and the WCET measured **on this** board:

```
→ zero (dev) · from --node
luz.pisca  100,000 runs · Pulley · p50 150 µs · p99.9 240 µs · max 610 µs
warning[J0432]: measured WCET above the one declared by the app: 240 µs measured, 20 µs declared
  = note: the declared one is the author's estimate; RT admission uses the measured one, from the lock, for this board
RT admission  main 20 ms · 240 µs · 1.2% of the period ✓
written       joker.zero.lock [wcet] "luz.pisca" (board = "rpi-zero-w")
```

```toml
# joker.zero.lock (excerpt; [profiles] and [apps] the same as in joker.lab.lock)
version = 1
runtime = "1.0.0"
node = "zero"

[boards."rpi-zero-w@1"]
resolved = "1.0.0"
digest = "sha256:…"
source = "path:boards/rpi-zero-w.board.toml"

[fallback.esp]
outputs = ["esp.led"]
timeout = "2s"
value = false
configured = false
verified = "2026-09-25T15:02:11Z"

[discovered.esp]
firmware = "1"

[wcet]
"luz.pisca" = { p999 = "240us", max = "610us", samples = 100000, board = "rpi-zero-w", runtime = "1.0.0", app = "sha256:…", measured = "2026-09-25T15:10:40Z" }
```

In `joker.lab.lock`, the same task has `p999 = "7us"` and `board = "rpi3b-plus"`: the same app, some 35 times slower on an interpreted ARMv6 core. A WCET entry is only valid for the board, the app and the runtime it was measured on (`J0431`, ref/02 §2.1.13). That is why the WCET lives in each node's lock and not in the app, and part 11 will need this.

```sh
$ jk apply --env zero --node zero --confirm 5m
$ jk config set nodes.zero.address zero.local:7443     # confirm through the new path, as in part 2
$ jk confirm --node zero
$ jk read pi.act_led 'esp.*' --watch --node zero
```

The Zero's `ACT` LED blinks according to the Zero's CPU temperature; the ESP32's `BOOT` turns it on and off; the touch changes the rhythm; the ESP32's LED shows the state. All over Wi-Fi. Repeat the fault from part 5, now seen from the Zero: pull the ESP32's cable from the Pi 3 (it loses power and Wi-Fi) and watch `esp_offline` on zero and the `ACT` keep blinking.

## 9. What changed between the two boards

```sh
$ jk diff --env zero --against joker.lab.toml
```

```
effective manifest  joker.zero.toml (sha256:9d0e77b2…)  versus  joker.lab.toml (sha256:3f9a41c0…)

 [target]
-board = "rpi3b-plus@1"
+board = "rpi-zero-w@1"

 [resources]
-rt = { cpus = [3] }
-io = { cpus = [2] }
-bg = { cpus = [0, 1] }
+rt = { cpus = [0] }          # default (recommended of rpi-zero-w)
+io = { cpus = [0] }          # default
+bg = { cpus = [0] }          # default

-[buses.esp32]
-kind = "serial"
-port = "/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0"
-baud = 115200
-dtr  = "low"
-rts  = "low"
-…                            # parity, stop, data_bits, turnaround, duplex (defaults)

 [devices.esp]
-driver  = "modbus-rtu"
-bus     = "esp32"
+driver  = "modbus-tcp"
+address = "192.168.1.57:502"

same: [project], [devices.pi], [security.api], [signals] (2), [outputs] (2), [cycles.main],
      [calc] (2), [alarms] (2), [telemetry.record] (2), [apps.luz] (luz 0.4.1, the same binds and params)
```

The difference between the two boards is **board, resources and transport**. The app, the equipment profile, the alarms, the `calc`s and the scenarios did not change. `sim/scenarios/falha_esp.scenario.toml` remains the test of both nodes, because it tests the site and not the board.

## 10. What happened underneath

| In this part | Where it is |
|---|---|
| linearization and merge of the overlays; `extends` relative to the file that declares it | ref/01 §7.1, §7.2, §7.5 |
| the effective manifest hash does not change when the files are rearranged | ref/01 §8.3 |
| board profile: `[board]`, peripherals, fixed IO with `fallback`, `recommended`; resolution in `boards/`, in the registry and among the built-in ones | ref/03 §2.3, §3 ("Resolution") |
| unknown `[target] board` `J0172` (in the build `J0827`); CPUs outside the board `J0144`; missing resource `J0828`; inconsistent board `J0829` | ref/01 §5.3, §5.4; ref/03 §2.9 |
| no RT and no free CPU core: `J0352`, the expected degraded behavior | document 23 §5.2; document 01 §22 |
| `modbus` family with two drivers (`modbus-rtu`, `modbus-tcp`) and the same profile | ref/03 §2.2.2; ref/01 appendix A |
| one arbiter per node; the site with several nodes is v2 | document 16 §1; document 21 §8 |
| one lock per input manifest, WCET per board, fallback captured by the node | ref/02 §2.1.1, §2.1.12, §2.1.13 |

## 11. Exercises

1. Change the `poll` of `esp` only on zero to `"200ms"`. In which file? `jk diff --against joker.lab.toml` shows one more line; did `joker.sim.toml` change?
2. On zero, run `stress-ng --cpu 1 --timeout 2m` and watch `jk status --node zero`: the jitter of the group `main` and the overruns. Compare with `stress-ng --cpu 4` on the Pi 3. What was `J0352` warning about?
3. Write `boards/rpi-zero-w.board.toml` in a version `1.1.0` that adds `[i2c]` after you turn I²C on with `raspi-config`. Is it minor or major? Does `joker.zero.lock` change by itself?
4. Connect both nodes to the ESP32 at the same time, for 30 s, and follow `jk decisions --output esp.led` on both. How many decisions do you see, and who controls the LED?

```sh
$ git add -A && git commit -qm "track(06): joker.rpi.toml, board rpi-zero-w and the ESP32 over Modbus TCP on node zero"
```

**State at the end of this part:** app `luz` 0.4.1, unchanged, running on zero (generation 1, context `zero.local:7443`); lab at generation 3 (the one from part 3, without `esp`). New: `joker.rpi.toml`, `joker.zero.toml`, `joker.zero.lock`, `boards/rpi-zero-w.board.toml`; `joker.lab.toml` now has `extends = ["joker.rpi.toml"]`, without changing the effective manifest. One client at a time: part 7 starts by restoring lab.

**Next:** [Part 7 — Classes: the same system at the equipment layer](07-classes.md).
