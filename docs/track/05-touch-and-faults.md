# Part 5 — Touch as an adjustment, quality, faults, fallback and replay

**Goal:** the ESP32's touch sensor becomes a "rotary knob" that shifts the hot limit of the blink; then you break communication with the ESP32 on purpose, in the simulator and on the bench, and watch the system keep blinking through the Pi, the alarm go off, the equipment drop to its fallback, and the recorded trace replay a fixed version of the app on the laptop.

**You will learn:** noisy analog inputs · acquisition `deadband` and `stuck_detect` · dimensionless `calc` · `get_or` and substituted quality in the trace · three quality policies in the same app · scenarios with faults (`[[inject]] disconnect`) · real loss of communication · `jk device stop` · equipment fallback (M1) and keepalive · `jk status`, `jk trace`, `jk decisions` · `jk record` and `jk replay --diff` · determinism: the clock is an input.

**Before you start:** part 4 done. The app `light` is at 0.3, the profile `local/bench-esp32@1` exists, `sim/esp.sim.toml` simulates the ESP32, and `joker.lab.toml` talks to the ESP32 over Modbus RTU through the USB bridge. The ESP32 is connected by micro-USB to the Pi 3, with the bench firmware.

---

## 1. Touch is an analog, noisy signal

The ESP32's T0 pin (GPIO4) measures the pin's capacitance. Without a finger, the raw reading is around 60; with a finger on it, it drops to ~15. It never stays still: it varies with humidity, temperature and the cable. It is the bench's "potentiometer" (document 23, §3).

```sh
$ jk read esp.touch --watch --node lab
```

```
esp.touch   61   good   age 40 ms    (modbus-rtu esp32 · in[0] · deadband 5)
esp.touch   58   good   age 60 ms
esp.touch   17   good   age 30 ms    ← finger
```

The profile from part 4 already declares `deadband = 5` on `touch`: variations below 5 counts do not update the signal. It is an **acquisition** filter (document 01, §3.3): it runs in the core, before any app, and spares `changed` events, the historian and the trace.

Your numbers may be different: measure them before you continue, without a finger and with a finger, and use them in §2.

**`stuck_detect`.** A signal that should always vary and stays bit-for-bit the same for a long time is suspicious: it may be the firmware stuck, always answering the same register. `stuck_detect = "10m"` marks it `uncertain(frozen)` (document 02, §4). Mind the acquisition order (ref/03 §3): `deadband` → `stuck_detect`. `stuck_detect` sees the value **after** the deadband, and a touch at rest can stay within the 5 counts for a long time. That is why the time is long (10 min, and not 30 s), and why the app treats `uncertain` the way it treats `bad`: a false positive only makes the app keep the last good value, which is the same.

## 2. From the raw value to an adjustment

The app does not care about counts: it wants an adjustment from 0 to 1. The conversion belongs to the site, and it stays in the common manifest:

```toml
# joker.toml (common) — what changes compared with part 4
[signals."esp.touch"]
stuck_detect = "10m"

[calc]
temp_avg  = "avg(pi.cpu_temp, 10s)"
touch_level = "clamp((60 - esp.touch) / 45, 0, 1)"   # 60 without a finger → 0; 15 with the finger → 1

[alarms.esp_offline]
when     = "!good(esp.uptime)"
delay_on = "5s"
severity = "high"
message  = "ESP32 without communication"

[apps.light]
path   = "apps/light"
bind   = { led = "pi.act_led", temp = "calc.temp_avg", button = "esp.button", led_status = "esp.led", adjust = "calc.touch_level" }
params = { half_period = "500 ms", t_cold = "45 °C", t_hot = "70 °C" }
cycles = { blink = "main" }
```

- `esp.touch` has no unit (`1`), so the literals `60` and `45` are accepted, and so is `clamp(…, 0, 1)` (document 16, §4). The port `adjust` is `unit = "1"`: it matches.
- **Quality in expressions:** the `calc` has the worst quality of its operands (16 §3.2). With the ESP32 down, `calc.touch_level` becomes `bad(comm_failure)`, and never an invented number.
- **The alarm `esp_offline`** watches `esp.uptime`, the signal that always changes while the ESP32 is alive. `good(x)` is a meta function (document 02, §10): it returns `false` with good quality when `x` is not `good`, so the alarm does not go `unknown` precisely when the equipment goes down.

**What `jk check` says.** A common reflex is to put the same `stuck_detect` on the button:

```toml
[signals."esp.button"]
stuck_detect = "10m"
```

```
error[J0176]: adjustment incompatible with the signal's type, direction or origin: `stuck_detect` on a `bool` signal
  --> joker.toml:14:1
   |
14 | stuck_detect = "10m"
   | ^^^^^^^^^^^^^^^^^^^^ `esp.button` is `bool`: a button that does not change is the normal state
   = help: for a `bool`, use `debounce`; to know whether the ESP32 is alive, watch `esp.uptime`
```

Remove those lines.

## 3. The app, version 0.4

Touch shifts the hot limit: without a finger, 55 °C; with the finger, 80 °C (`t_hot` − 15 K + 25 K × `adjust`). And the app becomes **tolerant of ESP32 faults**: without it, the system keeps blinking through the Pi.

```rust
// apps/light/src/lib.rs
use joker::prelude::*;

#[joker::app(name = "light", version = "0.4.0")]
pub struct Light {
    #[output]             led: Output<bool>,          // pi.act_led
    #[output]             led_status: Output<bool>,   // esp.led
    #[input(unit = "°C")] temp: Input<f64>,           // calc.temp_avg
    #[input]              button: Input<bool>,         // esp.button
    /// 0 = no finger, 1 = finger on it (calc.touch_level).
    #[input(unit = "1")]  adjust: Input<f64>,

    #[param(unit = "ms", default = 500.0)] half_period: Param<f64>,
    #[param(unit = "°C", default = 45.0)]  t_cold: Param<f64>,
    #[param(unit = "°C", default = 70.0)]  t_hot: Param<f64>,
}

/// Touch shifts the hot limit between t_hot − 15 K and t_hot + 10 K.
const BACKOFF_K: f64 = 15.0;
const SPAN_K: f64 = 25.0;
/// The adjustment that gives exactly the manifest's t_hot: it is the "last good" before the first reading.
const NEUTRAL_ADJUST: f64 = BACKOFF_K / SPAN_K; // 0.6

/// `persist`: the state goes through a hot swap and enters `jk record` (§7).
#[joker::state(persist)]
struct State { enabled: bool, pressed: bool, adjust: f64, elapsed_ms: f64, lit: bool }

impl Default for State {
    fn default() -> Self {
        // Starts on, like 0.3.
        State { enabled: true, pressed: false, adjust: NEUTRAL_ADJUST, elapsed_ms: 0.0, lit: false }
    }
}

#[joker::cycle(period = "20ms", wcet = "20us")]
fn blink(io: &mut CycleIo<Light>, st: &mut State) -> Result<()> {
    // 1. Button. Without quality it counts as not pressed: get_or(false) never creates an edge.
    let pressed = io.button.get_or(false);
    if pressed && !st.pressed {
        st.enabled = !st.enabled;
        st.elapsed_ms = 0.0;
        st.lit = st.enabled;
    }
    st.pressed = pressed;
    io.led_status.set(st.enabled);

    // 2. Adjustment. Without quality, the last good one stays; every substitution stays in the trace.
    st.adjust = io.adjust.get_or(st.adjust);

    if !st.enabled {
        io.led.set(false);
        return Ok(());
    }

    // 3. Temperature. Without it there is no honest rhythm: `?` → Err → lease expires → safe (as in 0.2).
    let temp = io.temp.get()?;
    let t_cold = io.t_cold.get();
    let t_hot = io.t_hot.get() - BACKOFF_K + SPAN_K * st.adjust;
    let x = ((temp - t_cold) / (t_hot - t_cold).max(1.0)).clamp(0.0, 1.0);
    let half_ms = io.half_period.get() - x * (io.half_period.get() - 100.0);

    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;
    if st.elapsed_ms >= half_ms {
        st.elapsed_ms -= half_ms;
        st.lit = !st.lit;
    }
    io.led.set(st.lit);
    Ok(())
}
```

Three inputs, three policies, all written in the code:

| Input | Call | Without quality | Why |
|---|---|---|---|
| `temp` | `get()?` | the task returns `Err`; the proposals already made stay (ref/04 §2.6.1), the lease of `led` expires → `safe` | without a temperature, the blink rhythm would be a lie |
| `button` | `get_or(false)` | not pressed: there are no edges | a button you cannot see did not press anything |
| `adjust` | `get_or(last good)` | the previous value, **marked** | it is comfort; the last value is the best estimate |

`get_or` never hides anything: the table of document 16 §3.1 requires recording in the trace that the reading was substituted (`note-substituted`, ref/04 §2.3). A `bad` sample is not silently converted into a number (invariant 4 of `AGENTS.md`).

## 4. The fault in the simulator

Confirm that `sim/esp.sim.toml` has the fault key (part 4 left it ready; if not, add it):

```toml
# sim/esp.sim.toml (add)
[faults]
panel = { key = "d", disconnect = true }   # "d" disconnects the whole ESP32 (ref/03 §2.4.8)
```

```sh
$ jk dev --env sim --panel     # "b" turns on, the touch slider moves the rhythm, "d" disconnects the ESP32
```

Now the test. This scenario is the track's fault-tolerance contract: **it will pass unchanged on versions 0.4, 0.5 and 0.6 of the app**. That is why it only talks about device signals and the alarm, never about internal details of the app or of the `calc`s.

```toml
# sim/scenarios/esp_fault.scenario.toml
schema = 1

[scenario]
name     = "the ESP32 drops midway: bad samples, alarm, the Pi keeps blinking"
manifest = "joker.sim.toml"
duration = "60s"
tags     = ["faults"]

# Fixed conditions: CPU at 60 °C and touch at 42 → adjustment 0.4 → hot limit 70 − 15 + 10 = 65 °C
# → x = (60 − 45) / (65 − 45) = 0.75 → half period = 500 − 0.75 × 400 = 200 ms.
# (0.4 is not NEUTRAL_ADJUST: a fixed substitute value would give another rhythm.)
# The system starts on: there is no initial press.
[[set]]
at     = "0s"
signal = "pi.cpu_temp"
value  = "60 °C"

[[set]]
at     = "0s"
signal = "esp.touch"
value  = 42

[[expect]]
during        = "5s .. 19s"
signal        = "pi.act_led"
toggles_every = "200ms ± 20ms"

[[expect]]
during = "5s .. 19s"
signal = "esp.led"
equals = true

[[expect]]
at     = "19s"
alarm  = "esp_offline"
active = false

# The fault: the whole ESP32 stops responding for 20 s.
[[inject]]
at         = "20s"
device     = "esp"
disconnect = true
for        = "20s"

# A press during the fault does not count: the button cannot be seen.
[[press]]
signal = "esp.button"
at     = "30s"

[[expect]]
during  = "21s .. 39s"
signal  = "esp.uptime"
quality = "bad(comm_failure)"

[[expect]]
during        = "21s .. 39s"
signal        = "pi.act_led"
toggles_every = "200ms ± 20ms"

[[expect]]
at     = "24s"
alarm  = "esp_offline"
active = false                   # still within the delay_on

[[expect]]
within = "25s .. 26s"
alarm  = "esp_offline"
active = true

[[expect]]
at     = "39s"
alarm  = "esp_offline"
active = true

# Recovery: the system stays on, and the ESP32's LED shows it again.
[[expect]]
during = "41s .. 60s"
signal = "esp.led"
equals = true

[[expect]]
during        = "41s .. 60s"
signal        = "pi.act_led"
toggles_every = "200ms ± 20ms"

[[expect]]
at     = "42s"
alarm  = "esp_offline"
active = false
```

Notice three choices. Touch at 42 gives a rhythm that depends on the adjustment (at 45 °C, the adjustment would not count) and that does not coincide with any fixed substitute value. The rhythm **during** the fault is the same as before: only `get_or(last good)` achieves that. And the press at 30 s proves that the button without quality does not turn anything on or off: at 41 s the system is still on.

```sh
$ jk test sim/scenarios/esp_fault.scenario.toml --env sim
```

```
scenario esp_fault · "the ESP32 drops midway: …" · 60 virtual s in 0.8 s
  ✓ 11 expectations
```

## 5. On the bench: unplug the cable

```sh
$ jk build
$ jk lock --env lab --node lab
$ jk apply --env lab --node lab --confirm 5m
$ jk confirm --node lab
```

The system starts on; choose a rhythm with your finger. Then **unplug the ESP32's micro-USB cable**.

The cable carries two things: the serial port **and the power**. Without it, the ESP32 has no power:

- the ESP32's LED goes off **at once**. This is not the fallback: it is the lack of power;
- the port `/dev/serial/by-id/usb-Silicon_Labs_CP2102_…` disappears from the Pi 3, and the driver declares the device down;
- when you plug it back in, the ESP32 **boots again**: `esp.uptime` starts again from zero (it is `monotonic`: the decrease counts in the metric `joker_counter_resets_total`, and that is how you tell "it restarted" from "the cable failed"), the Wi-Fi reconnects, and the LED stays off until Joker resends the last applied value (the *resync*, ref/06 §2.1).

```sh
$ jk status --node lab
```

```
→ lab (dev) · from --node
generation    5 · light 0.4.0 · active
groups        main 20 ms · overruns 0
devices       pi    sysfs        up
              esp   modbus-rtu   down   bad(comm_failure) for 9 s · port missing
alarms        esp_offline (high) active for 4 s · not acknowledged · "ESP32 without communication"
apps          light 0.4.0 running
outputs       pi.act_led   control:12 · blinking
              esp.led      control:12 true · sending failing (comm_failure)
```

The Pi's LED keeps blinking, at the same rhythm. Follow both ends of the fault:

```sh
$ jk trace calc.touch_level --node lab
```

```
calc.touch_level = clamp((60 - esp.touch) / 45, 0, 1)
  esp.touch       bad(comm_failure)    last good 22 9.4 s ago
  calc            bad(comm_failure)
  read by         light.blink, port adjust
    get_or → 0.844    substituted    (every tick since 10:14:05.140)
```

```sh
$ jk trace esp.led --node lab
```

```
esp.led   output · esp (modbus-rtu, bus esp32) · coil 0, FC 5
  proposal   light.blink · control:12 · true
  arbiter    control:12 active · lease 60 ms
  envelope   accepted
  commit     true → driver mailbox (latest-wins)
  driver     failed: comm_failure   since 10:14:05.140
  readback   esp.led_rb bad(comm_failure)
  fallback   local/bench-esp32@1: without requests for 2 s, the equipment turns off (value = false)
```

And the decision log (M3), which keeps every write with its result and the inputs it depended on:

```sh
$ jk decisions --output pi.act_led --cause calc.touch_level --since 2m --node lab
```

```
10:14:05.160  control:12  true   accepted   adjust = 0.844 (substituted) · temp = 58.1 °C
10:14:05.420  control:12  false  accepted   adjust = 0.844 (substituted) · temp = 58.1 °C
…
```

Every decision taken with a substituted value says so. Plug the cable back in: within a few seconds the device goes `up`, the alarm stops being active and the ESP32's LED shows the state again.

## 6. The equipment fallback, seen for real

With the cable unplugged you cannot see the fallback: the ESP32 has no power. To see it, the ESP32 has to stay powered and stop hearing requests. In part 4 you killed the whole `jokerd`; now stop only the device:

```sh
$ jk device stop esp --reason "see the ESP32 fallback" --node lab
```

```
→ lab (dev) · from --node · lab.local:7443
esp: stopped (orderly close; volatile: until `jk device start esp`, the next jokerd start or the next generation)
```

- after **2 s** without Modbus requests, the bench firmware turns the ESP32's LED off by itself. It is the profile's `[fallback]` (`timeout = "2s"`, `value = false`), the M1 of document 01, §8;
- `jokerd` keeps running, and the `ACT` keeps blinking at the same rhythm. The `esp.*` signals become `bad(not_connected)`, and not `bad(comm_failure)`: it was the software that stopped, and not the cable (ref/06 §2.8). For `light` it is the same, and `esp_offline` goes off after 5 s;
- stopping a device belongs to the `operator` role and stays in the audit (`POST /v1/devices/esp/stop`, ref/09 §5.12).

```sh
$ jk device start esp --node lab
```

**Why the fallback never fires in normal operation**, even when the LED does not change for minutes? Because the core renews the write at every *keepalive*, which by default is 1/5 of the equipment timeout, read from the lock: 400 ms. The deadband and `min_interval` never suppress a keepalive (document 01, §8). `jk check` confirms that the numbers add up (`J0276`: the equipment timeout must be greater than the keepalive plus the restart time of `jokerd`).

And notice that the equipment fallback (`false`) is equal to the output's `safe` (`[outputs."esp.led"] safe = false`). If they were different, it would be `J0205`: when Joker goes down, the equipment must go to the same safe side that Joker would choose.

## 7. Record the fault and replay it on the laptop

Start recording, with the system as it is. The recording runs on the Pi 3, and the file comes to the laptop at the end:

```sh
$ jk record --for 5m --out fault.jktrace --node lab
```

During the 5 minutes: turn the system off and on again with `BOOT`, play with the touch, unplug the ESP32's cable for about 40 s, plug it back in.

```
→ lab (dev) · from --node
recording group main (20 ms) · 3 inputs, 2 outputs · generation 5 (light 0.4.0)
state     light.blink: export-state between two ticks (state segment, 41 B)
recorded  5m00s · 15,000 ticks · 2.0 MB (0.3 MB compressed) → fault.jktrace
```

First, the determinism proof: the same app, over the same trace, gives the same bytes.

```sh
$ jk replay fault.jktrace --diff
```

```
trace      fault.jktrace · node lab · generation 5 · light 0.4.0 · 15,000 ticks
rerun      light 0.4.0 (the recorded one) · from the recorded state
result     0 differences: proposals and results identical byte for byte
```

Now look at what the trace shows during the fault: the app kept proposing `esp.led = true` on every tick, the envelope accepted, and Joker kept `true` as the applied value, while the real LED was off (no power, or through the fallback). Nothing dangerous, but Joker and the equipment tell different stories. The fix is small: the app only proposes the ESP32's LED when it sees the ESP32. Without a proposal, the lease expires and the output goes to its `safe`, which is `false`: the same as what the equipment does by itself.

```rust
// apps/light/src/lib.rs — what changes from 0.4.0 to 0.4.1
#[joker::app(name = "light", version = "0.4.1")]
// …
    // The ESP32's LED is only proposed when the ESP32 is seen (the button comes from the same equipment).
    // Without a proposal, the lease expires and the output goes to its safe (false), the value of the equipment fallback.
    if io.button.sample().quality.is_good() {
        io.led_status.set(st.enabled);
    }
```

```sh
$ jk build
$ jk replay fault.jktrace --diff --app light@0.4.1
```

```
trace      fault.jktrace · node lab · generation 5 · light 0.4.0 · 15,000 ticks
rerun      light 0.4.1 (apps/light · sha256:5be1…) · recorded state, through migrate

output       differing ticks    from         to
pi.act_led   0                  —            —
esp.led      1,963              1m42.380s    2m21.640s

esp.led · tick 5,119 (1m42.380s)
  recorded  proposal light control:12 true → accepted · applied true
  new       no proposal · lease of light until 1m42.440s · applied true
esp.led · tick 5,122 (1m42.440s)
  recorded  proposal light control:12 true → accepted · applied true
  new       no active writer → safe · applied false
```

The difference is only where it should be: on `esp.led`, only during the fault. The Pi's LED, over 15,000 ticks, did not change a bit. This is what you take to a review before an update. The scenario of §4 passes the same with 0.4.1 (it says nothing about `esp.led` during the fault), and 0.4.1 goes to lab:

```sh
$ jk test --env sim
$ jk apply --env lab --node lab --confirm 5m && jk confirm --node lab
$ jk bench --env lab --node lab     # the app changed digest: the old WCET no longer holds (J0431)
```

The `.jktrace` does not go into Git: it is data, not code.

## 8. What happened underneath

| In this part | Where it is |
|---|---|
| deadband and `stuck_detect` belong to the acquisition stage, in this order, before any app | document 01 §3.3; ref/03 §3 ("Acquisition") |
| a `calc` inherits the worst quality of its operands; `good(x)` is meta | document 16 §3.2; document 02 §10 |
| `get_or` returns the substitute value and leaves a note in the trace; `get()?` in a cycle task keeps the proposals already made | document 16 §3.1; ref/04 §2.3, §2.6.1 |
| device down → `bad(comm_failure)` on every point; a dead driver would be `bad(not_connected)` | ref/06 §2.8 (quality that comes from the link) |
| keepalive, equipment fallback (M1) and consistency with the `safe` (`J0205`, `J0276`) | document 01 §8; ref/01 §5.8 |
| every write, with result, cause and inputs | M3; document 07 §4 (`jk decisions`) |
| the fault in the scenario (`[[inject]] device … disconnect`) and the expectations `quality`, `active`, `toggles_every` | ref/03 §2.5.3, §2.5.4 |

**Determinism: the clock is an input.** Each tick of the `.jktrace` keeps `t_mono`, `t_utc` and the clock quality, the inputs of each port of the group (16 B each), the proposals and the results (document 16, §7). In the rerun, `io.now()` and `io.dt()` come from the trace, and not from the laptop's clock (ref/04 §2.7: those of the latch); the core does not read the clock, it receives it (invariant 1, ADR 0007). The arbiter and the envelope that run in the replay are the same code that runs on the Pi. That is why the same app over the same trace gives **the same bytes** (Phase 0 exit gate), and a different app gives differences that can be counted tick by tick.

**The starting state.** `jk record` keeps, right after the header, the `export-state` of every app with `persist` state (the same as in a hot swap, ref/04 §4.12), and `jk replay` starts from that state, and not from zero: that is why you can start recording with the system on or off (document 16, §7; document 04, §7). With `--app light@0.4.1`, the state of 0.4.0 comes in through `migrate`, which matches fields by name. A trace from the continuous ring (`jk trace-dump`) does not have that segment: there the replay starts from `init`, and the first ticks may diverge from the recording until the state converges (`--diff` marks them "before the state").

## 9. Exercises

1. Change `calc.touch_level` to `clamp((60 - avg(esp.touch, 500ms)) / 45, 0, 1)`. Does the rhythm stop trembling when the finger is half touching? How many lines change in the app? (Answer: zero.)
2. Write a scenario in which the CPU goes from 60 °C to `bad(comm_failure)` for 5 s (`[[inject]] signal = "pi.cpu_temp"`). Which quality policy of the table in §3 decides, and what does `pi.act_led` do? Use `level = "none"` in an expectation.
3. Record 2 minutes in which you only play with the touch, and replay with 0.4.0 and 0.4.1. How many differing ticks do you expect? Confirm.
4. Replace `get_or(st.adjust)` with `get_or(NEUTRAL_ADJUST)` and run `esp_fault.scenario.toml`. Which expectation fails, with what rhythm seen, and why would a touch at 33 in the scenario not have caught it?

```sh
$ git add -A && git commit -qm "track(05): touch as an adjustment, ESP32 faults, fallback and replay (light 0.4.1)"
```

**State at the end of this part:** app `light` 0.4.1, running on lab (generation 6; 0.4.0 was 5). New or changed: `joker.toml` (`[signals."esp.touch"]`, `calc.touch_level`, `[alarms.esp_offline]`, port `adjust`), `sim/esp.sim.toml` (fault key), `sim/scenarios/esp_fault.scenario.toml`, `apps/light/`, `joker.lab.lock`.

**Next:** [Part 6 — Switching boards: the same project on the Pi Zero W, and the board that does not exist yet](06-switching-boards.md).
