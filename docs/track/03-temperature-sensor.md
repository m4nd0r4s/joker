# Part 3 — A real sensor: the CPU temperature, alarms, historian and the API

**Goal:** the Pi 3's `ACT` LED blinks faster the hotter the CPU is; above 70 °C an alarm goes off; the temperature is recorded, and you also read all of this without `jk`, through the API.

**You will learn:** a simulated dynamic · `calc` signals with windows · units end to end (`°C` versus `K`) · quality, the `?` and the `safe` · alarms (lifecycle, `ack`, `shelve`) · historian · the HTTP and WebSocket API · roles · the Python client.

**Before you start:** part 2 done. The node `lab` is enrolled, `joker.lab.toml` has `[devices.pi]` with the `sysfs` driver, and the app `luz` 0.1 blinks the `ACT`.

---

## 1. The temperature in the simulator

There are no sensors wired to the Pi, but there is one that comes with it: the CPU temperature, in `/sys/class/thermal/thermal_zone0/temp` (in m°C). It heats up under load and cools down by itself, with a time constant of one to two minutes: it is a genuine first-order plant, and it is what gives the track dynamics without buying anything (document 23, §3). First, in the simulator:

```toml
# sim/pi.sim.toml
schema = 1

[model]
description = "Raspberry Pi 3: ACT LED and CPU temperature"

[signals]
act_led  = { type = "bool", direction = "output", panel = { label = "ACT", kind = "lamp" } }
carga    = { type = "f64", unit = "1", init = 0.0,
             panel = { label = "Load", kind = "slider", keys = ["left", "right"], step = 0.1, min = 0.0, max = 1.0 } }
cpu_temp = { type = "f64", unit = "°C", init = "45 °C", panel = { label = "CPU", kind = "gauge" } }

[dynamics]
# First order: tends to 45 °C at rest and to 75 °C at full load, with τ = 60 s.
cpu_temp = "lag(45 °C + 30 K * carga, 60s)"
```

- `carga` is a signal that **only exists in the simulator**: it stands in for `stress-ng`. No app reads it.
- `30 K` and not `30 °C`: it is a temperature **difference**. `°C + K` gives `°C`; `°C + °C` is `J0301` (document 16, §4).
- `lag` has state, so `init` is required (`J0833` without it, ref/03 §2.4.6).

With `jk dev --env sim --panel`, the panel shows the `ACT`, the `Load` (← → arrows) and the `CPU`. Set the load to 1 and watch the CPU rise: 63% of the way in one minute, almost all of it in five.

## 2. A `calc`: the 10 s average

The raw temperature has steps and noise (the real Pi's does). The app should not react to every reading, and the decision "average over how long?" belongs to the site, not to the app:

```toml
# joker.toml (add)
[calc]
temp_media = "avg(pi.cpu_temp, 10s)"
```

`calc.temp_media` is a signal like the others: it has a value, a unit (`°C`, inferred), a quality and a path. You read it with `jk read calc.temp_media` and record it in the historian.

## 3. The app, version 0.2

The rule from the track bible: the half period goes down linearly from `half_period` (at `t_frio`) to 100 ms (at `t_quente`), and stays at those extremes outside the range.

```rust
// apps/luz/src/lib.rs
use joker::prelude::*;

/// Minimum half period, when hot.
const RAPIDO_MS: f64 = 100.0;

#[joker::app(name = "luz", version = "0.2.0")]
pub struct Luz {
    #[output]
    led: Output<bool>,

    /// CPU temperature (already filtered by the manifest).
    #[input(unit = "°C")]
    temp: Input<f64>,

    #[param(unit = "ms", range = "20..5000", default = 500.0)]
    half_period: Param<f64>,
    #[param(unit = "°C", range = "0..100", default = 45.0)]
    t_frio: Param<f64>,
    #[param(unit = "°C", range = "0..110", default = 70.0)]
    t_quente: Param<f64>,
}

#[derive(Default)]
struct Pisca {
    elapsed_ms: f64,
    on: bool,
}

#[joker::cycle(period = "20ms", wcet = "5us")]
fn pisca(io: &mut CycleIo<Luz>, st: &mut Pisca) -> Result<()> {
    // 1. The temperature first. If it is not Good, the task ends here without proposing anything.
    let t = io.temp.get()?;

    // 2. x = 0 when cold, 1 when hot.
    let (frio, quente) = (io.t_frio.get(), io.t_quente.get());
    let x = ((t - frio) / (quente - frio)).clamp(0.0, 1.0);
    let lento = io.half_period.get();
    let meio = lento + x * (RAPIDO_MS - lento);

    // 3. The blink from part 1, with a half period that changes.
    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;
    if st.elapsed_ms >= meio {
        st.elapsed_ms -= meio;
        if st.elapsed_ms > meio {
            st.elapsed_ms = 0.0;          // the half period suddenly shrank
        }
        st.on = !st.on;
    }
    io.led.set(st.on);
    Ok(())
}
```

```toml
# joker.toml
[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led", temp = "calc.temp_media" }
params = { half_period = "500 ms", t_frio = "45 °C", t_quente = "70 °C" }
cycles = { pisca = "main" }
```

The app does not know that the temperature comes from a kernel file, or that it is a 10 s average. It knows it has a port `temp` in `°C`.

## 4. Units: what `jk check` says

By mistake, change the port's unit to `K` (`#[input(unit = "K")]`) and run `jk check --env sim`:

```
error[J0108]: incompatible quantity in the binding
  --> joker.toml:24:39
   |
24 | bind   = { led = "pi.act_led", temp = "calc.temp_media" }
   |                                       ^^^^^^^^^^^^^^^^^ `calc.temp_media` is a temperature (°C);
   |                                                         the port `temp` expects a temperature difference (K)
   = note: °C is affine: an absolute value is written in °C, a difference in K (document 16, §4)
   = help: declare the port with `unit = "°C"`
```

`K` and `°C` have the same dimension, but not the same **quantity**: a 70 K difference is not 70 °C. With `unit = "%"` the error would be `J0107` (incompatible unit), because there not even the dimension matches. The same holds for values: `t_quente = "70 K"` in `params` is `J0108`, and `when = "calc.temp_media > 70"` (without a unit) is `J0301`. Put back `°C`.

## 5. Quality and the `?`

`io.temp.get()?` returns the value only if the sample is `Good` (document 16, §3.1). If it is not, the task returns `Err` (`J-E0102`) **before** calling `io.led.set`. In that tick there is no proposal, the app's lease (3 × 20 ms = 60 ms) is not renewed, it expires, and the arbiter applies the `safe`: the LED goes off.

Why this is the right choice here: without a temperature, the app does not know at what rhythm to blink, and blinking at an invented rhythm would lie to whoever is watching. An `Err` is not a trap: it does not count toward the crashloop, and the app proposes again on the first tick in which the temperature comes back (ref/04 §8, item 8).

**How quality reaches the average.** `calc.temp_media` is a window: it becomes `Bad` when `Bad` samples cover at least 10% of the window (1 s out of 10 s), and goes back to `Good` when they fall below that, that is, about 9 s after the sensor recovers (document 16, §3.2). You will see both instants in the scenario of §7.

## 6. An alarm

```toml
# joker.toml (add)
[alarms.cpu_quente]
when     = "calc.temp_media > 70 °C"
delay_on = "10s"
severity = "medium"
message  = "CPU at {calc.temp_media}"
```

The lifecycle is a simplified ISA-18.2 (document 02, §11):

```
normal ──(condition 10 s)──▶ active_unacked ──ack──▶ active_acked
   ▲                              │                     │
   │                   condition false             condition false
   │                              ▼                     │
   └──────────ack─────────── rtn_unacked                │
   └────────────────────────────────────────────────────┘
```

With `calc.temp_media` `Bad`, the alarm becomes `unknown`: it is not known whether it is hot. With `jk dev` running and the load at 1, wait two minutes:

```sh
$ jk alarms
```

```
id           severity    state            since      message
cpu_quente   medium      active_unacked   12 s ago   CPU at 70.9 °C
```

```sh
$ jk alarm ack cpu_quente
$ jk alarm shelve cpu_quente 30m "load test on the bench"
```

```
cpu_quente: active_acked
cpu_quente: shelved until 11:32 (30 min) · "load test on the bench"
```

Shelving does not change the state of the alarm: it suppresses the notification during the TTL, which is required and limited by `max_shelve` (default 24 h). `jk alarm shelve cpu_quente 48h "…"` gives `J-E1202`. An `ack` on a `normal` alarm gives `J-E1204`: there is nothing to acknowledge. Every alarm is also a signal: `jk read alarm.cpu_quente.state`.

## 7. The scenario `aquece.scenario.toml`

```toml
# sim/scenarios/aquece.scenario.toml
schema = 1

[scenario]
name     = "hotter blinks faster; alarm at 70 °C; without temperature, safe"
manifest = "joker.sim.toml"
duration = "460s"

[[expect]]
during        = "2s .. 9s"
signal        = "pi.act_led"
toggles_every = "500ms ± 20ms"      # 45 °C: half period of 500 ms
[[set]]
at     = "10s"
signal = "pi.carga"
value  = 1.0
[[expect]]
at    = "125s"
alarm = "cpu_quente"
state = "normal"                    # the average passes 70 °C at around 123 s; the 10 s of delay_on are still missing
[[expect]]
at    = "140s"
alarm = "cpu_quente"
state = "active_unacked"
[[ack]]
at    = "200s"
alarm = "cpu_quente"
[[expect]]
at    = "201s"
alarm = "cpu_quente"
state = "active_acked"
[[expect]]
during        = "320s .. 340s"
signal        = "pi.act_led"
toggles_every = "100ms ± 20ms"      # ~75 °C: above t_quente, minimum half period
[[set]]
at     = "360s"
signal = "pi.carga"
value  = 0.0
[[expect]]
at    = "390s"
alarm = "cpu_quente"
state = "normal"                    # it cooled down, and it was already acknowledged
[[inject]]
at      = "420s"
signal  = "pi.cpu_temp"
quality = "bad(comm_failure)"
for     = "20s"
[[expect]]
at      = "430s"
signal  = "calc.temp_media"
quality = "bad"
[[expect]]
during = "422s .. 439s"
signal = "pi.act_led"
equals = false                      # without temperature: no proposal, lease expired, safe
[[expect]]
at    = "430s"
alarm = "cpu_quente"
state = "unknown"
[[expect]]
within = "450s .. 452s"
signal = "pi.act_led"
equals = true                       # the average went back to Good at around 449 s; the blink returns
```

```sh
$ jk test
```

```
apps/luz (cargo test) ........................ ok
sim/scenarios/pisca.scenario.toml ..................... ok
sim/scenarios/aquece.scenario.toml .................... ok (460 simulated s in 0.4 s)
```

The `pisca.scenario.toml` from part 1 keeps passing unchanged: at rest the simulator is at 45 °C, and at 45 °C the half period is `half_period`. The unit tests from part 1 need one more line, `.input("temp", 45.0)`: without it the port `temp` has no sample, `get()?` fails, and the LED never turns on. The two new tests, with `joker-test` (ref/04 §6):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use joker_test::prelude::*;

    #[test]
    fn quente_pisca_a_100ms() {
        let mut h = Harness::<Luz>::new().group("pisca", 20.ms()).input("temp", 72.0);
        h.run_for(5.s());
        assert!(h.output("led").after(1.s()).toggles_every(100.ms()));
    }

    #[test]
    fn sem_temperatura_apaga() {
        let temp = Series::constant(50.0).with_quality(2.s(), 3.s(), Quality::bad(CommFailure));
        let mut h = Harness::<Luz>::new().group("pisca", 20.ms()).input("temp", temp);
        h.run_for(4.s());
        assert!(h.output("led").between(2100.ms(), 3.s()).all(|v| v == false));
        assert!(!h.errors().is_empty());           // the Errs from `?`, recorded; no trap
    }
}
```

## 8. On the Pi 3

The real `pi.cpu_temp` comes from the same driver as the `ACT`. In `joker.lab.toml`, add one line to the device:

```toml
# joker.lab.toml (add)
[devices.pi.signals]
act_led  = { led = "ACT", direction = "output" }
cpu_temp = { thermal = "thermal_zone0" }          # m°C in the file; °C in the signal (ref/01, appendix A)
```

And ask the historian to record the temperature (a signal that no entry picks up is not recorded, ref/01 §5.17.1):

```toml
# joker.toml (add)
[telemetry.record]
"pi.cpu_temp"     = { every = "1s", keep = { raw = "7d", "1m" = "90d" } }
"calc.temp_media" = { every = "1s", keep = { raw = "7d" } }
```

```sh
$ jk check --env lab
$ jk build --target rpi3b-plus
$ jk apply --env lab --node lab --confirm 5m
$ jk confirm --node lab
$ jk read pi.cpu_temp calc.temp_media alarm.cpu_quente.state pi.act_led --watch --node lab
```

```
pi.cpu_temp              47.2 °C   good    age 0.6 s
calc.temp_media          47.0 °C   good
alarm.cpu_quente.state   normal    good
pi.act_led               true      good    (control:12 · luz)
```

Now heat the CPU for real, in another terminal:

```sh
lab$ sudo apt install stress-ng
lab$ stress-ng --cpu 4 --timeout 5m
```

In two or three minutes, the `ACT` starts blinking fast and, without a heat sink, the average passes 70 °C and the alarm goes off (the Pi 3 B+ lowers its clock from 60 °C upward, so the rise slows down near the end). When `stress-ng` finishes, the temperature goes down by itself, and the alarm goes to `rtn_unacked`: the condition passed, but nobody saw it. A `jk alarm ack cpu_quente --node lab` closes it.

The historian has everything:

```sh
$ jk history pi.cpu_temp --from -15m --step 10s --node lab
```

```
ts                     avg       min       max       count  quality
2026-09-25T10:41:00Z   47.3 °C   46.7 °C   47.8 °C   10     good
2026-09-25T10:41:10Z   55.9 °C   49.4 °C   60.1 °C   10     good
…
2026-09-25T10:44:30Z   72.6 °C   72.0 °C   73.1 °C   10     good
```

## 9. The API: `jk` is just a client

Everything `jk` has done so far was an HTTP request to the local API of `jokerd` (document 07, first paragraph; ref/09 §9.1). On the Pi, the Unix socket is always on:

```sh
lab$ curl -sS --unix-socket /run/joker/api.sock 'http://joker/v1/values?sel=pi.cpu_temp'
```

```json
{"items": [{"path": "pi.cpu_temp", "value": 52.6, "unit": "°C", "quality": "good", "reason": "none",
            "ts_acq": "2026-09-25T10:52:07.412603118Z", "seq": 3811}]}
```

It is the sample of ref/00 §4.2: the value in the signal's unit, **always with the quality**, an acquisition timestamp in UTC and the `seq`. The history is the same, on another endpoint (`GET /v1/history?sel=pi.cpu_temp&from=-15m&step=10s`), and so are the alarms (`GET /v1/alarms?state=active`).

**Subscribe.** To see changes without asking, there is the WebSocket `GET /v1/ws` with the subprotocol `joker.v1` (ref/09 §6). With `websocat`:

```sh
lab$ websocat --protocol joker.v1 --ws-c-uri ws://joker/v1/ws ws-c:unix:/run/joker/api.sock
{"op":"subscribe","id":"1","topic":"signals","sel":["pi.cpu_temp","calc.temp_media"],"min_interval":"1s"}
```

```
{"op":"welcome","session":"01J8ZC…","role":"admin","heartbeat":"15s","resume_window":"60s",…}
{"op":"subscribed","id":"1","sub":"s1","topic":"signals","resolved":2,"min_interval":"1s"}
{"op":"resync","sub":"s1","seq":1,"items":[{"path":"pi.cpu_temp","value":52.6,…},{"path":"calc.temp_media",…}]}
{"op":"data","sub":"s1","seq":2,"items":[{"path":"pi.cpu_temp","value":52.9,"unit":"°C","quality":"good",…}]}
```

State topics are **sampled**, not queued: a slow client receives less often, and never holds up the core (ref/09 §6.5 and §6.6). The WebSocket is only for watching: in 1.0 there are no writes through it. For scripts with `curl`, the same engine is available as SSE: `curl -N --unix-socket /run/joker/api.sock 'http://joker/v1/stream?topic=signals&sel=pi.cpu_temp'`.

**From the laptop, in Python.** The client `joker_client` resolves the node by the same rules as `jk` (ref/05 §22.2): the `lab` context from part 2 serves both.

```python
# temperatura.py   (pip install "joker-client[pandas]")
from datetime import datetime, timedelta, timezone
from joker_client import Client

jk = Client.from_config("lab")
fim = datetime.now(timezone.utc)
h = jk.history(["pi.cpu_temp", "calc.temp_media"], start=fim - timedelta(hours=1), end=fim, step="10s")
df = h.to_pandas()                                   # columns (path, field)
temp = df.xs("value", axis=1, level="field")
ok = df.xs("quality", axis=1, level="field") == "good"
print(temp.where(ok).describe())                     # what is not good becomes NaN, in plain sight
```

Quality always comes with the value, and there is no option to throw it away (ref/05 §26.1).

**Roles.** Every request has a principal (the Unix user on the socket, the certificate `CN` on TCP with mTLS) and a role, ordered (ref/09 §2.3):

| Role | May | Examples |
|---|---|---|
| `viewer` | read: values, history, alarms, decisions, subscriptions | a dashboard, a notebook |
| `operator` | act on the process: manual writes, `ack`, `shelve`, actions, `refresh` | whoever operates the bench |
| `admin` | change the node: generations, `apply`, `confirm`, secrets, audit | whoever deploys |

In `mode = "dev"`, without `[security.roles]`, any authenticated principal is `admin` (ref/01 §5.20): that is why the `welcome` above says `"role":"admin"`. In `prod`, `operator` and `admin` require the groups `joker-ops` and `joker-admin` by default, and the rest stays `viewer`.

**Why a write asks for a TTL and a reason.** Force the `ACT` on through the API, without a TTL:

```sh
lab$ curl -sS --unix-socket /run/joker/api.sock -X POST http://joker/v1/outputs/pi.act_led/manual \
       -H Content-Type:application/json -d '{"value":true,"reason":"API test"}'
```

```json
{"type":"urn:joker:code:J-E1101","title":"manual write without TTL (A10)","status":400,"code":"J-E1101",
 "help":"add \"ttl\" (1 s up to max_manual, 8 h by default)"}
```

With `"ttl":"30s"` it passes, and it is the same as `jk write pi.act_led true --ttl 30s --reason "API test"`. The TTL **is** the lease of the manual write: a forgotten write does not control forever, and the blink comes back by itself after 30 s (document 16, A10). The reason goes to the decision log and to the audit, linked by the `request_id` (ref/09 §4): a month from now, "who turned this on and why" has an answer. No write through the API goes around the arbiter and the envelope (ref/09 §2.6).

## 10. What happened underneath

- **Derived signals.** `calc.temp_media` runs in the fastest group that consumes it (`main`, because the app reads it), with the window ring preallocated (ref/01 §3.7; document 02, §10).
- **Quality.** The windows rule and the `get()` rule are those of document 16, §3; the alarm without a good input goes to `unknown` and not to `normal`.
- **The `?`** ends the task without proposals; the lease (16, A2 and A9) does the rest. It is the first layer of the safe state from part 2, now triggered by an input and not by a stopped app.
- **Alarms** are signals (`alarm.cpu_quente.state`, `.active`) and events recorded with `fsync` (ref/01 §5.15; ref/09 §5.8).
- **Historian:** it only records what `[telemetry.record]` asks for, in tiers (`raw`, `1m`) with their own retention (document 01, §12).
- **The API** is the node's only door; `jk`, `curl`, `websocat` and Python are its clients, with the same roles and the same audit (document 01, §14; ref/09).

## 11. Exercises

1. Add `delay_off = "30s"` to the alarm and repeat the `stress-ng`. What changes at the end of the test? (If your Pi has a heat sink and does not go above 65 °C, lower the limit to `"60 °C"` just for the test, and put it back.)
2. Replace the 10 s average with `"avg(pi.cpu_temp, 1m)"`. Which instants of `aquece.scenario.toml` stop passing, and why? Fix the scenario, not the app.
3. Write a Python script that subscribes to the topic `alarms` (`jk.subscribe(alarms=True)`) and prints every transition of `cpu_quente`.
4. With `jk decisions --output pi.act_led --since 5m --node lab`, find the manual write from §9 and its reason.

```sh
$ git add -A && git commit -qm "track(03): CPU temperature, alarm cpu_quente, historian and API; luz 0.2"
```

**State at the end of this part:** app `luz` 0.2.0, on lab at generation 3; `sim/pi.sim.toml` with `carga` and `cpu_temp`; `joker.toml` with `[calc] temp_media`, `[alarms.cpu_quente]` and `[telemetry.record]`; `joker.lab.toml` with `pi.cpu_temp`; new scenario `sim/scenarios/aquece.scenario.toml`.

**Next:** [Part 4 — the ESP32 as Modbus equipment: writing the profile, the button and the LED](04-esp32-modbus.md).
