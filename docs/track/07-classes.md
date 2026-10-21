# Part 7 — Classes: the same system at the equipment layer

**Module B · Classes.** In Module A you wrote everything at the **signal** layer: `Output<bool>`, `Input<f64>`, edges detected by hand, and the translation from "signal" to "meaning" inside the app. Now you climb one layer, and the behavior does not change.

**Goal:** rewrite `light` on top of **equipment classes** (`Led`, `Button`, `TempSensor`, `AnalogIn`), see the three ways an instance is born on the bench, and prove with the scenario `esp_fault` from part 5, without touching it, that the system does exactly the same.

**You will learn:** class, instance and member · `jk class list` and `jk class show` · implicit instances (`[implements]` in a profile) and what v1.0 does not let you do · `jk profile diff` · composite instances in the manifest · `#[device]` and class handles · methods and queries computed by the core · `raw()` · `jk status --instances` and `jk explain <instance>` · `jk class verify` on the hardware.

**Before you start:** part 6 done: `joker.rpi.toml`, `joker.lab.toml`, `joker.zero.toml`, `light` **0.4.1** running on zero, lab at generation 3 (without `esp`), and the scenario `sim/scenarios/esp_fault.scenario.toml` passing. The ESP32 connected by USB to the Pi 3 (parts 4–5) and visible from the Pi Zero over Wi-Fi (part 6).

---

**First, restore lab.** One client at a time (part 6, §7): stop zero, and put lab back on the complete `joker.lab.toml`, with `light` 0.4.1 and the ESP32 over RTU (generation 7). The rest of the part starts in the simulator.

```sh
zero$ sudo systemctl stop jokerd
$ jk apply --env lab --node lab --confirm 5m && jk confirm --node lab
```

## 1. Why, if it already works?

Reread the task `blink` of 0.4.1 and sort its lines into two piles. In one, **what the system does**: on or off, the half period as a function of the temperature, the touch moving `t_hot`, what to do when the ESP32 disappears. In the other, **how you talk to things**: keeping the button's previous value to see the edge, knowing that `esp.touch` at 60 is "no finger", knowing that `pi.act_led` is a `bool`. The second pile repeats itself in every app with a button or an LED. Classes take it out of your app.

```
 layer 3   your app: "one click turns it on, the hotter the faster"
 layer 2   classes: std/led@1, std/button@1, std/temp_sensor@1, std/analog_in@1   ← this part
 layer 1   signals: pi.act_led, pi.cpu_temp, esp.led, esp.button, esp.touch      ← Module A
```

Classes are a layer **on top of** signals, and not instead of them (document 14, §1 and §3). They compile to ports: the envelope, the arbiter, quality, the historian and replay stay the same, and layer 1 is still there for whoever needs it.

## 2. Get to know the classes

```sh
$ jk class list --std
```

```
std/led@1              1.0.0   Light indicator
std/button@1           1.0.0   Momentary push button
std/analog_in@1        1.0.0   Normalized analog input (0..1)
std/temp_sensor@1      1.0.0   Temperature sensor
std/humidity_sensor@1  1.0.0   Humidity sensor
std/relay@1            1.0.0   Relay
std/pump@1             1.2.0   Pump with start/stop and state feedback
std/meter.ac3@1, std/inverter.pv@2, std/battery.bess@1, …
```

```sh
$ jk class show std/button@1
```

```
std/button@1 (1.0.0) — Momentary push button
  members     pressed     in     bool
  queries     clicked     bool   rose(pressed)        true for a single tick of the group that reads it
              released    bool   fell(pressed)
              long_hold   bool   held(pressed, 1 s)
```

Notice the query `clicked`. It is the `pressed && !st.pressed` of your 0.4.1, with `pressed` kept in the task state. Here what keeps the previous value is the **core**: queries are derived signals, evaluated at the latch of each group that reads them, with the same evaluator as the manifest expressions (document 14, A7). Your app no longer has `pressed`, and Rust, TypeScript and the CLI see the same edge.

`jk class show std/led@1` shows the other side: a member `out on: bool @safe_hint(false)`, the methods `on()`, `off()`, `set(v)` and `toggle()`, and the optional capability `dimmable`. When a method has the name of a member, the handle gives the method and the member stays in `raw().on` (document 17, §3.2). Two things for section 6: a **method** is not code, it is an assignment that the core expands into a proposal (document 17, §4.1); and `@safe_hint` is only a suggestion. The `safe` of `pi.act_led` and `esp.led` is still the one in your `joker.toml` (ADR 0006).

## 3. Three ways to be born

An **instance** says "this equipment is, for programs, a `std/led@1`". On the bench you will use all three ways (document 14, §5):

| Way | Who declares it | When | On the bench |
|---|---|---|---|
| **implicit** | the equipment profile, with `[implements]` | the device **is** the equipment | `esp`, through the profile `local/bench-esp32@1` |
| **composite** | the manifest, with `[instances.<i>] map` | the equipment is made of loose signals, the device has no profile, or the site wants a name and a calibration of its own | `led_pi` and `cpu` (the `sysfs` driver has no profile); `led_esp`, `button` and `touch` |
| **virtual** | an app, with `#[joker::implements]` | the class needs behavior | `indicator`, in part 8 |

## 4. Implicit: the profile says what the ESP32 is

What knows what the bench firmware is, is the profile you wrote in part 4: an LED on a coil, a button on a discrete input, an analog reading on an input register. Add to it the classes it fulfills:

```toml
# profiles/local/bench-esp32@1.profile.toml (add at the end)
[implements."std/led@1"]
map = { on = "led" }

[implements."std/button@1"]
map = { pressed = "button" }

# value = 1 − raw/100: the firmware's nominal scale (0 to 100), and not the calibration of this bench
[implements."std/analog_in@1"]
map = { value = { signal = "touch", scale = -0.01, offset = 1 } }
```

**Several `[implements]`, a single instance.** The key of each table is the class, and a profile may fulfill up to 16 (ref/03 §2.1.5). But each device with the profile is **one** implicit instance, with the device's name (ref/03 §2.2.14): your `esp` becomes, at the same time, a `std/led@1`, a `std/button@1` and a `std/analog_in@1`. The members get paths in the instance (`esp.on` is an alias of `esp.led`, `esp.pressed` of `esp.button`), and that is why three classes fit in the same instance: there are no two members with the same name, nor a member with the name of another signal of the device (document 16, P3; otherwise, `J0604`). The direction is checked against the profile (an `out` only goes to an `output`: `J0824`), and the `scale`/`offset` of `value` is the adapter of 17 §5.2.

### What `jk check` says if you want another name

The temptation is to give the button a name of its own, `button`, right in the profile:

```toml
[implements."std/button@1"]
map       = { pressed = "button" }
instances = { button = { pressed = "button" } }
```

```
error[J0807]: key reserved for a future version: `instances` (1.x)
 --> profiles/local/bench-esp32@1.profile.toml:24:1
   |
24 | instances = { button = { pressed = "button" } }
   | ^^^^^^^^^ several implicit instances per device arrive in 1.x
   = note: in 1.0, each device with this profile is an implicit instance with the device's name (`esp`)
   = help: a different name is a composite instance in the manifest (ref/03 §2.2.14)
```

Delete the line. The profile says **what the equipment is**, for any site that uses it; your site's names come in section 5.

### The profile version goes up

A new `implements` is a **minor** change (ref/03 §2.2.17). Ask `jk`, against the version that is in Git:

```sh
$ git show HEAD:profiles/local/bench-esp32@1.profile.toml > /tmp/bench-esp32-before.profile.toml
$ jk profile diff /tmp/bench-esp32-before.profile.toml profiles/local/bench-esp32@1.profile.toml
```

```
local/bench-esp32   1.0.0 → 1.0.0
  + implements std/led@1          minor
  + implements std/button@1       minor
  + implements std/analog_in@1    minor
requires: minor (≥ 1.1.0); the file is still at 1.0.0
```

Change to `version = "1.1.0"` in `[profile]` and repeat: `1.0.0 → 1.1.0 ✓ minor`. A `local/` profile is never published (`J0848`), but the version matters all the same: `joker.lab.lock` and `joker.zero.lock` pin the profile by its version and by the digest of the file's bytes (ref/02 §2.1.5), and `jk check` in `dev` updates them with a note.

**In the simulator, the same.** The `sim` driver has no profile (ref/01, appendix A): the model declares its signals, and its classes too. Add the same three tables to `sim/esp.sim.toml` (ref/03 §2.4.9), so that `esp` is the same instance in both worlds.

## 5. Composite: the site's names and calibrations

The device `pi` uses the `sysfs` driver, which reads the signals directly from the manifest (`led = "ACT"`, `thermal = "thermal_zone0"`), without a profile. Without a profile there is no `[implements]`, and the Pi's instances are written in the manifest. So are the ESP32's, for three reasons that section 4 already hinted at. Since the paths `pi.*`, `esp.*` and `calc.*` exist in the simulator and on both boards, they all go into the **common** part:

```toml
# joker.toml (add)
[instances.led_pi]
class = "std/led@1"
map   = { on = "pi.act_led" }

[instances.cpu]
class = "std/temp_sensor@1"
map   = { temperature = "calc.temp_avg" }

[instances.led_esp]
class = "std/led@1"
map   = { on = "esp.led" }

[instances.button]
class = "std/button@1"
map   = { pressed = "esp.button" }

[instances.touch]
class = "std/analog_in@1"
map   = { value = "calc.touch_level" }
```

- **The names belong to the site.** In 1.0, the implicit instance is called `esp` and is all three things at once. The site wants to talk about `button` and `led_esp`, and that is a composite (the help of `J0807`).
- **The names survive the transport.** In part 9, the ESP32 becomes a Joker Micro and the device `esp` stops using this profile: `joker-link` generates its own from the firmware (ref/03 §2.2.16), and the implicit instance `esp` goes away with the profile. The paths `esp.led` and `esp.button` stay, and with them the composites, `light` and the scenarios.
- **`touch` and `cpu` have this bench's calibration.** `esp` is already an `analog_in@1`, with the firmware's nominal scale; but "60 without a finger, 15 with the finger" belongs to this ESP32 and this finger, and it stays in the `calc.touch_level` of part 5. It is the rule of document 14, §4.3: normalization belongs to the profile or to the instance, and here each says what it knows. In the same way, `cpu` points to `calc.temp_avg` (`avg(pi.cpu_temp, 10s)`), which is what 0.4 read: a composite binds members to **any** path, and a `calc` is a signal with the quality of whatever feeds it (document 16, §3).
- **Nothing else changes.** The `safe = false` of `pi.act_led` and `esp.led`, the `calc`s and the alarms stay where they were. The envelope is declared through **one** of a signal's paths (`esp.led`, `esp.on` or `led_esp.on`), never through two (`J0215`); leave it on the signal's own path.

The implicit instance stays all the same: it is what the profile promises to whoever uses it, and it is what a `jk class verify` against `esp` would confirm before you publish it in a registry.

## 6. The app, version 0.5

```rust
// apps/light/src/lib.rs
use joker::prelude::*;
use joker::classes::std::{AnalogIn, Button, Led, TempSensor};

#[joker::app(name = "light", version = "0.5.0")]
pub struct Light {
    #[device] led: Led,              // led_pi
    #[device] led_status: Led,       // led_esp
    #[device] temp: TempSensor,      // cpu
    #[device] button: Button,         // button
    /// 0 = no finger, 1 = finger on it.
    #[device] adjust: AnalogIn,      // touch

    #[param(unit = "ms", default = 500.0)] half_period: Param<f64>,
    #[param(unit = "°C", default = 45.0)]  t_cold: Param<f64>,
    #[param(unit = "°C", default = 70.0)]  t_hot: Param<f64>,
}

const BACKOFF_K: f64 = 15.0;
const SPAN_K: f64 = 25.0;
const NEUTRAL_ADJUST: f64 = BACKOFF_K / SPAN_K; // 0.6

#[joker::state(persist)]
struct State { enabled: bool, adjust: f64, elapsed_ms: f64, lit: bool }   // `pressed` is gone

impl Default for State {
    fn default() -> Self {
        State { enabled: true, adjust: NEUTRAL_ADJUST, elapsed_ms: 0.0, lit: false }   // starts on
    }
}

#[joker::cycle(period = "20ms", wcet = "20us")]
fn blink(io: &mut CycleIo<Light>, st: &mut State) -> Result<()> {
    // 1. Button. The edge is the query `clicked`, from the core. Without quality it is Err: not pressed.
    if io.button.clicked().unwrap_or(false) {
        st.enabled = !st.enabled;
        st.elapsed_ms = 0.0;
        st.lit = st.enabled;
    }
    // The ESP32's LED is only proposed when the ESP32 is seen (the 0.4.1 fix).
    if io.button.pressed().is_ok() {
        io.led_status.set(st.enabled);
    }

    // 2. Adjustment. The class has no get_or; the raw member has, and the substitution stays in the trace.
    st.adjust = io.adjust.raw().value.get_or(st.adjust);

    if !st.enabled {
        io.led.off();
        return Ok(());
    }

    // 3. Temperature. `?` → Err → the led's lease expires → safe (as in 0.2).
    let temp = io.temp.temperature()?;
    let t_cold = io.t_cold.get();
    let t_hot = io.t_hot.get() - BACKOFF_K + SPAN_K * st.adjust;
    let x = ((temp - t_cold) / (t_hot - t_cold).max(1.0)).clamp(0.0, 1.0);
    let half_ms = io.half_period.get() - x * (io.half_period.get() - 100.0);

    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;
    if st.elapsed_ms >= half_ms {
        st.elapsed_ms -= half_ms;
        st.lit = !st.lit;
    }
    io.led.set(st.lit);                   // on every tick: renews the lease
    Ok(())
}
```

```toml
# joker.toml ([apps.light] changes only in the bind)
[apps.light]
path   = "apps/light"
bind   = { led = "led_pi", led_status = "led_esp", temp = "cpu", button = "button", adjust = "touch" }
params = { half_period = "500 ms", t_cold = "45 °C", t_hot = "70 °C" }
cycles = { blink = "main" }
```

Everything else is 0.4.1 line by line: the constants, the `Default`, the half-period arithmetic. The ports have the **same names**; what changes are the types (from `Input<bool>` to `Button`) and, in the `bind`, the values (from signal paths to instance names). `jk check` confirms that each instance fulfills the class the port asks for, with the same major (`J0630`). The port name is the **role** in the app (`led_status`: the LED that shows whether it is on); the instance name is the **equipment** on the site (`led_esp`).

Compared with 0.4.1:

| Left the app | Where it went |
|---|---|
| `pressed` in the state and the `pressed && !st.pressed` | query `clicked` of `std/button@1`, computed by the core |
| `button.sample().quality.is_good()` | `button.pressed().is_ok()`: reading a member gives `Ok` only with `good` quality (ref/04 §4.8.2) |
| `button.get_or(false)` | `clicked()` returns `Err` when `pressed` is not `good`; `unwrap_or(false)` is the same rule: without a button, there are no edges |
| `temp.get()?` | `temperature()?`: `Err` (`J-E0101`) without `good` quality, as before |
| `adjust.get_or(st.adjust)` | `raw().value.get_or(st.adjust)`: the class has no `get_or`, the raw member has (document 14, §3) |
| `led.set(false)` | `led.off()`, a method of the class |
| signal paths in the bindings | instance names |

**Why not `io.led.toggle()`?** It was the temptation, and it would be a mistake. Proposing is renewing the lease (part 1): a `toggle()` every half period would leave the app hundreds of milliseconds without proposing, the 60 ms lease would expire, and the arbiter would apply the `safe` between two changes. `toggle()` is for "invert on request", as on a click, and not for a signal that is held on every tick. The app keeps storing `lit` and always calling `set`.

The **app tests** change, because the ports changed type: where 0.4 did `.input("temp", 70.0)`, 0.5 does `.device("temp", Sim::temp_sensor().constant(70.0))`, and the assertions read members (`h.device("led").member("on")`) instead of ports (ref/04 §6.6). The **scenarios** do not change, and that is the next section.

## 7. The proof: the scenario from part 5, unchanged

```sh
$ git status --short sim/scenarios/
$ jk test --env sim
```

```
apps/light (cargo test, wasm component on wasmtime) ... 6 passed
sim/scenarios/blink.scenario.toml (joker.sim.toml, 10 virtual s) ...... passed
sim/scenarios/heat_up.scenario.toml (joker.sim.toml) .................... passed
sim/scenarios/button.scenario.toml (joker.sim.toml) ..................... passed
sim/scenarios/esp_fault.scenario.toml (joker.sim.toml) ................. passed
```

`git status` shows nothing: no scenario was touched. `esp_fault` disconnects the simulated ESP32 at 20 s, checks that `pi.act_led` keeps blinking at 200 ms, that `esp_offline` goes off, that the press at 30 s does not count and that, at 41 s, the system is still on, and it passes. It does not know that the app changed layer, because it only looks at the site's signals, alarms and qualities. It is the best test of a refactoring: **the observable behavior is the same**.

`jk inspect apps/light` shows why: once the classes are expanded, the app has the same five ports as 0.4 (`led.on`, `temp.temperature`, `button.pressed`, `led_status.on`, `adjust.value`) plus the query `button.clicked`, and it is that flat list that the host, the arbiter and the trace see (document 14, §3). `jk inspect` lists the methods and queries each handle **knows**, and not the ones the code calls: that would require static analysis of the Wasm (ref/04 §5 and §9).

## 8. On the Pi 3 and on the Zero

Nothing changes in `joker.rpi.toml`, `joker.lab.toml` or `joker.zero.toml`. The composite instances are in the common part, and the implicit one comes from the profile, which both nodes use (Modbus RTU on the Pi 3, Modbus TCP on the Zero). Zero has been stopped since the beginning of the part: lab first.

```sh
$ jk check --env lab
```

```
✓ manifest      joker.lab.toml ← joker.rpi.toml ← joker.toml · 2 devices, 2 outputs, 1 app, 1 group
✓ instances     esp (std/led@1 + std/button@1 + std/analog_in@1, implicit through the profile local/bench-esp32@1)
                led_pi, led_esp (std/led@1) · button (std/button@1) · cpu (std/temp_sensor@1) · touch (std/analog_in@1): composite
✓ bindings      light.led → led_pi · light.led_status → led_esp · light.temp → cpu · light.button → button · light.adjust → touch
note: joker.lab.lock updated (profiles."local/bench-esp32@1" 1.1.0; classes std/led@1, std/button@1, std/analog_in@1, std/temp_sensor@1)
```

```sh
$ jk build --target rpi3b-plus
$ jk apply --env lab --node lab --confirm 5m
```

```
plan for lab (generation 7 → 8)
  profile    local/bench-esp32@1   1.0.0 → 1.1.0   (implements only: the driver does not restart)
  instances  + esp (implicit) · + led_pi, + led_esp, + button, + cpu, + touch (composite)
  apps       light 0.4.1 → 0.5.0     restarts (ports changed type)
  confirmation requested: without `jk confirm` within 5 min, back to generation 7
```

```sh
$ jk confirm --node lab
$ jk status --instances --node lab
```

```
instance   classes                                      origin                              health
button      std/button@1                                 manifest (joker.toml:49)            Ok
cpu        std/temp_sensor@1                            manifest (joker.toml:41)            Ok
esp        std/led@1 + std/button@1 + std/analog_in@1   profile local/bench-esp32@1.1.0     Ok
led_esp    std/led@1                                    manifest (joker.toml:45)            Ok
led_pi     std/led@1                                    manifest (joker.toml:37)            Ok
touch      std/analog_in@1                              manifest (joker.toml:53)            Ok
```

The `ACT` LED blinks as before, `BOOT` turns it on and off, the touch changes the rhythm. The complete chain of an instance:

```sh
$ jk explain button --node lab
```

```
button · std/button@1 · composite instance (joker.toml:49)
  pressed    in bool    button.pressed → esp.button · profile local/bench-esp32@1: discrete input 0 (FC 2)
                        device esp, modbus-rtu, unit 1 (joker.lab.toml:18) · debounce 20 ms (joker.toml:15)
                        poll 100 ms · false, good, age 38 ms
  clicked    query      rose(pressed), at the latch of main · read by light.button
  released   query      fell(pressed) · no readers
  long_hold  query      held(pressed, 1 s) · no readers
  health     Ok
  also       esp.pressed (implicit instance esp) is the same signal
```

`jk explain cpu --node lab` does the same: `cpu.temperature → calc.temp_avg = avg(pi.cpu_temp, 10s) → sysfs thermal_zone0 (joker.rpi.toml)`, with the value and the quality of each link; and `jk explain esp` shows the three classes of the implicit instance, with the profile lines that map them. That is what `jk explain` gives: each member down to the register or the `sysfs` file, the origin of each line (profile, manifest, overlay), the queries and who reads them.

On the Zero it is the same, with another transport. Switch clients (`lab$ sudo systemctl stop jokerd`, `zero$ sudo systemctl start jokerd`) and:

```sh
$ jk build --target rpi-zero-w
$ jk apply --env zero --node zero --confirm 5m && jk confirm --node zero
```

`jk status --instances --node zero` gives the same list, and `jk explain button --node zero` shows `modbus-tcp` where the Pi 3 showed `modbus-rtu`. The rest is the same. Go back to lab for the rest of the part and for part 8 (`zero$ sudo systemctl stop jokerd`, `lab$ sudo systemctl start jokerd`).

## 9. Verify a class on the hardware

The `std/` classes bring conformance cases (`classes/std/led.conformance.toml`, in the schema of ref/03 §2.6) that any instance must pass. Running those cases on `led_esp` proves that the instance, the profile and the site's envelope do what the class promises, with the real ESP32. In the simulator you run it without a second thought; on the hardware, the cases turn the LED on and off for real:

```sh
$ jk class verify std/led@1 --instance led_esp --env lab --node lab
```

```
error[J0640]: `jk class verify` on a non-simulated instance without `--allow-actuation`
  = note: `led_esp` is modbus-rtu on node lab: the cases will actuate the equipment
  = help: --allow-actuation (only on a node with mode = "dev"; the test writer goes through the site's envelope)
```

On a real instance, the test writer is not the simulator's `control:12`: it writes as `manual:3`, with a TTL equal to the duration of the suite, and beats the apps without stopping them. `light` keeps running and stays `overridden(manual:3)` while the test lasts. If a `safety` or `manual` level were already active on the instance's outputs (a `jk write` of yours, for example), `verify` would refuse with `J0641`: you do not test on top of someone's decision.

```sh
$ jk class verify std/led@1 --instance led_esp --allow-actuation --env lab --node lab
```

```
std/led@1 · instance led_esp (composite: on → esp.led, modbus-rtu) · node lab · mode dev
warning: real actuation; writer test:std/led@1 at manual:3, TTL 2.5 s (the duration of the suite)
  on_sets_on ......................... ok
  off_clears_on ...................... ok
  toggle_inverts_own ................. ok
  lease_expiry_goes_safe ............. skipped (sim_only)
4 cases · 3 ok · 1 skipped (sim_only) · 1.4 s
```

The last case is marked `sim_only = true` in `led.conformance.toml`: it expects that, with nobody in control, the LED goes to `safe`, and that requires **exclusive ownership** of the instance. Here `light` keeps proposing underneath `manual:3`, and when the test's lease expires `esp.led` goes back to `light`'s value, and not to `safe`. Instead of letting it fail, `verify` skips it and does not count it in the exit code (ref/03 §2.6.3, rule 6); that is why it is no longer necessary to stop `light` to obtain that ownership: the only case that needed it does not run on the hardware.

In the simulator, the same case runs like the others: `jk class verify std/led@1 --instance led_esp --env sim` gives `4 cases · 4 ok · 0 skipped`. There the case only passes because the site's `safe` coincides with the class's `@safe_hint` (`false`). If your `esp.led` had `safe = true`, it would be skipped with `J0846`: the `safe` belongs to the site, and the suite cannot presume it. With `--instance esp`, the same cases verify the mapping of the profile itself, and with `--report bench-esp32.cbor` they keep the report that publishing a profile with `implements` requires (ref/03 §2.8).

## 10. Swapping the LEDs: one line

Do you want the ESP32's LED to blink and the Pi's to show the state? Or do you have an ESP32 board without an LED on GPIO2 (some do not, document 23 §3)? The app does not know which LED it is: it knows it has one that blinks and one that shows the state. You change the `bind` line:

```toml
bind = { led = "led_esp", led_status = "led_pi", temp = "cpu", button = "button", adjust = "touch" }
```

With `jk dev --env sim --panel` running, save: the ESP32 lamp starts blinking and the Pi's stays steadily on. No line of Rust changed, no envelope changed (`esp.led` already had `safe`), and `jk check` has nothing to say.

Now run `jk test`: `esp_fault` **fails**, with `J0842` on `pi.act_led`. And it should fail. The scenario checks the behavior of the **site** ("with the ESP32 down, the Pi keeps blinking"), and you changed the site's behavior, even though you did not change the app. Put the line back as it was.

## 11. What happened underneath

- **Classes are resolved at build time and in `jk check`** (document 14, A12). Each member is a port; each method is a record of ~16 B in the proposal buffer, which the core expands in step 5a of the tick (document 17, §4.1); each query is a derived signal. There is no dynamic dispatch in the tick.
- **The implicit instance comes from the profile, and its shape is fixed in 1.0**: one device, one instance, with the device's name and the classes the profile declares (ref/03 §2.2.14). The classes' `.contract.json` files go inside the generation, and the lock pins the version and the contract hash of each one (ref/02 §2.1.6; `J0422` if the hash changes without the version).
- **Quality still belongs to the signal.** `clicked()` and `temperature()` return `Err` for the same reason as 0.4's `get()`: the input is not `good`. The health of an instance (`health()`, document 17 §4.5) combines that of the required members, without inventing averages. `jk trace pi.act_led` still shows `app:light.blink` proposing: the bindings changed layer, the signals did not.

## 12. Exercises

1. Use `io.button.long_hold()` so that a 1 s press always turns the system off, whether it is on or not. With the current code, a long press also fires `clicked` at the start: how do you prevent it from turning the system on before turning it off? Write the test first, with `Sim::button().press_at(1.s(), 1200.ms())`.
2. Change `adjust = "touch"` to `adjust = "esp"` in the `bind` (the implicit instance is also an `analog_in@1`). What changes in the blink rhythm when you touch? Why? Which of the two normalizations should go into a published profile?
3. Change `cpu` to `temperature = "pi.cpu_temp"` and run `heat_up.scenario.toml`. What happens to the LED's rhythm, and which part of the scenario fails?
4. Unplug the ESP32's USB cable with the system running on the Pi 3 and look at `jk status --instances --node lab` and at `jk explain button`. Which members become `bad(comm_failure)`, and what stays `Ok`?

```sh
$ git add -A && git commit -qm "track(07): light 0.5 on top of classes; implements in the ESP32 profile; composite instances"
```

**State at the end of this part:** app `light` **0.5**, on lab (generation 8) and on zero (generation 2, stopped). Instances: `led_pi`, `led_esp`, `button`, `cpu`, `touch` (composite, in `joker.toml`) and `esp` (implicit, through the profile). Files changed: `joker.toml` (the five `[instances.*]` and the `bind` of `light`), `profiles/local/bench-esp32@1.profile.toml` (three `[implements]`, version 1.1.0), `sim/esp.sim.toml` (the same `[implements]`), `apps/light/`, `joker.sim.lock`, `joker.lab.lock`, `joker.zero.lock`. The scenarios in `sim/scenarios/` did not change.

**Next:** [Part 8 — Your own class: `status_light@1`](08-your-own-class.md).
