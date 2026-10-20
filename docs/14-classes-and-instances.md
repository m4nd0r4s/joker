# 14 — Equipment classes, instances and abstraction layers

The question that gave rise to this document: *"I like `led: Output<bool>`, but I'd like to be able to write `led: Led` and do `led.toggle()`. And in an app with 15 pieces of equipment, each with 5 signals and actions, a struct with 75 ports gets awkward."*

Short answer: **yes, with one condition.** Classes are a layer **on top of** signals, not instead of them. They compile to ports and signals, and so the envelope, the arbiter, quality, the historian and replay keep working without changing anything. Nobody is required to use them, and whoever wants to can still go down to the raw signal.

The goal is to serve **two audiences** well at the same time: those who want low-level access to **build abstractions**, and those who only want to **use abstractions** to write business logic.

This document defines primitive **M24** (document 10) and ADR 0017, and attacks the design with 14 cases (§8).

---

## 1. Three layers, four people

```
 ┌────────────────────────────────────────────────────────────────────────────┐
 │ LAYER 3 — BUSINESS LOGIC                                                   │
 │   station.pumps.iter().filter(|p| p.failed_to_start()) …                   │
 │   uses classes; never sees registers, pins, protocols                      │
 ├────────────────────────────────────────────────────────────────────────────┤
 │ LAYER 2 — CLASSES AND INSTANCES                                            │
 │   led@1, button@1, analog_in@1, pump@1, meter.ac3@1, inverter.pv@2 …       │
 │   implicit (profile), composite (manifest) and virtual (app) instances     │
 ├────────────────────────────────────────────────────────────────────────────┤
 │ LAYER 1 — SIGNALS AND PORTS (document 02)                                  │
 │   Input<f64>, Output<bool>, Action, events, blocks, streams                │
 ├────────────────────────────────────────────────────────────────────────────┤
 │ CORE — process image, arbiter, envelope, quality, trace                    │
 └────────────────────────────────────────────────────────────────────────────┘
```

| Person | Works in | Writes | Does not need to know |
|---|---|---|---|
| **Hardware integrator** | layer 1 → 2 | equipment profiles with `implements` (declarative, TOML) | the business logic |
| **Abstraction author** | layer 2 | classes, active implementations in Wasm, class conformance tests | what equipment is underneath |
| **Business programmer** | layer 3 | apps that use classes | registers, pins, protocols, scales |
| **Operator** | all, reading | nothing: sees instances by class in `jk status` and in the HMI | — |

The same person can do all three things, and in the learning track did. What matters is that **each layer is complete**: you can stay in just one of them and produce something useful.

---

## 2. Concepts

| Concept | What it is | Example |
|---|---|---|
| **Class** | versioned contract of what an equipment type **is to a program**: members, capabilities, methods, queries | `led@1`, `pump@1`, `inverter.pv@2` |
| **Member** | an element of the class: `input`, `output`, `param`, `action`, `event`, `query` | `pump.running` (input), `pump.run_cmd` (output) |
| **Capability** | optional group of members; an instance either has it or not | `led` with `dimmable`; `pump` with `pressure` |
| **Method** | sugar over writes to output members, defined in the class | `led.toggle()`, `pump.run_at(60 %)` |
| **Query** | value derived from the members, computed **by the core** | `button.clicked` = `rose(pressed)` |
| **Instance** | a concrete piece of equipment that fulfills a class, with the members bound to signals | `p1: pump@1` |

**A class describes the role, not the chip.** The track's ESP32 is a *device* (it speaks Modbus RTU with the bench firmware and has a profile, `local/bench-esp32@1`). Its touch sensor, pad T0, is an *instance* of `analog_in@1`: the profile's implicit one, `esp`, with the firmware's nominal scale, and the site's composite `toque`, with that bench's calibration (track, part 7, §5). The app wants "a normalized analog value", not "pad T0 of an ESP32". The practical rule: if swapping the component for another equivalent one should not touch the app, then the component is a device, and the role is a class.

The same rule holds for composite classes. `local/bench_io@1` (track, part 8, "Extra") is acceptable: it describes **the bench's I/O node**, with a status LED, a button, a touch input and a heartbeat signal, and another node with the same parts would fulfill it without touching the app. A class called `esp32@1` is the mistake: it names the chip, not the role, and the day the node becomes an RP2350 the class lies. A class's name answers "what is this for in the program?", and never "what part is this?".

---

## 3. Layer 1: unchanged, and it is the escape hatch

Nothing in document 02 changes. `Input<f64>`, `Output<bool>`, bindings by path, `get()?`, `sample()`, `write_at`, actions and events remain the base. Two guarantees:

1. **A class is always expandable to ports.** `jk inspect` shows the flat list of ports that an app with classes actually uses, and that list is what the Wasm host, the arbiter and the trace see.
2. **Every class handle gives access to the raw members:** `io.led.raw().on` is an output handle (`CycleOutput<bool>` in the cycle). Whoever needs a port operation on a member that the class does not expose as a method has it. When a method has the name of a member (in `led@1`, the `out on` and the `method on()`), the handle gives the method, and the member stays in `raw()` (document 17, §3.2).

---

## 4. Layer 2: defining a class

Classes live in a versioned **class registry** (like profiles, document 05, §6). There is a standard library `std/` and project classes in `classes/`. They are written in the **`.jkc`** IDL (document 17, ADR 0018), and `jkc` generates from them the types for Rust, TypeScript and Python. That way, the three languages see exactly the same members and methods. Document 17 is the language reference; this section shows the classes of the track and of the rest of this document.

### 4.1 `std/led@1`

```
package std

/// Light indicator.
class led@1.0.0 {
    out on: bool                       @safe_hint(false)

    method on()           { on = true }
    method off()          { on = false }
    method set(v: bool)   { on = v }       // JC0206 if the argument were called `on`
    method toggle()       { on = !own(on) } // see §8, A6; the name `on` gives the method, the member stays in raw()

    capability dimmable {
        out brightness: f64 %          @safe_hint(0 %)
        method dim(pct: f64 %) { brightness = pct; on = pct > 0 % }
    }
}
```

### 4.2 `std/button@1`

```
package std

/// Momentary push button.
class button@1.0.0 {
    in pressed: bool

    query clicked:   bool = rose(pressed)          // true for a single tick
    query released:  bool = fell(pressed)
    query long_hold: bool = held(pressed, 1 s)
}
```

### 4.3 `std/analog_in@1`

```
package std

/// Normalized analog input (0..1), with the physical value available.
class analog_in@1.0.0 {
    in value: f64 1                    // the normalization belongs to the profile or the instance

    query smooth: f64 1 = avg(value, 200 ms)

    capability physical {
        in reading: f64                // physical value; unit given by the instance (V, mA…)
    }
}
```

The physical member is called `reading`, not `raw`: `raw` is a reserved name of the generated handles (`io.led.raw()` gives the raw members; `JC0207`, document 17 §3.2).

### 4.4 `std/pump@1`: a real piece of equipment

The complete definition is in document 17, §3.1. The essentials: `out run_cmd`, `in running`, `in fault`, the action `reset`, the methods `start`/`stop`, the queries `failed_to_start`/`failed_to_stop`, and the capabilities `variable_speed` (with `speed_set`, `speed` and the atomic method `run_at`), `pressure` and `hour_counter`. A fixed-speed pump is an instance without `variable_speed`.

### 4.5 The rules of a class

1. **There is no free code.** Methods only assign values to `out` members and call methods of fixed members (ADR 0027), with expressions of the language of document 02, §10, plus `own(x)`. Queries are expressions evaluated **by the core** (§8, A7). Whoever needs real behavior writes an **active implementation** (§7) or an extension trait (§6.7).
2. **There is no inheritance.** There is composition (§4.6) and capabilities. Inheritance in equipment models produces the hierarchies that no integrator manages to maintain.
3. **A class does not define safety.** `@safe_hint` is a suggestion. The `safe`, the range, the rate and the interlocks remain in the site manifest (ADR 0006), and `jk check` shows the class's suggestion next to the `J0201` error.
4. **Semver versions** with the rules of document 17, §9. Apps depend on the major (`std/pump@1`); the lock pins the exact version and the contract hash.

### 4.6 Composition

```
package local

class station@1.0.0 {
    member pumps: std/pump@1[1..8]
    member tank:  local/tank@1          // std/ has no tanks (§10): it is a project class
    in flow: f64 m3/h

    query any_pump_failed: bool = any(pumps.failed_to_start)
}
```

The maximum composition depth is 3 (`JC0501`). It is counted by the longest chain of class members, counting the class itself as 1: `station` = 1, `pump` = 2, a pump's `drive` = 3 (document 17, §8). More than that is almost always an asset model (which belongs in an asset management system) disguised as control. Implementation layers (a virtual instance whose app uses other virtual instances) are counted separately, with the same limit of 3 (`J0620`, §7.2). The two limits do not add up.

Using a composite class, end to end:

| Step | How | Where the rule is |
|---|---|---|
| bind the members | in the `map`, the value of a class member is the name of an instance; in a collection, a list of names | §5.2; ref/01 §5.13 |
| reach the signals | nested paths: `esp_io.boot.clicked` is the same signal as `botao.clicked`; in a collection, `station.pumps.p1.running` | document 16, P6 |
| share a member | an instance may be a member of several compositions; a single envelope and a single arbiter per `out`; warning `J0655` when the sharing reaches different apps | document 16, P6 |
| call methods of the members | in a method of the class, only of fixed members (`drive.stop()`) | document 17, §4.1 |
| health | the worst of the class's and that of the required members | document 17, §4.5 |
| a single piece of equipment that is the whole composition | the profile fulfills the class with `members.*` subtables | ref/03 §2.2.14 |
| in the SDK | `io.esp.boot.clicked()`, `io.station.pumps.iter()` | ref/04 §4.8.6; ref/05 §14.6 |

The complete case, with three levels, a virtual instance and a Joker Micro, is in §9.3.

---

## 5. Instances: three ways to be born

### 5.1 Implicit: the profile says the equipment fulfills the class

For the common case of one piece of equipment per instance. The profile declares the mapping, and **each device with that profile becomes an instance**, with the device's name.

```toml
# profiles/acme/vfd@3.profile.toml (excerpt)
[implements."std/pump@1"]
capabilities = ["variable_speed"]
map = { run_cmd = "cmd.run", speed_set = "speed_ref", running = "status.running", speed = "speed_act", fault = "status.fault", reset = "actions.reset_fault" }
```

```toml
# joker.toml
[devices.vfd1]
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 11
profile = "acme/vfd@3"          # vfd1 is, automatically, an instance of pump@1
```

### 5.2 Composite: the logical equipment spreads across several devices

In industry, **what is controlled is rarely a communication device**. A real pump is a contactor on a digital output, a feedback on a digital input, a thermal relay on another, and a 4–20 mA pressure transmitter on an analog card. Three devices, one piece of equipment.

```toml
[instances.p1]
class = "std/pump@1"
capabilities = ["pressure"]                                   # fixed-speed pump: no variable_speed
map = { run_cmd = "io.do_1", running = "io.di_1", fault = "io.di_2", pressure_out = "ai.ch1" }
```

- The members also get a path in the instance (`p1.run_cmd` is an alias of `io.do_1`). The envelope is declared by **one** of the two paths, never by both (`J0215`, document 16, P2).
- `const:` binds an `in` or `param` member to a constant, with `Good` quality. On an `out` it is `J0603`: a piece of equipment that does not have an output does not have the capability that contains it.
- The available adapters (`scale`, `invert`, `edge`, `enum_map`, `on_rise`…) are in document 17, §5.2.
- Unmapped members: required → `J0601`; of a declared capability → `J0602`.
- **Class members.** The value is the name of an instance, and in a collection a list of names, in the order of the collection. The instance may be born in any way (implicit, composite, virtual or simulated):

```toml
[instances.station]
class = "local/station@1"
map   = { pumps = ["p1", "p2", "p3"], tank = "t_head", flow = "fm1.flow" }
```

`jk check` verifies that the instances exist, that the class and the major are those of the member (with the capabilities of `@requires`), that the number fits in `[min..max]` and that there are no cycles (`J0650`–`J0654`, ref/01 §5.13). `p1` remains an instance with its own name; `station.pumps.p1.running` and `p1.running` are the same signal (document 16, P6).

### 5.3 Virtual: an app implements the class

This is the piece the **abstraction author** was missing, and it is detailed in §7. An app declares `#[joker::implements(Pump)]` (the class `pump@1`): the class's outputs become **virtual commands** that it reads, and the class's inputs become signals that it publishes. For whoever uses the instance, there is no difference between a real, composite or virtual pump.

---

## 6. Layer 3: using classes in the SDK

### 6.1 The simple case

```rust
use joker::prelude::*;
use joker::classes::std::{Led, Button};

#[joker::app(name = "hello_world", version = "0.5.0")]
pub struct HelloWorld {
    #[device] led: Led,
    #[device] botao: Button,
}

#[joker::cycle(period = "10ms")]
fn control(io: &mut CycleIo<HelloWorld>, _st: &mut ()) -> Result<()> {
    if io.botao.clicked()? {
        io.led.toggle();
    }
    Ok(())
}
```

```toml
[apps.hello_world.bind]
led   = "painel_led"         # an instance of led@1
botao = "painel_botao"       # an instance of button@1
```

The `st.premido_antes` of part 4 of the track is gone: the edge detection is the class's `clicked` query, computed by the core (track, part 7).

### 6.2 Many pieces of equipment

```rust
use joker::classes::std::{MeterAc3, Pump};                     // meter.ac3@1 → MeterAc3
use crate::classes::local::Tank;                               // tank@1 belongs to the project (classes/local/tank.jkc)

#[joker::app(name = "station", version = "1.0.0")]
pub struct Station {
    #[device] pumps: Many<Pump, 1, 8>,                         // between 1 and 8; size fixed at init
    #[device] tank: Tank,
    #[device(requires = ["pressure"])] booster: Pump,         // only accepts pumps with the `pressure` capability (A4)
    #[device] meter: MeterAc3,
    #[param(unit = "m", default = 2.5)] level_high: Param<f64>,
    #[param(unit = "m", default = 1.0)] level_low: Param<f64>,
}
```

`Many<C, MIN, MAX>` takes the limits as two const parameters (the form `Many<Pump, 1..=8>` is not valid Rust, ref/04 §4.8.3). The Rust names of classes are the class name in UpperCamelCase, without dots (`meter.ac3` → `MeterAc3`, `hvac.vav` → `HvacVav`), and can be imported under another name (`use … MeterAc3 as Meter`). A realistic example (duty/standby with rotation) is in §9.1. The operations of a `Many`:

| Operation | Semantics |
|---|---|
| `iter()`, `iter_mut()` | stable order: that of the manifest, or by name; the handle methods take `&self`, and `iter()` is enough to propose |
| `get("p2")` → `Option`, `by_name("p2")` → `Result` (`J-E0503`), `index(1)` | access by instance name or position |
| `len()` | fixed at `init`; changes only with a new generation (A10) |
| `p.name()`, `p.index()` | identity of the instance (name in the manifest, position) |
| `healthy()` | iterator over the instances with `health() == Ok` |
| `read_all()?` | snapshot of all; `Err` if any required one is failing |

### 6.3 Reading a whole instance

```rust
let p1 = io.pumps.by_name("p1")?;                      // get() returns Option; by_name() returns Result
let s: PumpState = p1.read()?;                         // generated struct: running, fault, speed: Option<f64>, pressure_out: Option<f64>
match p1.health() {
    Health::Ok => {}
    Health::Degraded(m) => log::warn!("p1 degraded: {m:?}"),   // failing capabilities
    Health::Failed(m) => return Err(Error::quality(m)),          // failing required members
}
```

### 6.4 Cycle and async

The handles exist in both views, with the same split as the ports (document 04, §3.4):

| View | Can |
|---|---|
| `CycleIo` | read members and queries from the latch; call methods (proposals); `read()`, `health()` |
| `TaskIo` | everything that is async: `pump.run_at(60.0).await` returns the `WriteOutcome`s of the expanded writes (`MethodOutcome`), after the commit; **actions**: `pump.reset().await`; `history()` of a member (in `raw()`) |

### 6.5 Local classes: organizing a large app without a registry

Not everything deserves a shared class. To organize the ports of an app, you declare a class **in the app itself**:

```rust
#[derive(joker::Class)]
pub struct Zone {
    #[input(unit = "°C")]  temp: Input<f64>,
    #[input]               occupied: Input<bool>,
    #[output(unit = "°C")] setpoint: Output<f64>,
    #[output(unit = "%")]  damper: Output<f64>,
}

#[joker::app(name = "floor_ctl", version = "0.1.0")]
pub struct FloorCtl {
    #[device] zones: Many<Zone, 1, 64>,
}
```

And it is bound by path pattern, without repeating it 64 times:

```toml
[apps.floor_ctl.bind.zones]
each = "bms.zone_*"                                  # zone_01 … zone_24
map  = { temp = "{}.temp", occupied = "{}.occ", setpoint = "{}.sp", damper = "{}.damper" }
```

This answers the question of the struct with 75 ports directly: **ports are grouped by the equipment they belong to**, with registry classes when the concept is shared and with local classes when it is only organization.

### 6.6 TypeScript and Python

```ts
import { app, device, cycle } from "@joker/sdk";
import { Led, Button } from "@joker/classes-std";

export default app({
  name: "hello_world", version: "0.5.0",
  devices: { led: device(Led), botao: device(Button) },
  cycles: { control: cycle("100ms", (io) => { if (io.botao.clicked()) io.led.toggle(); }) },
});
```

```python
from joker_sdk import App, app, cycle, device
from joker_classes.std import Led, Button

@app(name="hello_world", version="0.5.0")
class HelloWorld(App):
    led = device(Led)
    botao = device(Button)

    @cycle("500ms")
    def control(self) -> None:
        if self.botao.clicked():
            self.led.toggle()
```

The periods are the tier 2 minimums (100 ms in TypeScript, 500 ms in Python, `J-E0854`; document 04, §4.3). With them, a click shorter than the period is lost, because `clicked` is evaluated at the latch of whoever consumes it (A7). For buttons in TS or Python, the right form is a `task.on(…)` task on the button's signal, or an active implementation in Rust (ref/05 §14.3).

---

### 6.7 Free Rust: where behavior lives

The methods of a shared class are declarative (§4.5) because **anyone can call them**: apps in Rust, TypeScript or Python, the CLI (`jk write luz.mode fault`), the OPC UA bridge, an approved intent. For all of them to execute the same `toggle`, the core must know how to execute it, and `jk check` must know which outputs it writes. That does not rule out free Rust: it only puts it in the right place. There are three, depending on **whom the behavior belongs to**.

| Where | Free Rust? | Runs | State | Who can use it | What for |
|---|---|---|---|---|---|
| **extension trait** on the handle | yes | in the calling app | the calling app's | only Rust apps that import the crate | conveniences for the user: "how I like to use an LED" |
| **`impl` of a local class** (§6.5) | yes | in the app | the app's | only that app | organize and give behavior to grouped ports |
| **active implementation** (§7) | yes | in an app of its own, in a cycle | the equipment's, a single one | all callers, including CLI, HMI and bridges | behavior that belongs to the equipment: sequences, protections, patterns |

**Extension traits: behavior on the caller's side.** They require nothing of the core and are distributed as crates:

```rust
// crate joker-led-ext (by whoever wants it)
pub trait LedExt {
    fn toggle_debounced(&mut self, st: &mut ToggleState, now: Instant);
}

impl LedExt for LedCycle<'_> {                 // the cycle handle; `Led` is only the declaration
    fn toggle_debounced(&mut self, st: &mut ToggleState, now: Instant) {
        if now - st.last >= 500.ms() {   // free Rust
            self.toggle();               // in the end, uses the contract methods
            st.last = now;
        }
    }
}
```

The code runs in the app's Wasm instance, is deterministic and replay is exact. It has two limits, which are also the rule for choosing: it only exists in Rust, and each app that uses it has its own copy of the state. If the behavior must be **the same for all callers**, or has state that belongs to the equipment (a blink, a protection against short cycling), the place is the active implementation, like the track's `status_light_ctl` (part 8).

**The rule, in one line:** the *contract* (members, methods, queries) is declarative and neutral; the *behavior* is Rust, on the caller's side (extension trait) or on the equipment's side (active implementation).

## 7. For the abstraction author: active implementations

Declarative classes cover the mapping. What they do not cover is **behavior**: a start sequence with a ramp, protection against short cycling, automatic alternation between two pumps, a valve that needs to "open 100% for 2 s and then regulate". This is exactly what an abstraction author wants to write once and reuse across a hundred sites.

### 7.1 An app that implements a class

```rust
use joker::prelude::*;
use joker::classes::std::Pump;

/// Implements pump@1 over a contactor and a feedback, with protection against short cycling.
#[joker::app(name = "pump_ctl", version = "1.2.0")]
#[joker::implements(Pump, capabilities = ["hour_counter"])]   // generates `io.cmd` (received commands) and `io.state` (to publish)
pub struct PumpCtl {
    #[output] contactor: Output<bool>,        // layer 1: the real hardware
    #[input]  feedback: Input<bool>,
    #[input]  thermal_trip: Input<bool>,
    #[param(unit = "s", default = 180.0)] min_off: Param<f64>,
    #[param(unit = "s", default = 60.0)]  min_on: Param<f64>,
}

struct Ctl {
    on: bool,
    since_s: f64,
}

impl Ctl {
    // Initial state: as if the pump had been stopped for `min_off`, so that the first start is immediate.
    fn init(io: &CycleIo<PumpCtl>) -> Self { Ctl { on: false, since_s: io.min_off.get() } }
}

#[joker::cycle(period = "100ms", init = Ctl::init)]   // without `init`, the state starts at Default
fn run(io: &mut CycleIo<PumpCtl>, st: &mut Ctl) -> Result<()> {
    st.since_s += io.dt().as_secs_f64();
    let want = io.cmd.run_cmd.get()?;                   // what the layer above asked for

    let can_start = !st.on && st.since_s >= io.min_off.get();
    let can_stop  =  st.on && st.since_s >= io.min_on.get();
    if (want && can_start) || (!want && can_stop) {
        st.on = want;
        st.since_s = 0.0;
    }
    io.contactor.set(st.on);

    // Publish the class state: whoever uses the pump sees this.
    io.state.running.set(io.feedback.get()?);
    io.state.fault.set(io.thermal_trip.get()?);
    // Without `variable_speed` in the capability list, `io.state.speed` does not exist (compile error).
    // io.state.hours (hour_counter): counted here and persisted in kv by an async task (omitted)
    Ok(())
}
```

```toml
[instances.p1]
class = "pump@1"
implemented_by = "app:pump_ctl"        # virtual instance: an instance of the app, with the bind of [apps.pump_ctl]

[apps.pump_ctl]
path = "apps/pump_ctl"
bind = { contactor = "io.do_1", feedback = "io.di_1", thermal_trip = "io.di_2" }

[outputs."io.do_1"]
safe = false                           # envelope of the real hardware

[outputs."p1.run_cmd"]                 # envelope of the virtual command: yes, it has one too
safe = false
max_actuations = "6/h"
```

### 7.2 Semantics of the layers

- **Virtual commands are outputs like any other:** they have an arbiter, an envelope and `safe`. Two business apps commanding the same virtual pump are resolved by the arbiter, **before** reaching the implementation.
- **Safety at every layer:** if the business app dies, the lease of `p1.run_cmd` expires and the command goes to `safe` (stop), and `pump_ctl` stops the pump *respecting `min_on`*. If `pump_ctl` dies, the lease of `io.do_1` expires and the contactor opens. No layer depends on the one above to be safe.
- **Latency:** a layer adds **one tick** of the implementation's group (the command is applied at the end of the consumer's tick and read at the next latch). With both in the same group and `order = ["consumer", "implementer"]` in the group (document 03, §7), the core arbitrates and applies the envelope to the virtual commands right after the task that produces them (document 01, §4, step 4b), and the latency is zero. `jk explain p1` shows the latency of each layer.
- **Depth:** at most 3 virtual layers in a chain (`J0620`). More than that is rarely abstraction: it is indirection.

### 7.3 Class conformance tests

A class may bring **scenarios that any implementation must pass**. That is what gives confidence to whoever uses someone else's pump.

```toml
# classes/std/pump.conformance.toml
schema = 1

[suite]
kind  = "class"
class = "std/pump@1"

[[case]]
id     = "start_running_or_failed"
name   = "start → running in ≤ 10 s, or failed_to_start"
do     = [{ at = "1s", method = "start" }]
expect = [{ any_of = [{ member = "running", equals = true, before = "11s" },
                      { query = "failed_to_start", equals = true, at = "11.1s" }] }]

[[case]]
id       = "stop_zeroes_speed"
name     = "stop takes speed_set to 0 %"
requires = ["variable_speed"]
do       = [{ at = "1s", method = "run_at", args = { pct = "60 %" } }, { at = "5s", method = "stop" }]
expect   = [{ member = "speed_set", equals = "0 %", at = "5.2s" }]

[[case]]
id       = "safe_stops_pump"
name     = "command in safe → the pump stops"
sim_only = true           # exclusive ownership: on a real instance, `skipped (sim_only)` (ref/03 §2.6.3)
do       = [{ at = "1s", method = "start" }, { at = "20s", lease_expire = "run_cmd" }]
expect   = [{ member = "running", equals = false, before = "90s" }]     # the class allows up to 60 s of minimum run time
```

It is the **same schema** as the core conformance cases (`[suite]`, `[[case]]`, `do`, `expect`, `at`, `equals`), with the canonical literals in lowercase; only the *fixture* changes. The file has the name of the `.jkc` (`pump.jkc` ↔ `pump.conformance.toml`), and each case has an `id`. The complete schema is in ref/03 §2.6.

```sh
$ jk class verify pump@1 --instance p1 --env sim               # runs the cases against the instance, in simulation
$ jk class new valve@1                                         # class skeleton + conformance file
$ jk class publish classes/local/station.jkc
```

A profile with `implements` and an app with `#[implements]` can only be published to the registry if they pass the class's cases.

---

## 8. Stress: 14 attacks on the design

**A1 — The simple LED.** `led.toggle()` instead of `Output<bool>` and a manual state. Holds. The runtime cost is minimal: the method goes as a record in the proposal buffer and the core expands it into a proposal on the `on` member (document 17, §4.1).

**A2 — The pump across three devices.** Composite instances (§5.2) solve the case that "one device = one class" models (like the *device profiles* of many IoT platforms) do not. A member of a composite instance has no acquisition coherence with the others, because they come from different requests: `read()` is coherent per tick (latch), not per request (document 02, §6.2). This is written in the documentation of `read()`.

**A3 — 120 VAVs in a building.** With implicit instances (BACnet profile with `implements."hvac.vav@1"`), the 120 devices are 120 instances without one extra line, and the app binds `vavs = "class:hvac.vav@1"` (all) or `"vav_3*"` (one floor). In the process image, 120 × 6 members = 720 ports = ~17 KB per tick, within budget (document 09, attack 2).

**A4 — Inverters from five manufacturers.** The class `inverter.pv@2` has a core that all of them fulfill (active power, power limit, state) and capabilities for the rest (`reactive`, `curtail_ramp`, `mppt_detail`). An app that needs a capability declares it (`#[device(requires = ["reactive"])]`), and `jk check` fails (`J0602`) on the sites where some instance does not have it, **before** the deploy. What is truly specific to a manufacturer stays accessible through `raw()` or through a layer 1 port alongside. The mistake to avoid is the class "that has everything": each extra required member is one less manufacturer that fulfills it.

**A5 — A method that writes to two outputs.** `battery.set_power(p)` must write the mode and the setpoint coherently. With `atomic = true` and `order` in the method definition, the instance generates an atomic group (M20) over those members. Whoever writes the method does not need to remember this at each site.

**A6 — `toggle()` with an active manual override.** If the operator forced the LED to `true` at level `manual:4` and the app calls `toggle()`, should the app invert the **effective** value (the operator's) or **its own**? Decision: **its own**. `own(on)` is the app's last proposal at its arbiter level; without a previous proposal (first call, or expired lease), it is the applied value (document 16, §4). If it were the effective one, each toggle during the override would invert the operator's value in the app's layer, and when the override expired the LED would end up in a state nobody chose. With `own`, the app's layer stays coherent with its own logic, and the override does not contaminate it.

**A7 — `clicked` needs state (the previous value).** Where does that state live? Hypotheses: (a) in the SDK, inside the handle, in the app's memory; (b) in the core, as a derived signal. With (a), there would be three implementations of `rose()` (Rust, TS, Python) besides the core's, diverging, which is the mistake Cravo forbids (sharing semantics without sharing code). **Decision: (b).** Class queries are **derived signals evaluated by the core**, at the latch of each group that consumes them, with the same bytecode as the manifest expressions. A single evaluator, and replay stays exact. The cost is that queries are limited to the expression language, and whoever needs more uses an active implementation. There is also a deliberate limit: queries have **volatile state** (windows ≤ 15 min, edges, timers) and start over on a restart. What must survive restarts, like running hours, is a **member** published by whoever knows it (the drive, or an active implementation that keeps it in `kv`), not a query.

**A8 — Quality of an instance.** With 7 members from 3 devices, "is the pump good?" has no obvious answer. Decision: `health required(running, fault)` in the class (document 17, §3.1). `health()` is `Ok` if the required members are `Good` and the capabilities too, `Degraded` if only capabilities fail, and `Failed` if any required member fails. `read()?` fails on `Failed`. Individual members remain accessible with their quality. No average quality is invented.

**A9 — `pump.reset()` in a cycle task.** It is an action, so it only exists in `TaskIo` (document 02, §8). The cycle handle does not have the method, and it is a compile error, not a runtime one.

**A10 — A site gets a fourth pump.** The size of a `Many<Pump, 1, 8>` is fixed at `init`. A generation that changes the set of instances restarts the app with `migrate` (document 01, §9), and the per-instance state (`PerInstance`, ref/04 §4.8.3) is kept **by instance name**, so that `p1`–`p3` keep their state and `p4` starts from `Default`. Outside the declared limits, `J0610` in `jk check`.

**A11 — Versions.** An app that depends on `pump@1` accepts instances of `pump@1.x`, because the minor only adds optional members and capabilities. An instance of `pump@2` does not bind (`J0630`). Major changes of standard classes follow the M21 policy, with the previous version supported for one major of the runtime.

**A12 — Performance.** Classes are resolved at build and in `jk check`: each member is a port, each method is a ~16 B record expanded by the core, each query is a derived signal. There is no dynamic dispatch, reflection or lookup by name in the tick. An app with 15 pieces of equipment × 5 members has 75 ports, 1.8 KB of image, and the same cost as an app written with raw ports.

**A13 — Testing without hardware.** Each standard class has a **class simulator** (`classes/<pacote>/<nome>.sim.toml`, ref/03 §2.4.11): `[instances.p1] class = "pump@1", sim = true` creates a simulated pump with plausible dynamics (delayed start, injectable faults, pressure proportional to speed) and presence in the `jk dev` dashboard. The class conformance tests (§7.3) run against it.

**A14 — Too many classes.** The opposite risk: a class for everything, 400 classes in a registry nobody masters. Containment: the `std/` library is small and reviewed (§10); project classes are free; app-local classes (§6.5) do not enter the registry. And the golden rule: **if two apps from different sites do not need to talk about the same concept, it is not a shared class.**

---

## 9. Three large cases

### 9.1 Pumping station: 15 pieces of equipment

3 pumps (composite: contactor + feedback + thermal + pressure), 2 tanks (4–20 mA level + alarm floats), 1 flow meter (Modbus), 1 energy meter (Modbus), 4 motorized valves (open/close/limit switches), 3 intrusion and flood alarms (DI), 1 generator (Modbus).

```toml
# Three pumps, the same implementation: one instance of the app per pump, each with its own bindings.
[instances.p1]
class = "pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_1", feedback = "io.di_1", thermal_trip = "io.di_2" } }
[instances.p2]
class = "pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_2", feedback = "io.di_3", thermal_trip = "io.di_4" } }
[instances.p3]
class = "pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_3", feedback = "io.di_5", thermal_trip = "io.di_6" } }

[instances.t_suction]
class = "local/tank@1"                                # project class: std/ has no tanks
map = { level = "ai.ch2", high_float = "io.di_7", low_float = "io.di_8" }
[instances.t_head]
class = "local/tank@1"
map = { level = "ai.ch3", high_float = "io.di_9", low_float = "io.di_10" }

# meters, valves and generator: implicit instances through the profiles

[apps.station.bind]
pumps  = ["p1", "p2", "p3"]
suction = "t_suction"
head    = "t_head"
valves  = "class:valve.motor@1"
flow    = "fm1"
energy  = "em1"
genset  = "gen1"
alarms  = ["io.di_11", "io.di_12", "io.di_13"]      # layer 1 port, no class: it does not need one
```

```rust
use joker::prelude::*;
use joker::classes::std::{Genset, MeterAc3, Pump, ValveMotor};
use crate::classes::local::{FlowMeter, Tank};        // project classes: not in std/ (§10)

#[joker::app(name = "station", version = "1.0.0")]
pub struct Station {
    #[device(requires = ["hour_counter"])] pumps: Many<Pump, 1, 8>,
    #[device] suction: Tank,
    #[device] head: Tank,
    #[device] valves: Many<ValveMotor, 0, 16>,
    #[device] flow: FlowMeter,
    #[device] energy: MeterAc3,
    #[device] genset: Genset,
    #[input(many)] alarms: InputMany<bool>,
    #[param(unit = "m", default = 2.5)] start_level: Param<f64>,
    #[param(unit = "m", default = 3.6)] stop_level: Param<f64>,
}

#[derive(Default)]
struct Duty {
    demand: bool,
    lead: usize,          // lead pump, chosen at the start of each demand
    failed: [bool; 8],    // failed_to_start in the previous tick: only the edge counts
}

#[joker::cycle(period = "500ms")]
fn control(io: &mut CycleIo<Station>, st: &mut Duty) -> Result<()> {
    // Process safety first: with no water at the suction, nothing starts.
    if io.suction.low_float()? || io.alarms.iter().any(|a| a.get().unwrap_or(true)) {
        io.pumps.iter().for_each(|p| p.stop());
        st.demand = false;
        return Ok(());
    }

    let level = io.head.level()?;
    if !st.demand && level < io.start_level.get() {
        // Start of a demand: the lead is the healthy one with the fewest hours. Chosen only here,
        // and not on every tick, two pumps with close hours do not swap in the middle of the demand.
        st.demand = true;
        let lead = io.pumps.healthy().min_by(|a, b| {
            a.hours().unwrap_or(f64::INFINITY).total_cmp(&b.hours().unwrap_or(f64::INFINITY))
        });
        if let Some(p) = lead {
            st.lead = p.index();
        }
    } else if st.demand && level > io.stop_level.get() {
        st.demand = false;
    }

    let n = io.pumps.len();                        // outside the loop: iter() borrows io.pumps
    for p in io.pumps.iter() {                     // the methods are &self: no need for iter_mut()
        let i = p.index();
        if st.demand && i == st.lead { p.start() } else { p.stop() }
        let failed = p.failed_to_start()?;
        if failed && !st.failed[i] {
            st.lead = (st.lead + 1) % n;           // the lead failed to start: move on to the next
        }
        st.failed[i] = failed;
    }
    Ok(())
}
```

The first version of this example re-chose the lead on every tick by the lowest hour count: with close hours, the pumps swapped lead with the pump running, and the `rotate_every` parameter was not used. Now the rotation is per demand, which is what evens out the hours without extra starts (ref/05 §20.2 has the same logic in TypeScript and Python). It also used `enumerate()` as the index, which only matches `p.index()` by chance, and `io.pumps.len()` inside an `iter_mut()`, which the borrow checker rejects.

Some forty lines of logic for 15 pieces of equipment, without a single register, pin or scale. The profiles, the composite instances and `pump_ctl` (protection against short cycling) live in other layers, made and tested by people who know hardware.

### 9.2 Building: 120 VAVs and 4 AHUs

```rust
use joker::prelude::*;
use joker::classes::std::{HvacAhu, HvacVav};          // hvac.ahu@1, hvac.vav@1

#[joker::app(name = "hvac_opt", version = "2.0.0")]
pub struct HvacOpt {
    #[device] ahus: Many<HvacAhu, 1, 8>,
    #[device] vavs: Many<HvacVav, 1, 512>,
}

#[joker::task(every = "5m")]
async fn optimize(io: &TaskIo<HvacOpt>) -> Result<()> {
    for ahu in io.ahus.iter() {
        let worst = io.vavs.iter()
            .filter(|v| v.served_by().is_ok_and(|s| s == ahu.name()))   // served_by() is a Result
            .filter_map(|v| v.temp_error().ok())
            .fold(0.0_f64, f64::max);
        let sp = if worst > 1.0 { ahu.supply_sp()? - 0.5 } else { ahu.supply_sp()? + 0.2 };
        ahu.set_supply_sp(sp).await?;
    }
    Ok(())
}
```

`served_by` is a **param** of the class `hvac.vav@1`, given by the profile or by the instance. This is how relations between pieces of equipment (this VAV is served by that AHU) come in without a separate graph model. If more is ever needed, the relation is a member, not a new structure.

### 9.3 Heat pump: three levels, one implementation and a Joker Micro

An air-to-water heat pump in a house: a variable-speed compressor on a Modbus drive, two probes (supply and return), a three-phase meter and the SG-Ready input (two contacts). For the business app it is a single thing: it produces heat with a supply temperature, and accepts grid signals. It is the U2 controller of document 19.

**The classes.** Three levels: `heat_pump` → `compressor` → `std/drive@1`.

```
// classes/local/compressor.jkc
package local

/// Variable-speed compressor; the drive is a std/drive@1.
class compressor@1.0.0 {
    member drive: std/drive@1

    in discharge_temp: f64 °C

    method start(pct: f64 %) { drive.run_at(pct) }       // calls the member's method (17 §4.1)
    method stop()            { drive.stop() }

    query overheated: bool = held(discharge_temp > 110 °C, 10 s)

    health required(discharge_temp)
}
```

```
// classes/local/sg_ready.jkc
package local

/// SG-Ready input: two contacts, four states (1:0 block, 0:0 normal, 0:1 boost, 1:1 forced).
class sg_ready@1.0.0 {
    out c1: bool                  @safe_hint(false)
    out c2: bool                  @safe_hint(false)

    atomic method block()  order(c1, c2) { c1 = true;  c2 = false }
    atomic method normal() order(c1, c2) { c1 = false; c2 = false }
    atomic method boost()  order(c1, c2) { c1 = false; c2 = true }
}
```

```
// classes/local/heat_pump.jkc
package local

enum HpMode { off = 0, heat = 1, boost = 2 }

/// Heat pump: produces heat for the building, with a requested supply temperature.
class heat_pump@1.0.0 {
    member compressor: local/compressor@1
    member t_supply:   std/temp_sensor@1
    member t_return:   std/temp_sensor@1
    member meter:      std/meter.ac3@1
    member grid:       local/sg_ready@1       @optional

    out mode:    HpMode                        @safe_hint(HpMode.off)
    out flow_sp: f64 °C                        @safe_hint(35 °C) @range_hint(20 °C, 55 °C)

    method off() { mode = HpMode.off }
    atomic method heat(sp: f64 °C) order(flow_sp, mode) {
        flow_sp = clamp(sp, 20 °C, 55 °C)
        mode    = HpMode.heat
    }

    query delta_t:    f64 K = t_supply.temperature - t_return.temperature
    query overheated: bool  = compressor.overheated
}
```

- `std/drive@1` (§10) does not have its definition written yet. The example uses its `run_at(pct)`, `stop()` and `speed`, in the shape of `pump@1`.
- `heat_pump@1` has no `health`: health comes from the members (document 17, §4.5). With the supply probe missing, `t_supply` is `Failed`, and so is the pump. With the SG-Ready missing, the pump is `Degraded`, because `grid` is `@optional`.
- **The `heat_pump` methods only write to its own `out`s.** An `off()` that called `compressor.stop()` would be natural in a composite instance, but this one is virtual: the one commanding the compressor is the implementation, and the call would make the business app a second writer of the drive. `jk check` rejects it (`J0656`, document 17, §4.1). The `compressor`, which is composite, calls the methods of the `drive` without a problem.

**On the Pi, with the implementation in Wasm.**

```toml
# joker.toml (excerpt): the envelopes of vfd1 and of the outputs io.do_1 and io.do_2 are omitted
[devices.vfd1]                    # implicit instance of std/drive@1, through the profile
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 1
profile = "acme/hp-drive@2"

[devices.em1]                     # implicit instance of std/meter.ac3@1
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 2
profile = "acme/em3@1"

[instances.comp1]
class = "local/compressor@1"
map   = { drive = "vfd1", discharge_temp = "ai.ch3" }

[instances.t_ida]
class = "std/temp_sensor@1"
map   = { temperature = "ai.ch1" }

[instances.t_retorno]
class = "std/temp_sensor@1"
map   = { temperature = "ai.ch2" }

[instances.sg]
class = "local/sg_ready@1"
map   = { c1 = "io.do_1", c2 = "io.do_2" }

[instances.hp]
class          = "local/heat_pump@1"
implemented_by = "app:hp_ctl"
map            = { compressor = "comp1", t_supply = "t_ida", t_return = "t_retorno", meter = "em1", grid = "sg" }   # class members only

[outputs."hp.mode"]
safe = "off"

[outputs."hp.flow_sp"]
safe = "35 °C"

[apps.hp_ctl]
path = "apps/hp_ctl"
bind = { compressor = "comp1", supply = "t_ida", ret = "t_retorno", grid = "sg" }

[apps.conforto]
path = "apps/conforto"
bind = { hp = "hp", exterior = "meteo.t_ext" }
```

In a virtual instance, the `map` only binds the class members; the scalars (`mode`, `flow_sp`) are virtual commands, and the `in`s, if there were any, would be published by the implementation (ref/01 §5.13).

```rust
// apps/hp_ctl/src/lib.rs: the implementation (the curve is just for the example)
#[joker::app(name = "hp_ctl", version = "1.0.0")]
#[joker::implements(HeatPump)]
pub struct HpCtl {
    #[device] compressor: Compressor,
    #[device] supply: TempSensor,
    #[device] ret: TempSensor,
    #[device] grid: Option<SgReady>,
}

#[joker::cycle(period = "1s")]
fn run(io: &mut CycleIo<HpCtl>, _st: &mut ()) -> Result<()> {
    let mode = io.cmd.mode.get()?;
    match mode {
        HpMode::Off => io.compressor.stop(),
        _ => {
            let erro_k = io.cmd.flow_sp.get()? - io.supply.temperature()?;
            io.compressor.start((30.0 + 10.0 * erro_k).clamp(0.0, 100.0));   // in %
        }
    }
    if let Some(g) = &io.grid {
        if mode == HpMode::Boost { g.boost() } else { g.normal() }
    }
    Ok(())
}
```

```rust
// apps/conforto/src/lib.rs: the business app
#[joker::app(name = "conforto", version = "1.0.0")]
pub struct Conforto {
    #[device] hp: HeatPump,
    #[input(unit = "°C")] exterior: Input<f64>,
}

#[joker::cycle(period = "10s")]
fn run(io: &mut CycleIo<Conforto>, _st: &mut ()) -> Result<()> {
    io.hp.heat((45.0 - 0.8 * io.exterior.get()?).clamp(25.0, 55.0));   // heating curve
    if io.hp.compressor.overheated()? {                                  // read through the nested member
        log::warn!("compressor hot: supply {:.1} °C", io.hp.t_supply.temperature()?);
    }
    Ok(())
}
```

`conforto` reads the compressor and the probes through the nested handles (ref/04 §4.8.6), but does not write to them: they belong to `hp_ctl`. The paths `hp.compressor.drive.speed`, `comp1.drive.speed` and `vfd1.speed` are the same signal (document 16, P6).

```sh
$ jk explain hp
```

```
hp · local/heat_pump@1 · virtual instance, implemented by app:hp_ctl (joker.toml:31)
  mode          out HpMode    writers: conforto (control:12) · safe = off
  flow_sp       out f64 °C    writers: conforto (control:12) · safe = 35 °C
  delta_t       query         t_supply.temperature - t_return.temperature
  overheated    query         compressor.overheated
  compressor    comp1 · local/compressor@1 · composite · Ok
    drive       vfd1 · std/drive@1 · implicit (acme/hp-drive@2) · Ok
  t_supply      t_ida · std/temp_sensor@1 · composite · Ok
  t_return      t_retorno · std/temp_sensor@1 · composite · Ok
  meter         em1 · std/meter.ac3@1 · implicit (acme/em3@1) · Ok
  grid          sg · local/sg_ready@1 · composite · Ok · optional
  health        Ok
  depth         3: heat_pump → compressor → drive (JC0501: maximum 3)
  layers        conforto → hp → hp_ctl: 1 virtual layer (J0620: maximum 3)
```

**With `hp_ctl` on a Joker Micro.** On the U2 controller (ESP32-C6), `hp_ctl` is a native MCU app, and the instances `comp1`, `t_ida`, `t_retorno`, `sg`, `em1`, `vfd1` and `hp` are in the MCU manifest (`micro/hp/joker.toml`), with the same blocks as above. The Pi sees the MCU as a device:

```toml
# the Pi's joker.toml (excerpt)
[devices.bomba]
driver    = "joker-link"
bus       = "rs485"
micro     = "micro/hp/joker.toml"
instances = { hp = "bomba_calor" }          # only the top-level name is renamed
```

The `HELLO` catalog carries the composite instance with the member tree (ref/07 §6.3). The Pi sees **one** instance, `bomba_calor`, of `local/heat_pump@1`, with the same nested paths: `bomba_calor.compressor.drive.speed`, `bomba_calor.t_supply.temperature`. The instances that on the MCU are only members (`comp1`, `t_ida`…) do not get a name on the Pi: they exist through the paths of `bomba_calor`. `conforto` runs on the Pi without changing a line, with `bind = { hp = "bomba_calor" }`; its writes to `bomba_calor.mode` are proposals over the link, and the `safe` of `mode` is applied on the MCU (document 19, §6).

**Where it hits the limit.** An apartment building with four heat pumps asks for a site class:

```
// classes/local/site.jkc: does not compile
package local

class site@1.0.0 {
    member hps: local/heat_pump@1[1..4]
    query any_overheated: bool = any(hps.overheated)
}
```

```
error[JC0501]: composition with depth > 3 (counted as in §8): depth 4
 --> classes/local/site.jkc:4:5
  |
4 |     member hps: local/heat_pump@1[1..4]
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ site → heat_pump → compressor → drive
  = help: aggregate the site in an app (`Many<HeatPump, 1, 4>`) or in an active implementation
```

The site is not a piece of equipment: it is a policy over four. It goes into an app:

```rust
#[joker::app(name = "site_ctl", version = "1.0.0")]
pub struct SiteCtl {
    #[device] hps: Many<HeatPump, 1, 4>,
    #[input(unit = "kW")] limite: Input<f64>,
}

#[joker::cycle(period = "5s")]
fn run(io: &mut CycleIo<SiteCtl>, _st: &mut ()) -> Result<()> {
    let limite = io.limite.get()?;
    let mut total = 0.0;
    for h in io.hps.iter() {                          // bind order: the last ones are cut first
        total += h.meter.p_total().unwrap_or(0.0);    // p_total: active power of meter.ac3@1
        if total > limite { h.off() }                 // with no proposal, conforto is in command
    }
    Ok(())
}
```

`site_ctl` and `conforto` both write to the `mode` of each heat pump. With `site_ctl` at `writer = { level = "supervisor" }`, the cut wins over the heating curve while it lasts, and when `site_ctl` stops proposing, the lease expires and `conforto` is in command again (document 16, A5). At the same level, it would be `J0210`.

---

## 10. Initial standard library (`classes/std/`) and standards

**This table is the normative list of the 1.0 `std/`.** Documents 11, 13 and 21 refer here; a class outside it enters in a minor, with an ADR.

| Class | Main members | Aligned with (to validate) |
|---|---|---|
| `led@1`, `lamp@1`, `relay@1` | `on` (+ `brightness`) | — |
| `button@1`, `switch@1` | `pressed` / `closed` | — |
| `analog_in@1`, `analog_out@1` | `value` (0..1) + `reading` (the physical value, capability `physical`; `raw` is reserved, `JC0207`) | — |
| `temp_sensor@1`, `humidity_sensor@1`, `pressure_sensor@1` | value with unit | — |
| `pump@1`, `fan@1` | `run_cmd`, `speed_set`, `running`, `speed`, `fault` | CiA 402 when it is a drive |
| `valve.motor@1`, `valve.modulating@1` | `open_cmd`/`position_set`, limit switches | — |
| `meter.ac3@1` | P, Q, S, U/I per phase, f, energy | SunSpec 201–204, IEC 61850 MMXU |
| `inverter.pv@2` | P, P limit, state; capabilities `reactive`, … | SunSpec 103 and 700 series |
| `battery.bess@1` | SOC, SOH, P set/actual, mode, limits | SunSpec 800 series, IEC 61850 ZBAT |
| `evse@1` | state, maximum current, session | OCPP (seen from the charger side) |
| `hvac.ahu@1`, `hvac.vav@1` | temperatures, setpoints, flow rates, state | BACnet objects, Brick/Haystack |
| `drive@1` | mode, speed, torque, status word | CiA 402 / PROFIdrive |
| `genset@1` | state, P, fuel, start/stop | — |

The rule: **where there is a standard with a data model, the class follows it**, and the SunSpec profile implements `inverter.pv@2` almost for free. A new standard class requires an ADR. Tanks, flow meters and the rest of what belongs to one sector remain as project classes (`local/`) or package classes (`acme/water`), until two apps from different sites need to talk about the same concept (A14). The generated handles are published as `joker-classes-std` (Rust, re-exported in `joker::classes::std`), `@joker/classes-std` (TypeScript) and `joker_classes.std` (Python).

---

## 11. What does not change (and why this does not break the design)

| Invariant | With classes |
|---|---|
| the app never writes to the hardware (ADR 0005) | methods = proposals; virtual instances = outputs with an envelope |
| mandatory safe state (ADR 0006) | `safe_hint` is a suggestion; `safe` remains in the manifest, also for virtual commands |
| quality never disappears | `health()` and `read()?` over the members; no averages |
| a single expression evaluator | queries are derived signals of the core (A7) |
| determinism and replay | everything compiles to ports and signals that the trace already records |
| cost on the tick | zero beyond that of the equivalent ports (A12) |

---

## 12. Rejected decisions

| Proposal | Why it was left out |
|---|---|
| inheritance between classes | hierarchies impossible to maintain; composition + capabilities are enough |
| methods with free code in the class definition | they must be callable by everyone (other languages, CLI, bridges); free behavior goes to extension traits or active implementations (§6.7); pure Wasm methods remain open (§13) |
| queries evaluated in the SDK | a second evaluator that diverges from the core (A7) |
| classes defining `safe`, ranges and interlocks | safety belongs to the site, not to the equipment type (ADR 0006) |
| dynamic discovery of classes at runtime ("what things exist?") | bindings are resolved in `jk check`; an app does not discover new equipment behind anyone's back |
| `toggle` on the effective value | it would contaminate the app's layer with other people's overrides (A6) |
| mandatory classes | layer 1 must remain complete: there are cases (diagnostics, exotic protocols, prototypes) where the raw signal is the right one |

---

## 13. Future decisions and alternatives

**Class definition format: decided.** Classes are written in `.jkc` (document 17, ADR 0018). Local classes in Rust crates remain valid inside apps; the automatic extraction of a crate to `.jkc` (`jk class extract`) is left for after v1.

**Methods as pure Wasm functions (future alternative).** A contract method could be a pure Rust function compiled to Wasm, with the writes declared in the signature, and executed by the host whenever someone called it:

```rust
#[joker::method(writes = [on])]
fn toggle(cx: &MethodCx<Led>) -> Writes<Led> {
    Writes::new().on(!cx.own().on)       // free Rust, without state and without I/O
}
```

It would give Rust in shared methods, callable by everyone, and `jk check` would still know what is written. It costs a new piece in the host, ~1–2 µs per call, and only works for pure functions: state still needs an active implementation. It is recorded, and only moves forward if a real case appears that extension traits and active implementations do not solve.
