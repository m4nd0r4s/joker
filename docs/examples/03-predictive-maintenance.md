# 03 — Predictive maintenance by vibration, with on-device training

The project of round 3 of document 08: 8 pumps, triaxial accelerometers on an ADC board with its own clock, features through a native pipeline, a Burn autoencoder **per pump** trained on site, and alarms that the operations team does not switch off on the third day.

Block signals, pipelines, models and training are **1.1** (ADR 0019); the manifest declares it in `joker = "1.1"`, and each block is marked "(1.1)".

---

## 1. Manifest (excerpt for pump 1; the others are generated from a template)

```toml
[project]
name = "etar-norte-bombas"
joker = "1.1"

[target]
board = "cm5"
kernel = "preempt_rt"

[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
ml = { cpus = [1], memory = "1 GiB" }
bg = { cpus = [0] }

[buses.spi0]
kind = "spi"
dev  = "/dev/spidev0.0"

[devices.adc]
driver  = "spi-adc"
profile = "acme/adc24x24@1"
bus     = "spi0"
drdy    = "gpio.line_5"
rate    = "25600 Hz"

[devices.adc.signals]
# (1.1) block signals
ch1 = { alias = "p1.acc.x", kind = "block", unit = "gn", block = 2560 }
ch2 = { alias = "p1.acc.y", kind = "block", unit = "gn", block = 2560 }
ch3 = { alias = "p1.acc.z", kind = "block", unit = "gn", block = 2560 }

[devices.vfd1]
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 11
profile = "acme/vfd@3"              # gives p1.speed_hz and p1.running

[devices.vfd1.signals]
freq_hz = { alias = "p1.speed_hz" }
running = { alias = "p1.running" }

[pipelines.p1_features]             # (1.1)
from  = ["p1.acc.x", "p1.acc.y", "p1.acc.z"]
steps = [
  { op = "window", n = 4096, hop = 25600 },    # 1 window per second per axis
  { op = "detrend" },
  { op = "fft", n = 4096, window = "hann" },
  { op = "bands", edges_hz = [10, 50, 100, 200, 500, 1000, 2000, 5000, 10000] },
  { op = "stats", fields = ["rms", "peak", "crest", "kurtosis"] },
  { op = "append", signals = ["p1.speed_hz"] },          # the operating regime enters the features (see §4)
]
to = "p1.features"

[pipelines.p1_anomaly]              # (1.1)
from  = "p1.features"
steps = [
  { op = "when", expr = "p1.running && held(p1.running, 2m)" },   # ignores startups
  { op = "model", name = "ae_p1" },
  { op = "to_signal", field = "recon_error" },
]
to = "p1.anomaly"

[models.ae_p1]                      # (1.1)
source   = "training:ae_p1"          # the current model of the local training (not an internal path of the node)
backend  = ["burn:cpu"]
resources = "ml"
deadline = "200ms"

[training.ae_p1]                    # (1.1)
arch      = "joker.ae@1"
hyper     = { layers = [37, 24, 8, 24, 37], epochs = 30, lr = 1e-3 }
data      = { signals = ["p1.features"], window = "7d", where = "p1.running && !any(alarm.p1_*.active)" }
schedule  = "after: 7d of data; then: weekly"
resources = "ml"
priority  = "idle"
budget    = { max_time = "2h", max_memory = "512 MiB" }
checkpoint = "10m"
validate  = { holdout = "20%", metric = "recon_p99", max = 0.08, threshold = "p99.9" }
output    = "candidate"

[models.ae_p1.shadow]
auto      = true                     # each candidate enters shadow by itself
until     = { duration = "48h" }
promote_if = ["false_alarm_rate(candidate, 48h) <= false_alarm_rate(current, 48h)"]

[alarms.p1_anomaly]
when      = "hyst(p1.anomaly / model_threshold(ae_p1), 0.9, 1.0) && p1.running"
delay_on  = "5m"
delay_off = "10m"
severity  = "medium"
suppress_when = "site.mode == SiteMode.maintenance"
message   = "Pump 1: anomalous vibration (score {p1.anomaly})"

[telemetry.record]
# (1.1) triggered block recording
"p1.acc.*"    = { blocks = "on_trigger", trigger = "rose(alarm.p1_anomaly.active)", pre = "10s", post = "60s", keep = { raw = "180d" } }
"p1.features" = { every = "1s", keep = { raw = "30d", "1m" = "730d" } }
"p1.anomaly"  = { every = "1s", keep = { raw = "90d", "1m" = "1825d" } }
```

There is no Wasm app: the whole monitoring is declarative (pipelines, model, training, alarm). An app comes in when something needs to be decided, for example opening a work order in the CMMS through an async task.

## 2. The lifecycle of the model

```
day 0        installation: no model; p1.anomaly = Bad(NotConnected); alarm in Unknown (does not fire)
day 0–7      the historian accumulates p1.features (~600 k windows)
day 7        training (SCHED_IDLE, ~minutes, to be measured) → candidate v1 → shadow (no current model: direct promotion, with a warning)
day 7+       p1.anomaly active; threshold = p99.9 of the error on the holdout, kept in the .jkm
every week   new candidate → 48 h in shadow → promoted only if it does not increase false alarms
```

```sh
jk train status --node etar-norte
jk model lineage ae_p1 --node etar-norte
```

```
ae_p1  v4 (current)  trained 2026-09-21 03:10  data 2026-09-14..21 (p1.features, 583,214 windows, filter: running && !alarm)
                     recon_p99 0.061 · threshold 0.094 · burn 0.22.1 · parent: v3
       v3            trained 2026-09-14 …
```

## 3. When the alarm fires

1. The 10 s ring of each channel is dumped to the historian (`on_trigger`), and the following 60 s are written directly.
2. The alarm goes over the uplink with the `alarms` priority.
3. The engineer opens the waveform: `jk history 'p1.acc.*' --around alarm:p1_anomaly@latest --export wav`.

## 4. Modeling note: operating regimes

A pump with a drive runs at 30 Hz by day and at 45 Hz at night. An autoencoder trained mostly on daytime data flags the night as an anomaly. There are two ways out:

- include the speed in the features (the `append` step above), so that the model learns the relationship;
- or one model per regime, with `when` in the pipeline choosing the model by speed band.

It is not a problem of the runtime, but it is the most common modeling mistake in this kind of project, and that is why it is here.
