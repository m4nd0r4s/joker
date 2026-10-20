# Part 10 — Logic on the MCU: a native app and the link over Wi-Fi

**Goal:** get a Rust app running **inside** the ESP32, which turns its LED on and off with the `BOOT` button without depending on the Pi 3, and then take the link off the cable and move it to Wi-Fi, encrypted.

**You will learn:** native apps on Joker Micro (the same SDK, with the native backend) · local and remote writers in the MCU's arbiter · `jk explain` of an MCU output and the `overridden` · what `jk build --target mcu` rejects (`J0720`) and warns about (`J0721`) · `jk bench` on the chip itself · `[net.wifi]` and a secret encrypted for the MCU · Joker Link over UDP (`J0179`) · what changes between serial and Wi-Fi.

**Before you start:** part 9 done. The ESP32 with Joker Micro, connected to the Pi 3 over Joker Link on serial, session `full`, and `luz` 0.6 running on the node `lab`. The bench Wi-Fi network, with the name and the password at hand.

---

## 1. Why logic on the MCU

In part 9, the `BOOT` button made a long trip: the MCU read it, sent it in a `SAMPLES` to the Pi 3, `luz` saw the edge in the next tick, proposed the LED, and the `PROPOSE` came back down the cable. A few dozen milliseconds, and **nothing** without the Pi 3.

Some things should stay close to the hardware: a reflex, a local interlock, a fast loop. Joker Micro runs native apps in Rust, with **the same SDK** as the apps of the Linux node (document 19, §5). The difference is in the build: on Linux the app compiles to Wasm and runs on wasmtime; on the MCU it compiles to `xtensa-esp32-none-elf` with the native backend (`backend-native`, ref/08 §2.11.5) and is linked into the firmware. There is no sandbox: a *panic* restarts the MCU, and the outputs stay in the reset state.

## 2. The app `reflexo`

```sh
$ cd micro/esp && jk app new reflexo --lang rust
```

```rust
// micro/esp/apps/reflexo/src/lib.rs
use joker::prelude::*;

#[joker::app(name = "reflexo", version = "0.1.0")]
pub struct Reflexo {
    #[input]  botao: Input<bool>,
    #[output] led:   Output<bool>,
    #[param(unit = "ms", range = "0..2000", default = 300.0)] bloqueio: Param<f32>,
}

#[derive(Default)]
struct Estado {
    ligado: bool,
    antes: bool,
    desde_ms: f32,          // time since the last accepted press
}

#[joker::cycle(period = "10ms")]
fn reflexo(io: &mut CycleIo<Reflexo>, st: &mut Estado) -> Result<()> {
    st.desde_ms = (st.desde_ms + io.dt().as_secs_f32() * 1000.0).min(60_000.0);
    let premido = io.botao.get_or(false);            // without a good reading, it counts as released
    if premido && !st.antes && st.desde_ms >= io.bloqueio.get() {
        st.ligado = !st.ligado;
        st.desde_ms = 0.0;
    }
    st.antes = premido;
    io.led.set(st.ligado);
    Ok(())
}
```

It is the same rule as `luz` since 0.3 (each rising edge toggles `ligado`), plus a 300 ms lockout against repeated presses. Nothing in this app is specific to the MCU: it would compile the same way to Wasm. A test with `joker-test` runs on the laptop, with a virtual clock, like those of `luz`.

## 3. The manifest: a local writer above the link

Add to `micro/esp/joker.toml`:

```toml
[apps.reflexo]
path   = "apps/reflexo"
bind   = { botao = "io.button", led = "io.led" }
cycles = { reflexo = "main" }
```

```sh
$ jk check
```

```
error[J0210]: two writers at the same level of the same output
 --> joker.toml:46:1
   |
46 | [apps.reflexo]
   | ^^^^^^^^^^^^^^ app:reflexo writes io.led at control:12 (the default for a single writer)
   = note: control:12 is in the remote_levels of io.led: it is the link's level, where luz from node lab writes
   = help: give reflexo another level, `writer = { level = "supervisor:8" }`, or remove control:12 from remote_levels
```

On the MCU, each level delegated to the link **is** a writer: the pair (link, level) (document 16, A12). `reflexo` cannot take `luz`'s level. The design decision is to put it **above** it, in the `supervisor` band (levels 6–8; 1 is the highest, ref/01 §3.11):

```toml
[apps.reflexo]
path   = "apps/reflexo"
bind   = { botao = "io.button", led = "io.led" }
writer = { level = "supervisor:8" }
cycles = { reflexo = "main" }
```

The list `remote_levels = ["manual:3", "manual:4", "manual:5", "control:12"]` from part 9 stays as it was, and that is why everything fits:

| Level in the MCU's arbiter | Who | Beats |
|---|---|---|
| `manual:3`–`manual:5` | a `jk write` on the node `lab`, over the link | everything below, during the TTL |
| `supervisor:8` | `app:reflexo`, **local** | `luz` |
| `control:12` | `app:luz`, remote | only the `safe` |

`supervisor:8` is not in `remote_levels`: no proposal from the Pi 3 can take it (it would be `Rejected(NotOwner)`, and in the node's `jk check`, `J0731`). And a human on the Pi 3 can still override `reflexo`, because the `manual` band is above it.

## 4. What `jk build --target mcu` does not let through

The first version of this app had two extra things. An async task to count the presses in the last hour:

```rust
#[joker::task(every = "1m")]
async fn estatistica(io: &TaskIo<Reflexo>) -> Result<()> {
    let h = io.botao.history(io.now() - 1.h(), io.now(), 1.mins()).await?;
    // …
}
```

```
error[J0720]: API or manifest section unavailable on Joker Micro (§2.11.5, ref/04 §4.14): `history`
  --> micro/esp/apps/reflexo/src/lib.rs:34:22
   |
34 |     let h = io.botao.history(io.now() - 1.h(), io.now(), 1.mins()).await?;
   |                      ^^^^^^^ an MCU has no historian (document 19, §3)
   = help: a core window in an MCU `[calc]` (`count`, `avg(x, 15m)`), or do the calculation on the Linux node,
            where `esp.button` is in the historian
```

The error is born in `rustc` (the function requires a *trait* that the native backend does not implement) and `jk build` translates the message into the Joker code, with the suggestion (ref/08 §2.11.5). And arithmetic in `f64`:

```rust
    #[param(unit = "ms", range = "0..2000", default = 300.0)] bloqueio: Param<f64>,
```

```
warning[J0721]: `f64` on an MCU without a double-precision FPU (`fpu` = `none` or `sp`, §2.14)
  --> micro/esp/apps/reflexo/src/lib.rs:9:74
   |
 9 |     #[param(unit = "ms", range = "0..2000", default = 300.0)] bloqueio: Param<f64>,
   |                                                                          ^^^^^^^^^^ the board has `fpu = "sp"`
   = note: software floating-point routines called by `reflexo::reflexo`: __adddf3, __ltdf2, __muldf3
   = help: use `f32`; measure the cost with `jk bench --target mcu:esp32-devkit`
```

`J0720` rejects: the API does not exist. `J0721` only warns: `f64` works, but it is emulated. The ESP32 has a single-precision FPU, and `jk build` looks in the app's object for the software double-precision routines to tell you which functions are to blame. On an ESP32-C6, with no FPU at all, even `f32` would give the warning `J0780` (document 19, §3.1). The version in section 2 is already the fixed one.

## 5. An app is firmware

A native app is **firmware code**. So adding `reflexo` is not just a new generation: it is a new generic firmware (with the app inside it) and a generation that uses it.

```sh
$ jk build --target mcu:esp32-devkit --generic
$ jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw && cd ../..
$ jk apply --env lab-link --node lab --confirm 5m
```

```
→ lab (dev) · from --node · lab.local:7443
plan of generation 14
  ~ MCU esp: firmware sha256:0be4… → sha256:61d9… (apps + reflexo 0.1.0) · 928 KiB over the link · restart
  ~ MCU esp: generation 4 → 5 · + app reflexo (supervisor:8 on io.led) · + group main: task reflexo
  no changes on the node
applied. confirmation pending: `jk confirm` within 5 min, or automatic rollback
```

The generation refers to the app by name, major and `meta_hash`, and is only accepted by a firmware that contains it (`J0772`). After the activation restart and the `HELLO` with generation 5 on trial, `jk confirm --node lab` confirms the firmware and the generation.

## 6. Who wins, and why

Press `BOOT`. The ESP32's LED changes **at once**, without the trip to the Pi 3. Ask who controls it:

```sh
$ jk explain esp.led --node lab
```

```
esp.led      bool · output · alias of esp.io.led · MCU esp (joker-link, serial)
  envelope   on the MCU: safe = false                               micro/esp/joker.toml
  owning group main 10ms, on the MCU
  delegated  manual:3 manual:4 manual:5 control:12
  levels     supervisor:8   true    app:reflexo · local on the MCU · lease 30 ms        ← active
             control:12     true    app:luz · remote (lab) · lease 100 ms
  applied    true   (decided by the MCU's arbiter; control:12 loses to supervisor:8)
```

```sh
$ jk read esp.led esp.button --watch --node lab
```

```
esp.led      true    good    (supervisor:8 · reflexo, on the MCU)   luz: overridden(supervisor:8)
esp.button   false   good    age 0.4 s
```

`luz` keeps proposing, as always: it did not change, and it does not know about `reflexo`. But its proposal loses, and the result that comes back in the `OUTCOMES` is `overridden(supervisor:8)`, recorded in the node's decision log (`jk decisions --output esp.led`). Since the rule is the same, the two almost always want the same value; what applies it is `reflexo`.

Now stop `jokerd`:

```sh
lab$ sudo systemctl stop jokerd
```

In part 9, the LED went to `safe` after 100 ms. **Now it does not.** The lease that expires is `luz`'s, at level `control:12`; `reflexo`, at `supervisor:8`, renews its own on every local tick, and the LED keeps obeying `BOOT`. The `safe` only applies when there is **no** active level (A6), and there is one: a writer that lives on the same chip as the output. `lab$ sudo systemctl start jokerd` and `luz` proposes again, and loses again.

## 7. The link over Wi-Fi

The serial cable served well, but it ties the ESP32 to the Pi 3. The ESP32 has Wi-Fi, and Joker Link runs over UDP (ref/07 §3.5). Three pieces: the network password, the MCU manifest and the device on the Pi 3.

**The password is a secret.** It does not go into Git or into the manifest in clear text (`J0294`):

```sh
$ jk secret set wifi_bancada --nodes esp
```

```
value of wifi_bancada: ••••••••••••
repeat: ••••••••••••
wifi_bancada · rev 1 · encrypted for node:esp (x25519:3c8a…, record 19, from the enrollment of part 9)
warning[W0926]: secret signature missing or invalid: entry without a `secrets` signature (in `prod` it is an error; part 11)
✓ secrets.toml.enc
```

The value is encrypted for the X25519 key of the **MCU**, which was born at enrollment and never left it. `jk build --generation` puts the encrypted entry in the `SECRETS` section of the blob (ref/08 §2.13.3), the node `lab` takes the blob to the MCU over the link, and the MCU decrypts it when it opens the generation. **The Pi 3 carries the password and cannot read it:** it is not a recipient. `secrets.toml.enc` stays at the project root, next to `joker.lab-link.toml`, which is what takes the MCU's generation to the node; it may go into Git (ref/02 §2.4).

**The MCU manifest.** Remove `[buses.uart0]` and replace `[link]`:

```toml
[net.wifi]
ssid     = "bancada"
password = "secret:wifi_bancada"
country  = "PT"

[link]
address = "udp:192.168.1.20:7471"   # the node lab (the Pi 3's IP): where the MCU sends to
peer    = "lab"
auth    = "required"
```

**The Pi 3.** In `joker.lab-link.toml`, the device leaves the bus and listens on UDP instead:

```toml
[devices.esp]
bus     = { unset = true }
address = "udp:192.168.1.57:7471"   # the MCU: the ESP32's IP and the link port
report  = "50ms"
refresh = "5s"

[apps.luz]
writer = { lease = "1s" }
```

The two `address` keys point at each other: on the Linux side, the device's `address` is the IP and port **of the MCU**; in the MCU manifest, `[link] address` is **the node's** (ref/07 §15; ref/01 §5.22). They are always IPs: reserve both in DHCP (the ESP32 keeps its MAC, and so the `192.168.1.57` it already had with the bench firmware; lab's is shown by `hostname -I`). The session is identified by the `sid` in the encrypted header, and not by the source address (ref/07 §3.5).

The key is the same as on the serial link (`secret:link.esp`): the link changed transport, not identity. And encryption is required. Try removing it:

```
error[J0179]: Joker Link without authentication on an IP transport
 --> joker.lab-link.toml:9:1
  |
9 | auth    = "none"
  | ^^^^^^^^^^^^^^^^ the device esp uses udp:192.168.1.57:7471
  = note: on UDP and TCP authentication is required, and configured on both sides without negotiation (ref/07 §4.6)
  = help: `auth = "required"` (the default); on a serial cable inside a cabinet, `none` would be the warning J0738
```

Without negotiation, an attacker on the network cannot "downgrade" the security of a link that requires it: both sides refuse clear-text frames (`J-E2104`).

**Why a 1 s lease.** With the 100 ms of part 9, `jk check` warns:

```
warning[W0730]: liveness condition not met: the lease of a remote writer does not survive one lost renewal
  = note: luz → esp.led over UDP (Wi-Fi): 2 × 20 ms + 10 ms (MCU group main) + 0.2 ms (1024 B block)
          + 100 ms (peak latency on Wi-Fi, ref/07 §11.4) + 1 ms = 151 ms > 100 ms
  = help: over Wi-Fi, leases of remote writers ≥ 1 s, and never fast control; in `prod` this is an error
```

With 1 s: `2 × 320 ms + 10 + 0.2 + 100 + 1 = 751 ms ≤ 1000 ms`. The price is in the table of section 8.

**Apply, for the last time over the cable.** The new generation reaches the MCU over the serial link, and it is what switches the link to UDP:

```sh
$ (cd micro/esp && jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw)
$ jk apply --env lab-link --node lab --confirm 5m
```

The MCU swaps generation, connects to Wi-Fi, and offers the `HELLO` over UDP; the node, already with the new generation, answers. If the password is wrong, the MCU never reaches the node, nobody confirms, and **after 5 min the MCU goes back by itself to the generation with the serial link**, and the node to the previous configuration: the confirmation window is the safety net of a network change (ADR 0015). With `jk status` showing the session `full` over UDP:

```
esp   joker-link   up · session full · Joker Link 1.0 · udp 192.168.1.57:7471 · encrypted (kid 9a1f…)
      generation 6 (flash, gen_b, confirming) · clock holdover (TIME, error 2.1 ms > time_tolerance 1 ms)
      frames rx 2,410 · lost 0.3% · retransmissions 4 · RTT 6.8 ms (p99 38 ms) · esp.rssi −58 good
```

`jk confirm --node lab`. `esp.rssi`, which in part 9 was `Bad(NotConnected)` (the MCU had no network), now has a value.

## 8. Serial versus Wi-Fi

| | Serial (part 9) | Wi-Fi, UDP (now) |
|---|---|---|
| RTT | 1.6 ms, stable | 3–15 ms, peaks of 100+ ms |
| encryption | optional (`J0738` without it) | required (`J0179`) |
| `keepalive` / `link_timeout` | 50 ms / 200 ms | 1 s / 5 s |
| `report` | 20 ms | 50 ms: less airtime, and the touch does not need more |
| `refresh` and `max_age` of the `esp.*` signals | 1 s → 3 s | 5 s → 15 s (3 × `refresh`; `J0739` below 2 ×) |
| lease of `luz` | 100 ms | 1 s |
| `jokerd` dead → the lease of `luz` expires on the MCU | ≤ 110 ms | ≤ 1.01 s |
| `esp_offline` (`!good(esp.uptime)`, 5 s) | ~5.2 s | ~10 s: 5 s of `link_timeout` + 5 s of `delay_on` |
| MCU clock | `synced` (δ < 1 ms) | `holdover` with the default tolerance; `time_tolerance = "5ms"` if you need `synced` |

The rule of ref/07 §11.4: over Wi-Fi, the Linux node supervises and adjusts; what has to be fast stays on the MCU. That is exactly what `reflexo` does.

## 9. The Pi 3 off, for real

With the link on the air, the ESP32 no longer needs the data cable. Plug it into the laptop, just for power, and turn the Pi off:

```sh
lab$ sudo poweroff
```

Press `BOOT`: the LED turns on and off as before. The MCU keeps offering a `HELLO` with *backoff* up to 5 s between attempts, and when the Pi 3 comes back the session comes up by itself.

With the ESP32 on the laptop, measure the app on the chip itself:

```sh
$ (cd micro/esp && jk bench --target mcu:esp32-devkit)
```

```
board esp32-devkit@1 · over serial (espflash) · cycle counter CCOUNT, 240 MHz · no probe
  reflexo.reflexo    10,000 runs        p50 1.4 µs   p99 1.9 µs   max 2.6 µs
  tick main (10 ms)  10,000 ticks       p50 11 µs    p99 14 µs    max 19 µs    0.2% of the period
✓ [wcet] in micro/esp/joker.lock · MCU firmware and generation restored
```

The original ESP32 has no JTAG without an external adapter, so the measurement is made by the core's own cycle counter and reported over serial (document 23, §5.1). With the `f64` version of section 4, `reflexo.reflexo` goes up to nearly 3 µs: that is what `J0721` means. Connect the ESP32 and the Pi 3 back as they were.

## 10. Joker Micro in the design

Module C closes here. Document 19 gives Joker Micro three patterns, and the bench touched two:

1. **I/O coprocessor of a Linux node** (part 9): the Pi thinks, the MCU holds the outputs with its envelope and its `safe`. If Linux dies, the safety stays. It is the answer to the `ACT` LED of part 2, which stayed as it was.
2. **Logic on the MCU** (this part): `reflexo` is an app like the others, with the same SDK, the same arbiter and the same decision log, only running close to the hardware.
3. **Standalone node** (not done here): an M2 MCU with a network can be the whole node and talk directly to the Deck through the micro profile of the agent protocol, over MQTT or CoAP (ref/10 §2.15; ref/07 §14). The ESP32 has the hardware for that; the rest of the track keeps it behind the Pi 3.

What is **not** in v1: Wasm apps on the MCU. A native app is firmware, and changing it is a firmware OTA with a restart (section 5). 1.4 brings `wasmi` to M2 MCUs, with app swaps without reflashing (the OTA type `app` stays reserved for that). ML on the MCU is also 1.4. And what this bench does not prove: the reference targets (RP2350, ESP32-C6) only in QEMU, and the link's half-duplex and CAN without a real test (document 23, §8).

## 11. What happened underneath

- **The same SDK, another backend.** The macro `#[joker::app]` generates, with `backend-native`, the `NativeApp` contract instead of the WIT exports; the app reads the latch image and writes to the group's proposal buffer, in the same format as on Linux, without the Wasm boundary (ref/08 §2.11.5).
- **Local and remote writers in the same arbiter.** `reflexo` is a writer of the blob's `WRITERS` table; `luz` is another, (link, `control:12`). The two `jk check`s, the MCU's (`J0210`) and the node's (`J0731`), guarantee that they never contend for the same level (16 A5, A12).
- **Secrets for whoever uses them.** The password goes encrypted for the MCU and travels inside the blob; whoever carries it cannot read it (ref/02 §2.4.8; ref/08 §2.13.3).
- **UDP with the same session.** The same encrypted L2, one datagram per frame, the CRC-32C all the same, and IP timings (ref/07 §3.5, §5.6).
- **Changing network with a safety net.** The transport switch is an MCU generation like any other, with the confirmation window counted on the MCU (ref/07 §9.4).

## 12. Exercises

1. Turn the Pi 3 on after the exercise of section 9 with the ESP32's LED on. Do the Pi's LED (the instance `sinal`) and the ESP32's agree? Why? Who is the source of truth of `ligado`, and how would you change that without touching `luz`?
2. Do `jk write esp.led false --ttl 1m --reason "…" --node lab` with `reflexo` active. What does `jk explain esp.led` show? And if you remove the `manual` band from `remote_levels`?
3. Change `reflexo` to `writer = { level = "safety:2" }`. Does `jk check` accept it? What argument do you have for not doing it?
4. Put `requires_clock = "synced"` in `[apps.reflexo]` and leave the link on Wi-Fi with the default tolerance. What happens to the LED, and which code explains why?

```sh
$ git add -A && git commit -qm "track(10): app reflexo on the MCU; Joker Link over Wi-Fi (UDP, encrypted)"
```

**State at the end of this part:** app `luz` **0.6, unchanged**; MCU app **`reflexo` 0.1.0**. New files: `micro/esp/apps/reflexo/`, `micro/esp/joker.lock` (the chip's `[wcet]`). Changed: `secrets.toml.enc` (the entry `wifi_bancada`), `micro/esp/joker.toml` (the app, `[net.wifi]`, `[link]` over UDP) and `joker.lab-link.toml` (`esp` over UDP, `report` and `refresh`, 1 s lease). The ESP32 runs the firmware with `reflexo` and generation 6, over Wi-Fi; the Pi 3 at generation 15 of `joker.lab-link.toml`, confirmed. End of Module C.

**Next:** [Part 11 — Production and fleet: signatures, secrets, the Deck and two nodes](11-production-and-fleet.md).
