# Part 4 — The ESP32 as Modbus equipment: writing the profile, the button and the LED

**Goal:** get the ESP32 to act as a piece of Modbus RTU equipment connected to the Pi 3, write its profile from the register map, and use the `BOOT` button to turn the system on and off, with the ESP32's LED showing the state.

**You will learn:** Modbus RTU over a USB-serial bridge · equipment profiles, from the CSV to the reviewed file · `jk profile import`, `check` and `test` · readback and `safe_hint` · equipment fallback (M1) · edges and debounce · bus budget · `jk lock --node`.

**Before you start:** part 3 done (`luz` 0.2 blinking with the temperature). The ESP32 connected by micro-USB to a USB port of the Pi 3. On the laptop, the Rust toolchain for Xtensa (`espup`) and `espflash`.

---

## 1. What the ESP32 will be

In this part the ESP32 does not run Joker yet (that is part 9). It runs the **bench firmware** of document 23, §4: a small piece of Modbus equipment, like a meter or a relay with communication, with this register table:

| Register | FC | Type | Signal |
|---|---|---|---|
| coil 0 | 1 (read), 5 (write) | `bool` | board LED (GPIO2) |
| discrete input 0 | 2 | `bool` | `BOOT` button (GPIO0), already inverted: 1 = pressed |
| input register 0 | 4 | `u16` | reading of the T0 touch sensor (GPIO4) |
| input register 1 | 4 | `i16` | Wi-Fi RSSI in dBm (0 without Wi-Fi) |
| input registers 2–3 | 4 | `u32` | seconds since boot |
| holding register 0 | 3 | `u16` | version of the bench firmware |

And a behavior that any serious piece of equipment has: **with no Modbus request for 2 s, it turns the LED off by itself.** It is the equipment fallback (M1, document 01, §8), and it is the lesson of this part.

The board's USB-serial bridge (CP2102 or CH340) is a real serial port for the Pi. No RS-485 transceiver or wires are needed: the micro-USB cable provides power and the bus (document 23, §2).

## 2. Flash the firmware

The firmware is in `tools/bench/esp32-modbus/` in the Joker repository. It serves Modbus RTU on UART0 (the USB) and Modbus TCP on Wi-Fi (port 502), with the same table; Wi-Fi will only be needed in part 6, but you flash everything at once now. The credentials go into the build through environment variables and never go into Git:

```sh
$ git clone https://git.example.com/joker/joker.git && cd joker/tools/bench/esp32-modbus
$ export BENCH_WIFI_SSID="bancada"
$ read -rs BENCH_WIFI_PASSWORD && export BENCH_WIFI_PASSWORD      # no echo, out of the history
$ cargo build --release
$ espflash flash --monitor target/xtensa-esp32-none-elf/release/esp32-modbus
```

```
esp32-modbus 1.0.0 · Modbus RTU on UART0, 115200 8N1, unit 1
wifi: connected to "bancada", IP 192.168.1.57 · Modbus TCP on :502
fallback: LED turns off without requests for 2 s
uart0: from here on, Modbus only
```

The last line matters: UART0 is the console and also the bus, and after boot the firmware stops writing to it. Write down the IP (part 6). The binary has the Wi-Fi password inside it: do not share it or keep it in the project.

## 3. The ESP32 seen from the Pi

Connected to the Pi, the ESP32 appears at `/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0` (`lab$ ls /dev/serial/by-id/`). `/dev/ttyUSB0` also exists, but the number changes if you plug in another adapter. Always use the `by-id` path (on a board with a CH340 it is `usb-1a86_USB_Serial-if00-port0`). The `jokerd` user must be able to open the port (group `dialout`); `jk doctor --node lab` checks it.

## 4. The profile: from the register map to the file

Modbus does not describe itself: what each register means is in the datasheet, and in Joker that datasheet becomes a **profile** (ref/03 §2.2). Writing the profile of a new piece of equipment is the most common job of an integrator, and it is done in three steps: import, review, test.

### 4.1 From the CSV to the draft

The table from §1, copied into a spreadsheet and exported to `mapa-esp32.csv`, at the project root. The header is required and fixed, and empty columns take the default:

```
signal,fc,at,type,direction,unit,scale,offset,label,description
led,5,0,bool,output,,,,,
led_rb,1,0,bool,,,,,,
button,2,0,bool,,,,,BOOT,
touch,4,0,u16,,,,,,raw value of the T0 touch sensor (GPIO4); drops when touched
rssi,4,1,i16,,,,,,Wi-Fi RSSI in dBm; 0 without connection
uptime,4,2,u32,,s,,,,
fw,3,0,u16,param,,,,,
```

- `led` is coil 0 **written** with FC 5; `led_rb` is the same coil **read** with FC 1. A profile separates the write from the readback: it is the only case of two points at the same address (ref/03 §2.2.5).
- `at` is the protocol address, 0-based. A datasheet that says "30001" means input register 0.

```sh
$ jk profile import csv mapa-esp32.csv --name local/bench-esp32 --driver modbus --out profiles/local/bench-esp32@1.profile.toml
```

```
draft written to profiles/local/bench-esp32@1.profile.toml: 7 signals (1 output, 5 inputs, 1 param)
review: [profile] description · [defaults] poll · output `led` without readback or safe_hint ·
        no [fallback] · deadband, limits and monotonic do not come from the CSV
```

`local/` is the space for the project's profiles, which are never published (ref/03 §2.1.2). The importer only knows what the table says. The rest is knowledge about the equipment, and it is yours.

Notice the name: `<model>@<major>.profile.toml`. It is the same rule as `joker.sim.toml` and `sim/pi.sim.toml` in part 1: the suffix states the type, so the editor opens the file already with the profile schema (completion of the `[signals]` keys, errors while you type), and `jk check` rejects with `J0803` a `.profile.toml` that does not start with `[profile]`. The manifest keeps referring to it without the suffix, `profile = "local/bench-esp32@1"`: resolution adds it (ADR 0026).

### 4.2 Review by hand

What you add to the draft, and why:

| Where | What | Why |
|---|---|---|
| `[profile]` | `description`, `license` | for whoever reads the file a year from now |
| `[profile]` | `identity = { firmware = "fw" }` | this Modbus does not describe itself: the register `fw` becomes the equipment's firmware version, and the lock keeps it |
| `[defaults]` | `poll = "100ms"` | a button read every 1 s (the default) is no good; 100 ms is the rhythm of a person |
| `led` | `readback = "led_rb"` | suggests the output's `verify` to the manifest (`jk check` shows it) |
| `led` | `safe_hint = false` | a suggestion for the `safe`, shown next to `J0201`; **it is never applied** (ADR 0006) |
| `touch` | `deadband = 5` | touch is noisy: variations of ±5 counts are not signal |
| `uptime` | `monotonic = true` | it is a counter: the historian aggregates it by the last value, not by the average |
| `[fallback]` | `timeout = "2s"`, `value = false` | what the firmware does by itself; Joker compares it with the `safe` |

The final profile:

```toml
# profiles/local/bench-esp32@1.profile.toml
schema = 1

[profile]
name        = "local/bench-esp32"
version     = "1.0.0"
driver      = "modbus"
description = "ESP32 bench firmware (document 23, §4): LED, button, touch, RSSI"
license     = "Apache-2.0"
identity    = { firmware = "fw" }

[defaults]
poll = "100ms"

[signals]
"led"     = { at = 0, fc = 5, type = "bool", direction = "output", readback = "led_rb", safe_hint = false }
"led_rb"  = { at = 0, fc = 1, type = "bool" }
"button"  = { at = 0, fc = 2, type = "bool", label = "BOOT" }
"touch"   = { at = 0, fc = 4, type = "u16", deadband = 5, description = "raw value of the T0 touch sensor (GPIO4); drops when touched" }
"rssi"    = { at = 1, fc = 4, type = "i16", description = "Wi-Fi RSSI in dBm; 0 without connection" }
"uptime"  = { at = 2, fc = 4, type = "u32", unit = "s", monotonic = true }
"fw"      = { at = 0, fc = 3, type = "u16", direction = "param" }

[fallback]
timeout = "2s"
value   = false
```

It is, signal by signal, the profile of document 23, §4, with the project's name. Notice the resulting types (ref/03 §2.2.7): `touch` and `fw` become `u64`, `rssi` becomes `i64`, and `uptime`, because it has a unit, becomes `f64` in seconds. `rssi` is logarithmic and takes no unit: the description says it is dBm. `fw` is a `param`, so it is read only on request (`jk read esp.fw --refresh`), and not on every poll. Without `applies_to`, `[fallback]` covers the profile's outputs, and here there is only one.

### 4.3 `jk profile check`

A classic mistake when reviewing by hand is copying the read FC to the output (`"led" = { at = 0, fc = 1, …, direction = "output" }`):

```
error[J0810]: invalid address or function code for the driver or for the direction
  --> profiles/local/bench-esp32@1.profile.toml:14:13
   |
14 | "led"     = { at = 0, fc = 1, type = "bool", direction = "output", … }
   |                       ^^^^^^ FC 1 reads coils; an output is written with FC 5 (one coil) or 15 (several)
   = help: fc = 5; to read the value back, an input point on the same coil with fc = 1 (`led_rb`)
```

With `fc = 5`:

```sh
$ jk profile check profiles/local/bench-esp32@1.profile.toml
```

```
✓ schema           7 signals: 1 output, 5 inputs, 1 param; [fallback] over `led`; identity.firmware = fw
✓ addresses        coil 0 (write and readback) · discrete 0 · input 0–3 · holding 0; no overlaps
✓ types            touch u16→u64 · rssi i16→i64 · uptime u32→f64 s · fw u16→u64
✓ requests (plan)  per poll: FC1 × 1 bit, FC2 × 1 bit, FC4 × 4 registers · on request: FC3 × 1 register
```

`check` does not need the equipment: it validates the schema, the units and the `plan` of the Modbus driver, which is pure (ref/03 §3).

### 4.4 `jk profile test` on the hardware

Now against the real ESP32, through the Pi, with reads only:

```sh
$ jk profile test profiles/local/bench-esp32@1.profile.toml --node lab \
     --uri "modbus-rtu:/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0?baud=115200&unit=1"
```

```
signal   register   raw      value     quality    latency
led_rb   coil[0]    0        false     good       4 ms
button   di[0]      0        false     good       4 ms
touch    in[0]      61       61        good       5 ms
rssi     in[1]      0xFFCA   -54       good       (same request)
uptime   in[2..3]   1204     1204 s    good       (same request)
fw       hr[0]      1        1         good       4 ms
led      coil[0]    —        (output: a test does not write)
3 requests per poll + 1 on request · measured latency 4–5 ms (the profile assumes the default, 10 ms)
```

Run it again with `BOOT` pressed (`button` at `true`) and again with a finger on pin GPIO4 (`touch` drops to ~15). The three numbers match the world, and the profile is right.

## 5. The ESP32 in the simulator

A simulation model can reuse the signals of a profile (`[model] profile = "local/bench-esp32@1"`, ref/03 §2.4). Here they are repeated on purpose, with the same names and the types the core sees, because the model needs to say more than the profile: the panel, the noise, the bounce and the dynamics.

```toml
# sim/esp.sim.toml
schema = 1

[model]
description = "ESP32 with the bench firmware (document 23, §4)"

[signals]
led    = { type = "bool", direction = "output", panel = { label = "ESP32 LED", kind = "lamp" } }
led_rb = { type = "bool" }
button = { type = "bool", init = false, bounce = "5ms", panel = { label = "BOOT", kind = "momentary", key = "b" } }
touch  = { type = "u64", init = 60, noise = 1, resolution = 1,
           panel = { label = "Touch", kind = "slider", keys = ["down", "up"], step = 5, min = 0, max = 100 } }
rssi   = { type = "i64", gen = { kind = "noise", mean = -55, std = 3 } }
uptime = { type = "f64", unit = "s", init = 0 }
fw     = { type = "u64", direction = "param", init = 1 }

[dynamics]
led_rb = "led"                     # the readback follows the applied value
uptime = "integrate(1)"            # 1 s per second

[faults]
panel = { key = "d", disconnect = true }   # "d" simulates the ESP32 disconnected (part 5)
```

```toml
# joker.sim.toml (add)
[devices.esp]
driver = "sim"
model  = "sim/esp.sim.toml"
```

`bounce = "5ms"` puts the bounce of a real button on the laptop, so that the bug shows up here and not only on the bench.

## 6. The common manifest

```toml
# joker.toml (add)
[outputs."esp.led"]
safe = false

[signals."esp.button"]
debounce = "20ms"

[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led", temp = "calc.temp_media", botao = "esp.button", led_estado = "esp.led" }
params = { half_period = "500 ms", t_frio = "45 °C", t_quente = "70 °C" }
cycles = { pisca = "main" }
```

Forget `[outputs."esp.led"]` and `jk check` reminds you, with the profile's suggestion:

```
error[J0201]: output without `safe`
  --> joker.toml:26:1
   = note: `esp.led` is an output of the profile local/bench-esp32@1
   = help: the profile suggests `safe = false` (safe_hint); write in [outputs."esp.led"] the value that is safe on this site
```

The `debounce` stays in the common part, so that both worlds behave the same. The Modbus driver does not debounce, so it is the acquisition stage that does it (ref/06 §2.4): on a `bool` read by poll, the value only changes after consecutive equal readings that cover at least the `debounce` (ref/01, key `debounce`; document 16). In the simulator, it erases the 5 ms bounce; on the hardware, with a 100 ms poll, it is two equal readings in a row, and an isolated reading in the middle of a bounce does not get through. The cost is one reading of delay: ~100 ms between pressing and the system seeing it.

## 7. The app, version 0.3

Rule: each rising edge of the button turns the system on or off; when off, the `ACT` stays off; the ESP32's LED shows whether it is on.

```rust
// apps/luz/src/lib.rs — what changes compared with 0.2
#[joker::app(name = "luz", version = "0.3.0")]
pub struct Luz {
    #[output] led: Output<bool>,
    #[input(unit = "°C")] temp: Input<f64>,
    #[input] botao: Input<bool>,
    #[output] led_estado: Output<bool>,
    // half_period, t_frio, t_quente: as in 0.2
}

#[derive(Default)]
struct Pisca {
    elapsed_ms: f64,
    on: bool,
    ligado: bool,
    premido_antes: bool,
}

fn arranque(_io: &CycleIo<Luz>) -> Pisca {
    Pisca { ligado: true, ..Default::default() }     // starts on, like 0.2
}

#[joker::cycle(period = "20ms", wcet = "5us", init = arranque)]
fn pisca(io: &mut CycleIo<Luz>, st: &mut Pisca) -> Result<()> {
    // 1. Both inputs before any proposal: without them, nothing is proposed in this tick.
    let t = io.temp.get()?;
    let premido = io.botao.get()?;

    // 2. Rising edge: pressed now, released in the previous tick.
    if premido && !st.premido_antes {
        st.ligado = !st.ligado;
        st.elapsed_ms = 0.0;
        st.on = st.ligado;                       // when turning on, light up at once: immediate feedback
    }
    st.premido_antes = premido;
    io.led_estado.set(st.ligado);

    if !st.ligado {
        io.led.set(false);
        return Ok(());
    }
    // 3. The blink from 0.2, unchanged.
    let (frio, quente) = (io.t_frio.get(), io.t_quente.get());
    let x = ((t - frio) / (quente - frio)).clamp(0.0, 1.0);
    let meio = io.half_period.get() + x * (RAPIDO_MS - io.half_period.get());
    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;
    if st.elapsed_ms >= meio {
        st.elapsed_ms -= meio;
        if st.elapsed_ms > meio {
            st.elapsed_ms = 0.0;
        }
        st.on = !st.on;
    }
    io.led.set(st.on);
    Ok(())
}
```

- **The edge lives in the task state** (`premido_antes`), because `ligado` is the app's memory. The manifest also has `rose(cond)` in expressions, and it serves alarms and permissives (document 02, §10), but what a press does is decided by the app.
- **It starts on** (`init = arranque`), so that `pisca.scenario.toml` and `aquece.scenario.toml` pass unchanged.
- **`botao` with `get()?`**: without the ESP32, the app stops proposing on **both** outputs, and both LEDs go to `safe`. It is a deliberately harsh choice; part 5 makes the app tolerant of ESP32 faults.

## 8. Scenario `botao.scenario.toml`

```toml
# sim/scenarios/botao.scenario.toml
schema = 1

[scenario]
name     = "BOOT turns the system on and off; the ESP32 LED shows the state"
manifest = "joker.sim.toml"
duration = "8s"

[[expect]]
during = "0.5s .. 1.9s"
signal = "esp.led"
equals = true                        # starts on
[[press]]
signal = "esp.button"
at     = "2s"
for    = "300ms"
[[expect]]
during = "2.2s .. 3.9s"
signal = "esp.led"
equals = false
[[expect]]
during = "2.2s .. 3.9s"
signal = "pi.act_led"
equals = false                       # off: ACT off
[[press]]
signal = "esp.button"
at     = "4s"
for    = "300ms"
[[expect]]
during        = "4.5s .. 8s"
signal        = "pi.act_led"
toggles_every = "500ms ± 20ms"       # on again, at 45 °C
[[expect]]
during  = "1s .. 8s"
signal  = "esp.led"
toggles = 2                          # two presses, two edges: the bounce does not count
```

```sh
$ jk test
```

```
apps/luz (cargo test) ........................ ok
sim/scenarios/pisca.scenario.toml ..................... ok
sim/scenarios/aquece.scenario.toml .................... ok (460 simulated s in 0.5 s)
sim/scenarios/botao.scenario.toml ..................... ok (8 simulated s in 14 ms)
```

Remove the `debounce` from `joker.toml` and run again: `toggles = 2` fails on some presses (`J0842`), with the instant and the decision chain in the report. That is the bounce, caught on the laptop. In `jk dev --env sim --panel`, the `b` key is `BOOT`.

## 9. On the Pi 3

```toml
# joker.lab.toml (add)
[buses.esp32]
kind = "serial"
port = "/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0"
baud = 115200
dtr  = "low"            # the default, written out in full: opening the port does not reset the ESP32
rts  = "low"

[devices.esp]
driver  = "modbus-rtu"
bus     = "esp32"
unit_id = 1
profile = "local/bench-esp32@1"
```

On ESP32 boards, the DTR and RTS of the USB-serial bridge are wired to `EN` and `GPIO0`: it is the *auto-reset* that `espflash` uses to flash. With `"low"` (the default, ref/01 §5.6), `jokerd` opens the port without asserting them, and the ESP32 does not reset every time the node starts. `"keep"` leaves them as the kernel has them.

```sh
$ jk check --env lab
```

```
✓ manifest           2 devices (sysfs, modbus-rtu), 2 outputs, 1 app, 1 group, 1 calc, 1 alarm
✓ bindings           luz: led → pi.act_led · temp → calc.temp_media (°C) · botao → esp.button · led_estado → esp.led
✓ profile            local/bench-esp32@1 · equipment fallback: esp.led → false in 2 s (consistent with safe)
✓ bus esp32          115200 8N1, poll 100 ms: FC1 (14 B), FC2 (14 B), FC4 (21 B)
                     4.3 ms on the wire + 3 × 0.3 ms of silence + 3 × 10 ms of latency (default) ≈ 35 ms
                     + keepalive of esp.led (FC5, 16 B) every 400 ms ≈ 3 ms per poll
                     → occupancy 38% (maximum 80%)
note: `esp.led` has a readback suggested by the profile: verify = { readback = "esp.led_rb" }
warning[W0205]: device fallback missing or inconsistent with `safe`: `pi.act_led`   (the one from part 2)
warning[W0451]: profile fallback without verification pinned in the lock: the profile of `esp` declares [fallback]
   = help: jk lock --env lab --node lab
```

Read the budget calmly (document 05, §5): what weighs is not the bytes, it is the server's latency, which the profile leaves at the 10 ms default; `jk profile test` measured 4–5 ms. With a 20 ms poll, the same calculation would give close to 190%, and `jk check` would reject it (`J0370`).

And notice what does **not** appear: there is no `W0205` for `esp.led`. With `[fallback]` declared in the profile, that output **has** an equipment fallback, unlike the `ACT` of part 2. The `W0451` only says that nobody has confirmed it on the node yet:

```sh
$ jk lock --env lab --node lab
```

```
joker.lab.lock: profile local/bench-esp32@1 (1.0.0, path:) · fallback.esp captured (esp.led → false in 2 s) · discovered.esp firmware "1"
```

```toml
# joker.lab.lock (excerpt; [profiles."local/bench-esp32@1"] has the shape of [boards] in part 2)
[fallback.esp]
outputs = ["esp.led"]
timeout = "2s"
value = false
configured = false
verified = "2026-09-25T11:04:12Z"

[discovered.esp]
firmware = "1"
```

The lock pins the digest of the profile file (changing a register without changing the lock is `J0420`) and the fallback seen on the equipment; `configured = false` because the firmware has a fixed fallback, and the core only reads it and compares it (ref/02 §2.1.12). The equipment's firmware comes from the profile's `identity`: `jk lock --node` read `fw` (`1`) and wrote it into `[discovered.esp]` (ref/02 §2.1.11). If one day the ESP32 shows up with another firmware, it is the drift `J0401 kind=firmware`, without any discovery (ref/03 §2.2.2).

```sh
$ jk build --target rpi3b-plus
$ jk apply --env lab --node lab --confirm 5m
$ jk confirm --node lab
$ jk read 'esp.*' --watch --node lab
```

```
esp.led      true      good    (control:12 · luz)
esp.led_rb   true      good    age 0.07 s
esp.button   false     good    age 0.07 s
…            (touch 61, rssi -54, uptime 1,391 s: good, age 0.07 s; fw 1, param, on request)
```

Press `BOOT`: the ESP32's LED goes off, and so does the `ACT`. Again: both come back.

**The fallback, for real.** With the system on (ESP32 LED on), kill `jokerd` without giving it time to put anything in `safe`, and prevent systemd from restarting it:

```sh
lab$ sudo systemctl kill -s KILL jokerd; sudo systemctl stop jokerd
```

Within 2 s, the ESP32's LED goes off. Nobody told it to: the firmware stopped receiving requests and applied its fallback. The `ACT`, on the other hand, stays as it was, because a board's LED has no watchdog (part 2). It is the third layer of the safe state, working on one piece of equipment and missing on the other, exactly as `jk check` said. `sudo systemctl start jokerd` restores everything.

## 10. What happened underneath

- **The profile belongs to the equipment model; the manifest belongs to the site.** The profile states addresses, types, scales, readback and fallback; the manifest states the `safe`, the debounce and the bindings (ref/03 §2.2; ref/01 §5.8.1). `safe_hint` only suggests: the `safe` is always a written decision of whoever installs (ADR 0006).
- **Acquisition.** The driver delivers raw values; the acquisition stage applies type, deadband and debounce in the order of ref/03 §3, and the link quality on top (document 05, §12).
- **Bus.** A serial bus serializes: one request at a time, with priority to writes (document 05, §5). The four input registers are read in a single request, because they are contiguous and of the same FC. An output is not read in a cycle: its value is the applied one, and the readback is another point.
- **Writes.** The app proposes `esp.led` every 20 ms, but it only goes to the wire when it changes, plus the keepalive (01 §8), which keeps the equipment seeing requests even with the LED unchanged.
- **Fallback (M1).** The core compares the profile's fallback with the `safe` and the `safer` (`J0205`), the lock pins it, and the drift is `J0401 kind=fallback` (ref/02 §3.1.5).

## 11. Exercises

1. With the system off, write `true` to coil 0 through the Wi-Fi Modbus TCP (the firmware serves both at the same time), with a tool like `mbpoll`. The ESP32's LED turns on and off again: in how long, and who turned it off? Then add `verify = { readback = "esp.led_rb" }` to `[outputs."esp.led"]` and repeat.
2. With the latency measured in `jk profile test`, add `quirks = { latency = "5ms" }` to a **copy** of the profile (`local/bench-esp32-rapido@1`) and compare the occupancy in `jk check`. What is the minimum poll that fits in 80%?
3. Make a long press (> 2 s) on `BOOT` put the system into "always blink fast", until the next press. Write the scenario first.
4. Repeat the fallback test with the system off (ESP32 LED off). What does it prove, and what does it not prove?

```sh
$ git add -A && git commit -qm "track(04): ESP32 as Modbus RTU equipment; profile local/bench-esp32; luz 0.3"
```

**State at the end of this part:** app `luz` 0.3.0, on lab at generation 4; `profiles/local/bench-esp32@1.profile.toml` (and the `mapa-esp32.csv` it came from); `sim/esp.sim.toml` and `[devices.esp]` in `joker.sim.toml`; `joker.toml` with `[outputs."esp.led"]`, the debounce of `esp.button` and the new `bind`; `joker.lab.toml` with `[buses.esp32]` (with `dtr`/`rts`) and `[devices.esp]`; `joker.lab.lock` with the profile, `[fallback.esp]` and `[discovered.esp]`; new scenario `sim/scenarios/botao.scenario.toml`.

**Next:** [Part 5 — touch as an adjustment, quality, faults, fallback and replay](05-touch-and-faults.md).
