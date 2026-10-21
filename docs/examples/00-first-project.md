# 00 — First project: a tank, a pump, zero hardware

Guided walkthrough. At the end: a Rust app controlling the level of a simulated tank, with envelope, alarm, tests and replay, ready to run on a real Raspberry Pi by changing only the devices block of the manifest.

Prerequisites: stable Rust with the `wasm32-wasip2` target, and `jk` installed.

---

## 1. Create

```sh
jk new tank --template sim-tank
cd tank
```

```
tank/
  joker.toml              the manifest
  apps/level/             Cargo.toml, src/lib.rs
  sim/tank.sim.toml       simulated plant (simulation model)
  sim/scenarios/          test scenarios (*.scenario.toml)
```

## 2. The simulated plant

```toml
# sim/tank.sim.toml
schema = 1

[signals]
level  = { type = "f64", unit = "m", init = 1.0 }
pump   = { type = "f64", unit = "%", direction = "output" }
inflow = { type = "f64", unit = "m3/h", gen = { kind = "sine", mean = 2.0, amp = 1.5, period = "20m" } }

[dynamics]
level = "integrate((inflow - 0.06 m3/h/% * pump) / 3.2 m2, min = 0 m, max = 3 m)"   # area 3.2 m²
```

## 3. The manifest

```toml
# joker.toml
[project]
name = "tank"
mode = "dev"

[devices.tank]
driver = "sim"
model  = "sim/tank.sim.toml"

[cycles.ctl]
period = "100ms"

[outputs."tank.pump"]
range = ["0 %", "100 %"]
rate  = "20 %/s"                   # do not start the pump from 0 to 100 in one tick
safe  = "0 %"
interlock = "tank.level > 0.2 m"   # never pump with the tank almost empty
requires_good = ["tank.level"]

[alarms.high_level]
when      = "tank.level > 2.7 m"
delay_on  = "5s"
severity  = "high"

[apps.level]
path   = "apps/level"
writer = { level = "control", priority = 10, lease = "1s" }
bind   = { level = "tank.level", pump = "tank.pump" }
params = { target = "1.5 m", kp = 80.0 }
cycles = { control = "ctl" }
```

## 4. The app

```rust
// apps/level/src/lib.rs
use joker::prelude::*;

#[joker::app(name = "level", version = "0.1.0")]
pub struct Level {
    #[input(unit = "m")]
    level: Input<f64>,

    #[output(unit = "%")]
    pump: Output<f64>,

    #[param(unit = "m", range = "0..3", default = 1.5)]
    target: Param<f64>,

    #[param(unit = "1", range = "0..500", default = 80.0)]
    kp: Param<f64>,
}

#[joker::cycle(period = "100ms", wcet = "10us")]
fn control(io: &mut CycleIo<Level>, _st: &mut ()) -> Result<()> {
    // High level → pump more. A quality error ends the tick here,
    // nothing is written, the lease runs and, if it continues, the pump goes to 0 %.
    let err = io.level.get()? - io.target.get();
    io.pump.set((err * io.kp.get()).clamp(0.0, 100.0));
    Ok(())
}
```

## 5. Run

```sh
jk dev
```

```
joker 0.1.0 · dev · tank · gen 1
  cycles   ctl 100ms  jitter p99 0.4ms (no RT)   overruns 0
  devices  tank (sim)  up
  apps     level 0.1.0  running  wcet 3µs
  outputs  tank.pump = 34.2 % (Accepted, level=control:10)
```

In another terminal:

```sh
jk read 'tank.**' --watch
jk trace tank.pump          # from the sim to the envelope, tick by tick
```

Saving the file `lib.rs` recompiles the app and hot-swaps it, without losing ticks.

## 6. Watch the envelope at work

```sh
jk write tank.pump 100 --ttl 30s --reason "test"
```

```
tank.pump: manual write (manual:4) for 30 s
  envelope:  rate 20 %/s → ramping up (≈ 3.3 s to 100 %)
  interlock: tank.level > 0.2 m  ✓ (1.43 m)
  prevails over: control:10 (app level)
confirm? [y/N] y
```

When the level drops below 0.2 m, the interlock cuts the pump even with the manual write active. 30 s later, the manual level expires and the app takes control again.

## 7. Test

```toml
# sim/scenarios/sensor-fault.scenario.toml
schema = 1

[scenario]
name = "sensor fails for 20 s"
duration = "5m"

[[inject]]
at = "2m"
signal = "tank.level"
quality = "bad(comm_failure)"
for = "20s"

[[expect]]
during = "2m+1.2s .. 2m+20s"      # after the 1 s lease and one tick
signal = "tank.pump"
equals = "0 %"

[[expect]]
at = "2m+25s"
signal = "tank.pump"
outcome = "accepted"
```

```sh
jk test
```

```
sim/scenarios/sensor-fault.scenario.toml ........ ok (5m simulated in 0.4 s)
apps/level (cargo test) ................ ok
```

## 8. Replay

```sh
jk record --for 10m --out session.jktrace
# change kp in the app, recompile
jk replay session.jktrace --diff
```

`--diff` shows, tick by tick, where the proposals of the new version diverge from the recorded ones.

## 9. To a real Raspberry Pi

You replace the simulated device with hardware (a 4–20 mA level sensor on an I²C ADC and a drive over Modbus RTU). **The app does not change**, nor do the envelope, the alarm or the binding: the paths `tank.level` and `tank.pump` become aliases of the real signals. Remove `[devices.tank]` and add:

```toml
[target]
board = "rpi5"

[buses.rs485]
kind = "serial"
port = "/dev/ttyUSB0"
baud = 19200
parity = "even"

[buses.i2c1]
kind = "i2c"
dev  = "/dev/i2c-1"

[devices.adc]
driver  = "i2c"
bus     = "i2c1"
address = 0x48
profile = "ti/ads1115@1"
[devices.adc.signals]
# 4–20 mA over a 150 Ω shunt resistor → 0.6–3.0 V at input A0
a0 = { alias = "tank.level", scale = { from = ["0.6 V", "3.0 V"], to = ["0 m", "3 m"] } }

[devices.vfd]
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 3
profile = "acme/vfd-mini@2"
[devices.vfd.signals]
speed_pct = { alias = "tank.pump" }
```

```sh
jk doctor --node pi.local
jk apply --node pi.local --confirm 5m
jk confirm --node pi.local
```
