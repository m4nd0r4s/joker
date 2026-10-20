# 02 — Visual inspection with timed ejection

The project of round 2 of document 08, from the trained model to the promotion of a new version through shadow mode. Hardware: Raspberry Pi 5, AI HAT+ (Hailo-8L), camera with hardware trigger, photoeye and ejection valve on GPIO.

Almost everything here is **1.1** (ADR 0019): models, streams and pipelines, the `v4l2` camera, timed outputs (`timed`, `schedule_depth`, `on_late`), `write_at` and the camera simulator. It is marked "(1.1)" in the blocks; on a 1.0 runtime these keys give `J0290`.

---

## 1. From the model to the `.jkm`

The model is trained off the device (for example, a small YOLO in PyTorch). The payload for the Hailo (HEF) is compiled with the vendor's tool. The `.jkm` bundles the two payloads, the HEF for production and the ONNX for fallback and the bench:

```sh
jk model build yolo-defects.onnx \
  --name yolo-defects --version 7.0.0 \
  --payload hailo=yolo-defects-h8l.hef \
  --signature signature.toml \
  --lineage lineage.toml \
  --sign keys/models.key
jk model inspect yolo-defects-7.0.0.jkm
jk model bench yolo-defects-7.0.0.jkm --node line2 --backend hailo,burn:cpu,ort:cpu
```

```
backend     p50      p99      memory
hailo       11.8 ms  14.1 ms  48 MB
ort:cpu     212 ms   240 ms   180 MB
burn:cpu    — (burn-onnx: missing operators: NonMaxSuppression; use native postprocessing `nms`)
```

*(illustrative figures; to be measured)*

## 2. Manifest

```toml
[project]
name = "line2-inspection"
mode = "prod"
joker = "1.1"                       # camera, streams, models, pipelines and timed outputs

[target]
board = "rpi5"
kernel = "preempt_rt"

[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
ml = { cpus = [1, 2], accel = ["npu:hailo0"] }
bg = { cpus = [0] }

[devices.gpio]
driver = "gpio"
chip = "gpiochip0"

[devices.gpio.signals]
line_17 = { line = 17, alias = "line.photoeye", edges = "rising", timestamp = "kernel" }
line_22 = { line = 22, alias = "line.ejector", direction = "output" }
line_23 = { line = 23, alias = "line.running" }

[devices.cam0]                       # (1.1)
driver = "v4l2"
device = "/dev/video0"
format = { width = 1920, height = 1080, pixfmt = "NV12", fps = 30 }
trigger = { kind = "hardware", input = "line.photoeye" }

[streams."cam0.frames"]              # (1.1)
pool = 6
mode = "latest"
record = false                      # images are not recorded by default

[models.detector]                    # (1.1)
source = "models/yolo-defects-7.0.0.jkm"
backend = ["hailo", "ort:cpu"]
resources = "ml"
deadline = "40ms"
isolation = "process"
warmup = 5

[pipelines.inspect]                  # (1.1)
from = "cam0.frames"
steps = [
  { op = "correlate", with = "line.photoeye", window = "5ms" },
  { op = "crop", roi = [480, 0, 960, 1080] },
  { op = "letterbox", size = [640, 640] },
  { op = "model", name = "detector" },
  { op = "nms", iou = 0.45, score = 0.30, max = 16 },
]
to = "vision.detections"
deadline = "120ms"

[outputs."line.ejector"]
safe = false
timed = true                         # (1.1)
schedule_depth = 16                  # (1.1)
pulse = "50ms"
on_late = "apply_now"                # (1.1)
interlock = "line.running"
max_actuations = "7200/h"           # 2 parts/s at most
accept_missing_fallback = "pneumatic ejector with spring return: without a pulse, it does not actuate"   # GPIO without fallback (J0205 in prod)

[apps.inspector]
path = "apps/inspector"
writer = { level = "control", priority = 10, lease = "1s" }
bind = { detections = "vision.detections", ejector = "line.ejector" }
params = { travel = "400 ms", min_score = 0.6 }

[alarms.eject_rate_high]
when = "rate(app.inspector.ejected, 10m) > 0.1 * rate(app.inspector.inspected, 10m)"   # > 10% rejections
delay_on = "2m"
severity = "high"
message = "rejection rate above 10%"

[telemetry.record]
"app.inspector.**" = { every = "1s", keep = { raw = "30d" } }
"vision.detections" = { keep = { raw = "7d" } }       # only the boxes, no images
```

## 3. The app

```rust
use joker::prelude::*;

#[derive(Event, Clone)]
pub struct Detections {
    frame_seq: u64,
    frame_ts_hw: MonoNanos,           // photoeye edge (via correlate), monotonic clock
    items: BoundedVec<BBox, 16>,
    quality: Quality,                 // Bad(Deadline) if the pipeline did not finish in time
}

#[derive(joker::Enum)]
pub enum Class { Ok = 0, Defect = 1 }

#[derive(Record, Clone, Copy)]
pub struct BBox { class: Class, score: f32, x: f32, y: f32, w: f32, h: f32 }

#[joker::app(name = "inspector", version = "2.1.0")]
pub struct Inspector {
    #[input(event)]                   detections: EventInput<Detections>,
    #[output]                         ejector: Output<bool>,
    #[param(unit = "ms", default = 400.0)] travel: Param<f64>,
    #[param(unit = "1", default = 0.6)]    min_score: Param<f64>,
    #[publish(unit = "1")]            ejected: Counter,
    #[publish(unit = "1")]            inspected: Counter,
}

#[joker::task(on_event = "detections")]
async fn decide(io: &TaskIo<Inspector>, d: Detections) -> Result<()> {
    io.inspected.inc();

    // Late decision or failed pipeline: the safe side on this line is to eject.
    let defect = d.quality.is_bad()
        || d.items.iter().any(|b| b.class == Class::Defect && b.score as f64 >= io.min_score.get());

    if defect {
        let t = d.frame_ts_hw + io.travel.get().ms();
        io.ejector.write_at(true, t).await?;           // (1.1) 50 ms pulse (manifest)
        io.ejected.inc();
    }
    Ok(())
}
```

## 4. Bench test

```toml
# sim/scenarios/line.scenario.toml
schema = 1

[scenario]
name = "10,000 parts, 3% defective, 1% late frames"
duration = "84m"

[devices.cam0]                       # (1.1) camera simulator
driver = "sim-camera"
frames = "sim/frames/*.png"          # real images with labels
defect_rate = 0.03
late_rate = 0.01

[[expect]]                           # (1.1) expectations on metrics
metric = "eject_timing_error_p99"
max = "5ms"

[[expect]]
metric = "missed_defects"
equals = 0
```

## 5. A new version, through shadow

```sh
jk shadow start detector models/yolo-defects-8.0.0.jkm --node line2 \
   --until "frames=50000" \
   --promote-if "agreement>=0.98" --promote-if "latency_p99<=30ms"
jk shadow status detector --node line2
```

```
detector: 7.0.0 (active) vs 8.0.0 (shadow) — 31,204 frames
  agreement                98.7%
  7.0 defect / 8.0 ok      112   (see: jk shadow diff detector --disagree --frames)
  7.0 ok / 8.0 defect      294
  p99 latency              13.9 ms / 15.2 ms
  OOD                      0.2% / 0.1%
  criteria                 agreement ✓  latency ✓   → automatic promotion at 50,000 frames
```

The frames of the cases in disagreement are kept temporarily (with `record_disagreements = true` and short retention) for human review before the promotion.
