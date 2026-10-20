# 01 — BESS with PV: peak shaving and curtailment on order of the grid operator

The complete project of round 1 of document 08: a shared template, the manifest of a site, the two Rust apps, the test scenarios and the commissioning procedure. It is the reference example for energy projects.

The load forecast (`[models.load_forecast]`, the `model:` binding and the plan's `Tensor`) is **1.1** (ADR 0019): it is marked "(1.1)" in the blocks. Without it, the rest runs on a 1.0 runtime, with the port `forecast` declared `optional`.

---

## 1. Structure

```
bess/
  templates/joker.bess-v2.toml    overlay shared by all BESS sites
  sites/lisboa-01/joker.toml      only what belongs to this site
  sites/lisboa-01/joker.lock
  sites/lisboa-01/joker.sim.toml  the same site with the devices on driver = "sim" (§6)
  apps/peak-shaver/               peak shaving app + plan
  apps/rse-curtail/               ripple-control curtailment app
  models/load-tcn-v3.jkm
  sim/*.sim.toml                  plant: load, PV, battery, meter
  sim/scenarios/*.scenario.toml   test scenarios (§6)
```

## 2. The template

```toml
# templates/joker.bess-v2.toml
[project]
joker = "1.1"                        # uses [models] (1.1, ADR 0019)
mode  = "prod"

[target]
kernel = "preempt_rt"

[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
ml = { cpus = [1], memory = "512 MiB" }
bg = { cpus = [0], memory = "512 MiB" }

[cycles.fast]
period = "200ms"
resources = "rt"
trace = { continuous = "24h" }

[outputs."pcs.p_set"]
rate  = "25 kW/s"
safe  = "0 kW"
safer = "equal"
interlock = "meter.f > 49.8 Hz && meter.f < 50.2 Hz && bms.soc >= 5 % && bms.soc <= 98 % && !alarm.bms_fault.active"
requires_good = ["meter.p_total", "bms.soc"]
grace = "1s"
verify = { readback = "pcs.p_set_rb", tolerance = "1 kW", within = "2s" }
max_lease = "15m"

[outputs."inv*.w_max_lim_pct"]
range = ["0 %", "100 %"]
safe  = "0 %"
safer = "lower"
max_lease = "10s"

[alarms.bms_fault]
when = "bms.state == BmsState.fault"
severity = "critical"
actions = { trip = ["pcs.p_set"] }

[alarms.grid_limit_exceeded]
when = "avg(meter.p_total, 1m) > calc.grid_limit"
delay_on = "1m"
severity = "high"

[apps.peak_shaver]
source = "oci://registry.example.com/joker/peak-shaver:1.4.2"
writer = { level = "optimizer", priority = 13, lease = "2s" }
cycles = { control = "fast" }

[apps.rse_curtail]
source = "oci://registry.example.com/joker/rse-curtail:1.0.0"
writer = { level = "supervisor", priority = 7, lease = "2s" }
cycles = { track = "fast" }

# (1.1) ML models: ADR 0019, ref/01 §6.1
[models.load_forecast]
source = "oci://registry.example.com/joker/models/load-tcn:3"
backend = ["burn:cpu"]
resources = "ml"
deadline = "5s"

[telemetry.record]
"meter.**" = { every = "1s", keep = { raw = "14d", "1m" = "730d" } }
"pcs.**"   = { every = "1s", keep = { raw = "14d", "1m" = "730d" } }
"inv*.**"  = { every = "5s", keep = { raw = "14d", "15m" = "730d" } }
"rse.**"   = { on_change = true, keep = { raw = "730d" } }        # proof of compliance

[telemetry.uplink.cloud]
kind = "mqtt-sparkplug"
broker = "mqtts://mqtt.example.com:8883"
group_id = "bess"
budget = "500 MB/month"
classes = { alarms = "alarm.**", high = ["meter.p_total", "pcs.p_ac", "bms.soc", "rse.**"], low = "**" }
```

## 3. The site

```toml
# sites/lisboa-01/joker.toml
extends = ["../../templates/joker.bess-v2.toml"]

[project]
name = "lisboa-01"

[target]
board = "revpi-connect-5"

[devices.meter]
driver = "modbus-tcp"
address = "192.168.10.20:502"
profile = "janitza/umg604@2"
poll = "200ms"

[devices.pcs]
driver = "modbus-tcp"
address = "192.168.10.30:502"
profile = "acme/pcs-125@3"
poll = "200ms"
align = "meter"
fallback = { configure = true }

[devices.bms]
driver = "modbus-tcp"
address = "192.168.10.31:502"
profile = "acme/bms-250@1"
poll = "1s"

[devices.inv1]
driver = "modbus-tcp"
address = "192.168.10.41:502"
profile = "sunspec"
discover = true
poll = "500ms"
fallback = { configure = true }

[devices.inv2]
driver = "modbus-tcp"
address = "192.168.10.42:502"
profile = "sunspec"
discover = true
poll = "500ms"
fallback = { configure = true }

[devices.inv3]
driver = "modbus-tcp"
address = "192.168.10.43:502"
profile = "sunspec"
discover = true
poll = "500ms"
fallback = { configure = true }

[devices.io]
driver = "revpi"

[devices.io.signals]
di_1 = { alias = "rse.k1" }
di_2 = { alias = "rse.k2" }
di_3 = { alias = "rse.k3" }
di_4 = { alias = "rse.k4" }

[calc]
grid_limit = "400 kW"
rse_limit = "if rse.k4 then 0 % else if rse.k3 then 30 % else if rse.k2 then 60 % else 100 %"

[outputs."pcs.p_set"]
range = ["-125 kW", "125 kW"]

[apps.peak_shaver.bind]
grid_power = "meter.p_total"
soc        = { signal = "bms.soc", as = "fraction" }
battery    = "pcs.p_set"
forecast   = "model:load_forecast"   # (1.1); on a 1.0 runtime, remove it and declare the port `optional`

[apps.peak_shaver.params]
limit   = "400 kW"
reserve = "10 %"

[apps.rse_curtail.bind]
limit    = "calc.rse_limit"
inverters = "inv*.w_max_lim_pct"
```

## 4. The app `rse-curtail`

Small on purpose: the distribution policy stays here, and not in the core (document 08, 1.4d).

```rust
use joker::prelude::*;

#[joker::app(name = "rse_curtail", version = "1.0.0")]
pub struct Curtail {
    #[input(unit = "%")]
    limit: Input<f64>,

    /// A "fan-out" port: it binds to a selector (inv*.w_max_lim_pct) and gives an array.
    #[output(unit = "%", many)]
    inverters: OutputMany<f64>,
}

#[joker::cycle(period = "200ms", wcet = "5us")]
fn track(io: &mut CycleIo<Curtail>, _st: &mut ()) -> Result<()> {
    // Without a good reading of the ripple control → do not write. The lease expires and the inverters
    // go to their safe (0 %). A broken contact is treated as the most restrictive order.
    let limit = io.limit.get()?;
    for inv in io.inverters.iter_mut() {
        inv.set(limit);            // equal distribution; other policies replace this line
    }
    Ok(())
}
```

## 5. The app `peak-shaver`

The declaration and the cycle are in document 04, §3.1–3.3. What is missing is the plan. `from_forecast` uses `Tensor`, which is 1.1 (ref/04 §12), like the model that feeds it:

```rust
#[derive(Record, Clone, Copy, Default)]
pub struct Plan {
    target_soc: f64,     // fraction
    limit_kw: f64,       // effective limit for the next period
    valid_until: UtcSeconds,
}

impl Plan {
    /// Without a reliable forecast: hold the battery for the worst case.
    fn conservative(now: Instant) -> Self {
        Plan { target_soc: 0.8, limit_kw: f64::INFINITY, valid_until: now.utc_seconds() + 30 * 60 }
    }

    /// With a forecast: reserve enough energy for the peaks of the next 24 h.
    fn from_forecast(f: &Tensor, limit: f64, now: Instant) -> Self {
        let load: &[f32] = f.as_slice();                  // 96 steps of 15 min, in kW
        let excess_kwh: f64 = load.iter().map(|&p| (p as f64 - limit).max(0.0) * 0.25).sum();
        let capacity_kwh = 250.0;
        let target = (excess_kwh / capacity_kwh * 1.2 + 0.1).clamp(0.2, 0.95);
        Plan { target_soc: target, limit_kw: limit, valid_until: now.utc_seconds() + 30 * 60 }
    }
}
```

## 6. Test scenarios

```toml
# sim/scenarios/rse-zero.scenario.toml
schema = 1

[scenario]
name = "0 % order fulfilled within ≤ 1 tick after the reading"
manifest = "sites/lisboa-01/joker.sim.toml"     # site overlay with the devices on driver = "sim" (ref/03 §2.5.3)
duration = "10m"

[[set]]
at = "3m"
signal = "rse.k4"
value = true

[[expect]]
at = "3m+400ms"                    # ≤ 1 poll of the DI + 1 tick
signal = "inv*.w_max_lim_pct"
equals = "0 %"

[[expect]]
during = "3m+400ms .. 10m"
signal = "inv*.ac.p"
max = "1 kW"
```

```toml
# sim/scenarios/frequency.scenario.toml
schema = 1

[scenario]
name = "under-frequency → PCS at 0 kW"
manifest = "sites/lisboa-01/joker.sim.toml"     # site overlay with the devices on driver = "sim" (ref/03 §2.5.3)
duration = "2m"

[[set]]
at = "30s"
signal = "meter.f"
value = "49.7 Hz"

[[expect]]
at = "30s+200ms"
signal = "pcs.p_set"
equals = "0 kW"
outcome = "rejected(interlock)"
```

## 7. Commissioning

```sh
jk doctor --node lisboa-01 --rt            # kernel, isolcpus, watchdog, clock
jk discover sunspec://192.168.10.41 --node lisboa-01
jk profile test acme/pcs-125@3 --node lisboa-01 --uri modbus-tcp://192.168.10.30
jk lock --node lisboa-01                   # SunSpec maps, firmware, fallbacks (M1, M2)
jk bench --node lisboa-01
jk check
jk apply --node lisboa-01 --confirm 10m
# functional tests: force the ripple-control DIs, confirm the inverters' response
jk decisions --output 'inv*.w_max_lim_pct' --since 10m --node lisboa-01
jk confirm --node lisboa-01
```

A compliance report for the grid operator comes out of the decision log (M3):

```sh
jk decisions --output 'inv*.w_max_lim_pct' --cause rse.k3 --from 2026-09-01 --to 2026-09-15 --report pdf
```
