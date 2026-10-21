# 03 — The manifest (`joker.toml`) and `joker.lock`

A site is described by a manifest. The manifest says **what exists** (hardware, buses, devices, signals), **what is permitted** (envelopes, arbiters, roles) and **what runs** (cycles, apps, models, pipelines, telemetry). Apps say *what they need*, and the manifest says *where it is*.

> **This document is the explanation.** The exhaustive reference of the manifest (all sections and keys, types, defaults, validation, codes, overlays, effective manifest) is **`ref/01`**; that of `joker.lock`, `node.toml` and secrets is **`ref/02`**; that of profiles, boards and simulation is **`ref/03`**. Where they diverge, the reference prevails (ADR 0023). Keys marked **1.1** already exist in the schema, but a 1.0 runtime rejects them with `J0290` (D5, ADR 0019).

---

## 1. Files of a project

```
site-lisboa-01/
  joker.toml                          site manifest (normative)
  joker.lock                          pinned resolution — generated, versioned (§14)
  joker.sim.toml                      simulation overlay (`jk dev --env sim`)
  joker.sim.lock                      its lock
  profiles/acme/pcs-125@3.profile.toml       local equipment profiles (in addition to the registry)
  boards/revpi-connect-5.board.toml          local board profiles (optional)
  classes/local/station.jkc           local classes, with station.conformance.toml and station.sim.toml
  apps/                               source code of local apps (optional)
  models/                             local .jkm artifacts (optional, 1.1)
  sim/pcs.sim.toml                    simulation models for `jk dev` and tests
  sim/scenarios/pico.scenario.toml    `jk test` scenarios
  keys/                               public keys (.pub) referenced in [security.trust]; never private keys
  secrets.toml.enc                    secrets encrypted per recipient (HPKE); may go into Git (ADR 0024)
  .taplo.toml                         maps each suffix to its schema, for the editor (written by `jk init`)
```

- **The type is in the name** (ADR 0026): `<name>.<type>.toml` (`.profile.toml`, `.board.toml`, `.sim.toml`, `.scenario.toml`, `.conformance.toml`, `.package.toml`), and the manifest layers are called `joker.toml` or `joker.<variant>.toml`, in any directory. The content must match the suffix (`J0803`). The full table is in `ref/00` §2.
- **One lock per input manifest** (D8): `joker.toml` gives `joker.lock`, and each input overlay `joker.<variant>.toml` gives `joker.<variant>.lock` (`joker.rpi.toml` → `joker.rpi.lock`). `jk <command> --env rpi` chooses the overlay and the lock together (`ref/01` §7.7).
- **`secrets.toml.enc`** has the names and the recipients in the clear and the values encrypted for each node's key. It may be versioned, and that is how the Deck distributes secrets without being able to read them (D6, `ref/02` §2.4).

TOML because it is well known, has good parsers and good editors, and is reviewable in a PR. Quantities with a unit go in strings (`"125 kW"`). The JSON schema of the manifest is **generated** from the types of `joker-manifest` (16 §10) and published, which gives completion in any editor with taplo or LSP; the project's `.taplo.toml` maps the schema of each type to its suffix. An unknown key is an error (`J0140`); `x_*` keys are extensions and are ignored (`ref/00` §1).

---

## 2. Overlays (M13)

```toml
extends = ["../templates/joker.bess-v2.toml", "../templates/joker.org-telemetry.toml"]
```

`extends` is the first key of the file. Paths are relative to the file that declares them, and there are only local files (`ref/01` §7.1). Merge rules, applied in the order of the list and then by the current file (the linearization of a chain with diamonds is in `ref/01` §7.1):

| Type | Rule |
|---|---|
| table | recursive merge |
| scalar | the later one replaces |
| array | replaces by default; `key = { append = [...] }` appends |
| remove | `key = { unset = true }` |
| envelopes | **may only tighten**: an overlay may reduce the range or the rate, but not widen them, unless with `widen = "<justification>"`, which goes into the lock with the widened value. The exact list of keys that only tighten is in `ref/01` §7.4 |

`jk diff` shows the effective manifest and the origin of each key (`jk explain pcs.p_set.range`).

---

## 3. Project, target, resources, clock

```toml
[project]
name    = "site-lisboa-01"
joker   = "1.0"                 # minimum runtime version
mode    = "prod"                # dev | prod (default: prod; prod: signatures required, fallback warnings = errors)

[target]
board   = "revpi-connect-5"     # board profile ("name", or "name@major"): known GPIO/IO, watchdog, CPU cores, accelerators
arch    = "aarch64"             # inferred from the board if omitted
kernel  = "preempt_rt"          # preempt_rt | standard (default) ; jk doctor checks it

[resources]
rt  = { cpus = [3] }
io  = { cpus = [2] }
ml  = { cpus = [1, 2], memory = "1 GiB" }     # accel = ["npu:hailo0"] is 1.1
bg  = { cpus = [0], memory = "512 MiB" }

[clock]
sync      = "ntp"               # ntp | ptp | gnss | none ; on Joker Micro: link | extern
servers   = ["pool.ntp.org"]
max_holdover = "24h"            # after this in holdover: ClockUnsynced
```

- The default of `mode` is `prod`, so that forgetting the key on a real site does not turn off signatures; `jk init` writes `mode = "dev"`. In `prod`, `[project] joker` and `[target] board` are required.
- On **Joker Micro**, `[clock] sync = "link"` (the time comes from the Joker Link `TIME`) or `"extern"` (the integrator provides it with `jk_time_set_utc`, `ref/08`). The sections specific to an MCU (`[link]`, `[target.capacity]`, `[target.micro]`, `[target.extern]`) are in `ref/01` §5.22 and in document 19.

---

## 4. Buses

```toml
[buses.plant]
kind = "ethernet"
iface = "eth1"

[buses.rs485]
kind   = "serial"
port   = "/dev/ttyRS485-0"
baud   = 19200
parity = "even"                 # none | even | odd
stop   = 1
turnaround = "5ms"              # minimum silence between requests (RTU)

[buses.can0]
kind    = "can"
iface   = "can0"
bitrate = 500000

[buses.ecat]
kind  = "ethercat"
iface = "eth2"
cycle = "1ms"
```

A `serial`, `i2c`, `spi` or `ethernet` bus serializes access: two Modbus RTU devices on the same RS-485 share a single request scheduler (document 05, §5). A `can` bus does not serialize (frames are arbitrated by the bus itself), and a TCP device without `bus` uses the system's routing, with no common scheduler; with a `bus` of `kind = "ethernet"`, it shares it (useful when a PLC accepts only one connection). Different protocols on the same serial bus are `J0171`. The keys of each `kind` are in `ref/01` §5.6.

---

## 5. Devices

```toml
[devices.meter]
driver  = "modbus-tcp"
bus     = "plant"
address = "192.168.10.20:502"
unit_id = 1
profile = "janitza/umg604@2"     # profile registry; major version pinned
poll    = "200ms"
timeout = "300ms"
retries = 1

[devices.pcs]
driver   = "modbus-tcp"
address  = "192.168.10.30:502"
profile  = "sunspec"             # short form of "joker/sunspec@1"
discover = true                  # discovers SunSpec models; the result goes into the lock (M2)
poll     = "500ms"
fallback = { configure = true, keepalive = "5s" }  # M1: configure and verify the profile's fallback; renew the write every 5 s

[devices.bms]
driver  = "canopen"
bus     = "can0"
node_id = 5
eds     = "profiles/bms-x.eds"

[devices.io]
driver = "revpi"                 # piControl: DIO/AIO modules detected from the PiCtory config
config = "/var/www/revpi/pictory/projects/_config.rsc"

[devices.cam0]                    # 1.1: the v4l2 driver and streams (M4) are 1.1
driver = "v4l2"
device = "/dev/video0"
format = { width = 1920, height = 1080, pixfmt = "NV12", fps = 30 }
trigger = { kind = "hardware", input = "io.di_3" }

[devices.sim_tank]
driver = "sim"
model  = "sim/tank.sim.toml"         # simulated plant (document 04, §6)
```

Apart from `driver`, `bus`, `profile`, `poll`, `timeout`, `retries`, `align`, `discover`, `fallback`, `signals` and `actions`, the keys belong **to the driver** (`address`, `unit_id`, `node_id`, `eds`, `config`…): the catalog is in appendix A of `ref/01`, and tier W and X drivers publish their schema (`ref/06`). The `config` of a device is validated by the profile's `[config]` (`ref/03` §2.2.10).

### 5.1 Device signals

The profile defines the signals. The manifest only **adds or adjusts**:

```toml
[devices.meter.signals]
p_total = { max_age = "1s", record = { every = "1s" } }
f       = { limits = ["45 Hz", "55 Hz"], stuck_detect = "1m" }   # physical limits → Bad(OutOfRange); frozen → Uncertain(Frozen)

[devices.io.signals]
di_3 = { alias = "line.photoeye" }                  # alias: the signal now has two paths
do_1 = { alias = "line.ejector" }

[devices.adc.signals]
a0 = { alias = "tank.level", scale = { from = ["0.6 V", "3.0 V"], to = ["0 m", "3 m"] } }   # two-point scale
```

### 5.2 Device actions

```toml
[devices.pcs.actions.reset_fault]
permit  = "pcs.state == PcsState.fault"
role    = "operator"
timeout = "5s"
```

The action must exist in the profile. The **implementation** (the steps, the registers) belongs to the profile, and not to the manifest (`ref/03` §2.2.12); the manifest only configures it.

---

## 6. Outputs: envelope, arbiter, fallback

All outputs (signals with direction `output`) need an entry here. **A missing `safe` is `J0201`.**

```toml
[outputs."pcs.p_set"]
range      = ["-125 kW", "125 kW"]     # also accepts expressions: ["-bms.p_max_discharge", "bms.p_max_charge"] (document 16, E6)
on_range   = "clamp"                # clamp | reject | trip
rate       = "25 kW/s"
deadband   = "0.5 kW"
min_interval = "200ms"
max_actuations = "120/h"          # actuation budget (relays, contactors, starts)
safe       = "0 kW"                 # value, or "relinquish" (hands control back to the equipment's own logic, §6.1)
safer      = "equal"                # lower | higher | equal — to compare with the device fallback
safe_ramp  = "50 kW/s"              # optional: also go to safe along a ramp
interlock  = "meter.f > 49.8 Hz && meter.f < 50.2 Hz && bms.soc >= 5 % && !alarm.bms_fault.active"
on_interlock = "safe"               # safe (default) | hold
requires_good = ["meter.p_total", "bms.soc"]
grace      = "2s"
max_lease  = "15m"
max_manual = "8h"
verify     = { readback = "pcs.p_set_rb", tolerance = "1 kW", within = "2s" }
reset_when = "bms.state != BmsState.fault"   # leaves the trip by itself when the condition holds (document 16, E7)
group      = "fast"                 # owning cycle group; by default, rule A11 (document 16)

[outputs."line.ejector"]
safe       = false
pulse      = "50ms"                 # pulse output: returns to `safe` after 50 ms
schedule_depth = 32                 # 1.1 (M6): scheduled writes; default 0 = disabled
timed      = true                   # 1.1 (M6)
on_late    = "apply_now"            # 1.1 (M6): required with schedule_depth > 0
```

- `schedule_depth`, `timed`, `on_late` and `late_margin` are **1.1** (scheduled writes, document 02, §7.2).
- The outputs of a `joker-link` device are governed by the envelope of Joker Micro: declaring them here is `J0234` (document 19, §6).

### 6.1 `safe = "relinquish"`

There are outputs for which the safe side is not a value, but **handing control back** to whoever had it before: the equipment's local logic, a building management system or the existing PLC. `safe = "relinquish"` does that. In drivers with a *priority array* (BACnet), it releases Joker's priority; in equipment with an "active remote control" mode, it writes the local mode (declared in the profile in `relinquish = { … }`). It is only accepted in drivers that support it (`J0207`), and `safer` no longer applies.

---

## 7. Cycle groups

```toml
[cycles.fast]
period    = "200ms"
resources = "rt"
overrun   = "skip"                  # skip | catch_up | degrade | trip
trip_after = 10
trace     = { continuous = "6h" }   # exact trace ring on disk (jk trace-dump); cost ≈ N_in × 16 B per tick (document 16, §7)
order     = ["peak_shaver"]         # order of tasks in the tick (those not listed run afterwards, in lexicographic order of <app>.<task>)

[cycles.slow]
period    = "5s"
resources = "rt"
```

The `SCHED_FIFO` priority is assigned automatically (rate-monotonic). `jk check` applies real-time admission (M23). The lexicographic order of the unlisted tasks, and not "the manifest order", makes the result independent of the order of the overlay layers.

Reserved names of implicit groups (`ref/01` §5.11): `default` (100 ms, the owning group of outputs written only by async, bridges, CLI or async implementations, 16 A11), `default_<period>` (tasks of apps without `cycles`) and `calc`.

---

## 8. Apps

```toml
[apps.peak_shaver]
source  = "oci://registry.example.com/joker/peak-shaver:1.4.2"   # or path = "apps/peak-shaver"
digest  = "sha256:9f2c…"            # pinned in the lock if omitted
writer  = { level = "optimizer", priority = 13, lease = "2s" }
resources = { cycle = "rt", async = "bg", memory = "32 MiB" }
requires_clock = "synced"           # any (default) | holdover | synced

[apps.peak_shaver.bind]
grid_power = "meter.p_total"
soc        = { signal = "bms.soc", as = "fraction" }
battery    = "pcs.p_set"
# forecast = "model:load_forecast"  # model port: 1.1 (the port is declared `optional` to run on a 1.0)

[apps.peak_shaver.secrets]          # local name in the app = site secret
tariff_token = "secret:tariff_api_token"

[apps.peak_shaver.params]
limit    = "400 kW"
reserve  = "10 %"

[apps.peak_shaver.cycles]
control = "fast"                    # cycle task `control` → group `fast`

[apps.peak_shaver.net]
allow = ["api.tariffs.example.com:443"]

[apps.peak_shaver.shadow]          # M16: this version runs in shadow against another
of        = "peak_shaver@1.3.9"
until     = { duration = "72h", max_divergence = "5 kW p95" }
```

- An unbound app port is `J0110`, unless it is `optional` in the code.
- A parameter outside the range declared by the app is `J0120`.
- `writer.level` + `priority` define the level in the arbiter; two writers at the same level of the same output is `J0210`. If an output has a **single writer**, `writer` may be omitted: it becomes `control:12` with a lease of 3 periods of the group. With two or more writers, it is required (`J0211`).
- Without `cycles`, the app's cycle tasks run in an implicit group `default_<period>` with the period requested by the app (and with no guaranteed RT). The `default` group (100 ms) is another one: it is the one of rule A11 of document 16.
- `writer.kind = "proposal"` (M18, **1.1**): the app may only `propose`, and direct writes are `J0212`.
- **Secrets** (D6): `[apps.<app>.secrets]` is a **table** that binds each local name the app declares in its code to a site secret (`"secret:<name>"`). Only the async instance may request them. A required secret with no binding, or a binding to a name the app does not declare, is an error (`ref/02` §2.4.7). The other keys that accept `secret:` (credentials of uplinks, bridges and devices) are marked in `ref/01`.
- **Memory:** without `resources.memory`, the default depends on the app's tier: 16 MiB in the cycle and 64 MiB in async in Rust, 64/64 MiB in TypeScript, 128/128 MiB in Python (`ref/05` §19). The SDK puts the suggestion in `joker:meta`, and the manifest wins.
- **`requires_clock`:** the stricter of what the app declares in its code and what the manifest says prevails (`ref/04`).

---

## 9. Models, streams and pipelines (**1.1**)

Everything in this block is 1.1 (ADR 0019): 1.0 validates the form and rejects it with `J0290`.

```toml
[models.load_forecast]
source   = "models/load-tcn-v3.jkm"
backend  = ["burn:cuda", "burn:vulkan", "burn:cpu"]   # fallback chain
resources = "ml"
deadline = "2s"
isolation = "thread"                # thread | process
warmup   = 3

[models.detector]
source   = "models/yolo-s-parts.jkm"
backend  = ["hailo", "ort:cpu"]
resources = "ml"
deadline = "40ms"
isolation = "process"

[streams."cam0.frames"]
pool     = 8                        # buffers
mode     = "latest"                 # latest | queue(n)

[pipelines.inspect]
from   = "cam0.frames"
steps  = [
  { op = "crop",      roi = [480, 0, 960, 1080] },
  { op = "resize",    size = [640, 640], keep_aspect = true },
  { op = "normalize", mean = [0.0, 0.0, 0.0], std = [255.0, 255.0, 255.0] },
  { op = "model",     name = "detector" },
  { op = "nms",       iou = 0.45, score = 0.30, max = 32 },
]
to     = "vision.detections"        # event: list<detection, 32> + frame ts
```

The detail is in document 06.

### 9.1 Other keys referred to in other documents

| Key | Where | Meaning | Detail |
|---|---|---|---|
| `align = "<device>"` | `[devices.*]` | schedules the poll right after that of the indicated device (reduces skew) | 08, round 1 |
| `fallback.keepalive` | `[devices.*]` | renewal interval of writes for the equipment's watchdog | 01 §8 |
| `requires_clock` | `[apps.*]` | `"any"` \| `"holdover"` \| `"synced"`; without the requested quality, the app's outputs go to `safe` | 01 §18 |
| `permit_uncertain = true` | `[outputs.*]` | an `Uncertain` permissive counts as true | 02 §4 |
| `stuck_detect` | signals | `Uncertain(Frozen)` after the indicated time without change | 02 §4 |
| `record = false` | `[streams.*]` | the stream is not recorded (default); **1.1** | 01 §11 |
| `trace = { continuous, async }` | `[cycles.*]` | exact trace ring; `async = true` also records async reads | 09, attack 10 |
| `[signals."<path>"]` | root | adjustments to a signal independent of the device that provides it (`debounce`, `deadband`, `max_age`, `limits`, `stuck_detect`); useful in the common manifest of a sim/hardware pair of overlays | track, parts 4–5 |
| `[training.*]` | root | on-device training jobs; **1.1** | 06 §9 |
| `[apps.*.fallback]` | apps | fallback controller and condition (KPI grammar); **1.1** | 06 §7 |
| `[apps.*.shadow]` | apps | shadow mode and promotion criteria of an app | 06 §10 |
| `[models.*.shadow]` | models | the same for models; **1.1** | 06 §10 |
| `max_shelve` | `[alarms.*]` | maximum TTL of a `jk alarm shelve` (default 24 h) | `ref/01` §5.15 |

### 9.2 Class instances (M24)

```toml
[instances.p1]
class = "std/pump@1"                       # "<package>/<name>@<major>"; std/ may be omitted
map = { run_cmd = "io.do_1", running = "io.di_1", fault = "io.di_2", pressure_out = "ai.ch1" }
capabilities = ["pressure"]                # fixed speed: without the variable_speed capability

[instances.p2]
class = "pump@1"
implemented_by = "app:pump_ctl"       # virtual instance (document 14, §7)
# an instance is born from map, implemented_by or sim (only one); sim = true, or a table of parameters of the class simulator
```

`implemented_by` accepts `"app:<name>"` (one instance of the app, with the `bind` of `[apps.<name>]`) or `{ app = "<name>", bind = { … } }` (one instance of the app per virtual instance). The actions of composite and virtual instances are configured in `[instances.<i>.actions.<a>]` (`permit`, `role`, `timeout`), and they win over the class's suggestions (document 17, §4.2).

Devices whose profile declares `[implements."<class>"]` are implicit instances with the name of the device. In app bindings, a class port accepts an instance name, a list, a selector (`"vav_3*"`) or `"class:<class>"` (all instances). The detail is in document 14. In a `map`, a class member (`member boot: std/button@1`) binds to the name of an instance, and a collection to a list of names (ref/01 §5.13).

---

## 10. Derived signals, alarms, atomic groups

```toml
[calc]
site_load = "meter.p_total - pcs.p_ac"
pv_total  = "inv1.ac.p + inv2.ac.p + inv3.ac.p"

[alarms.bms_fault]
when     = "bms.state == BmsState.fault"
severity = "critical"
actions  = { trip = ["pcs.p_set"] }

[atomic.pcs_cmd]
outputs = ["pcs.mode", "pcs.p_set"]
order   = ["pcs.mode", "pcs.p_set"]
on_rate = "independent"             # independent (default) | proportional
```

The `calc` signals run in the fastest cycle group that consumes them, or in an implicit `calc` group of 100 ms.

---

## 11. Telemetry

```toml
[telemetry.record]                  # local historian
"meter.**"   = { every = "1s", keep = { raw = "7d", "1m" = "400d" } }
"pcs.p_set"  = { on_change = true, keep = { raw = "90d" } }
"acc*.x"     = { blocks = true, keep = { raw = "48h" } }   # 1.1: block signals (M7)
"joker.**"   = { every = "10s", keep = { raw = "7d" } }

[telemetry.store]
max_disk = "8 GiB"
fsync    = "batch"

[telemetry.uplink.cloud]
kind     = "mqtt-sparkplug"
broker   = "mqtts://broker.example.com:8883"
group_id = "acme"
node_id  = "site-lisboa-01"
budget   = "300 MB/month"           # M12
classes  = { alarms = "alarm.**", high = ["meter.p_total", "pcs.**"], low = "**" }
live     = { every = "5s" }
history  = { delivery = "at_least_once" }
password = "secret:mqtt_password"   # credentials only by reference to a secret
```

The default of `classes` is `{ alarms = "alarm.**" }`: nothing else leaves the node without being requested (document 12, §10). `**` is always a whole segment.

---

## 12. Bridges

```toml
[bridges.scada]
kind   = "opcua-server"
listen = "0.0.0.0:4840"
expose = ["meter.**", "pcs.**", "alarm.**"]
writer = { level = "supervisor", priority = 7, lease = "30s", writable = ["pcs.p_set"] }
security = { policy = "Basic256Sha256", mode = "SignAndEncrypt", trust = ["keys/scada-client.der"] }

[bridges.ros]
kind   = "zenoh-ros2"
expose = { "base.odom" = "/odom", "lidar.scan" = "/scan" }
subscribe = { "/cmd_vel" = { type = "geometry_msgs/msg/Twist", fields = ["linear.x", "angular.z"], to = ["bridge.ros.cmd_v", "bridge.ros.cmd_w"], max_age = "200ms" } }
# a bridge may also write directly to an output: to = ["pcs.p_set"], writer = { level = …, priority = …, lease = … }
```

- In `subscribe`, `type` is the ROS 2 message type and `fields` says which field goes to which signal of `to`, in order (`ref/01` §5.18).
- A write through a bridge comes from a declared writer, with level, lease and envelope. The bridge authenticates through its own protocol (OPC UA session with a certificate; Modbus has none), and does not carry the API nonce (`ref/00` §9).

---

## 13. Security and roles

```toml
[security]
trust = { apps = ["keys/org-apps.pub"], config = ["keys/site-ops.pub"] }   # only restricts the node's trust bundle
api   = { listen = ["unix:/run/joker/api.sock", "tcp:0.0.0.0:7443"] }         # TCP is always mTLS

[security.roles]
operator = ["cert:CN=ops-*", "local:group=joker-ops", "deck:acme/group:operators"]
admin    = ["cert:CN=admin-*"]
```

- **`[security.trust]` only restricts** (D7, ADR 0025). The anchors and the policy (which key signs which role) come from the node's **trust bundle**, signed by an offline root (`ref/02` §2.5). In the manifest, each per-role list (`runtime`, `config`, `apps`, `drivers`, `profiles`, `boards`, `classes`, `firmware`, `secrets`, `deck`; `models` in 1.1) must be a subset of the bundle's keys with that role (otherwise `J0292`, ref/01 §5.19), and `revoked` adds revocations. A configuration must not declare its own root of trust: a generation signed by a key that it itself declares would prove nothing.
- **`api.listen`** and **`[security.roles]`** add to what `node.toml` already gives (the node's listeners and minimum roles, always in force), and never remove (`ref/02` §2.2). Changing the listeners or the roles requires a confirmation window in `prod` (the `api` reason, document 01, §15).
- **There is no `mtls`:** TCP is always mTLS. The old `mtls` key is `J0140`.
- **Principals:** `cert:<attribute>=<pattern>` (`CN`, `OU`, `O`, or `SAN`), `local:user=<name>`, `local:group=<name>`, and those of the Deck, `deck:<deck_id>/user:<id>` and `deck:<deck_id>/group:<g>` (`ref/09` §2.2, `ref/10`). Roles are cumulative (`admin` ⊇ `operator` ⊇ `viewer`); that of a principal is the highest of the rules that match it.

---

## 14. The `joker.lock` (M2)

Generated by `jk lock` and versioned. It pins everything that is not declarative. Excerpt of the canonical form (the full one, with all tables and fields, is in `ref/02` §2.1):

```toml
# This file is generated by jk. Do not edit by hand.
version = 1
runtime = "1.0.3"
node = "lisboa-01"

[boards.revpi-connect-5]
resolved = "1.2.0"
digest = "sha256:…"

[profiles."janitza/umg604@2"]
resolved = "2.3.1"
digest = "sha256:…"

[discovered.pcs]                     # result of SunSpec discovery (M2)
driver = "modbus-tcp"
profile = "joker/sunspec@1"             # the lock always uses the full form
at = "2026-09-20T10:14:03Z"
serial = "A1B2C3"
firmware = "PCS-FW 4.12.0"
digest = "sha256:…"                  # hash of the effective map
models = [{ id = 1, at = 40002, len = 66 }, { id = 103, at = 40070, len = 50 }, { id = 704, at = 40122, len = 101 }]

[fallback.pcs]                       # M1
outputs = ["pcs.p_set"]
register = "model704.WMaxLimPctRvrtTms"
timeout = "60s"
value = "0 %"
configured = true
verified = "2026-09-20T10:14:05Z"

[apps.peak_shaver]
version = "1.4.2"
digest = "sha256:9f2c…"
wit = "joker:runtime@1.0.0"

[classes."std/pump@1"]               # M24: exact version and contract hash of each class used
resolved = "1.2.0"
contract = "sha256:…"

[wcet]                               # measured by `jk bench` on the target (M23)
"peak_shaver.control" = { p999 = "38us", max = "51us", samples = 100000, board = "revpi-connect-5", runtime = "1.0.3", app = "sha256:9f2c…", measured = "2026-09-20T10:20:00Z" }

[accepted]                           # justified exceptions, with who approved them and the approved value
accept_missing_fallback."panel.led" = { reason = "indicator LED; no physical consequences", by = "user:dl", at = "2026-09-18T09:02:11Z" }
widen."pcs.p_set.range" = { reason = "new PCS with 150 kW rated; approved by DL on 2026-09-18", value = ["-150 kW", "150 kW"], by = "user:dl", at = "2026-09-18T09:02:11Z" }
```

- The lock also pins tier W and X drivers (`[drivers.*]`, with the digest of the executable for the target's architecture), the MCUs (`[mcu.*]`) and, in 1.1, the models. The hash of the effective manifest is in `ref/01` §8.3; the **generation hash**, which covers manifest, lock, contracts and artifacts, is that of the `joker.config.v1+cbor` descriptor (`ref/10` §2.6.2).
- `[wcet]` holds the p99.9 and the maximum. Tier 2 apps enter admission by the **maximum** (D15).
- In `[accepted]`, `value` exists so that an approval for 150 kW does not serve for 200 kW.

**Drift:** at startup, the driver compares the real state (firmware, discovered models, fallback) with the lock. If there is a difference, it raises `J0401 device drift` and, in `prod`, **does not enable that device's outputs** until someone runs `jk lock --update pcs`, reviews and applies. A firmware change that touches the register map must not go unnoticed. On an MCU, drift also compares the generation hash and the catalog hash announced in the `HELLO` (`ref/02` §3.1.5, `ref/07`).

---

## 15. Validation codes (selection)

The `J…` are errors. In `mode = "dev"`, those that only matter in production (for example, a missing device fallback) appear as `W…` warnings with the same number (`W0205`). The full registry, with severities, is in document 16, §9; the manifest codes, in `ref/01` §10, and those of the lock, in `ref/02` §2.6.

| Code | Meaning |
|---|---|
| `J0101` | unknown signal (with a suggestion by edit distance) |
| `J0140` | unknown key (with a suggestion) |
| `J0107` | incompatible unit in the binding |
| `J0108` | incompatible quantity kind (W vs var) |
| `J0110` | required port with no binding |
| `J0120` | parameter outside the range declared by the app |
| `J0121` | unknown parameter (the app does not declare it) |
| `J0201` | output without `safe` |
| `J0202` | `rate` with a unit incoherent with the output |
| `J0205` | device fallback incoherent with `safe` (M1) |
| `J0206` | output with scheduled writes without `on_late` |
| `J0207` | `safe = "relinquish"` on a driver without relinquish support |
| `J0210` | two writers at the same arbiter level |
| `J0211` | output with several writers and one of them without a declared `writer` |
| `J0212` | proposal-only writer with a direct write |
| `J0220` | overlay widens the envelope without `widen` |
| `J0290` | key reserved for 1.1 on a 1.0 runtime |
| `J0301` | expression with incompatible dimensions |
| `J0305` | expression window above the maximum |
| `J0350` | unschedulable cycle group (M23) |
| `J0351` | group below 10 ms without `kernel = "preempt_rt"` |
| `J0352` | (warning) group ≥ 10 ms without PREEMPT_RT: jitter not guaranteed |
| `J0370` | overloaded bus (document 05, §5) |
| `J0380` | estimated site memory above 70% of the board's RAM |
| `J0360` | resource class without CPUs or with CPUs overlapping `rt` |
| `J0401` | device drift against the lock |
| `J0501` | model with no available backend on the target board |
| `J0601` | required member of a class with no mapping in the instance |
| `J0602` | capability required by the app (`requires`) or declared in the instance without the mapped members |
| `J0603` | `const:` mapping on an `out` member (document 14, §5.2) |
| `J0604` | name collision between an instance and a device in the first segment (document 16, P3) |
| `J0610` | number of instances outside the bounds of `Many<C, MIN, MAX>` |
| `J0620` | more than 3 layers of chained virtual instances |
| `J0630` | major version of the instance's class different from the one requested by the app |
| `J0640` | `jk class verify` on a non-simulated instance without `--allow-actuation` (ref/03 §2.6.3) |
| `J0641` | `jk class verify` rejected: a `safety` or `manual` level is active on the instance's outputs (ref/03 §2.6.3) |
| `J0650` | class member bound to a name that is not an instance (ref/01 §5.13) |
| `J0651` | instance bound to a class member with another class or another major |
| `J0652` | instance bound to a class member without the capabilities the member requires (`@requires`) |
| `J0653` | number of instances bound to a class member outside `[min..max]`, or a list on a fixed member |
| `J0654` | cycle between composite instances: an instance is a member of itself |
| `J0655` | (warning) instance with `out` members that reaches different apps through different paths, one of them through a composition (document 16, P6) |
| `J0656` | virtual instance of a class whose methods call methods of the members (document 17, §4.1) |
| `J0502` | `.jkm` with a Burn version incompatible with the runtime |
| `J0510` | pipeline with an incompatible format between steps |
