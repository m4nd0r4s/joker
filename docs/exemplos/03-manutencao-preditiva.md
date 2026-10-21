# 03 — Manutenção preditiva por vibração, com treino no dispositivo

O projeto da rodada 3 do documento 08: 8 bombas, acelerómetros triaxiais numa placa ADC com relógio próprio, features por pipeline nativo, um autoencoder Burn **por bomba** treinado no local, e alarmes que a equipa de operação não desliga ao terceiro dia.

Sinais em bloco, pipelines, modelos e treino são **1.1** (ADR 0019); o manifesto declara-o em `joker = "1.1"`, e cada bloco vai marcado "(1.1)".

---

## 1. Manifesto (excerto para a bomba 1; as outras são geradas por template)

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

[buses.rs485]
kind = "serial"
port = "/dev/ttyAMA0"
baud = 19200

[devices.adc]
driver  = "spi-adc"
profile = "acme/adc24x24@1"
bus     = "spi0"
drdy    = "gpio.line_5"
rate    = "25600 Hz"

[devices.adc.signals]
# (1.1) sinais em bloco
ch1 = { alias = "p1.acc.x", kind = "block", unit = "gn", block = 2560 }
ch2 = { alias = "p1.acc.y", kind = "block", unit = "gn", block = 2560 }
ch3 = { alias = "p1.acc.z", kind = "block", unit = "gn", block = 2560 }

[devices.vfd1]
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 11
profile = "acme/vfd@3"              # dá p1.speed_hz e p1.running

[devices.vfd1.signals]
freq_hz = { alias = "p1.speed_hz" }
running = { alias = "p1.running" }

[pipelines.p1_features]             # (1.1)
from  = ["p1.acc.x", "p1.acc.y", "p1.acc.z"]
steps = [
  { op = "window", n = 4096, hop = 25600 },    # 1 janela por segundo por eixo
  { op = "detrend" },
  { op = "fft", n = 4096, window = "hann" },
  { op = "bands", edges_hz = [10, 50, 100, 200, 500, 1000, 2000, 5000, 10000] },
  { op = "stats", fields = ["rms", "peak", "crest", "kurtosis"] },
  { op = "append", signals = ["p1.speed_hz"] },          # o regime entra nas features (ver §4)
]
to = "p1.features"

[pipelines.p1_anomaly]              # (1.1)
from  = "p1.features"
steps = [
  { op = "when", expr = "p1.running && held(p1.running, 2m)" },   # ignora arranques
  { op = "model", name = "ae_p1" },
  { op = "to_signal", field = "recon_error" },
]
to = "p1.anomaly"

[models.ae_p1]                      # (1.1)
source   = "training:ae_p1"          # o modelo corrente do treino local (não um caminho interno do nó)
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
auto      = true                     # cada candidato entra em sombra sozinho
until     = { duration = "48h" }
promote_if = ["false_alarm_rate(candidate, 48h) <= false_alarm_rate(current, 48h)"]

[alarms.p1_anomaly]
when      = "hyst(p1.anomaly / model_threshold(ae_p1), 0.9, 1.0) && p1.running"
delay_on  = "5m"
delay_off = "10m"
severity  = "medium"
suppress_when = "site.mode == SiteMode.maintenance"
message   = "Bomba 1: vibração anómala (score {p1.anomaly})"

[telemetry.record]
# (1.1) gravação de blocos por gatilho
"p1.acc.*"    = { blocks = "on_trigger", trigger = "rose(alarm.p1_anomaly.active)", pre = "10s", post = "60s", keep = { raw = "180d" } }
"p1.features" = { every = "1s", keep = { raw = "30d", "1m" = "730d" } }
"p1.anomaly"  = { every = "1s", keep = { raw = "90d", "1m" = "1825d" } }
```

Não há nenhuma app Wasm: a monitorização inteira é declarativa (pipelines, modelo, treino, alarme). Uma app entra quando for preciso decidir alguma coisa, por exemplo abrir uma ordem de trabalho no CMMS por uma tarefa async.

## 2. O ciclo de vida do modelo

```
dia 0        instalação: sem modelo; p1.anomaly = Bad(NotConnected); alarme em Unknown (não dispara)
dia 0–7      o historian acumula p1.features (~600 k janelas)
dia 7        treino (SCHED_IDLE, ~minutos a medir) → candidato v1 → sombra (sem modelo atual: promoção direta, com aviso)
dia 7+       p1.anomaly ativo; limiar = p99,9 do erro no holdout, guardado no .jkm
cada semana  novo candidato → 48 h em sombra → promovido só se não aumentar os falsos alarmes
```

```sh
jk train status --node etar-norte
jk model lineage ae_p1 --node etar-norte
```

```
ae_p1  v4 (atual)   treinado 2026-09-21 03:10  dados 2026-09-14..21 (p1.features, 583 214 janelas, filtro: running && !alarm)
                    recon_p99 0.061 · limiar 0.094 · burn 0.22.1 · pai: v3
       v3           treinado 2026-09-14 …
```

## 3. Quando o alarme dispara

1. O anel de 10 s por canal é despejado para o historian (`on_trigger`), e os 60 s seguintes são escritos diretamente.
2. O alarme vai por uplink com a prioridade `alarms`.
3. O engenheiro abre a forma de onda: `jk history 'p1.acc.*' --around alarm:p1_anomaly@latest --export wav`.

## 4. Nota de modelação: regimes de funcionamento

Uma bomba com variador trabalha a 30 Hz de dia e a 45 Hz à noite. Um autoencoder treinado sobretudo com dados diurnos marca a noite como anomalia. Há duas saídas:

- incluir a velocidade nas features (o passo `append` acima), para o modelo aprender a relação;
- ou um modelo por regime, com `when` no pipeline a escolher o modelo pela banda de velocidade.

Não é um problema da runtime, mas é o erro de modelação mais comum neste tipo de projeto, e por isso fica aqui.
