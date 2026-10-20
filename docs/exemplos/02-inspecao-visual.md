# 02 — Inspeção visual com ejeção temporizada

O projeto da rodada 2 do documento 08, do modelo treinado à promoção de uma versão nova por modo sombra. Hardware: Raspberry Pi 5, AI HAT+ (Hailo-8L), câmara com trigger de hardware, fotocélula e válvula de ejeção em GPIO.

Quase tudo aqui é **1.1** (ADR 0019): modelos, streams e pipelines, a câmara `v4l2`, as saídas temporizadas (`timed`, `schedule_depth`, `on_late`), `write_at` e o simulador de câmara. Vai marcado "(1.1)" nos blocos; numa runtime 1.0 estas chaves dão `J0290`.

---

## 1. Do modelo ao `.jkm`

O modelo é treinado fora do dispositivo (por exemplo, um YOLO pequeno em PyTorch). O payload para a Hailo (HEF) é compilado com a ferramenta do fabricante. O `.jkm` junta os dois payloads, o HEF para produção e o ONNX para fallback e bancada:

```sh
jk model build yolo-defects.onnx \
  --name yolo-defects --version 7.0.0 \
  --payload hailo=yolo-defects-h8l.hef \
  --signature signature.toml \
  --lineage lineage.toml \
  --sign keys/models.key
jk model inspect yolo-defects-7.0.0.jkm
jk model bench yolo-defects-7.0.0.jkm --node linha2 --backend hailo,burn:cpu,ort:cpu
```

```
backend     p50      p99      memória
hailo       11.8 ms  14.1 ms  48 MB
ort:cpu     212 ms   240 ms   180 MB
burn:cpu    — (burn-onnx: operadores em falta: NonMaxSuppression; use pós-processamento nativo `nms`)
```

*(números ilustrativos; a medir)*

## 2. Manifesto

```toml
[project]
name = "linha2-inspecao"
mode = "prod"
joker = "1.1"                       # câmara, streams, modelos, pipelines e saídas temporizadas

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
record = false                      # imagens não são gravadas por omissão

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
max_actuations = "7200/h"           # 2 peças/s no máximo
accept_missing_fallback = "ejetor pneumático com retorno por mola: sem impulso, não atua"   # GPIO sem fallback (J0205 em prod)

[apps.inspector]
path = "apps/inspector"
writer = { level = "control", priority = 10, lease = "1s" }
bind = { detections = "vision.detections", ejector = "line.ejector" }
params = { travel = "400 ms", min_score = 0.6 }

[alarms.eject_rate_high]
when = "rate(app.inspector.ejected, 10m) > 0.1 * rate(app.inspector.inspected, 10m)"   # > 10% de rejeições
delay_on = "2m"
severity = "high"
message = "taxa de rejeição acima de 10%"

[telemetry.record]
"app.inspector.**" = { every = "1s", keep = { raw = "30d" } }
"vision.detections" = { keep = { raw = "7d" } }       # só as caixas, sem imagens
```

## 3. A app

```rust
use joker::prelude::*;

#[derive(Event, Clone)]
pub struct Detections {
    frame_seq: u64,
    frame_ts_hw: MonoNanos,           // flanco da fotocélula (via correlate), relógio monotónico
    items: BoundedVec<BBox, 16>,
    quality: Quality,                 // Bad(Deadline) se o pipeline não terminou a tempo
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

    // Decisão atrasada ou pipeline falhado: o lado seguro desta linha é ejetar.
    let defect = d.quality.is_bad()
        || d.items.iter().any(|b| b.class == Class::Defect && b.score as f64 >= io.min_score.get());

    if defect {
        let t = d.frame_ts_hw + io.travel.get().ms();
        io.ejector.write_at(true, t).await?;           // (1.1) impulso de 50 ms (manifesto)
        io.ejected.inc();
    }
    Ok(())
}
```

## 4. Teste de bancada

```toml
# sim/scenarios/linha.scenario.toml
schema = 1

[scenario]
name = "10 000 peças, 3% defeituosas, 1% de frames atrasados"
duration = "84m"

[devices.cam0]                       # (1.1) simulador de câmara
driver = "sim-camera"
frames = "sim/frames/*.png"          # imagens reais com etiquetas
defect_rate = 0.03
late_rate = 0.01

[[expect]]                           # (1.1) expectativas sobre métricas
metric = "eject_timing_error_p99"
max = "5ms"

[[expect]]
metric = "missed_defects"
equals = 0
```

## 5. Uma versão nova, por sombra

```sh
jk shadow start detector models/yolo-defects-8.0.0.jkm --node linha2 \
   --until "frames=50000" \
   --promote-if "agreement>=0.98" --promote-if "latency_p99<=30ms"
jk shadow status detector --node linha2
```

```
detector: 7.0.0 (ativo) vs 8.0.0 (sombra) — 31 204 frames
  concordância             98,7 %
  7.0 defeito / 8.0 ok     112   (ver: jk shadow diff detector --disagree --frames)
  7.0 ok / 8.0 defeito     294
  latência p99             13,9 ms / 15,2 ms
  OOD                      0,2 % / 0,1 %
  critérios                agreement ✓  latency ✓   → promoção automática a 50 000 frames
```

Os frames dos casos em desacordo ficam guardados temporariamente (com `record_disagreements = true` e retenção curta) para revisão humana antes da promoção.
