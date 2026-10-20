# 08 — Rodadas de estresse (casos de uso)

Seis casos de uso, escolhidos para atacar eixos diferentes do desenho: energia e regulação, latência determinista com visão, dados de alta taxa com treino no dispositivo, robótica multi-ritmo, frota heterogénea e ML a atuar ao lado de humanos. Em cada um: modelar a sério, encontrar o que não cabe e decidir o que fazer. As conclusões alimentam o documento 10. O documento 09 faz o mesmo à arquitetura em abstrato.

Critério de sucesso de uma rodada: **não é** conseguir modelar tudo. É saber exatamente o que não cabe e porquê, e decidir se é uma **lacuna a fechar** (nova primitiva, M-n) ou uma **fronteira a declarar** (documento 12).

As rodadas 2 (visão), 3 (vibração) e 6 (ML a atuar) usam funcionalidades da **v1.1** (ML, streams, pipelines, sinais em bloco, escritas agendadas, propostas), e a rodada 1 usa-as na previsão. Continuam válidas como estresse do desenho, e as primitivas que produziram estão marcadas para a 1.1 no documento 10; nos blocos, as chaves 1.1 levam a nota "(1.1)" (ADR 0019). Os blocos seguem as referências (`ref/01`, `ref/03`), que prevalecem se divergirem.

Os números de latência de hardware (inferência em Hailo, readout de câmara, jitter em PREEMPT_RT) são **estimativas a medir**, e não medições.

---

## Rodada 1 — Armazenamento em bateria com FV e peak shaving (RevPi Connect 5)

### 1.1 O cenário

Um site comercial com uma ligação à rede contratada a **400 kW** e uma carga entre 150 e 650 kW. Tem **FV de 300 kWp** (3 inversores SunSpec em Modbus TCP), uma **bateria de 250 kWh / 125 kW** (PCS em Modbus TCP, BMS em Modbus TCP), um **medidor no ponto de ligação** (Modbus TCP, 200 ms) e um **recetor de telecomando do operador de rede** com 4 contactos (100/60/30/0% de injeção FV) ligados às entradas digitais do RevPi DIO.

Objetivos: (1) nunca ultrapassar 400 kW na rede; (2) cumprir a redução de injeção pedida pelo operador em poucos segundos; (3) maximizar o autoconsumo; (4) planear o SOC com uma previsão de 24 h.

### 1.2 O modelo

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
# inv2, inv3 idênticos
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
safe  = "0 %"                          # sem controlo, sem injeção: é o lado seguro para o operador de rede
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

A app `peak_shaver` é a do documento 04, §3: o ciclo de 200 ms segue o limite, e uma tarefa async a cada 15 min pede a previsão e publica o plano num `Internal<Plan>`.

### 1.3 O que funcionou

- **Portabilidade.** A `peak_shaver` não sabe que o PCS é da Acme. Noutro site, com outro PCS e outro medidor, só mudam o perfil e as ligações. A conversão W→kW e %→fração é da runtime.
- **Interlock de frequência.** Com a rede fora de 49,8–50,2 Hz, o PCS vai para 0 kW no próprio tick, qualquer que seja a opinião da app.
- **Separação de prioridades.** A redução do operador de rede (`supervisor`, 7) atua nos inversores, e o otimizador (`optimizer`, 13) na bateria. Se amanhã o otimizador também quiser mexer nos inversores, o árbitro garante que a redução ganha.
- **A previsão não atrasa o controlo.** O TCN demora ~0,3–1 s em CPU (a medir) e corre no domínio async, com o controlo a seguir o último plano válido. O plano expirado dispara um alarme e o controlo passa ao modo conservador.
- **Store-and-forward.** Uma queda de 3 dias no LTE não perdeu dados: o cursor do uplink retomou do historian.

### 1.4 O que quebrou

**(a) Dois watchdogs que não se conhecem.** O PCS tem o seu próprio timeout de comunicação: sem escrita em 60 s, vai para um valor de fallback configurável. O Joker tem um lease de 2 s. Três configurações erradas aparecem logo no primeiro site:
- fallback do PCS a "manter o último valor": com o `jokerd` morto a descarregar a 125 kW, a bateria descarrega até ao limite do BMS;
- timeout do PCS de 5 s com o reinício da placa a demorar 25 s: o PCS entra em fallback a cada atualização da runtime, e isso não é um erro, mas é ruído e desgaste;
- inversores com fallback a 100% de injeção: com o controlo perdido durante uma ordem de 0%, o site injeta tudo.

O desenho tinha estado seguro, mas não tinha o **estado seguro do equipamento**. **Lacuna a fechar → M1** (fallback do dispositivo declarado no perfil, configurado e verificado no arranque, com regras de coerência face ao `safe` e ao tempo de reinício).

**(b) O mapa de registos muda sem aviso.** O SunSpec descobre-se: o mapa depende da cadeia de modelos do inversor. Uma atualização de firmware acrescentou um modelo a meio e deslocou o 704 (controlos DER) em 50 registos. Um mapa estático teria escrito o limite de injeção no registo errado. **Lacuna a fechar → M2** (descoberta com resultado fixado no `joker.lock`; deriva em `prod` bloqueia as saídas até haver revisão).

**(c) "Prove que cumpriu a ordem do operador de rede."** Numa auditoria, o operador pede prova de que o site reduziu para 30% em menos de X segundos depois do sinal, durante os 14 dias em que houve ordens. O historian tem os sinais, mas não tem a **causalidade**: que tick leu o `rse.k3`, que proposta saiu, e quando foi aplicada e confirmada. **Lacuna a fechar → M3** (rastreio causal no registo de decisões: cada escrita aplicada guarda o id do tick, os `(SignalId, seq)` que a tarefa leu e os instantes de proposta, commit e readback).

**(d) Distribuir a redução pelos 3 inversores.** Um limite de site de 30% pode ser distribuído de muitas maneiras (igual por todos, proporcional à potência disponível, ou prioritário para o inversor com pior rendimento). Considerou-se uma primitiva de "saídas agrupadas com alocação" e **rejeitou-se**: é política, não mecanismo, e pertence à app. O núcleo fica pequeno de propósito.

**(e) Skew entre medidor e PCS.** `calc.site_load = meter.p_total - pcs.p_ac` junta dois pedidos diferentes, com até 200 ms entre eles. Num degrau de carga de 200 kW, o erro momentâneo é grande. Não há primitiva nova: a coerência só existe dentro de um pedido (documento 02, §6.2), e o remédio é o `align = "meter"` no poll do PCS (ler logo depois do medidor), ou tratar o transitório na app. Fica documentado.

**(f) Contagem fiscal.** O cliente quer faturar a energia com os dados do Joker. **Fronteira a declarar → 12, §9**: o Joker não é um contador certificado. Transporta e guarda os valores do contador certificado, mas não os certifica.

### 1.5 Veredicto

O modelo aguentou. As três lacunas (M1–M3) são de **robustez**, não de expressividade, e são exatamente as que rebentam no primeiro ano de operação. O caso típico da IZAAC cabe no Joker sem código de infraestrutura na app.

---

## Rodada 2 — Inspeção visual com ejeção temporizada (RPi 5 + AI HAT+ Hailo-8L)

### 2.1 O cenário

Uma linha de embalagem com **2 peças/s** num tapete a **0,5 m/s**. Uma fotocélula dispara a câmara (trigger de hardware). A câmara fica **200 mm antes** do ejetor, o que dá **400 ms** entre a foto e a passagem da peça. O ejetor é uma válvula pneumática numa saída digital: impulso de 50 ms, e tolerância de **±5 ms** para acertar na peça. O modelo é um YOLO pequeno para defeitos, a 640×640, na Hailo-8L.

Orçamento de latência (estimativas a medir):

| Etapa | Tempo |
|---|---|
| trigger → exposição | ~1 ms |
| readout + DMA (1920×1080 NV12, 3,1 MB) | ~33 ms |
| crop + letterbox + conversão (NEON) | ~4 ms |
| inferência Hailo-8L | ~10–15 ms |
| NMS + evento | ~1 ms |
| decisão (app) + agendamento | < 1 ms |
| **total** | **~55 ms**, contra 400 ms disponíveis |

### 2.2 O modelo

```toml
# (1.1): câmara, streams, modelos, pipelines e saídas temporizadas
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
  { op = "correlate", with = "line.photoeye", window = "5ms" },   # associa o frame ao flanco do trigger
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
on_late = "apply_now"                   # obrigatório (J0206); ver 2.4(c)

[apps.inspector]
path   = "apps/inspector"
writer = { level = "control", priority = 10, lease = "1s" }
bind   = { detections = "vision.detections", ejector = "line.ejector" }
params = { travel = "400 ms" }
```

A app é a do documento 04, §3.5.

### 2.3 O que funcionou

- **O modelo de sinais aguenta a lógica.** A app tem 20 linhas e não sabe o que é V4L2, Hailo ou GPIO.
- **Fallback de backend.** Sem a AI HAT+ (numa bancada de testes), o mesmo `.jkm` cai para `ort:cpu` com o payload ONNX e corre a ~250 ms por frame (a medir). É lento para a linha, mas serve para desenvolver.
- **Isolamento do NPU.** O runtime da Hailo corre em `joker-ml-detector`. Um encravamento do driver mata esse processo e não o `jokerd`, e o ejetor continua sob controlo.

### 2.4 O que quebrou

**(a) Um frame não é um sinal.** 3,1 MB a 30 fps são 93 MB/s. Não cabem num slot de 32 bytes, e copiá-los para um historian ou para uma fila de eventos é absurdo. **Lacuna a fechar → M4** (streams: pool de buffers dmabuf/memfd, frames com contagem de referências, semântica `latest`/`queue`).

**(b) Frames através do Wasm.** A primeira versão dava o frame à app, que chamava o modelo. Cada frame atravessava a fronteira Wasm duas vezes (3,1 MB + tensor de 1,2 MB), o que a 30 fps são ~130 MB/s de cópias numa CPU que também faz NEON. **Lacuna a fechar → M5** (pipelines nativos: stream → pré-processamento → modelo → pós-processamento → evento, sem passar pela memória Wasm; a app recebe 16 caixas).

**(c) A decisão é async, e a atuação tem de ser precisa.** A decisão sai ~55 ms depois do trigger com jitter de dezenas de ms, e o ejetor tem de disparar a 400 ms ±5 ms. Um grupo de ciclo de 1 ms daria ±1 ms mas obrigaria a um grupo de 1 ms só para isto. **Lacuna a fechar → M6** (escritas agendadas: `write_at(v, t)` com `t` derivado do `ts_hw` (monotónico, do kernel) do trigger, executadas pela thread do driver com `timerfd` e precisão alvo de ±100 µs, com `on_late` explícito).

Ao modelar o `on_late`, apareceu a pergunta certa: **qual é o lado seguro?** Se a decisão chega tarde, ejetar a peça (possível falso positivo, custa uma peça) ou deixá-la passar (possível defeito entregue ao cliente)? Para esta linha, o seguro é **ejetar**. Por isso o pipeline tem `deadline = "120ms"`, e um frame que falha o deadline produz um evento `Bad(Deadline)` que a app trata como defeito. O `on_late = "apply_now"` do ejetor passa a ser irrelevante, porque a decisão atrasada já foi convertida em "ejetar". A lição geral é que o `on_late` não pode ter um valor por omissão, tal como o `safe` (`J0206`).

**(d) Correlação trigger ↔ frame.** Se um trigger se perde (a peça passou, mas a câmara não disparou) ou a câmara gera um frame espúrio, a associação "frame n ↔ peça n" desliza e o ejetor passa a acertar na peça errada **para sempre**. Resolve-se sem primitiva nova, com uma operação de pipeline: `correlate(with = "line.photoeye", window = "5ms")` associa cada frame ao flanco com timestamp de kernel mais próximo. Um frame sem flanco, ou um flanco sem frame, gera um evento de erro (e a peça correspondente é ejetada).

**(e) Compilar para a Hailo.** O HEF é gerado offline com o compilador do fabricante, que é proprietário e corre em x86. **Fronteira a declarar → 12, §6**: o Joker transporta e executa payloads de fabricante com assinatura declarada, mas não compila para NPUs.

**(f) O Burn não chega à NPU.** Confirmado na prática: o backend por omissão serve para desenvolvimento e fallback, e o HailoRT é necessário em produção. Reforça a ADR 0009 (Burn por omissão, **não exclusivo**).

**(g) Imagens com pessoas.** A câmara apanha operadores. **Fronteira → 12, §10**: por omissão, os streams não são gravados nem enviados (`record = false`), e há uma operação `blur_faces` opcional, mas o enquadramento legal (RGPD, acordos com trabalhadores) é do integrador.

### 2.5 Veredicto

A abstração de sinais aguentou a lógica, mas não os dados. Sem M4–M6, o Joker não servia para visão. Com eles, a app continua trivial e o caminho quente é nativo e sem cópias.

---

## Rodada 3 — Manutenção preditiva por vibração com treino no dispositivo (CM5)

### 3.1 O cenário

8 bombas numa estação de tratamento de água, cada uma com um acelerómetro triaxial ligado a uma placa ADC de 24 bits com relógio próprio (a amostragem é feita pelo hardware e lida por SPI a partir de uma FIFO). **25,6 kS/s × 3 eixos × 8 = 614 400 amostras/s**, ou seja ~2,5 MB/s em f32.

- Features a cada 1 s por eixo (uma janela de 4096 pontos por segundo): RMS, pico, fator de crista, curtose e 8 bandas de FFT → 24 × 12 = 288 features/s, ou 36 por bomba mais a velocidade.
- Um autoencoder (`joker.ae@1`) **por bomba**, treinado no local com 7 dias de funcionamento normal.
- Alarme quando o erro de reconstrução fica acima do limiar durante 5 min. Nada atua automaticamente: a proteção da bomba é do relé de proteção do motor.

### 3.2 O modelo

```toml
# (1.1): sinais em bloco, pipelines, modelos, treino e gravação por gatilho
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
"ch1" = { alias = "p1.acc.x", kind = "block", unit = "gn", block = 2560 }   # blocos de 100 ms
# … 24 canais

[pipelines.p1_features]
from  = ["p1.acc.x", "p1.acc.y", "p1.acc.z"]
steps = [
  { op = "window", n = 4096, hop = 25600 },    # 1 janela por segundo
  { op = "detrend" },
  { op = "fft", n = 4096, window = "hann" },
  { op = "bands", edges_hz = [10, 50, 100, 200, 500, 1000, 2000, 5000, 10000] },
  { op = "stats", fields = ["rms", "peak", "crest", "kurtosis"] },
]
to = "p1.features"                  # evento com 36 floats (+ velocidade no exemplo 03), 1/s

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

### 3.3 O que funcionou

- **Burn a treinar no dispositivo.** Um autoencoder pequeno (37 → 24 → 8 → 24 → 37, ~2 k parâmetros) com 600 k janelas treina em minutos num núcleo de CM5 (estimativa, poucas épocas), em `SCHED_IDLE`. É o argumento mais forte a favor do Burn: nenhum motor só de inferência faria isto sem Python no dispositivo.
- **Pipelines para sinais.** FFT, bandas e estatísticas correm nativamente com NEON. O custo estimado, 24 FFTs de 4096 pontos por segundo, é ~6 M operações de vírgula flutuante por segundo (5·N·log₂N), o que é desprezável.
- **Nada atua.** O Joker como sistema de monitorização é um subconjunto natural do desenho.

### 3.4 O que quebrou

**(a) 614 k amostras/s não são sinais escalares.** Passar cada amostra pelo slot, pela imagem de processo e pelo historian é inviável: são ~0,6 M escritas de slot por segundo só para estes sinais (~2 M operações contando latch e historian), cada uma com 32 bytes de metadados para 4 bytes de dados, e sem noção do relógio de amostragem, que é o que a FFT precisa. **Lacuna a fechar → M7** (sinais em bloco: `t0`, período, n amostras contíguas, qualidade por bloco, anel por sinal).

**(b) Amostragem com relógio do Linux.** A primeira ideia foi ler o ADC por SPI a cada 39 µs a partir de uma thread RT. Mesmo com PREEMPT_RT, o jitter de dezenas de µs destrói a FFT. **Fronteira a declarar → 12, §3**: acima de ~1 kHz, a amostragem **tem de** ser feita por hardware (ADC com relógio próprio e FIFO/DMA), e o Joker lê blocos. O Joker não é um sistema de aquisição de sinais com timing garantido por software.

**(c) 2,5 MB/s em disco são 212 GB/dia.** Guardar tudo é impossível num eMMC de 32 GB, mas a forma de onda *à volta de uma anomalia* é o dado mais valioso para o engenheiro de fiabilidade. A política de gravação ganhou um modo `on_trigger` com `pre` e `post`: um anel em RAM de 10 s por canal (24 canais × 25 600 × 4 B × 10 s ≈ 25 MB) que é despejado quando o gatilho dispara, com o `post` escrito diretamente para disco. Um `pre` de 60 s custaria ~150 MB de RAM, e por isso o `jk check` soma estes anéis no `J0380`. Isto passou a fazer parte do **M7**.

**(d) O treino disputa CPU com o controlo.** Num CM5 com 4 núcleos, o treino semanal de 8 autoencoders em simultâneo satura os núcleos e sobe a temperatura, e o *throttling* térmico aumenta o jitter de todo o sistema, incluindo o núcleo `rt`. **Lacuna a fechar → M8** (classes de recursos com cpusets e limites de memória; treino em `SCHED_IDLE`, preemptível, com pausa sob pressão térmica; um treino de cada vez por omissão).

**(e) "Que modelo disparou este alarme, e treinado com quê?"** Seis meses depois, o alarme da bomba 3 dispara e o engenheiro pergunta se o modelo foi treinado com o rolamento já degradado. **Lacuna a fechar → M10** (linhagem: intervalo do historian, hash dos segmentos, filtro `where`, versão do Burn, métricas de validação, `.jkm` pai).

**(f) Alarmes que piscam.** Com um limiar fixo, o erro de reconstrução oscila à volta do limite e o alarme liga e desliga 40 vezes por dia. A equipa de operação desliga-o ao terceiro dia. **Lacuna a fechar → M9** (alarmes de primeira classe com `delay_on`/`delay_off`, histerese, shelving com TTL e supressão por desenho, `p1.running`). O limiar é aprendido na validação (p99,9 do erro no holdout), guardado no `.jkm` e lido por `model_threshold()`.

**(g) Uma bomba que muda de regime.** Uma bomba com variador trabalha a 30 Hz de dia e a 45 Hz à noite, e o autoencoder treinado sobretudo de dia marca a noite como anomalia. Isto é um problema de modelação (incluir a velocidade nas features) e não de runtime. Fica como nota de modelação no exemplo 03.

### 3.5 Veredicto

O modelo de sinais precisava de uma forma de onda (M7) e o sistema precisava de governar recursos (M8). A combinação **pipelines nativos + Burn a treinar no local + alarmes a sério** é um produto em si.

---

## Rodada 4 — AGV com EtherCAT, ROS 2 e laser de segurança (PC industrial x86)

### 4.1 O cenário

Um AGV diferencial com 2 servodrives CiA 402 em EtherCAT a **1 kHz** (os drives fecham internamente as malhas de corrente e velocidade a 16 kHz; o Joker envia setpoints de velocidade em modo cíclico síncrono). A odometria e o controlo de trajetória local correm a **100 Hz**. O Nav2 (ROS 2) corre no mesmo PC e envia `cmd_vel` a 10–20 Hz. Há um LiDAR 2D (UDP, 10 Hz, ~1 440 pontos por volta), uma bateria em CAN e um **laser scanner de segurança + PLC de segurança** com STO cablado aos drives.

### 4.2 O modelo

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
[devices.safety] # só leitura
driver = "ethercat"
bus = "ecat"
position = 2
profile = "acme/safety-plc-fsoe-status@1"
[devices.lidar]
driver = "x:sick-tim"
address = "192.168.0.1"
[drivers.sick-tim]                      # tier X: processo à parte (ref/06 §5)
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

[atomic.wheels] # M20: mesmo frame EtherCAT
outputs = ["drive_l.v_set", "drive_r.v_set"]
on_rate = "proportional"

[outputs."drive_*.v_set"]
range = ["-2.0 m/s", "2.0 m/s"]
rate  = "1.5 m/s2"
safe  = "0 m/s"
safe_ramp = "2.5 m/s2"                  # travagem controlada; a de emergência é o STO do PLC de segurança
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

### 4.3 O que funcionou

- **Idade e lease como homem-morto.** O `cmd_vel` do ROS chega a `bridge.ros.cmd_v/w` com `max_age = 200ms`. Se o Nav2 morrer ou encravar, em ≤ 200 ms os valores ficam `Stale`, o `get()?` da app falha, a app deixa de escrever, o lease de 5 ms das rodas expira e os setpoints vão para 0 com a rampa de travagem. Isto não depende do código do Nav2 estar certo.
- **Multi-ritmo.** O grupo de 1 ms só faz a cinemática inversa e o envio, e o de 10 ms faz a odometria. A semântica entre grupos (documento 02, §6.3) chega: a odometria vê as posições aplicadas até ao seu latch.
- **O permissivo da segurança.** `safety.run_permit` vem do PLC de segurança. Sem ele, o Joker nem tenta mexer. O envelope junta-se à segurança funcional sem fingir que a substitui.

### 4.4 O que quebrou

**(a) Rodas que recebem setpoints de ticks diferentes.** Se o árbitro limitar a roda esquerda (rampa) e aceitar a direita tal como vem, o AGV curva sem ninguém ter pedido. As duas escritas têm de passar ou falhar juntas e de sair no mesmo frame. Isto já tinha aparecido no documento 09, ataque 1, e a rodada confirma-o. **→ M20** (grupos atómicos). Um detalhe novo: a **limitação de taxa num grupo atómico tem de ser proporcional**. Se uma roda for limitada a 60% do passo pedido, a outra também é, para preservar a curvatura. Passa a ser a regra `on_rate = "proportional"` dos grupos atómicos.

**(b) Integração com ROS 2.** Sem uma ponte, cada projeto escreveria um nó ROS que lê a API do Joker, com latência, tipos e perda de lease. **Lacuna a fechar → M11** (pontes declarativas: ROS 2 via Zenoh/`rmw_zenoh`, servidor OPC UA, servidor Modbus, com a regra de que **uma escrita que chega por uma ponte é uma escrita de um escritor declarado**, com nível, lease e envelope).

**(c) 1 kHz com jitter apertado.** O EtherCAT com DC tolera algum jitter no mestre, mas um p99,9 de 100 µs a 1 kHz é a fronteira do que o Linux com PREEMPT_RT garante num x86 com núcleo isolado (a medir no alvo). Com 4 kHz ou malhas de posição no mestre, deixa de ser razoável. **Fronteira → 12, §2**: o Joker faz controlo cíclico até ~1 kHz em hardware qualificado; acima disso, as malhas vivem nos drives.

**(d) Segurança funcional.** Há a tentação de pôr a monitorização de velocidade segura, as zonas do laser ou a paragem de emergência no Joker. **Fronteira → 12, §1**: o Joker não é, nem vai ser, um sistema certificado SIL/PL. O nível `safety` do árbitro serve para ações de proteção *de software* e nunca substitui a cadeia de segurança certificada.

**(e) Nuvens de pontos.** O LiDAR a 10 Hz é pequeno (~12 KB por volta), mas um LiDAR 3D são MB/s. Confirma o M4 (streams) para dados de robótica, com um formato `pointcloud` nos metadados do frame.

**(f) Wi-Fi intermitente.** O AGV perde a rede em partes do armazém. O store-and-forward já cobria a telemetria. Os comandos da frota (missões) não passam pelo Joker: vêm pelo ROS.

### 4.5 Veredicto

O Joker encaixa como a **camada entre o ROS e o hardware** (o que costuma ser um `ros2_control` com código à medida), com envelope, leases e EtherCAT nativo. As fronteiras (segurança funcional e taxas acima de 1 kHz) são claras e têm de ficar escritas em destaque.

---

## Rodada 5 — Frota heterogénea de 3 000 controladores de bombas de calor

### 5.1 O cenário

3 000 edifícios (moradias e pequenos comerciais), cada um com uma bomba de calor (Modbus RTU), dois relés SG-Ready (2 saídas digitais), um contador de calor M-Bus e, em ~30%, um inversor FV (Modbus TCP). A **redução por ordem do operador de rede para consumidores controláveis** é aplicada através dos relés ou de um limite de potência. A app é a mesma em todo o lado: otimização térmica com tarifa dinâmica e previsão de FV.

| Hardware | Quantidade | CPU | RAM | Armazenamento | Ligação |
|---|---|---|---|---|---|
| CM4 em caixa DIN | 1 800 | aarch64 4×A72 | 2 GB | eMMC 16 GB | Ethernet/LTE |
| PC industrial | 900 | x86_64 | 4 GB | SSD 64 GB | Ethernet |
| gateway antigo | 300 | **armv7** 2×A7 | **512 MB** | **eMMC 4 GB** | **LTE, 50 MB/mês** |

Atualizações: apps todos os meses, runtime a cada trimestre, e configuração quando o cliente muda de tarifa.

### 5.2 O modelo

```toml
# templates/joker.hp-v3.toml — partilhado por todos
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

# templates/joker.cm4-din.toml — overlay da placa CM4 em caixa DIN
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

### 5.3 O que funcionou

- **A mesma app em três arquiteturas.** O `hp-optimizer.wasm` é o mesmo ficheiro nos 3 000 dispositivos. O `jk build --target` produz o `.cwasm` para aarch64 e x86_64, e o armv7 usa o Pulley (§5.4a).
- **Aliases por placa.** A app liga-se a `sgready.a`, e o overlay de cada placa diz que GPIO ou relé USB é esse. O código não conhece placas.
- **Perfil por modelo de bomba.** 14 modelos de bomba de calor no parque dão 14 perfis e nenhuma app nova.

### 5.4 O que quebrou

**(a) armv7 sem Cranelift.** O Cranelift não gera código para ARM de 32 bits, e o wasmtime corre lá com o interpretador Pulley, estimado em uma ordem de grandeza mais lento do que o AOT (a medir). Para uma app com ciclo de 10 s e ~200 µs de cálculo, isto são ~2–4 ms, o que é aceitável. O desenho não muda, mas o `jk check` passa a **usar o WCET medido no alvo real** (`jk bench`), e não o do portátil (M23). Os modelos ML nativos (Burn Flex compilado para armv7) não são afetados, mas os modelos como componente Wasm (documento 06, §3.4) sim.

**(b) 50 MB por mês.** Com 40 sinais a 1 min em Sparkplug B (~60 bytes por métrica com overhead), são ~100 MB/mês, o dobro do orçamento, e o alarme de uma falha na bomba não pode esperar pela próxima janela. **Lacuna a fechar → M12** (orçamentos de uplink com classes de prioridade, projeção mensal e degradação por agregação antes de descartar; alarmes nunca degradados).

**(c) 3 000 manifestos.** Copiar e colar manifestos garante deriva ao fim de seis meses. **Lacuna a fechar → M13** (overlays com `extends`, regras de fusão explícitas, envelopes que só podem apertar sem `widen` justificado, `jk diff`/`jk explain` com a origem de cada chave).

**(d) Arrancar em 1970.** Os gateways armv7 não têm pilha de RTC. Depois de um corte de energia arrancam com a data de 1970 até o NTP responder, o que pode demorar minutos com LTE fraco. Os timestamps saem errados, a tarifa horária é aplicada à hora errada e as janelas das expressões ficam confusas. **Lacuna a fechar → M14** (qualidade do relógio por amostra e por tick, correspondência monotónica↔UTC corrigida retroativamente no historian, `requires_clock = "synced"` por app: sem relógio, as saídas da app vão para `safe`, e com as bombas de calor em SG-Ready "normal" o edifício continua aquecido).

**(e) Desgaste do eMMC de 4 GB.** Com ~2 KB/s de escrita do historian mais o WAL, são ~63 GB/ano. Um eMMC de 4 GB com ~3 000 ciclos P/E aguenta ~12 TB, portanto ~190 anos no papel e muito menos com amplificação de escrita do controlador (a medir). É aceitável, mas o `fsync = "batch"` e os blocos de 4 KiB são obrigatórios, e o `jk doctor` passa a mostrar o desgaste estimado (`/sys/block/mmcblk0/device/life_time`).

**(f) Rollouts, coortes e estado desejado de 3 000 dispositivos.** Quem decide que 5% recebem primeiro a 3.2.0, e que se a taxa de alarmes subir se para? **Fronteira → 12, §7**: o Joker não é o servidor de frota. Fornece o **agente**: documento de estado desejado assinado (runtime, apps, modelos, config), estado reportado, saúde, aplicação com confirmação e rollback local. O servidor (coortes, ondas, painéis) é outro produto: pode ser construído por cima, ou usar-se Mender, balena ou similar para a camada de SO. O protocolo do agente fica especificado para isso ser possível.

**(g) Três modelos de consumo de memória.** 512 MB com o SO, o `jokerd`, 3 apps Rust e o historian: ~120 MB no total, com folga. Uma app TypeScript (motor JS embutido, ~40 MB) já aperta. **Nota de desenho:** o `jk check` estima a memória total do site e falha com `J0380` acima de 70% da RAM da placa.

### 5.5 Veredicto

O "corre em qualquer lado" aguentou, com o custo conhecido do Pulley em armv7. As lacunas (M12–M14) são todas de **operação em escala**, e é nelas que os projetos de frota morrem. A fronteira do servidor de frota tem de ficar explícita para não virar *scope creep*.

---

## Rodada 6 — ML a atuar ao lado de humanos: AVAC com política aprendida e assistente LLM

### 6.1 O cenário

Um edifício de escritórios com 12 000 m², 4 UTAs e 120 caixas VAV em BACnet/IP, e um sistema de gestão técnica (SGTC) existente que os operadores usam. Uma política (MPC com modelo aprendido, treinada offline num gémeo digital) propõe a cada 5 min a temperatura de insuflação das UTAs e os caudais mínimos das VAV, com o objetivo de −15% de energia sem sair da banda de conforto. Um LLM local (Jetson Orin) responde a perguntas dos operadores ("porque é que o 3.º piso está quente?") e sugere ações.

### 6.2 O modelo

```toml
# (1.1): modelos, fallback por KPI e escritores só-proposta
[devices.bms] # prioridade BACnet usada pelo Joker
driver = "x:bacnet-ip"
network = "10.20.0.0/16"
priority = 12

[drivers.bacnet-ip]                   # tier X sobre uma biblioteca C (6.4 g)
tier = "x"
exec = "/usr/lib/joker/drivers/joker-drv-bacnet-ip"

[outputs."ahu*.supply_sp"]
range = ["13 °C", "20 °C"]
rate  = "0.5 °C/min"
safe  = "relinquish"                 # entregar ao SGTC: liberta a prioridade BACnet do Joker
interlock = "avg(zone.worst_temp_dev, 15m) < 1.5 K"   # 15 min é a janela máxima (J0305)
max_lease = "15m"

[apps.hvac_policy]
source = "oci://…/hvac-mpc:2.0.0"
writer = { level = "optimizer", priority = 14, lease = "12m" }
fallback = { controller = "app:hvac_rules", when = "ood_ratio(model:hvac_net, 30m) > 0.2 || !good(model:hvac_net)" }   # (1.1); `when` é um KPI, avaliado em bg (ref/01 §6.5)

[apps.hvac_policy.shadow]
of = "none"                          # primeira versão: sombra contra o que o SGTC faz hoje
until = { duration = "14d" }
promote = { outputs = "ahu3.**" }    # promoção parcial: só a UTA 3

[apps.assistant]
path   = "apps/ops-assistant"
writer = { kind = "proposal" }       # M18 (1.1)
bind   = { llm = "model:ops_llm" }   # (1.1)
```

### 6.3 O que funcionou

- **O envelope limita a política.** O modelo pode estar errado, mas não pode pedir 8 °C de insuflação, nem mudar 5 °C num minuto, nem continuar a otimizar com uma zona fora de conforto há 15 minutos.
- **O relinquish como estado seguro.** Aqui o "seguro" não é um valor: é **devolver o controlo ao SGTC**. `safe = "relinquish"` com um driver BACnet liberta da prioridade do Joker no *priority array* do equipamento, e o equipamento volta ao que o SGTC diz. O conceito de estado seguro do Joker generaliza sem esforço.

### 6.4 O que quebrou

**(a) "O operador mexeu e o modelo desfez."** Um operador sobe a insuflação da UTA 2 no SGTC por uma queixa de frio, e 5 minutos depois o modelo volta a baixá-la. Isto é o que mata a confiança num sistema de otimização. São precisas duas camadas: no **equipamento**, o Joker escreve numa prioridade BACnet (12) *inferior* à dos operadores do SGTC (8), e por isso o override manual no SGTC ganha no próprio equipamento; no **Joker**, as escritas manuais pela CLI/HMI e as do modelo precisam de prioridades com leases, com o manual a expirar sozinho. **Lacuna a fechar → M15** (árbitro com 16 níveis e leases, TTL obrigatório nos níveis manuais; o nível BACnet do Joker é declarado por dispositivo e verificado).

**(b) Pôr um modelo em produção sem fé.** "Ativamos e vemos" não é aceitável num edifício ocupado. **Lacuna a fechar → M16** (modo sombra: a política corre com as entradas reais e as suas escritas são gravadas como `Shadowed`, comparadas com o que o SGTC fez; promoção por critérios, **parcial por saídas** (só a UTA 3 primeiro) e com despromoção automática por KPI).

**(c) Um dia fora do treino.** Uma onda de frio a −12 °C, nunca vista no gémeo digital. A política dá valores plausíveis, mas errados. **Lacuna a fechar → M17** (guarda de entrada no `.jkm` com estatísticas de treino; `Uncertain(OutOfDistribution)`; controlador de fallback declarado que ocupa o nível do modelo enquanto a condição se mantém).

**(d) O LLM propõe, e alguém tem de aprovar.** O assistente sugere "subir o caudal mínimo das VAV do piso 3 para 40%". Deixá-lo escrever é inaceitável, e obrigar o operador a escrever o comando à mão perde o valor da sugestão. **Lacuna a fechar → M18** (escritores só-proposta: intenções com justificação e TTL, aprovadas por um humano com papel `operator`, executadas como escrita manual em nome de quem aprovou; tudo no registo de decisões).

**(e) Poupar energia à custa do conforto, devagar.** O envelope limita ações instantâneas, mas "a média de conforto piorou 0,4 K em duas semanas" é uma propriedade da política ao longo do tempo. As expressões de janela (`avg(…, 30m)`) cobrem parte, e os KPIs de despromoção do M16 cobrem o resto *depois* de o problema acontecer. **Fronteira → 12, §4**: o envelope limita o que um modelo pode fazer, não garante que o faça bem. A qualidade da política é responsabilidade de quem a treina e valida, e o Joker dá-lhe as ferramentas (sombra, KPIs, replay), não a garantia.

**(f) Explicar uma decisão três meses depois.** "Porque é que a UTA 1 insuflou a 19 °C no dia 12 às 14:05?" O M3 (rastreio causal) responde com o tick, as entradas, a versão da app e a versão do modelo. Com o M22 (saídas do modelo gravadas), o replay reproduz exatamente a decisão. Não é preciso nada novo, e a rodada confirma o valor de M3 + M22.

**(g) BACnet.** A pilha BACnet é complexa (descoberta, COV, segmentação), e o primeiro driver será tier X sobre uma biblioteca C madura. Não é uma lacuna de desenho, é trabalho de catálogo.

### 6.5 Veredicto

Esta é a rodada que justifica o Joker face a "um script Python com uma biblioteca de ML". O envelope, a arbitragem com leases, a sombra, a guarda de entrada e as propostas com aprovação transformam "um modelo a mexer no edifício" em algo que um responsável de manutenção aceita. A fronteira do §6.4(e) tem de ficar dita sem rodeios.

---

## Resumo

| # | Rodada | Eixo atacado | Lacunas fechadas | Fronteiras declaradas |
|---|---|---|---|---|
| 1 | BESS + FV (RevPi Connect 5) | robustez de campo, regulação | M1 fallback do dispositivo, M2 descoberta e lock, M3 rastreio causal | contagem fiscal |
| 2 | Inspeção visual (RPi 5 + Hailo) | dados volumosos, latência determinista | M4 streams, M5 pipelines nativos, M6 escritas agendadas | compilação para NPU, privacidade de imagem |
| 3 | Vibração + treino (CM5) | alta taxa, treino no local, alarmes | M7 sinais em bloco (e gravação por gatilho), M8 classes de recursos, M9 alarmes, M10 linhagem | amostragem por hardware acima de ~1 kHz |
| 4 | AGV (x86 + EtherCAT + ROS 2) | multi-ritmo, interop, segurança | M11 pontes (M20 confirmado, com taxa proporcional) | segurança funcional, controlo > 1 kHz |
| 5 | Frota heterogénea (3 000) | portabilidade, operação em escala | M12 orçamentos de uplink, M13 overlays, M14 qualidade do relógio | servidor de frota |
| 6 | AVAC + LLM | ML a atuar, humanos no circuito | M15 arbitragem, M16 sombra, M17 guarda + fallback, M18 propostas | qualidade das decisões de modelos |

**Nenhuma rodada partiu o modelo central** (sinal + imagem de processo + envelope). Todas as lacunas se fecharam com primitivas **acrescentadas à volta** do núcleo, e nenhuma obrigou a alterar a semântica de leitura ou de escrita já definida. É o sinal de que a abstração está no sítio certo, e é também o risco: 18 primitivas é muito, e o roadmap (documento 11) põe-nas por ordem.
