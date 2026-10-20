# Part 8 — Your own class: `status_light@1`

**Goal:** take out of `light` the last thing that is not business (the blink), turn it into a class of your own, `local/status_light@1`, with a contract, conformance cases and a reusable implementation, and end with a business app of ~20 lines that reads like the specification of the system.

**You will learn:** `jk class new` · the `.jkc` IDL: enums, `out`, methods, atomic methods, queries · the conformance file in the single schema · **virtual** instances and `#[joker::implements]` · virtual commands with envelope and `safe` · `order` in the group · `jk class verify` in the simulator and on the Pi 3 · safety at every layer · the four people of document 14.

**Before you start:** part 7 done: `light` **0.5** on top of classes, the composite instances `led_pi`, `led_esp`, `button`, `cpu`, `touch` and the implicit `esp`, `esp_fault` passing. The Pi 3 with the ESP32 over USB.

---

## 1. The smell

Reread the `blink` of 0.5. Half of it is still about **how** to show a state with an LED: `lit`, `elapsed_ms`, subtracting the half period, proposing on every tick so as not to lose the lease. None of this is about temperature. If tomorrow another project needs a light that blinks, it will copy these lines, and each copy will diverge.

The design question: **what does the business app want to say to the light?** Four things: off, on, blinking with a period, and alert. That is a contract, and a contract is a class.

## 2. Create the class

```sh
$ jk class new local/status_light@1
```

```
created classes/local/status_light.jkc
created classes/local/status_light.conformance.toml
note: the package `local` belongs to the project: it has no `.package.toml` and is never published (ref/03 §2.7)
```

The conformance file has the **same name** as the `.jkc` (`status_light.jkc` ↔ `status_light.conformance.toml`, ref/03 §2.7). Write the class:

```
// classes/local/status_light.jkc
package local

/// What a status light is showing.
enum LightMode { off = 0, on = 1, blink = 2, alert = 3 }

/// Status light: off, on, blinking with a period, or in alert.
/// It says what, and not how: the implementation may be an LED, a beacon or a screen.
class status_light@1.0.0 {
    /// The requested mode.
    out mode: LightMode              @safe_hint(LightMode.off)

    /// Full period of the blink, used in `blink` mode.
    out blink_period: f64 ms         @safe_hint(1000 ms) @range_hint(40 ms, 10000 ms)

    method off()   { mode = LightMode.off }
    method on()    { mode = LightMode.on }
    method alert() { mode = LightMode.alert }

    atomic method blink(period: f64 ms) order(blink_period, mode) {
        blink_period = clamp(period, 40 ms, 10000 ms)
        mode         = LightMode.blink
    }

    /// The light is asking for attention.
    query alerting: bool = mode == LightMode.alert
}
```

The decisions are yours, as the class author, and each has a reason in document 17:

- **`blink(period)` is not an enum variant.** An IDL enum is a list of names with numbers, without data (17 §3.3), because the `in`/`out` members are always scalars or enums. The period is a separate member, and the method writes both.
- **The method is `atomic`, with `order(blink_period, mode)`.** The period and the mode reach the implementation together: there is never a tick with `blink` and the old period (M20; document 14, A5). Without `order`, it is `JC0205`.
- **The current mode does not need a query.** Reading an `out` gives the **applied** value, after the arbiter and the envelope (17 §11): `io.indicator.mode()` in an app, `jk read indicator.mode` in the CLI. The query `alerting` is something else: a derived value that an alarm, an HMI or a bridge can use without knowing the enum.
- **Outputs only, and nothing about LEDs.** Whoever uses the light tells it what to show, and does not ask it anything (a pump, with feedback, would have `in`). There is no `on: bool`, no pins, no alert pattern: that belongs to the implementation.
- **`@safe_hint` and `@range_hint` are suggestions.** The `safe` belongs to the site (ADR 0006), and you will declare it in section 5.

### What `jkc check` says

The first version everybody writes calls the member `period`, like the argument. `jk class check classes/local/status_light.jkc` rejects it:

```
error[JC0206]: argument with the same name as a member: `period`
 --> classes/local/status_light.jkc:17:25
   |
17 |     atomic method blink(period: f64 ms) order(period, mode) {
   |                         ^^^^^^ members, arguments and queries share the class namespace
   = help: rename the member (`blink_period`) or the argument
```

With the names of the version above: `✓ local/status_light@1.0.0 · 2 members · 4 methods (1 atomic) · 1 query · 1 enum · contract sha256:5b0e…`.

From here on, `jk class show local/status_light@1` shows your class as it showed `std/button@1`, and the apps' `build.rs` generates `crate::classes::local::{StatusLight, LightMode}` (ref/04 §4.8.1).

## 3. The conformance contract

A class with outputs only has no feedback to observe. The cases check the **contract**: that the methods expand into what they promise, that `blink` is atomic and bounded, and that, with nobody in control, the light goes to `safe`. The blink itself is tested in the implementation's tests (section 4), and not here (ref/03 §2.6.5).

```toml
# classes/local/status_light.conformance.toml
schema = 1

[suite]
kind        = "class"
class       = "local/status_light@1"
description = "contract: method expansion, atomic and bounded blink, safe after the lease"

[[case]]
id     = "on_sets_mode"
do     = [{ at = "100ms", method = "on" }]
expect = [{ member = "mode", equals = "on", at = "100ms" }]

[[case]]
id     = "blink_is_atomic_and_clamped"
do     = [{ at = "100ms", method = "blink", args = { period = "10 ms" } }]
expect = [{ all_of = [{ member = "blink_period", equals = "40 ms", at = "100ms" },
                      { member = "mode", equals = "blink", at = "100ms" }] }]

[[case]]
id     = "alert_raises_query"
do     = [{ at = "100ms", method = "alert" }]
expect = [{ query = "alerting", equals = true, within = "100ms .. 200ms" }]

[[case]]
id       = "lease_expiry_goes_off"
sim_only = true          # needs exclusive ownership: on a real instance, the app keeps proposing
do       = [{ at = "100ms", method = "on" }, { at = "1s", lease_expire = "mode" }]
expect   = [{ member = "mode", equals = "off", level = "none", within = "1s .. 1.1s" }]
```

It is the **same schema** as the core and driver cases (`[suite]`, `[[case]]`, `id`, `do`, `expect`); only the *fixture* changes (ref/03 §2.6). The runner is a single writer, `test:local/status_light@1`, that keeps its proposals on every tick as an app would. The instants are multiples of the period of the instance's owning group (`main`, 20 ms). The last case has two conditions. It is marked `sim_only = true`: it expects `level = "none"`, and that can only be seen with **exclusive ownership** of the instance, which only simulation gives; on a real instance, the apps keep proposing underneath the test writer, and `verify` reports it `skipped (sim_only)` (ref/03 §2.6.3, rule 6). And it only runs if the site's `safe` coincides with the `@safe_hint`: otherwise, it is skipped with `J0846`, because the `safe` belongs to the site and the suite cannot presume it.

## 4. The implementation: `status_light_ctl`

The implementation receives the class's **commands** (`io.cmd.mode`, `io.cmd.blink_period`: the applied values, after the arbiter and the envelope) and uses a `Led` to show them. This is where the code that leaves `light` goes.

```sh
$ jk app new status_light_ctl --lang rust
```

```rust
// apps/status_light_ctl/src/lib.rs
use joker::prelude::*;
use joker::classes::std::Led;
use crate::classes::local::{LightMode, StatusLight};   // generated by build.rs from classes/local/

/// Implements local/status_light@1 on top of an LED.
#[joker::app(name = "status_light_ctl", version = "1.0.0")]
#[joker::implements(StatusLight)]
pub struct StatusLightCtl {
    #[device] led: Led,
}

#[derive(Default)]
struct Pattern {
    mode: Option<LightMode>,
    t_ms: f64,        // time since the start of the current pattern
    lit: bool,
}

#[joker::cycle(period = "20ms", wcet = "5us")]
fn run(io: &mut CycleIo<StatusLightCtl>, st: &mut Pattern) -> Result<()> {
    let mode = io.cmd.mode.get()?;                   // without a writer, the command is at its safe: off
    if st.mode != Some(mode) {                       // the mode changed: the pattern restarts, and the blink starts on
        *st = Pattern { mode: Some(mode), lit: mode == LightMode::Blink, ..Pattern::default() };
    }
    st.t_ms += io.dt().as_secs_f64() * 1000.0;

    st.lit = match mode {
        LightMode::Off => false,
        LightMode::On => true,
        LightMode::Blink => {
            let half = io.cmd.blink_period.get()? / 2.0;
            if st.t_ms >= half { st.t_ms -= half; !st.lit } else { st.lit }
        }
        LightMode::Alert => {
            let t = st.t_ms % 1000.0;                // double flash per second
            t < 100.0 || (200.0..300.0).contains(&t)
        }
    };
    io.led.set(st.lit);                            // on every tick: renews the LED's lease
    Ok(())
}
```

`#[joker::implements(StatusLight)]` generates `io.cmd`, with an input handle for each `out` of the class, and `io.state`, which is empty here because the class has no `in` (ref/04 §4.8.4). A class name that does not exist, or a capability it does not have, is `J-E0715` at build time.

The implementation has its own tests, independent of whoever uses it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use joker_test::prelude::*;

    #[test]
    fn blink_of_200ms_toggles_every_100ms() {
        let mut h = Harness::<StatusLightCtl>::new()
            .group("run", 20.ms())
            .device("led", Sim::of::<Led>())
            .command("mode", LightMode::Blink)
            .command("blink_period", 200.0);
        h.run_for(2.s());
        assert!(h.device("led").member("on").after(200.ms()).toggles_every(100.ms()));
    }
}
```

`.command(…)` gives the implementation a virtual command, as if someone had sent it (ref/04 §6.2). A second test, for `alert`, counts 4 changes per second (`between(500.ms(), 2500.ms()).toggles() == 8`).

## 5. The manifest: a virtual instance

```toml
# joker.toml (add)
[instances.indicator]
class          = "local/status_light@1"        # only std/ may be omitted
implemented_by = "app:status_light_ctl"        # virtual instance

# Virtual commands are outputs like the others: they have an arbiter, an envelope and a safe.
[outputs."indicator.mode"]
safe = "off"

[outputs."indicator.blink_period"]
safe = "1000 ms"

[apps.status_light_ctl]
path   = "apps/status_light_ctl"
bind   = { led = "led_pi" }
cycles = { run = "main" }
```

```toml
# joker.toml ([apps.light] and [cycles.main] change)
[apps.light]
path   = "apps/light"
bind   = { indicator = "indicator", led_status = "led_esp", temp = "cpu", button = "button", adjust = "touch" }
params = { half_period = "500 ms", t_cold = "45 °C", t_hot = "70 °C" }
cycles = { blink = "main" }

[cycles.main]
period = "20ms"
order  = ["light", "status_light_ctl"]           # who commands before who executes
```

- **`implemented_by = "app:status_light_ctl"`** uses the `bind`, `params` and `cycles` of `[apps.status_light_ctl]`, and serves one instance (ref/01 §5.13). For three lights with the same implementation, the table form (`{ app, bind }`) creates one instance of the app per virtual instance (document 17, §5.3).
- **`pi.act_led` now has another writer**: `status_light_ctl`, through the instance `led_pi`. `light` no longer touches it. Each output still has a single writer, and the default `writer` is enough.
- **`order`.** Without it, tasks run in lexicographic order of `<app>.<task>` (ref/01 §5.11), which by chance also puts `light` first. With it, the order is a written decision, and not an accident of the names: the core arbitrates and applies the envelope to the virtual commands right after `light`, and `status_light_ctl` reads them in the same tick. The latency between the layers is zero; the other way round, it would be one 20 ms tick (document 14, §7.2). An `order` entry that does not run in the group is `J0281`.

### What `jk check` says without the `safe`

Delete `[outputs."indicator.blink_period"]` and check:

```sh
$ jk check --env sim
```

```
error[J0201]: output without `safe`: `indicator.blink_period`
 --> joker.toml:61:1
   |
61 | [instances.indicator]
   | ^^^^^^^^^^^^^^^^^ virtual instance: each `out` of the class is a command with an envelope
   = note: suggestion from the class local/status_light@1: @safe_hint(1000 ms)
   = help: [outputs."indicator.blink_period"] safe = "1000 ms"
```

The class suggests, the site decides. Put the table back.

## 6. The business app, version 0.6

```rust
// apps/light/src/lib.rs
use joker::prelude::*;
use joker::classes::std::{AnalogIn, Button, Led, TempSensor};
use crate::classes::local::StatusLight;

#[joker::app(name = "light", version = "0.6.0")]
pub struct Light {
    #[device] indicator: StatusLight,    // what to show, and not how
    #[device] led_status: Led,
    #[device] temp: TempSensor,
    #[device] button: Button,
    #[device] adjust: AnalogIn,
    #[param(unit = "ms", default = 500.0)] half_period: Param<f64>,
    #[param(unit = "°C", default = 45.0)]  t_cold: Param<f64>,
    #[param(unit = "°C", default = 70.0)]  t_hot: Param<f64>,
}

#[joker::cycle(period = "20ms", wcet = "20us")]
fn blink(io: &mut CycleIo<Light>, st: &mut State) -> Result<()> {
    if io.button.clicked().unwrap_or(false) {
        st.enabled = !st.enabled;
    }
    if io.button.pressed().is_ok() {
        io.led_status.set(st.enabled);
    }
    st.adjust = io.adjust.raw().value.get_or(st.adjust);
    if !st.enabled {
        io.indicator.off();
        return Ok(());
    }
    let temp = io.temp.temperature()?;           // without temperature: the indicator's lease expires → off
    let t_cold = io.t_cold.get();
    let t_hot = io.t_hot.get() - BACKOFF_K + SPAN_K * st.adjust;
    let x = ((temp - t_cold) / (t_hot - t_cold).max(1.0)).clamp(0.0, 1.0);
    let half_ms = io.half_period.get() - x * (io.half_period.get() - 100.0);
    io.indicator.blink(2.0 * half_ms);
    Ok(())
}
```

The constants `BACKOFF_K`, `SPAN_K` and `NEUTRAL_ADJUST` and the `Default` of `State` are those of 0.5; `State` lost `elapsed_ms` and `lit`, which went to the implementation's `Pattern`, and becomes `State { enabled, adjust }`. It reads like the specification of part 5: *without temperature, nothing; a click turns it on and off; the ESP32's LED shows whether it is on; the touch adjusts `t_hot`; when on, it blinks with the half period of the temperature.* **This is layer 3.**

Notice that `blink()` and `off()` are called on every tick. Proposing the same value renews the lease of the virtual command, and the core sends nothing to the implementation when the applied value does not change. If `light` dies, the lease of `indicator.mode` expires and the light goes to `off`.

The business tests no longer talk about LEDs: with `.device("indicator", Sim::of::<StatusLight>())`, at 70 °C and adjustment 0.6 (`t_hot` = 70 °C), they check that `h.device("indicator").member("mode").last()` is `Some(LightMode::Blink)` and that `blink_period` is 200 ms. The blink itself is tested once, in the implementation.

## 7. In the simulator

```sh
$ jk test --env sim
```

```
apps/light (cargo test, wasm component on wasmtime) ............... 5 passed
apps/status_light_ctl (cargo test, wasm component on wasmtime) .. 2 passed
sim/scenarios/blink.scenario.toml (joker.sim.toml) .............................. FAILED
  error[J0842]: expectation failed: 8.1s pi.act_led: level expected "none", seen "control:12" (app:status_light_ctl)
sim/scenarios/{heat_up,button}.scenario.toml (joker.sim.toml) ..................... 2 passed
sim/scenarios/esp_fault.scenario.toml (joker.sim.toml) .......................... passed
```

`esp_fault` keeps passing, now with **two** layered apps between `light` and `pi.act_led`. Seen from outside, the behavior has been the same since 0.4.

The `blink.scenario.toml` from part 1 fails, and rightly so. It said "`light` stops, and the LED goes to `safe` without anybody writing" (`level = "none"`). Now `light` no longer writes to the LED: when it stops, it is the lease of `indicator.mode` that expires, the `safe` is applied **one layer up** (`off`), and `status_light_ctl` turns the LED off at `control:12`. The LED goes off all the same; what changed is who turns it off. Fix the expectation to say that, and not the app:

```toml
# sim/scenarios/blink.scenario.toml (the last expectation becomes two)
[[expect]]
at     = "8.1s"
signal = "pi.act_led"
equals = false

[[expect]]
at     = "8.1s"
signal = "indicator.mode"
equals = "off"
level  = "none"                     # the virtual command's safe: nobody controls the light
```

Now the contract, against the site's real instance (in simulation):

```sh
$ jk class verify local/status_light@1 --instance indicator --env sim
```

```
local/status_light@1 · instance indicator (virtual, app:status_light_ctl) · joker.sim.toml · virtual clock
  on_sets_mode ...................... ok
  blink_is_atomic_and_clamped ....... ok
  alert_raises_query ................ ok
  lease_expiry_goes_off ............. ok   (safe = off on the site, equal to the @safe_hint)
4 cases · 4 ok · 0 skipped · 1.1 simulated s in 22 ms
```

For a virtual instance, `verify` runs the cases against the app that implements it, with the real core and a virtual clock (ref/04 §6.7). Whoever else writes to the instance (`light`) does not take part.

And the whole chain:

```sh
$ jk explain indicator --env sim
```

```
indicator · local/status_light@1 · virtual instance, implemented by app:status_light_ctl (joker.toml:61)
  mode           out LightMode   writers: light (control:12) · safe = off (joker.toml:66) · safe_hint off ✓
  blink_period   out f64 ms      writers: light (control:12) · safe = 1000 ms (joker.toml:69) · safe_hint 1000 ms ✓
  atomic group   blink → (blink_period, mode)
  alerting       query           mode == LightMode.alert
  latency        light → status_light_ctl: 0 ticks (main, order declared)
  implementation status_light_ctl.led → led_pi (std/led@1) → pi.act_led → sim (laptop) / sysfs ACT (boards)
```

## 8. On the Pi 3

```sh
$ jk build --target rpi3b-plus
$ jk apply --env lab --node lab --confirm 5m
```

```
plan for lab (generation 8 → 9)
  classes    + local/status_light@1 1.0.0 (contract sha256:5b0e…)
  instances  + indicator (virtual) · outputs + indicator.mode, + indicator.blink_period
  apps       light 0.5.0 → 0.6.0 restarts · + status_light_ctl 1.0.0 · main: order = [light, status_light_ctl]
```

After `jk confirm --node lab`, the `ACT` LED blinks as before. `joker.lab.toml` did not change: the class, the instance, the outputs and the two apps are in the common part.

**The contract on the hardware.** The same cases, now with the real LED. The test writer writes at `manual:3`, and `light` keeps running, `overridden(manual:3)` while the suite lasts (as in part 7):

```sh
$ jk class verify local/status_light@1 --instance indicator --allow-actuation --env lab --node lab
```

```
local/status_light@1 · instance indicator (virtual, app:status_light_ctl) · node lab · mode dev
warning: real actuation; writer test:local/status_light@1 at manual:3 (light stays overridden)
  on_sets_mode ...................... ok
  blink_is_atomic_and_clamped ....... ok
  alert_raises_query ................ ok
  lease_expiry_goes_off ............. skipped (sim_only)
4 cases · 3 ok · 1 skipped (sim_only)
```

There is no need to stop `light`: the only case that required exclusive ownership of the instance, `lease_expiry_goes_off`, is marked `sim_only` and does not run here (in the simulator, it ran and passed). During those seconds, the `ACT` LED lights steadily, blinks very fast and does the double flash; at the end of the suite, `manual:3` is released and the LED goes back to `light`'s blink. It is the implementation responding to the contract, and not the test moving the LED. The "with nobody in control, it turns off" can be seen on the hardware in the first experiment below.

**Breaking the layers one by one.** Three experiments that show that each layer is safe on its own (document 14, §7.2):

1. **Stop the business app:** `jk app stop light --node lab`. The lease of `indicator.mode` expires, the arbiter applies the `safe` (`off`), and `status_light_ctl` turns the LED off. The implementer does not need to know that the one in command died. `jk app start light --node lab`.
2. **Stop the implementation:** `jk app stop status_light_ctl --node lab`. Now it is the lease of `pi.act_led` that expires, and the LED goes off through the `safe` of layer 1. `light` keeps sending `blink`, but nobody executes it, and `jk status --instances --node lab` shows `indicator … implementation stopped`. `jk app start status_light_ctl --node lab`.
3. **Command the middle layer.** Force the alert for one minute, to see whether the LED stands out from across the room:

```sh
$ jk call indicator.alert --ttl 1m --reason "visibility test" --node lab
```

The method is called as a manual writer (`manual:4`, which beats `light`'s `control:12`) and expanded by the core like any other (document 17, §4.1). The LED does the **real** double flash, generated by the implementation, and after one minute it goes back to `light`'s blink. A `jk write pi.act_led true` would only give you a lit LED.

## 9. What happened underneath

- **The class is a compiled contract.** `jkc` generated `status_light@1.0.0.contract.json`, with the contract hash that the lock pins (`[classes."local/status_light@1"]`, ref/02 §2.1.6), and the Rust types (`StatusLight`, `LightMode`, `StatusLightCmd`) that both apps import (document 17, §6). Changing the body of a method or removing a member requires major 2 (17 §9; `JC0601` if you do not bump it).
- **A method is a record, and not a call.** `io.indicator.blink(…)` puts `(indicator, blink, args)` in the proposal buffer; in step 5a of the tick, the core evaluates the assignments and creates two proposals in an atomic group, with the order of `order(…)` (document 17, §4.1). `jk call` goes through the same path.
- **Virtual commands are outputs.** They have an arbiter, an envelope and a `safe`, and two apps commanding the same light are resolved **before** they reach the implementation. With `order`, the arbitration of the commands happens right after the task that produces them (document 01, §4, step 4b). `light` → `indicator` → `led_pi` is one virtual layer; the limit is 3 (`J0620`).

## 10. Who did what: the four people, on your bench

Without noticing, you played all the roles of document 14, §1. In a team, they are usually different people, and each one only needs to know their own layer and the contract of the one below.

| Who | In your directory | Needs to know | Does not need to know |
|---|---|---|---|
| **The integrator** (hardware integrator) | `joker.lab.toml`, `joker.zero.toml`, `joker.rpi.toml`; the profile `local/bench-esp32@1` with its `[implements]`; the composite instances `led_pi`, `led_esp`, `button`, `cpu`, `touch` | registers, `sysfs`, calibrations, `safe` | the logic of `light` |
| **The abstraction creator** (abstraction author) | `classes/local/status_light.jkc` and its `.conformance.toml` | what programs want to say to a light | that there is an `ACT` LED underneath |
| **The implementer** (also an abstraction author, in document 14) | `apps/status_light_ctl/` and its tests | the class it implements and the class it uses (`std/led@1`) | who will command it, and why |
| **The user** (business programmer; and the operator, who only reads and commands through methods) | `apps/light/` 0.6; `jk status --instances`, `jk explain`, `jk call indicator.alert` | temperature, clicks, what the light should show | registers, pins, protocols, blink patterns |

Document 14 puts "the implementer" inside the abstraction author (a class and its active implementations usually go together) and gives the operator a role of its own. On the bench it is worth separating them, because `status_light_ctl` is the only piece that knows both classes: it is the seam between layer 2 and layer 1.

**End of Module B.** You have mastered layer 1 (Module A), and layer 2 used and created: standard classes, implicit, composite and virtual instances, and a class of your own with a contract, conformance and an implementation. Layer 3 says what the system does, and nothing more.

## 11. Exercises

1. Add a method `blink_once()` to the class (one 200 ms flash, then `off`). Can you write it with assignments only? If not, where does that behavior live? Would it be a minor or a major of `status_light@1`?
2. Write a second implementation, `status_light_duo`, that shows the mode on **both** LEDs (`on` on the Pi's, `alert` blinking both alternately). Change only the `implemented_by` and the `bind` of the new app. Does `light` change? And does `esp_fault` pass? Why?
3. Remove the `order` from `[cycles.main]` and rename the app to `zz_light`. What does `jk explain indicator` say about the latency? Which conformance case or scenario would catch it, if any?
4. Add to `light` the `alert` mode when the alarm `cpu_hot` is active, with a layer-1 port bound to `alarm.cpu_hot.active`. It is version 0.7 of part 11: write the test first.
5. (With the extra of section 12.) Write a case in `bench_io.conformance.toml` that checks that `online` becomes false when `uptime` stops being `good`. Is it a `sim_only` case? Why?

```sh
$ git add -A && git commit -qm "track(08): class local/status_light@1, status_light_ctl, light 0.6"
```

## 12. Extra: the whole ESP32 in a class

Optional, about 20 minutes, in the simulator only. The following parts do not assume it, so it is done on a separate branch, which you leave at the end.

```sh
$ git switch -c extra-bench-io
$ jk class new local/bench_io@1
```

```
created classes/local/bench_io.jkc
created classes/local/bench_io.conformance.toml
```

Up to here, the ESP32 reaches the apps as three loose instances (`led_esp`, `button`, `touch`) and two raw signals (`esp.rssi`, `esp.uptime`). A **composite class** brings them together into a single logical piece of equipment, with members that are other classes:

```
// classes/local/bench_io.jkc
package local

/// The bench I/O node: a status LED, a button, a touch, and whether it is alive.
/// It describes the role (the I/O node), and not the chip: another node with the same parts fulfills it.
class bench_io@1.0.0 {
    member status: std/led@1
    member boot:   std/button@1
    member knob:   std/analog_in@1

    /// Wi-Fi strength, in dBm; 0 without connection.
    in rssi:   i64
    /// Time since the node booted.
    in uptime: f64 s

    query online: bool = good(uptime)

    health required(uptime)
}
```

- **The name is the role.** `bench_io` says what it is for in the program; a class `esp32@1` would name the part, and would lie the day the node became another (document 14, §2).
- **The `member` members are classes**, and not signals. The depth is 2 (`bench_io` → `led`), below the maximum of 3 (`JC0501`, document 17, §8).
- **Health combines both things** (document 17, §4.5): `Failed` if `uptime` is not `good`, and the worst of the healths of `status`, `boot` and `knob`, which are required. `rssi` does not count: in part 9, without Wi-Fi, it becomes `Bad(NotConnected)`, and the bench stays `Ok`.

The `.conformance.toml` keeps the skeleton: `local` is never published (ref/03 §2.7), and exercise 5 asks you for a case.

**The instance.** Class members are bound to the **name of an instance** that already exists; scalars, to signals, as always (ref/01 §5.13):

```toml
# joker.toml (add, on the branch)
[instances.esp_io]
class = "local/bench_io@1"
map   = { status = "led_esp", boot = "button", knob = "touch", rssi = "esp.rssi", uptime = "esp.uptime" }
```

`jk check` checks that `led_esp`, `button` and `touch` exist and are of the right classes (`J0650`, `J0651`). The instances are still there, with their names: `esp_io.boot.clicked` and `button.clicked` are the **same signal**, and so are `esp_io.status.on`, `led_esp.on` and `esp.led` (document 16, P6).

**A diagnostics app.**

```rust
// apps/bench_diag/src/lib.rs
use joker::prelude::*;
use crate::classes::local::BenchIo;

#[joker::app(name = "bench_diag", version = "0.1.0")]
pub struct BenchDiag {
    #[device] bench: BenchIo,
    #[publish(unit = "1")] cliques: Counter,
}

#[joker::cycle(period = "20ms", wcet = "10us")]
fn run(io: &mut CycleIo<BenchDiag>, _st: &mut ()) -> Result<()> {
    if !io.bench.online()? {
        return Ok(());                                       // without the ESP32, it proposes nothing
    }
    if io.bench.boot.clicked()? {                          // nested handle: the button, through the bench
        io.cliques.inc();                                    // app.bench_diag.cliques
    }
    io.bench.status.set(io.bench.knob.value()? > 0.5);   // the ESP32's LED follows the touch
    Ok(())
}
```

`io.bench.boot` is a `Button` handle like the one in `light` (ref/04 §4.8.6): class members are fields, and the rest is the usual API.

```toml
# joker.toml (add, on the branch)
[apps.bench_diag]
path   = "apps/bench_diag"
bind   = { bench = "esp_io" }
cycles = { run = "main" }
writer = { level = "control", priority = 14 }     # below light, which is at control:12
```

### What `jk check` says

Without the `writer`, both apps would be at `control:12` on the same output, `esp.led`: it is the usual `J0210`. With it, it passes, with a warning:

```sh
$ jk check --env sim
```

```
warning[J0655]: instance with `out` members that reaches different apps by different paths, one of them through a composition: `led_esp`
 --> joker.toml:82:1
   |
82 | [instances.esp_io]
   | ^^^^^^^^^^^^^^^^^^ esp_io.status → led_esp (std/led@1) · out on → esp.led
   = note: light binds led_esp directly (led_status), at control:12
   = note: bench_diag reaches it through esp_io.status, at control:14
   = help: it is the same `esp.led`, with a single envelope and a single arbiter; light wins while it proposes
```

That is what the warning exists to say: the sharing cannot be seen in the `bind` of either app. Here it is on purpose.

```sh
$ jk explain esp_io --env sim
```

```
esp_io · local/bench_io@1 · composite instance (joker.toml:82)
  status        led_esp · std/led@1 · composite · Ok
  boot          button · std/button@1 · composite · Ok
  knob          touch · std/analog_in@1 · composite · Ok
  rssi          in i64      esp.rssi · -55 good
  uptime        in f64 s    esp.uptime · 1,204 s good
  online        query       good(uptime) · true
  health        Ok
  paths         esp_io.boot.clicked ≡ button.clicked · esp_io.status.on ≡ led_esp.on ≡ esp.led
  depth         2: bench_io → led, button, analog_in (JC0501: maximum 3)
```

In `jk dev --env sim --panel`, press the ESP32's button and watch `app.bench_diag.cliques` go up. The LED does not change, because `light` controls it. Stop `light` (`jk app stop light --node local`, in another terminal, with `jk dev` running): its lease expires, the level `control:14` becomes the active one, and the ESP32's LED follows the touch. With `jk app start light --node local`, `light` wins again. The scenarios keep passing: with the ESP32 failing, `bench_diag` does not propose either (`online` is false), like `light` 0.4.1.

When you are done, go back to the main branch. Part 9 starts from the commit of section 11:

```sh
$ git switch main
```

**State at the end of this part:** app `light` **0.6** and app `status_light_ctl` **1.0.0**. New files: `classes/local/status_light.jkc`, `classes/local/status_light.conformance.toml`, `apps/status_light_ctl/`. Changed: `joker.toml` (`[instances.indicator]`, `[outputs."indicator.mode"]`, `[outputs."indicator.blink_period"]`, `[apps.status_light_ctl]`, the `bind` of `light`, the `order` of `main`), `apps/light/`, `joker.sim.lock`, `joker.lab.lock`, `joker.zero.lock`, and the last expectation of `sim/scenarios/blink.scenario.toml`. `sim/scenarios/esp_fault.scenario.toml` did not change. Lab is at generation 9; zero, stopped, stayed at 0.5. The extra of section 12 (the class `local/bench_io@1`, the instance `esp_io` and the app `bench_diag`) stays on the branch `extra-bench-io`, and does not enter this state.

**Next:** [Part 9 — The ESP32 becomes a Joker Micro: Joker Link and the `safe` on the MCU](09-joker-micro.md).
