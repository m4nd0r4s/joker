# Part 9 — The ESP32 becomes a Joker Micro: Joker Link and the `safe` on the MCU

**Goal:** replace the ESP32's bench firmware with Joker Micro, connect it to the Pi 3 over Joker Link on the same USB cable, and move the envelope and the `safe` of the ESP32's LED into the MCU itself, without changing a line of the app `luz` 0.6.

**You will learn:** the manifest of a Joker Micro · the `gpio` and `esp32` drivers on an MCU · generic firmware and generation blob · `jk flash` over serial · the link key (`jk node enroll --link`) · how the Linux node names the signals of an MCU (catalog, aliases) · `J0234` · levels delegated to the link · OTA with A/B, confirmation window and resume · the most important exit gate of ADR 0020.

**Before you start:** parts 1–8 done (app `luz` 0.6, instances `led_esp`, `botao`, `toque` and `sinal`, the scenario `falha_esp` passing). The node `lab` enrolled and at the last confirmed generation of `joker.lab.toml`. The ESP32 connected to the Pi 3 by micro-USB, still with the Modbus bench firmware. The ESP32's micro-USB cable will go through the laptop once, to flash and enroll.

---

## 1. What changes, and what does not

Up to now the ESP32 was a **piece of equipment**: it served Modbus registers, and what decided the LED was the arbiter and the envelope of the Pi 3. The `safe` of the ESP32's LED depended on the fallback of the bench firmware itself (turns off after 2 s without requests), which `jk lock` checked against the profile.

From now on the ESP32 runs **the same core** as `jokerd`, built `no_std` for Xtensa (document 19, §2):

```
          part 5                                         part 9
 ┌─ Pi 3 (jokerd) ───────────────┐              ┌─ Pi 3 (jokerd) ──────────┐
 │ luz → arbiter → envelope ─────┼─ Modbus ─▶   │ luz ── PROPOSE (level,   │
 │        (esp.led, safe=false)  │   coil 0     │         lease) ──────────┼─ Joker Link ─▶
 └───────────────────────────────┘              └──────────────────────────┘   (same cable)
                          ESP32: bench firmware               ESP32: Joker Micro
                          fixed 2 s fallback                  arbiter → envelope → safe=false
```

- **Changes:** the ESP32's firmware, the device `esp` on the Pi 3 (from `modbus-rtu` to `joker-link`), and **who controls** the ESP32's LED: the MCU. `jokerd` becomes a remote writer, with a level and a lease (document 16, A12).
- **Does not change:** `luz` 0.6, `joker.toml`, the instances, `joker.sim.toml` and the scenarios. The paths `esp.led`, `esp.button`, `esp.touch`, `esp.rssi` and `esp.uptime` keep existing on the Pi 3 (section 8). It is the clearest before and after of ADR 0020 (document 23, §4).

## 2. The Xtensa toolchain

The original ESP32 is Xtensa LX6. Rust for Xtensa is not in mainline LLVM: it comes from Espressif, through `espup` (document 23, §5.1).

```sh
$ cargo install espup espflash
$ espup install                      # "esp" toolchain + LLVM with Xtensa
$ . ~/export-esp.sh                  # puts it on the PATH of this shell (add it to your profile)
$ rustup toolchain list | grep esp
esp
```

You already used `espflash` in part 4 to flash the bench firmware; now it is `jk` that calls it, because the board declares it.

## 3. The Joker Micro manifest

The MCU has its own manifest, in a separate directory: `micro/esp/joker.toml`. It is a `joker.toml` like the others, validated by the same `jk check` and **compiled** into a blob, because an MCU does not interpret TOML (ref/01 §5.22).

```toml
# micro/esp/joker.toml — the ESP32 as Joker Micro (part 9)
[project]
name  = "bancada-esp"
joker = "1.0"
mode  = "dev"

[target]
board = "esp32-devkit@1"      # already exists in the registry (document 23, §7): you do not write it

[clock]
sync = "link"                 # the clock comes from the Joker Link TIME messages

[buses.uart0]
kind = "serial"
port = "uart0"                # the board's UART0, through the USB bridge (gpio1/gpio3)
baud = 921600                 # the board's max_baud

[link]
bus  = "uart0"
peer = "lab"                  # only accepts the node lab
auth = "required"

# The LED and the button: the gpio driver, with the board's pins
[devices.io]
driver = "gpio"

[devices.io.signals]
led    = { line = "gpio2", direction = "output" }
button = { line = "gpio0", bias = "pull-up", active_low = true, debounce = "20ms" }

# Touch, RSSI and uptime: the esp32 driver (Joker Micro only)
[devices.chip]
driver = "esp32"

[devices.chip.signals]
touch  = { touch = 0, unit = "1", deadband = 5 }   # T0 = GPIO4; the deadband now happens on the MCU
rssi   = { rssi = true, unit = "1" }               # dBm; no unit in the catalog (logarithmic)
uptime = { uptime = true, unit = "s" }

[cycles.main]
period = "10ms"

# The LED's envelope and safe live here, and no longer on the Pi 3
[outputs."io.led"]
safe          = false
group         = "main"
remote_levels = ["manual:3", "manual:4", "manual:5", "control:12"]
```

Line by line, what is new:

- **`[target] board`** points to a board profile with `os = "none"`: `mcu_class = "m2"`, `fpu = "sp"`, `reset_safe = false`, the flash areas, `[probe] tool = "espflash"` and the `[capacities]` (256 signals, 64 outputs, 4 groups). `jk check` checks the generation against these capacities (`J0710`).
- **Two MCU drivers.** `gpio` is the same driver as on the Linux nodes; on an MCU, `line` is the name of the board's pin. `esp32` only exists in Joker Micro, on ESP32 boards: `touch = 0` is the T0 pad, `rssi = true` and `uptime = true` are the radio and the timer. The touch `deadband`, which in part 5 came from the profile and was applied on the Pi, is now applied in the MCU's acquisition: a variation below 5 does not even reach the wire.
- **`[outputs."io.led"]`**: the envelope, with the `safe`, belongs to the MCU. The 10 ms group `main` is the **owning group** of the output: it is at its latch that the Pi 3's proposals come in and that the leases expire.
- **`remote_levels`**: the levels of the MCU's arbiter that the link may occupy (16 A12, ref/07 §8.1). By default they would be all those that no local writer uses, that is, all 16, including `safety`. Here we delegate only what the Pi 3 needs: the `manual` band (for a `jk write`, which by default is `manual:4`) and `luz`'s `control:12`. Fewer delegated levels means fewer things a Linux node can do to this MCU.

## 4. The generic firmware

One firmware per configuration does not scale (document 19, §4.1). So you build one **generic firmware** per board and set of apps and drivers, and the site configuration goes in a separate **generation blob**, in flash.

```sh
$ cd micro/esp
$ jk build --target mcu:esp32-devkit --generic
```

```
compiling joker-micro 1.0.0 for xtensa-esp32-none-elf (toolchain esp)
  drivers   gpio, esp32, extern                 (features: drv-gpio, chip-esp; chip esp32)
  apps      none
  link      uart, udp, tcp · link-crypto · gen-flash · ota · persist · trace
✓ target/joker/firmware.bin    912 KiB   (firmware area: 1536 KiB)
✓ target/joker/firmware.jkfw   firmware descriptor · sha256:7f30…
```

`jk inspect target/joker/firmware.jkfw` shows the descriptor: board and `board_id`, blob format and ISA, capacities (with their hash), apps (none), drivers, transports, `fpu = "sp"`, flash areas and `security_version`. It is the **contract** between this firmware and the blobs it accepts: a blob made for other capacities is rejected before it uses up flash (ref/08 §2.14).

## 5. Flash over serial

`jk flash` is local: it talks to the board through the serial port of whoever runs it. Unplug the ESP32's micro-USB from the Pi 3 and plug it into the laptop, as in part 4.

```sh
$ jk flash
```

```
board esp32-devkit@1 · [probe] tool = espflash, protocol = serial
port /dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0 (detected)
  bootloader  0x001000   esp-idf, 2 slots
  firmware    0x010000   912 KiB   ✓ verified
  gen_a/gen_b empty: the MCU boots in maintenance mode
```

Without a generation, the MCU is in **maintenance mode** (ref/08 §2.12.3): pins in the reset state, no signals, and the link comes up with the board's default configuration (`uart0` at 115,200 baud), only for `HELLO`, `OTA`, time and catalog. The ESP32's LED stays as the reset leaves it.

## 6. The link key

Joker Link is encrypted with ChaCha20-Poly1305 and one key per MCU, the `K_link`, which is born at enrollment (ref/07 §4.7). With the ESP32 still on the laptop:

```sh
$ jk node enroll esp --link esp --node lab --ca ~/org-ca
```

```
→ lab (dev) · from --node · lab.local:7443
MCU in maintenance on port …CP2102…-port0 · board esp32-devkit@1 · hardware id 240ac45e913c
  K_link generated (256 bits) · kid 9a1f3c07d2b84e61
  MCU: IDENTITY area, record 16 (name "esp") and record 17 (link key "esp")
  MCU: record 19 (X25519 secrets key) · certificate CN=esp from CA bancada, with that key in the SAN
  secrets.toml.enc: + recipient node:esp (no entries yet; part 10)
  node lab: secret secret:link.esp (encrypted for the node)
enrolled. node lab recognizes the MCU "esp" on the joker-link device "esp"
```

The first `esp` is the **name of the MCU** (it goes in the `HELLO`); `--link esp` is the **device** of the Linux node that will use it. `--ca` is what gives the MCU an X25519 key attested by your CA: without it, the link would work, but no secret (the Wi-Fi password, in part 10) could be encrypted for the MCU (ref/02 §2.4.8). The key never appears in logs, in the API or in `jk doctor`; the `kid` may appear (ref/07 §4.7). Plug the ESP32 back into the Pi 3.

## 7. The MCU generation

```sh
$ jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw && cd ../..
```

```
✓ micro/esp/joker.toml     2 devices (gpio, esp32), 5 signals, 1 output, 1 group, 1 link
✓ capacities               5/256 signals · 1/64 outputs · 1/4 groups (J0710: fits)
warning[W0786]: board without safe outputs on reset (`reset_safe = false`); acceptable with `accept_unsafe_reset`: esp32-devkit
  = note: until the firmware configures the pins, GPIO2 is an input with the internal pull-down of a strapping pin (the LED stays off), but the board does not guarantee this for the other pins; on the bench there is no load
warning[W0788]: generation in flash not bound to a node (`node`)
warning[W0779]: unsigned generation blob
  = help: all three are errors in `mode = "prod"`; part 11 deals with signatures
✓ target/joker/bancada-esp.jkgen   6.1 KiB · gen 1 · generation sha256:5d21… · catalog sha256:3b9e…
```

The blob has the core's tables just as it uses them in memory (signal registry, envelopes, arbiters, expression bytecode) and the Joker Link **catalog**: the list of what the MCU exports, with a hash (ref/08 §2.13; ref/07 §6.3). The MCU uses it in place, without copying it.

## 8. What the paths look like on the Pi 3

This is the design decision of the part. The Linux node does not write the profile of an MCU: it generates it from the catalog (`joker/link-3b9e…`, ref/03 §2.2.16), and the `joker-link` driver puts each exported signal at **`<device>.<path on the MCU>`** (ref/07 §8.3). On the MCU, the paths are `io.led`, `io.button`, `chip.touch`, `chip.rssi` and `chip.uptime`, because every signal of a device has the device's prefix (ref/01 §3.8). So, on the Pi 3, the canonical paths become `esp.io.led`, `esp.chip.touch`, and so on.

Three ways to keep `esp.led`:

| Option | Where | Why not, or why |
|---|---|---|
| aliases on the MCU | `micro/esp/joker.toml` | does not work: the catalog only carries the canonical path of each signal (key 1.2), and the MCU's aliases do not leave it |
| change the `bind`s and the instances to `esp.io.led` | `joker.lab-link.toml` | works, but rewrites `luz`, the instances and the alarms in an overlay, and `joker.sim.toml` would end up with other paths |
| **aliases on the Linux node** | `joker.lab-link.toml`, in `[devices.esp.signals]` | **chosen**: an alias is a second path to the same `SignalId` (document 16, P1), and it is an adjustment allowed on a profile signal (ref/01 §5.8.1). `joker.toml`, the apps and the instances keep using `esp.led` |

`esp.led_rb` and `esp.fw` disappear, and they are not missed: the **applied** value comes in the sample of `esp.led` itself (what applies it is the MCU), and the firmware version comes in the `HELLO` (section 10). If some manifest referred to them, `jk check` would give `J0101`, with the suggestion.

## 9. The `joker.lab-link.toml`

```toml
# joker.lab-link.toml — the ESP32 as Joker Micro over Joker Link (part 9)
extends = ["joker.lab.toml"]

[buses.esp32]
baud = 921600                     # the same cable, faster

[devices.esp]
driver  = "joker-link"
micro   = "micro/esp/joker.toml"  # the MCU manifest that this device runs
unit_id = { unset = true }
profile = { unset = true }        # the profile is generated from the catalog
auth    = "required"
node    = "esp"                   # the name the HELLO must carry
report  = "20ms"
refresh = "1s"

[devices.esp.signals]
"io.led"      = { alias = "esp.led" }
"io.button"   = { alias = "esp.button" }
"chip.touch"  = { alias = "esp.touch" }
"chip.rssi"   = { alias = "esp.rssi" }
"chip.uptime" = { alias = "esp.uptime" }

[apps.luz]
writer = { lease = "100ms" }      # slack for one renewal lost over the link (see below)
```

One thing is missing on purpose. Run `jk check`:

```sh
$ jk check --env lab-link
```

```
error[J0234]: envelope on a path that is not an output of this node (or governed by a Joker Micro)
 --> joker.toml:14:1
   |
14 | [outputs."esp.led"]
   | ^^^^^^^^^^^^^^^^^^^ `esp.led` is an alias of `esp.io.led`, an output of the Joker Micro `esp`
   = note: the outputs of a joker-link device are governed by the MCU's envelope
           (micro/esp/joker.toml:44, safe = false); the Linux node does not arbitrate them (ref/07 §8.1)
   = help: in joker.lab-link.toml, `[outputs]` with `"esp.led" = { unset = true }`
```

Two envelopes for the same output, one on each side of the cable, would be two truths. Joker refuses. Add to `joker.lab-link.toml`:

```toml
[outputs]
"esp.led" = { unset = true }      # the envelope of this output now lives on the MCU
```

It is not a widening that needs `widen`: the envelope does not disappear, it changes owner, and the MCU's has the same `safe = false`.

```sh
$ jk check --env lab-link
```

```
✓ manifest           2 devices (sysfs, joker-link) · 1 bus (serial 921600)
✓ esp (joker-link)   micro/esp/joker.toml · catalog sha256:3b9e… · 5 signals, 1 output
✓ aliases            esp.led → esp.io.led · esp.button → esp.io.button · … (5)
✓ bindings           led_esp (std/led@1) → esp.led · writer luz control:12 (delegated to the link)
✓ liveness           lease 100 ms ≥ 54.7 ms = 2 × 20 ms (renewal) + 10 ms (MCU group main)
                     + 3.1 ms (256 B block at 921,600 baud) + 0.6 ms (PROPOSE) + 1 ms   (J0730)
✓ link traffic       D→H 7% · H→D 4%   (J0732 above 70%)
```

Two checks that only exist because there is a link. **Delegated levels:** `luz` writes at `control:12`, which the MCU delegates; with `luz` at `optimizer`, it would be `J0731`. **The liveness condition** (ref/07 §11.6): the lease must survive one lost renewal, the period of the owning group on the MCU, an OTA block midway in the same direction and the time on the wire. The default lease of a Linux writer on an MCU output is 3 × the period of the writer's Linux group (60 ms), capped by the link's `max_lease` (ref/07 §8.1): it would pass by a small margin; with 100 ms there is slack. The cost: the `safe` arrives, in the worst case, 100 ms after the Pi 3 dies. The instance `sinal`, on the Pi, also gets 100 ms leases, because an app has a single writer (ref/01 §5.12).

## 10. Apply: the node takes the blob to the MCU

```sh
$ jk apply --env lab-link --node lab --confirm 5m
```

```
→ lab (dev) · from --node · lab.local:7443
plan of generation 10
  ~ bus esp32: 115200 → 921600 baud
  ~ device esp: modbus-rtu → joker-link (auth required, kid 9a1f…, catalog sha256:3b9e…)
  - envelope esp.led (moves to the MCU esp)
  ~ app luz: lease 60 ms → 100 ms (reloads, without restarting)
  + MCU esp: generation 1 (bancada-esp.jkgen, 6.1 KiB) over the link · area gen_a · window 5 min
applied. confirmation pending: `jk confirm` within 5 min, or automatic rollback
```

What happens on the cable: `jokerd` opens the port at 921,600 baud; the MCU, in maintenance, offers a `HELLO` encrypted with the `K_link`; the node answers `maintenance` (there is no valid generation); the node sends the blob through `OTA`; the MCU verifies it **before** erasing the area, writes it to `gen_a`, activates it at a tick boundary and closes the session (`BYE generation_change`); new `HELLO`, now with a generation, and the node answers `full`.

```sh
$ jk status --node lab
```

```
devices
  pi    sysfs        up
  esp   joker-link   up · session full · Joker Link 1.0 · serial 921600 · encrypted (kid 9a1f…)
        MCU esp · esp32-devkit@1 · firmware 1.0.0 (slot A) · boots 3 (last: power_on)
        generation 1 (flash, gen_a, confirming, 4 min 31 s left) · clock synced (TIME, δ 0.4 ms)
        frames rx 1,882 · tx 1,907 · COBS/CRC 2 · authentication 0 · replay 0 · retransmissions 0
        RTT 1.6 ms · lease_expired 0
```

The two framing errors are the garbage from the ESP32's boot ROM at 115,200 baud, which COBS discards until the first `0x00` (document 23, §2). They are in counters separate from the authentication ones, and it is that separation that lets you tell noise from attack (ref/07 §3.1). The MCU's own counters are also signals: `jk read 'joker.device.esp.**'`.

The ESP32's LED shows `ligado` again, and `BOOT` turns it on and off as in part 4. `jk confirm --node lab` confirms the node's generation 10 and, over the link, the MCU's generation 1 (`OTA confirm`).

Pin in the lock what the `HELLO` said, with `jk lock --env lab-link --node lab`. `[mcu.esp]` keeps the target (`mcu:esp32-devkit`), the mode (`generation`), the Joker Micro and Joker Link versions, the features, the capacities and the digest of the generic firmware; `[discovered.esp]` keeps the hardware id (`240ac45e913c`), the firmware version and, in `digest`, the catalog hash (ref/02 §2.1.10–§2.1.11). The lock never pins the digest of the compiled generation, which is a function of the lock itself. `joker.lab-link.lock` no longer has the profile `local/bench-esp32@1` or `[fallback.esp]`: those stay in `joker.lab.lock`, because each input manifest has its own lock. In `prod`, an MCU that does not match this stays in `observe`: the node reads, does not write, and raises the drift alarm (ref/07 §7.3, `J-E2202`).

## 11. The proofs

**(a) Kill `jokerd`.** With the system on (the ESP32's LED on):

```sh
lab$ sudo systemctl kill -s KILL jokerd
```

The ESP32's LED **goes off in about 100 ms**. Nobody told it anything: `luz`'s last `PROPOSE` was consumed by the MCU in a tick, the 100 ms lease counted from that tick (A2: the link latency does not eat into it), expired at the latch of the MCU's group `main`, and the MCU's arbiter, with no active level, applied the `safe` (A6). The link is only declared lost 200 ms later (`link_timeout`), and that does not interfere with the output (ref/07 §7.4).

Compare with part 2 (c): there, the `ACT` LED stayed on, because `sysfs` has no fallback. And with part 5: there, the bench firmware turned the LED off after 2 s, through a rule fixed in the firmware that Joker could only check. **Here the `safe` is the design itself**, and it is the same rule that protects an output on Linux. It is the most important exit gate of ADR 0020 (document 23, §5.3).

systemd restarts `jokerd` within 1–2 s. The node sends `HELLO(request)`, the MCU obeys after 2 × `keepalive` of silence, the session comes up in a few milliseconds, and `luz` proposes again. `jk status` shows `boots 3`: the MCU did not restart, only the session is new. The LED lights up again: `luz` started again, with the initial state, `ligado = true`.

**(b) A new generation over OTA.** Change the touch `deadband` on the MCU from 5 to 3 and build and apply again:

```sh
$ (cd micro/esp && jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw)
$ jk apply --env lab-link --node lab --confirm 5m
```

```
plan of generation 11
  ~ MCU esp: generation 1 → 2 (sha256:5d21… → sha256:a07c…) · 6.1 KiB over the link · area gen_b
      ~ chip.touch.deadband 5 → 3
    hot swap at a tick boundary; confirmation window 5 min, counted on the MCU
  no changes on the node
```

The new blob goes to the **other** area (`gen_b`), while the MCU keeps running generation 1 in `gen_a`. Once verified, it swaps at a tick boundary: the LED keeps its applied value, the leases and the trips (A8), and the next `HELLO` shows generation 2 on trial. `jk confirm --node lab` confirms both.

Now repeat with `deadband = 8` and **do not confirm**. After 5 min:

```
esp   … generation 3 rolled_back → generation 2 (gen_b) · J-E2409: OTA: the update was not confirmed within the window; rollback
```

The MCU went back by itself, without anybody telling it anything. The window counts **on the MCU**, because the Linux node may be the cause of the problem (ref/07 §9.4; ADR 0015). The node also goes back to generation 11. Put `deadband = 3` back in the file.

**(c) Unplug the cable in the middle of an OTA.** The blob is too small to catch midway (less than 0.1 s), but the firmware is not. And there is a good reason for a new firmware: the 16 KiB trace ring keeps only a few seconds, and you want twice that. Capacities belong to the **firmware**, and not to the generation: add to `micro/esp/joker.toml` a table `[target.capacity]` with `trace_bytes = "32 KiB"`, and rebuild both, in the right order:

```sh
$ cd micro/esp && jk build --target mcu:esp32-devkit --generic
$ jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw && cd ../..
$ jk apply --env lab-link --node lab --confirm 5m
```

The new blob carries the capacities hash of the new firmware; sent to the old firmware, it would be rejected with `J-E2410`. The plan shows the firmware first (912 KiB, with a restart) and then the generation. When the bar passes 40%, **unplug the ESP32's micro-USB**:

```
MCU esp: OTA firmware   ███████░░░░░░░░░░░   372 / 912 KiB
MCU esp: link lost (J-E2205) · the transfer stays saved on the MCU
MCU esp: waiting for HELLO …
                        (the ESP32 lost power: it is the same cable. Plug it back in.)
MCU esp: new session · boots 3 → 4 (power_on) · firmware 1.0.0 (slot A), unchanged
MCU esp: OTA begin with the same manifest → ready at 376,832 B (last complete sector)
MCU esp: OTA firmware   ██████████████████   912 / 912 KiB · verified
MCU esp: activate → restart (BYE ota_activate) · new firmware on trial (slot B)
MCU esp: OTA generation 4 → gen_a · activate · session full
```

The MCU kept in `PERSIST` the pair (digest, confirmed offset), rounded down to the last sector, and the `begin` with the same manifest resumed from there (ref/07 §9.3). While the old firmware ran without a link, the LED was in `safe`; during the activation restart, it was in the hardware reset state, which on this board is not guaranteed (the `W0786` of section 7). The new firmware only marks itself as good after clean ticks and the `HELLO`; `jk confirm --node lab` closes both trials.

## 12. Part 5 versus part 9

| | Part 5 (Modbus RTU) | Part 9 (Joker Link) |
|---|---|---|
| who arbitrates and applies the LED's envelope | `jokerd` on the Pi 3 | the MCU |
| `safe` when the Pi dies | fixed firmware fallback, 2 s, checked by `jk lock` (`J0205`) | the `safe` of the MCU's envelope, at the end of the lease (≈ 100 ms) |
| reads | *polling* at 100 ms, the Pi asks | the MCU sends the changes (`report` 20 ms) and a `refresh` every 1 s |
| quality | made on the Pi (timeout, CRC) | made on the MCU, and carried as is; `Bad(CommFailure)` on the Pi if the link drops |
| write confirmation | `led_rb` (coil read back) | the applied value in the sample of `esp.led` and the `OUTCOMES` |
| security on the cable | none | ChaCha20-Poly1305, anti-replay, `K_link` per MCU |
| changing the ESP32's behavior | reflash the firmware by hand | a generation over OTA, with A/B and confirmation |

And in the simulator nothing changed, because `joker.sim.toml` extends `joker.toml`, and not `joker.lab-link.toml`: `jk test --env sim sim/scenarios/falha_esp.scenario.toml` keeps giving `ok` with `luz` 0.6.0. The behavior seen from outside has been the same since part 5; what changed is who guarantees it.

## 13. What happened underneath

- **One core, two hosts.** The MCU's arbiter and envelope are the same `joker-core` as in `jokerd`, `no_std`, with no allocation in the tick, and with the same conformance suite, byte for byte (document 19, §2; ADR 0020).
- **The firmware says what it can do, the generation says what to do.** The `.jkfw` descriptor and the `.jkgen` blob, verified at boot and on every OTA: format, integrity, capacities, anti-rollback (ref/08 §2.12–§2.14).
- **The `HELLO` decides the mode** (`maintenance`, `observe`, `full`), and the catalog travels by hash (ref/07 §6.3, §7.3).
- **Remote writers.** Each block of a `PROPOSE` is a writer (link, level) of the MCU; the lease counts from the MCU tick that consumes it; the results come back in a reliable `OUTCOMES`, because they are decisions (16 A12; ref/07 §8).
- **OTA without fear.** COSE manifest before the image, resume from a persisted offset, confirmation counted on the MCU (ref/07 §9).

## 14. Exercises

1. Change `luz` to `writer = { level = "optimizer", lease = "100ms" }` in `joker.lab-link.toml`. Which code does `jk check` give, and why? Where is it fixed: on the Pi or on the MCU?
2. Lower `luz`'s lease to `"40ms"`. Read `J0730` and redo the calculation of section 9 by hand. What happens to the calculation if you raise the link's `bulk_max` to 1024?
3. Do a `jk write esp.led true --ttl 30s --reason "…" --node lab` and kill `jokerd` midway. How long does the LED stay on? Relate the answer to the link's `max_lease` (ref/07 §7.4, A10).
4. Remove the `manual` band from `remote_levels` and repeat the `jk write`. What result appears, and where do you see it?

```sh
$ git add -A && git commit -qm "track(09): the ESP32 as Joker Micro over Joker Link; safe on the MCU"
```

**State at the end of this part:** app `luz` **0.6, unchanged**. New files: `micro/esp/joker.toml`, `joker.lab-link.toml`, `joker.lab-link.lock`, and `secrets.toml.enc`, for now with only the recipient `node:esp`. `micro/esp/target/` stays out of Git. Outside the project: the `esp` toolchain; the ESP32 with Joker Micro (firmware with a 32 KiB trace, generation 4) and enrolled on the node `lab` (`secret:link.esp`); the node `lab` at generation 13 (the one of `joker.lab-link.toml` with the new firmware), confirmed.

**Next:** [Part 10 — Logic on the MCU: a native app and the link over Wi-Fi](10-logic-on-the-mcu.md).
