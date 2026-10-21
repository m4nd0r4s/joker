# 08 — Stress rounds (use cases)

Six use cases, chosen to attack different axes of the design: energy and regulation, deterministic latency with vision, high-rate data with on-device training, multi-rate robotics, a heterogeneous fleet and ML actuating alongside humans. In each one: model it seriously, find what does not fit and decide what to do. The conclusions feed document 10. Document 09 does the same to the architecture in the abstract.

Success criterion of a round: it **is not** managing to model everything. It is knowing exactly what does not fit and why, and deciding whether it is a **gap to close** (new primitive, M-n) or a **boundary to declare** (document 12).

Rounds 2 (vision), 3 (vibration) and 6 (ML actuating) use **v1.1** features (ML, streams, pipelines, block signals, scheduled writes, proposals), and round 1 uses them in the forecast. They remain valid as stress on the design, and the primitives they produced are marked for 1.1 in document 10; in the blocks, the 1.1 keys carry the note "(1.1)" (ADR 0019). The blocks follow the references (`ref/01`, `ref/03`), which prevail if they diverge.

The hardware latency figures (inference on Hailo, camera readout, jitter on PREEMPT_RT) are **estimates to measure**, not measurements.

---

## Round 1 — Battery storage with PV and peak shaving (RevPi Connect 5)

### 1.1 The scenario

A commercial site with a grid connection contracted at **400 kW** and a load between 150 and 650 kW. It has **300 kWp of PV** (3 SunSpec inverters on Modbus TCP), a **250 kWh / 125 kW battery** (PCS on Modbus TCP, BMS on Modbus TCP), a **meter at the point of connection** (Modbus TCP, 200 ms) and a **grid operator ripple-control receiver** with 4 contacts (100/60/30/0% PV export) wired to the digital inputs of the RevPi DIO.

Goals: (1) never exceed 400 kW on the grid; (2) comply with the export reduction requested by the operator within a few seconds; (3) maximize self-consumption; (4) plan the SOC with a 24 h forecast.

### 1.2 The model

```toml
[target]
board = "revpi-connect-5"
kernel = "preempt_rt"

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
# inv2, inv3 identical
[devices.io]
driver = "revpi"

[devices.io.signals]
di_1 = { alias = "rse.k1" }   # 100%
di_2 = { alias = "rse.k2" }   # 60%
di_3 = { alias = "rse.k3" }   # 30%
di_4 = { alias = "rse.k4" }   # 0%

[calc]
rse_limit = "if rse.k4 then 0 % else if rse.k3 then 30 % else if rse.k2 then 60 % else 100 %"

[cycles.fast]
period = "200ms"
resources = "rt"

[outputs."pcs.p_set"]
range = ["-125 kW", "125 kW"]
rate  = "25 kW/s"
safe  = "0 kW"
safer = "equal"
interlock = "meter.f > 49.8 Hz && meter.f < 50.2 Hz && bms.soc >= 5 % && bms.soc <= 98 % && !alarm.bms_fault.active"
requires_good = ["meter.p_total", "bms.soc"]
verify = { readback = "pcs.p_set_rb", tolerance = "1 kW", within = "2s" }
max_lease = "15m"

[outputs."inv*.w_max_lim_pct"]
range = ["0 %", "100 %"]
safe  = "0 %"                          # no control, no export: it is the safe side for the grid operator
safer = "lower"
max_lease = "10s"

[apps.curtail]
source = "oci://…/rse-curtail:1.0.0"
writer = { level = "supervisor", priority = 7, lease = "2s" }
bind   = { limit = "calc.rse_limit", inverters = "inv*.w_max_lim_pct" }
cycles = { track = "fast" }

[apps.peak_shaver]
source = "oci://…/peak-shaver:1.4.2"
writer = { level = "optimizer", priority = 13, lease = "2s" }
bind   = { grid_power = "meter.p_total", soc = { signal = "bms.soc", as = "fraction" }, battery = "pcs.p_set", forecast = "model:load_forecast" }   # forecast: (1.1)
params = { limit = "400 kW", reserve = "10 %" }
cycles = { control = "fast" }

[models.load_forecast]                # (1.1)
source = "models/load-tcn-v3.jkm"
backend = ["burn:cpu"]
resources = "ml"
deadline = "5s"
```

The `peak_shaver` app is the one from document 04, §3: the 200 ms cycle tracks the limit, and an async task every 15 min requests the forecast and publishes the plan in an `Internal<Plan>`.

### 1.3 What worked

- **Portability.** The `peak_shaver` does not know the PCS is from Acme. On another site, with another PCS and another meter, only the profile and the bindings change. The W→kW and %→fraction conversion belongs to the runtime.
- **Frequency interlock.** With the grid outside 49.8–50.2 Hz, the PCS goes to 0 kW in the same tick, whatever the app's opinion.
- **Separation of priorities.** The grid operator's reduction (`supervisor`, 7) acts on the inverters, and the optimizer (`optimizer`, 13) on the battery. If tomorrow the optimizer also wants to touch the inverters, the arbiter guarantees that the reduction wins.
- **The forecast does not delay control.** The TCN takes ~0.3–1 s on CPU (to measure) and runs in the async domain, with control following the last valid plan. The expired plan raises an alarm and control switches to conservative mode.
- **Store-and-forward.** A 3-day LTE outage lost no data: the uplink cursor resumed from the historian.

### 1.4 What broke

**(a) Two watchdogs that do not know each other.** The PCS has its own communication timeout: without a write in 60 s, it goes to a configurable fallback value. Joker has a 2 s lease. Three wrong configurations show up on the very first site:
- PCS fallback set to "hold the last value": with `jokerd` dead while discharging at 125 kW, the battery discharges down to the BMS limit;
- PCS timeout of 5 s with the board restart taking 25 s: the PCS enters fallback on every runtime update, and that is not an error, but it is noise and wear;
- inverters with fallback at 100% export: with control lost during a 0% order, the site exports everything.

The design had a safe state, but it did not have the **equipment safe state**. **Gap to close → M1** (device fallback declared in the profile, configured and verified at startup, with coherence rules against `safe` and the restart time).

**(b) The register map changes without warning.** SunSpec is discovered: the map depends on the inverter's model chain. A firmware update added a model in the middle and shifted 704 (DER controls) by 50 registers. A static map would have written the export limit to the wrong register. **Gap to close → M2** (discovery with the result pinned in `joker.lock`; drift in `prod` blocks the outputs until there is a review).

**(c) "Prove that you complied with the grid operator's order."** In an audit, the operator asks for proof that the site reduced to 30% in less than X seconds after the signal, during the 14 days in which there were orders. The historian has the signals, but it does not have the **causality**: which tick read `rse.k3`, which proposal came out, and when it was applied and confirmed. **Gap to close → M3** (causal tracing in the decision log: each applied write stores the tick id, the `(SignalId, seq)` the task read and the proposal, commit and readback instants).

**(d) Distributing the reduction across the 3 inverters.** A site limit of 30% can be distributed in many ways (equal for all, proportional to available power, or prioritized for the inverter with the worst efficiency). A primitive of "grouped outputs with allocation" was considered and **rejected**: it is policy, not mechanism, and it belongs to the app. The core stays small on purpose.

**(e) Skew between meter and PCS.** `calc.site_load = meter.p_total - pcs.p_ac` combines two different requests, with up to 200 ms between them. On a 200 kW load step, the momentary error is large. There is no new primitive: coherence only exists within a request (document 02, §6.2), and the remedy is `align = "meter"` on the PCS poll (read right after the meter), or handling the transient in the app. It is documented.

**(f) Fiscal metering.** The customer wants to bill energy with Joker's data. **Boundary to declare → 12, §9**: Joker is not a certified meter. It carries and stores the values of the certified meter, but does not certify them.

### 1.5 Verdict

The model held. The three gaps (M1–M3) are about **robustness**, not expressiveness, and they are exactly the ones that blow up in the first year of operation. IZAAC's typical case fits in Joker with no infrastructure code in the app.

---

## Round 2 — Visual inspection with timed ejection (RPi 5 + AI HAT+ Hailo-8L)

### 2.1 The scenario

A packaging line with **2 parts/s** on a conveyor at **0.5 m/s**. A photoelectric sensor triggers the camera (hardware trigger). The camera sits **200 mm before** the ejector, which gives **400 ms** between the photo and the part passing. The ejector is a pneumatic valve on a digital output: a 50 ms pulse, and a tolerance of **±5 ms** to hit the part. The model is a small YOLO for defects, at 640×640, on the Hailo-8L.

Latency budget (estimates to measure):

| Stage | Time |
|---|---|
| trigger → exposure | ~1 ms |
| readout + DMA (1920×1080 NV12, 3.1 MB) | ~33 ms |
| crop + letterbox + conversion (NEON) | ~4 ms |
| Hailo-8L inference | ~10–15 ms |
| NMS + event | ~1 ms |
| decision (app) + scheduling | < 1 ms |
| **total** | **~55 ms**, against 400 ms available |

### 2.2 The model

```toml
# (1.1): camera, streams, models, pipelines and timed outputs
[devices.cam0]
driver = "v4l2"
device = "/dev/video0"
format = { width = 1920, height = 1080, pixfmt = "NV12", fps = 30 }
trigger = { kind = "hardware", input = "gpio.line_17" }
[devices.gpio]
driver = "gpio"
chip = "gpiochip0"
[devices.gpio.signals]
line_17 = { line = 17, alias = "line.photoeye", edges = "rising", timestamp = "kernel" }
line_22 = { line = 22, alias = "line.ejector", direction = "output" }

[streams."cam0.frames"]
pool = 6
mode = "latest"

[models.detector]
source = "models/yolo-s-defects.jkm"
backend = ["hailo", "ort:cpu"]
resources = "ml"
deadline = "40ms"
isolation = "process"

[pipelines.inspect]
from  = "cam0.frames"
steps = [
  { op = "correlate", with = "line.photoeye", window = "5ms" },   # associates the frame with the trigger edge
  { op = "crop", roi = [480, 0, 960, 1080] },
  { op = "letterbox", size = [640, 640] },
  { op = "model", name = "detector" },
  { op = "nms", iou = 0.45, score = 0.3, max = 16 },
]
to       = "vision.detections"
deadline = "120ms"

[outputs."line.ejector"]
safe = false
timed = true
schedule_depth = 16
pulse = "50ms"
on_late = "apply_now"                   # required (J0206); see 2.4(c)

[apps.inspector]
path   = "apps/inspector"
writer = { level = "control", priority = 10, lease = "1s" }
bind   = { detections = "vision.detections", ejector = "line.ejector" }
params = { travel = "400 ms" }
```

The app is the one from document 04, §3.5.

### 2.3 What worked

- **The signal model holds the logic.** The app has 20 lines and does not know what V4L2, Hailo or GPIO is.
- **Backend fallback.** Without the AI HAT+ (on a test bench), the same `.jkm` falls back to `ort:cpu` with the ONNX payload and runs at ~250 ms per frame (to measure). It is slow for the line, but it is good enough for development.
- **NPU isolation.** The Hailo runtime runs in `joker-ml-detector`. A driver hang kills that process and not `jokerd`, and the ejector stays under control.

### 2.4 What broke

**(a) A frame is not a signal.** 3.1 MB at 30 fps is 93 MB/s. It does not fit in a 32-byte slot, and copying it to a historian or to an event queue is absurd. **Gap to close → M4** (streams: pool of dmabuf/memfd buffers, reference-counted frames, `latest`/`queue` semantics).

**(b) Frames through Wasm.** The first version gave the frame to the app, which called the model. Each frame crossed the Wasm boundary twice (3.1 MB + 1.2 MB tensor), which at 30 fps is ~130 MB/s of copies on a CPU that also does NEON. **Gap to close → M5** (native pipelines: stream → preprocessing → model → postprocessing → event, without going through Wasm memory; the app receives 16 boxes).

**(c) The decision is async, and the actuation must be precise.** The decision comes out ~55 ms after the trigger with jitter of tens of ms, and the ejector must fire at 400 ms ±5 ms. A 1 ms cycle group would give ±1 ms but would force a 1 ms group just for this. **Gap to close → M6** (scheduled writes: `write_at(v, t)` with `t` derived from the trigger's `ts_hw` (monotonic, from the kernel), executed by the driver thread with `timerfd` and a target precision of ±100 µs, with explicit `on_late`).

While modeling `on_late`, the right question came up: **which is the safe side?** If the decision arrives late, eject the part (possible false positive, costs one part) or let it through (possible defect delivered to the customer)? For this line, the safe side is to **eject**. That is why the pipeline has `deadline = "120ms"`, and a frame that misses the deadline produces a `Bad(Deadline)` event that the app treats as a defect. The ejector's `on_late = "apply_now"` becomes irrelevant, because the late decision has already been converted into "eject". The general lesson is that `on_late` must not have a default, just like `safe` (`J0206`).

**(d) Trigger ↔ frame correlation.** If a trigger is lost (the part went by, but the camera did not fire) or the camera produces a spurious frame, the "frame n ↔ part n" association slips and the ejector starts hitting the wrong part **forever**. It is solved without a new primitive, with a pipeline operation: `correlate(with = "line.photoeye", window = "5ms")` associates each frame with the nearest edge with a kernel timestamp. A frame without an edge, or an edge without a frame, generates an error event (and the corresponding part is ejected).

**(e) Compiling for the Hailo.** The HEF is generated offline with the vendor's compiler, which is proprietary and runs on x86. **Boundary to declare → 12, §6**: Joker carries and executes vendor payloads with a declared signature, but does not compile for NPUs.

**(f) Burn does not reach the NPU.** Confirmed in practice: the default backend serves for development and fallback, and HailoRT is needed in production. It reinforces ADR 0009 (Burn by default, **not exclusive**).

**(g) Images with people.** The camera captures operators. **Boundary → 12, §10**: by default, streams are neither recorded nor sent (`record = false`), and there is an optional `blur_faces` operation, but the legal framework (GDPR, agreements with workers) belongs to the integrator.

### 2.5 Verdict

The signal abstraction held the logic, but not the data. Without M4–M6, Joker was of no use for vision. With them, the app stays trivial and the hot path is native and copy-free.

---

## Round 3 — Predictive maintenance by vibration with on-device training (CM5)

### 3.1 The scenario

8 pumps in a water treatment plant, each with a triaxial accelerometer connected to a 24-bit ADC board with its own clock (sampling is done by the hardware and read over SPI from a FIFO). **25.6 kS/s × 3 axes × 8 = 614,400 samples/s**, that is ~2.5 MB/s in f32.

- Features every 1 s per axis (one window of 4096 points per second): RMS, peak, crest factor, kurtosis and 8 FFT bands → 24 × 12 = 288 features/s, or 36 per pump plus the speed.
- One autoencoder (`joker.ae@1`) **per pump**, trained on site with 7 days of normal operation.
- Alarm when the reconstruction error stays above the threshold for 5 min. Nothing actuates automatically: pump protection belongs to the motor protection relay.

### 3.2 The model

```toml
# (1.1): block signals, pipelines, models, training and triggered recording
[buses.spi0]
kind = "spi"
dev  = "/dev/spidev0.0"

[devices.adc]
driver = "spi-adc"
profile = "acme/adc24x24@1"
bus = "spi0"
drdy = "gpio.line_5"
rate = "25600 Hz"

[devices.adc.signals]
"ch1" = { alias = "p1.acc.x", kind = "block", unit = "gn", block = 2560 }   # 100 ms blocks
# … 24 channels

[pipelines.p1_features]
from  = ["p1.acc.x", "p1.acc.y", "p1.acc.z"]
steps = [
  { op = "window", n = 4096, hop = 25600 },    # 1 window per second
  { op = "detrend" },
  { op = "fft", n = 4096, window = "hann" },
  { op = "bands", edges_hz = [10, 50, 100, 200, 500, 1000, 2000, 5000, 10000] },
  { op = "stats", fields = ["rms", "peak", "crest", "kurtosis"] },
]
to = "p1.features"                  # event with 36 floats (+ speed in example 03), 1/s

[pipelines.p1_anomaly]
from  = "p1.features"
steps = [{ op = "model", name = "ae_p1" }, { op = "to_signal", field = "recon_error" }]
to    = "p1.anomaly"

[alarms.p1_anomaly]
when      = "p1.anomaly > model_threshold(ae_p1) && p1.running"
delay_on  = "5m"
delay_off = "10m"
severity  = "medium"

[training.ae_p1]
arch = "joker.ae@1"
data = { signals = ["p1.features"], window = "7d", where = "p1.running && !any(alarm.p1_*.active)" }
schedule = "after: 7d; then: weekly"
resources = "ml"
priority = "idle"
output = "candidate"

[telemetry.record]
"p*.acc.*" = { blocks = "on_trigger", trigger = "rose(any(alarm.p*_anomaly.active))", pre = "10s", post = "60s" }
"p*.features" = { every = "1s", keep = { raw = "30d", "1m" = "730d" } }
```

### 3.3 What worked

- **Burn training on the device.** A small autoencoder (37 → 24 → 8 → 24 → 37, ~2 k parameters) with 600 k windows trains in minutes on one CM5 CPU core (estimate, few epochs), in `SCHED_IDLE`. It is the strongest argument in favor of Burn: no inference-only engine would do this without Python on the device.
- **Pipelines for signals.** FFT, bands and statistics run natively with NEON. The estimated cost, 24 FFTs of 4096 points per second, is ~6 M floating-point operations per second (5·N·log₂N), which is negligible.
- **Nothing actuates.** Joker as a monitoring system is a natural subset of the design.

### 3.4 What broke

**(a) 614 k samples/s are not scalar signals.** Passing each sample through the slot, the process image and the historian is unfeasible: that is ~0.6 M slot writes per second for these signals alone (~2 M operations counting latch and historian), each one with 32 bytes of metadata for 4 bytes of data, and with no notion of the sampling clock, which is what the FFT needs. **Gap to close → M7** (block signals: `t0`, period, n contiguous samples, per-block quality, ring per signal).

**(b) Sampling with the Linux clock.** The first idea was to read the ADC over SPI every 39 µs from an RT thread. Even with PREEMPT_RT, jitter of tens of µs destroys the FFT. **Boundary to declare → 12, §3**: above ~1 kHz, sampling **must** be done by hardware (ADC with its own clock and FIFO/DMA), and Joker reads blocks. Joker is not a signal acquisition system with software-guaranteed timing.

**(c) 2.5 MB/s on disk is 212 GB/day.** Storing everything is impossible on a 32 GB eMMC, but the waveform *around an anomaly* is the most valuable data for the reliability engineer. The recording policy gained an `on_trigger` mode with `pre` and `post`: a 10 s RAM ring per channel (24 channels × 25,600 × 4 B × 10 s ≈ 25 MB) that is flushed when the trigger fires, with the `post` written directly to disk. A `pre` of 60 s would cost ~150 MB of RAM, and that is why `jk check` adds up these rings in `J0380`. This became part of **M7**.

**(d) Training competes for CPU with control.** On a CM5 with 4 CPU cores, the weekly training of 8 autoencoders simultaneously saturates the cores and raises the temperature, and thermal *throttling* increases the jitter of the whole system, including the `rt` core. **Gap to close → M8** (resource classes with cpusets and memory limits; training in `SCHED_IDLE`, preemptible, paused under thermal pressure; one training at a time by default).

**(e) "Which model raised this alarm, and trained on what?"** Six months later, pump 3's alarm fires and the engineer asks whether the model was trained with the bearing already degraded. **Gap to close → M10** (lineage: historian interval, segment hash, `where` filter, Burn version, validation metrics, parent `.jkm`).

**(f) Flapping alarms.** With a fixed threshold, the reconstruction error oscillates around the limit and the alarm goes on and off 40 times a day. The operations team disables it on the third day. **Gap to close → M9** (first-class alarms with `delay_on`/`delay_off`, hysteresis, shelving with TTL and suppression by design, `p1.running`). The threshold is learned in validation (p99.9 of the error on the holdout), stored in the `.jkm` and read by `model_threshold()`.

**(g) A pump that changes regime.** A pump with a variable-frequency drive runs at 30 Hz by day and 45 Hz at night, and the autoencoder trained mostly by day flags the night as an anomaly. This is a modeling problem (include the speed in the features), not a runtime one. It stays as a modeling note in example 03.

### 3.5 Verdict

The signal model needed a waveform (M7) and the system needed to govern resources (M8). The combination **native pipelines + Burn training on site + proper alarms** is a product in itself.

---

## Round 4 — AGV with EtherCAT, ROS 2 and safety laser (x86 industrial PC)

### 4.1 The scenario

A differential AGV with 2 CiA 402 servo drives on EtherCAT at **1 kHz** (the drives close the current and velocity loops internally at 16 kHz; Joker sends velocity setpoints in cyclic synchronous mode). Odometry and local trajectory control run at **100 Hz**. Nav2 (ROS 2) runs on the same PC and sends `cmd_vel` at 10–20 Hz. There is a 2D LiDAR (UDP, 10 Hz, ~1,440 points per revolution), a battery on CAN and a **safety laser scanner + safety PLC** with STO hardwired to the drives.

### 4.2 The model

```toml
[buses.ecat]
kind = "ethercat"
iface = "enp2s0"
cycle = "1ms"
dc = true
[devices.drive_l]
driver = "ethercat"
bus = "ecat"
position = 0
profile = "acme/servo-cia402@2"
[devices.drive_r]
driver = "ethercat"
bus = "ecat"
position = 1
profile = "acme/servo-cia402@2"
[devices.safety] # read-only
driver = "ethercat"
bus = "ecat"
position = 2
profile = "acme/safety-plc-fsoe-status@1"
[devices.lidar]
driver = "x:sick-tim"
address = "192.168.0.1"
[drivers.sick-tim]                      # tier X: separate process (ref/06 §5)
tier = "x"
exec = "/usr/lib/joker/drivers/joker-drv-sick-tim"

[cycles.drive]
period = "1ms"
resources = "rt"
overrun = "trip"
trip_after = 3
[cycles.traj]
period = "10ms"
resources = "rt"

[atomic.wheels] # M20: same EtherCAT frame
outputs = ["drive_l.v_set", "drive_r.v_set"]
on_rate = "proportional"

[outputs."drive_*.v_set"]
range = ["-2.0 m/s", "2.0 m/s"]
rate  = "1.5 m/s2"
safe  = "0 m/s"
safe_ramp = "2.5 m/s2"                  # controlled braking; the emergency one is the safety PLC's STO
interlock = "safety.run_permit && !safety.field_warning"

[bridges.ros]
kind = "zenoh-ros2"
expose    = { "base.odom" = "/odom", "lidar.scan" = "/scan", "bms.soc" = "/battery_state" }
subscribe = { "/cmd_vel" = { type = "geometry_msgs/msg/Twist", fields = ["linear.x", "angular.z"], to = ["bridge.ros.cmd_v", "bridge.ros.cmd_w"], max_age = "200ms" } }

[apps.base]
path   = "apps/diff-base"
kind   = "wasm"
cycles = { wheels = "drive", odom = "traj" }
writer = { level = "control", priority = 10, lease = "5ms" }
bind   = { v_cmd = "bridge.ros.cmd_v", w_cmd = "bridge.ros.cmd_w", v_l = "drive_l.v_set", v_r = "drive_r.v_set", enc_l = "drive_l.pos", enc_r = "drive_r.pos" }
```

### 4.3 What worked

- **Age and lease as a dead man's switch.** ROS's `cmd_vel` arrives at `bridge.ros.cmd_v/w` with `max_age = 200ms`. If Nav2 dies or hangs, within ≤ 200 ms the values become `Stale`, the app's `get()?` fails, the app stops writing, the wheels' 5 ms lease expires and the setpoints go to 0 with the braking ramp. This does not depend on Nav2's code being correct.
- **Multi-rate.** The 1 ms group only does the inverse kinematics and the sending, and the 10 ms group does the odometry. The semantics between groups (document 02, §6.3) is enough: odometry sees the positions applied up to its latch.
- **The safety permissive.** `safety.run_permit` comes from the safety PLC. Without it, Joker does not even try to move. The envelope joins functional safety without pretending to replace it.

### 4.4 What broke

**(a) Wheels that receive setpoints from different ticks.** If the arbiter clamps the left wheel (ramp) and accepts the right one as it comes, the AGV turns without anyone having asked. The two writes must pass or fail together and must go out in the same frame. This had already appeared in document 09, attack 1, and the round confirms it. **→ M20** (atomic groups). A new detail: **rate limiting in an atomic group must be proportional**. If one wheel is clamped to 60% of the requested step, the other one is too, to preserve the curvature. It becomes the `on_rate = "proportional"` rule of atomic groups.

**(b) Integration with ROS 2.** Without a bridge, each project would write a ROS node that reads Joker's API, with latency, types and lease loss. **Gap to close → M11** (declarative bridges: ROS 2 via Zenoh/`rmw_zenoh`, OPC UA server, Modbus server, with the rule that **a write that arrives through a bridge is a write from a declared writer**, with level, lease and envelope).

**(c) 1 kHz with tight jitter.** EtherCAT with DC tolerates some jitter in the controller, but a p99.9 of 100 µs at 1 kHz is the boundary of what Linux with PREEMPT_RT guarantees on an x86 with an isolated CPU core (to measure on the target). With 4 kHz or position loops in the controller, it is no longer reasonable. **Boundary → 12, §2**: Joker does cyclic control up to ~1 kHz on qualified hardware; above that, the loops live in the drives.

**(d) Functional safety.** There is the temptation to put safe speed monitoring, the laser zones or the emergency stop in Joker. **Boundary → 12, §1**: Joker is not, and will not be, a SIL/PL certified system. The arbiter's `safety` level serves *software* protective actions and never replaces the certified safety chain.

**(e) Point clouds.** The LiDAR at 10 Hz is small (~12 KB per revolution), but a 3D LiDAR is MB/s. It confirms M4 (streams) for robotics data, with a `pointcloud` format in the frame metadata.

**(f) Intermittent Wi-Fi.** The AGV loses the network in parts of the warehouse. Store-and-forward already covered telemetry. Fleet commands (missions) do not go through Joker: they come through ROS.

### 4.5 Verdict

Joker fits as the **layer between ROS and the hardware** (what is usually a `ros2_control` with custom code), with envelope, leases and native EtherCAT. The boundaries (functional safety and rates above 1 kHz) are clear and must be written down prominently.

---

## Round 5 — Heterogeneous fleet of 3,000 heat pump controllers

### 5.1 The scenario

3,000 buildings (houses and small commercial buildings), each with a heat pump (Modbus RTU), two SG-Ready relays (2 digital outputs), an M-Bus heat meter and, in ~30%, a PV inverter (Modbus TCP). The **grid operator's reduction order for controllable consumers** is applied through the relays or a power limit. The app is the same everywhere: thermal optimization with dynamic tariff and PV forecast.

| Hardware | Quantity | CPU | RAM | Storage | Connection |
|---|---|---|---|---|---|
| CM4 in DIN enclosure | 1,800 | aarch64 4×A72 | 2 GB | eMMC 16 GB | Ethernet/LTE |
| industrial PC | 900 | x86_64 | 4 GB | SSD 64 GB | Ethernet |
| old gateway | 300 | **armv7** 2×A7 | **512 MB** | **eMMC 4 GB** | **LTE, 50 MB/month** |

Updates: apps every month, runtime every quarter, and configuration when the customer changes tariff.

### 5.2 The model

```toml
# templates/joker.hp-v3.toml — shared by all
[apps.hp_opt]
source = "oci://…/hp-optimizer:3.2.0"
writer = { level = "optimizer", priority = 13, lease = "20m" }
bind   = { sg_a = "sgready.a", sg_b = "sgready.b", flow_t = "hp.flow_temp", heat = "heatmeter.power", pv = { signal = "pv.p", optional = true } }
requires_clock = "synced"

[outputs."sgready.*"]
safe = false
max_lease = "30m"

[apps.grid_limit]
source = "oci://…/grid-limit:1.1.0"
writer = { level = "supervisor", priority = 7, lease = "5m" }

# templates/joker.cm4-din.toml — overlay for the CM4 board in a DIN enclosure
[devices.io]
driver = "gpio"
chip = "gpiochip0"
[devices.io.signals]
line_20 = { line = 20, alias = "sgready.a", direction = "output" }
line_21 = { line = 21, alias = "sgready.b", direction = "output" }

# sites/joker.hb-01234.toml
extends = ["../templates/joker.hp-v3.toml", "../templates/joker.cm4-din.toml"]
[project]
name = "hb-01234"
[devices.hp]
driver = "modbus-rtu"
bus = "rs485"
unit_id = 1
profile = "vendor-x/hp-12kw@2"
[apps.hp_opt.params]
tariff = "dynamic:epex-de"
comfort_min = "20.5 °C"
```

### 5.3 What worked

- **The same app on three architectures.** `hp-optimizer.wasm` is the same file on the 3,000 devices. `jk build --target` produces the `.cwasm` for aarch64 and x86_64, and armv7 uses Pulley (§5.4a).
- **Aliases per board.** The app binds to `sgready.a`, and each board's overlay says which GPIO or USB relay that is. The code does not know boards.
- **Profile per pump model.** 14 heat pump models in the installed base give 14 profiles and no new app.

### 5.4 What broke

**(a) armv7 without Cranelift.** Cranelift does not generate code for 32-bit ARM, and wasmtime runs there with the Pulley interpreter, estimated at an order of magnitude slower than AOT (to measure). For an app with a 10 s cycle and ~200 µs of computation, that is ~2–4 ms, which is acceptable. The design does not change, but `jk check` now **uses the WCET measured on the real target** (`jk bench`), not the one from the laptop (M23). Native ML models (Burn Flex compiled for armv7) are not affected, but models as a Wasm component (document 06, §3.4) are.

**(b) 50 MB per month.** With 40 signals at 1 min in Sparkplug B (~60 bytes per metric with overhead), that is ~100 MB/month, twice the budget, and the alarm for a pump fault cannot wait for the next window. **Gap to close → M12** (uplink budgets with priority classes, monthly projection and degradation by aggregation before discarding; alarms never degraded).

**(c) 3,000 manifests.** Copying and pasting manifests guarantees drift within six months. **Gap to close → M13** (overlays with `extends`, explicit merge rules, envelopes that may only tighten without a justified `widen`, `jk diff`/`jk explain` with the origin of each key).

**(d) Booting in 1970.** The armv7 gateways have no RTC battery. After a power outage they boot with the date of 1970 until NTP responds, which can take minutes with weak LTE. The timestamps come out wrong, the hourly tariff is applied at the wrong time and the expression windows get confused. **Gap to close → M14** (clock quality per sample and per tick, monotonic↔UTC mapping corrected retroactively in the historian, `requires_clock = "synced"` per app: without a clock, the app's outputs go to `safe`, and with the heat pumps in SG-Ready "normal" the building stays heated).

**(e) Wear of the 4 GB eMMC.** With ~2 KB/s of historian writes plus the WAL, that is ~63 GB/year. A 4 GB eMMC with ~3,000 P/E cycles withstands ~12 TB, so ~190 years on paper and much less with the controller's write amplification (to measure). It is acceptable, but `fsync = "batch"` and 4 KiB blocks are required, and `jk doctor` now shows the estimated wear (`/sys/block/mmcblk0/device/life_time`).

**(f) Rollouts, cohorts and desired state of 3,000 devices.** Who decides that 5% receive 3.2.0 first, and that if the alarm rate rises it stops? **Boundary → 12, §7**: Joker is not the fleet server. It provides the **agent**: signed desired-state document (runtime, apps, models, config), reported state, health, apply with confirmation and local rollback. The server (cohorts, waves, dashboards) is another product: it can be built on top, or Mender, balena or similar can be used for the OS layer. The agent protocol is specified so that this is possible.

**(g) Three memory consumption models.** 512 MB with the OS, `jokerd`, 3 Rust apps and the historian: ~120 MB in total, with headroom. A TypeScript app (embedded JS engine, ~40 MB) already makes it tight. **Design note:** `jk check` estimates the total memory of the site and fails with `J0380` above 70% of the board's RAM.

### 5.5 Verdict

"Runs anywhere" held, with the known cost of Pulley on armv7. The gaps (M12–M14) are all about **operation at scale**, and that is where fleet projects die. The fleet server boundary must be made explicit so that it does not turn into *scope creep*.

---

## Round 6 — ML actuating alongside humans: HVAC with a learned policy and an LLM assistant

### 6.1 The scenario

An office building with 12,000 m², 4 AHUs and 120 VAV boxes on BACnet/IP, and an existing building management system (BMS) that the operators use. A policy (MPC with a learned model, trained offline on a digital twin) proposes every 5 min the supply temperature of the AHUs and the minimum flow rates of the VAVs, with the goal of −15% energy without leaving the comfort band. A local LLM (Jetson Orin) answers operators' questions ("why is the 3rd floor hot?") and suggests actions.

### 6.2 The model

```toml
# (1.1): models, KPI-based fallback and proposal-only writers
[devices.bms] # BACnet priority used by Joker
driver = "x:bacnet-ip"
network = "10.20.0.0/16"
priority = 12

[drivers.bacnet-ip]                   # tier X over a C library (6.4 g)
tier = "x"
exec = "/usr/lib/joker/drivers/joker-drv-bacnet-ip"

[outputs."ahu*.supply_sp"]
range = ["13 °C", "20 °C"]
rate  = "0.5 °C/min"
safe  = "relinquish"                 # hand over to the BMS: releases Joker's BACnet priority
interlock = "avg(zone.worst_temp_dev, 15m) < 1.5 K"   # 15 min is the maximum window (J0305)
max_lease = "15m"

[apps.hvac_policy]
source = "oci://…/hvac-mpc:2.0.0"
writer = { level = "optimizer", priority = 14, lease = "12m" }
fallback = { controller = "app:hvac_rules", when = "ood_ratio(model:hvac_net, 30m) > 0.2 || !good(model:hvac_net)" }   # (1.1); `when` is a KPI, evaluated in bg (ref/01 §6.5)

[apps.hvac_policy.shadow]
of = "none"                          # first version: shadow against what the BMS does today
until = { duration = "14d" }
promote = { outputs = "ahu3.**" }    # partial promotion: only AHU 3

[apps.assistant]
path   = "apps/ops-assistant"
writer = { kind = "proposal" }       # M18 (1.1)
bind   = { llm = "model:ops_llm" }   # (1.1)
```

### 6.3 What worked

- **The envelope bounds the policy.** The model may be wrong, but it cannot ask for 8 °C supply, nor change 5 °C in one minute, nor keep optimizing with a zone out of comfort for 15 minutes.
- **Relinquish as the safe state.** Here "safe" is not a value: it is **handing control back to the BMS**. `safe = "relinquish"` with a BACnet driver releases Joker's priority in the equipment's *priority array*, and the equipment goes back to what the BMS says. Joker's safe-state concept generalizes effortlessly.

### 6.4 What broke

**(a) "The operator changed it and the model undid it."** An operator raises AHU 2's supply in the BMS because of a cold complaint, and 5 minutes later the model lowers it again. This is what kills trust in an optimization system. Two layers are needed: on the **equipment**, Joker writes at a BACnet priority (12) *lower* than that of the BMS operators (8), and so the manual override in the BMS wins in the equipment itself; in **Joker**, manual writes through the CLI/HMI and the model's writes need priorities with leases, with the manual one expiring on its own. **Gap to close → M15** (arbiter with 16 levels and leases, mandatory TTL on the manual levels; Joker's BACnet level is declared per device and verified).

**(b) Putting a model into production without faith.** "We switch it on and see" is not acceptable in an occupied building. **Gap to close → M16** (shadow mode: the policy runs with the real inputs and its writes are recorded as `Shadowed`, compared with what the BMS did; promotion by criteria, **partial by outputs** (only AHU 3 first) and with automatic demotion by KPI).

**(c) A day outside the training.** A cold wave at −12 °C, never seen in the digital twin. The policy gives plausible but wrong values. **Gap to close → M17** (input guard in the `.jkm` with training statistics; `Uncertain(OutOfDistribution)`; declared fallback controller that takes the model's level while the condition lasts).

**(d) The LLM proposes, and someone must approve.** The assistant suggests "raise the minimum flow rate of the floor 3 VAVs to 40%". Letting it write is unacceptable, and forcing the operator to type the command by hand loses the value of the suggestion. **Gap to close → M18** (proposal-only writers: intents with justification and TTL, approved by a human with the `operator` role, executed as a manual write on behalf of whoever approved; everything in the decision log).

**(e) Saving energy at the expense of comfort, slowly.** The envelope bounds instantaneous actions, but "average comfort worsened by 0.4 K in two weeks" is a property of the policy over time. Window expressions (`avg(…, 30m)`) cover part of it, and M16's demotion KPIs cover the rest *after* the problem happens. **Boundary → 12, §4**: the envelope bounds what a model can do, it does not guarantee that it does it well. The quality of the policy is the responsibility of whoever trains and validates it, and Joker gives them the tools (shadow, KPIs, replay), not the guarantee.

**(f) Explaining a decision three months later.** "Why did AHU 1 supply at 19 °C on the 12th at 14:05?" M3 (causal tracing) answers with the tick, the inputs, the app version and the model version. With M22 (model outputs recorded), the replay reproduces the decision exactly. Nothing new is needed, and the round confirms the value of M3 + M22.

**(g) BACnet.** The BACnet stack is complex (discovery, COV, segmentation), and the first driver will be tier X over a mature C library. It is not a design gap, it is catalog work.

### 6.5 Verdict

This is the round that justifies Joker against "a Python script with an ML library". The envelope, arbitration with leases, shadow, the input guard and proposals with approval turn "a model touching the building" into something a maintenance manager accepts. The boundary of §6.4(e) must be stated bluntly.

---

## Summary

| # | Round | Axis attacked | Gaps closed | Boundaries declared |
|---|---|---|---|---|
| 1 | BESS + PV (RevPi Connect 5) | field robustness, regulation | M1 device fallback, M2 discovery and lock, M3 causal tracing | fiscal metering |
| 2 | Visual inspection (RPi 5 + Hailo) | bulky data, deterministic latency | M4 streams, M5 native pipelines, M6 scheduled writes | compilation for NPU, image privacy |
| 3 | Vibration + training (CM5) | high rate, on-site training, alarms | M7 block signals (and triggered recording), M8 resource classes, M9 alarms, M10 lineage | hardware sampling above ~1 kHz |
| 4 | AGV (x86 + EtherCAT + ROS 2) | multi-rate, interop, safety | M11 bridges (M20 confirmed, with proportional rate) | functional safety, control > 1 kHz |
| 5 | Heterogeneous fleet (3,000) | portability, operation at scale | M12 uplink budgets, M13 overlays, M14 clock quality | fleet server |
| 6 | HVAC + LLM | ML actuating, humans in the loop | M15 arbitration, M16 shadow, M17 guard + fallback, M18 proposals | quality of model decisions |

**No round broke the central model** (signal + process image + envelope). All the gaps were closed with primitives **added around** the core, and none forced a change to the read or write semantics already defined. It is the sign that the abstraction is in the right place, and it is also the risk: 18 primitives is a lot, and the roadmap (document 11) puts them in order.
