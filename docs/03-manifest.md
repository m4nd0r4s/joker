# 03 — O manifesto (`joker.toml`) e o `joker.lock`

Um site é descrito por um manifesto. O manifesto diz **o que existe** (hardware, barramentos, dispositivos, sinais), **o que é permitido** (envelopes, árbitros, papéis) e **o que corre** (ciclos, apps, modelos, pipelines, telemetria). As apps dizem *o que precisam*, e o manifesto diz *onde está*.

> **Este documento é a explicação.** A referência exaustiva do manifesto (todas as secções e chaves, tipos, omissões, validação, códigos, overlays, manifesto efetivo) é a **`ref/01`**; a do `joker.lock`, do `node.toml` e dos segredos é a **`ref/02`**; a dos perfis, placas e simulação é a **`ref/03`**. Onde divergirem, vale a referência (ADR 0023). As chaves marcadas **1.1** já existem no esquema, mas uma runtime 1.0 recusa-as com `J0290` (D5, ADR 0019).

---

## 1. Ficheiros de um projeto

```
site-lisboa-01/
  joker.toml                          manifesto do site (normativo)
  joker.lock                          resolução fixada — gerado, versionado (§14)
  joker.sim.toml                      overlay de simulação (`jk dev --env sim`)
  joker.sim.lock                      o seu lock
  profiles/acme/pcs-125@3.profile.toml       perfis locais de equipamentos (além do registo)
  boards/revpi-connect-5.board.toml          perfis de placa locais (opcional)
  classes/local/station.jkc           classes locais, com station.conformance.toml e station.sim.toml
  apps/                               código-fonte de apps locais (opcional)
  models/                             artefactos .jkm locais (opcional, 1.1)
  sim/pcs.sim.toml                    modelos de simulação para `jk dev` e testes
  sim/scenarios/pico.scenario.toml    cenários do `jk test`
  keys/                               chaves públicas (.pub) referidas em [security.trust]; nunca chaves privadas
  secrets.toml.enc                    segredos cifrados por destinatário (HPKE); pode ir para o Git (ADR 0024)
  .taplo.toml                         associa cada sufixo ao seu esquema, para o editor (escrito pelo `jk init`)
```

- **O tipo está no nome** (ADR 0026): `<nome>.<tipo>.toml` (`.profile.toml`, `.board.toml`, `.sim.toml`, `.scenario.toml`, `.conformance.toml`, `.package.toml`), e as camadas do manifesto chamam-se `joker.toml` ou `joker.<variante>.toml`, em qualquer pasta. O conteúdo tem de bater com o sufixo (`J0803`). A tabela completa está no `ref/00` §2.
- **Um lock por manifesto de entrada** (D8): `joker.toml` dá `joker.lock`, e cada overlay de entrada `joker.<variante>.toml` dá `joker.<variante>.lock` (`joker.rpi.toml` → `joker.rpi.lock`). `jk <comando> --env rpi` escolhe o overlay e o lock juntos (`ref/01` §7.7).
- **`secrets.toml.enc`** tem os nomes e os destinatários em claro e os valores cifrados para a chave de cada nó. Pode ser versionado, e é assim que o Deck distribui os segredos sem os conseguir ler (D6, `ref/02` §2.4).

TOML porque é conhecido, tem bons parsers e bons editores, e é revisível em PR. Quantidades com unidade vão em strings (`"125 kW"`). O esquema JSON do manifesto é **gerado** a partir dos tipos do `joker-manifest` (16 §10) e publicado, o que dá completação em qualquer editor com taplo ou LSP; o `.taplo.toml` do projeto associa o esquema de cada tipo ao seu sufixo. Uma chave desconhecida é erro (`J0140`); as chaves `x_*` são extensões e ignoram-se (`ref/00` §1).

---

## 2. Sobreposições (M13)

```toml
extends = ["../templates/joker.bess-v2.toml", "../templates/joker.org-telemetry.toml"]
```

`extends` é a primeira chave do ficheiro. Os caminhos são relativos ao ficheiro que os declara, e só há ficheiros locais (`ref/01` §7.1). Regras de fusão, aplicadas pela ordem da lista e depois pelo ficheiro atual (a linearização de uma cadeia com diamantes está na `ref/01` §7.1):

| Tipo | Regra |
|---|---|
| tabela | fusão recursiva |
| escalar | o posterior substitui |
| array | substitui por omissão; `key = { append = [...] }` acrescenta |
| remover | `key = { unset = true }` |
| envelopes | **só podem apertar**: um overlay pode reduzir a gama ou a taxa, mas não alargá-las, a não ser com `widen = "<justificação>"`, que fica no lock com o valor alargado. A lista exata das chaves que só apertam está na `ref/01` §7.4 |

`jk diff` mostra o manifesto efetivo e a origem de cada chave (`jk explain pcs.p_set.range`).

---

## 3. Projeto, alvo, recursos, relógio

```toml
[project]
name    = "site-lisboa-01"
joker   = "1.0"                 # versão mínima da runtime
mode    = "prod"                # dev | prod (omissão: prod; prod: assinaturas obrigatórias, avisos de fallback = erros)

[target]
board   = "revpi-connect-5"     # perfil de placa ("nome", ou "nome@major"): GPIO/IO conhecidos, watchdog, núcleos, aceleradores
arch    = "aarch64"             # inferido da placa se omitido
kernel  = "preempt_rt"          # preempt_rt | standard (omissão) ; o jk doctor verifica

[resources]
rt  = { cpus = [3] }
io  = { cpus = [2] }
ml  = { cpus = [1, 2], memory = "1 GiB" }     # accel = ["npu:hailo0"] é 1.1
bg  = { cpus = [0], memory = "512 MiB" }

[clock]
sync      = "ntp"               # ntp | ptp | gnss | none ; num Joker Micro: link | extern
servers   = ["pool.ntp.org"]
max_holdover = "24h"            # após isto em holdover: ClockUnsynced
```

- A omissão de `mode` é `prod`, para que esquecer a chave num site real não desligue as assinaturas; o `jk init` escreve `mode = "dev"`. Em `prod`, `[project] joker` e `[target] board` são obrigatórios.
- Num **Joker Micro**, `[clock] sync = "link"` (a hora vem do `TIME` do Joker Link) ou `"extern"` (o integrador dá-a com `jk_time_set_utc`, `ref/08`). As secções próprias de um MCU (`[link]`, `[target.capacity]`, `[target.micro]`, `[target.extern]`) estão na `ref/01` §5.22 e no documento 19.

---

## 4. Barramentos

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
turnaround = "5ms"              # silêncio mínimo entre pedidos (RTU)

[buses.can0]
kind    = "can"
iface   = "can0"
bitrate = 500000

[buses.ecat]
kind  = "ethercat"
iface = "eth2"
cycle = "1ms"
```

Um barramento `serial`, `i2c`, `spi` ou `ethernet` serializa o acesso: dois dispositivos Modbus RTU no mesmo RS-485 partilham um único escalonador de pedidos (documento 05, §5). Um barramento `can` não serializa (as tramas são arbitradas pelo próprio barramento), e um dispositivo TCP sem `bus` usa o encaminhamento do sistema, sem escalonador comum; com `bus` de `kind = "ethernet"`, partilha-o (útil quando um PLC só aceita uma ligação). Protocolos diferentes no mesmo barramento série são `J0171`. As chaves de cada `kind` estão na `ref/01` §5.6.

---

## 5. Dispositivos

```toml
[devices.meter]
driver  = "modbus-tcp"
bus     = "plant"
address = "192.168.10.20:502"
unit_id = 1
profile = "janitza/umg604@2"     # registo de perfis; versão major fixada
poll    = "200ms"
timeout = "300ms"
retries = 1

[devices.pcs]
driver   = "modbus-tcp"
address  = "192.168.10.30:502"
profile  = "sunspec"             # forma curta de "joker/sunspec@1"
discover = true                  # descobre modelos SunSpec; o resultado vai para o lock (M2)
poll     = "500ms"
fallback = { configure = true, keepalive = "5s" }  # M1: configurar e verificar o fallback do perfil; renovar a escrita a cada 5 s

[devices.bms]
driver  = "canopen"
bus     = "can0"
node_id = 5
eds     = "profiles/bms-x.eds"

[devices.io]
driver = "revpi"                 # piControl: módulos DIO/AIO detetados pela config do PiCtory
config = "/var/www/revpi/pictory/projects/_config.rsc"

[devices.cam0]                    # 1.1: o driver v4l2 e os streams (M4) são da 1.1
driver = "v4l2"
device = "/dev/video0"
format = { width = 1920, height = 1080, pixfmt = "NV12", fps = 30 }
trigger = { kind = "hardware", input = "io.di_3" }

[devices.sim_tank]
driver = "sim"
model  = "sim/tank.sim.toml"         # planta simulada (documento 04, §6)
```

Fora de `driver`, `bus`, `profile`, `poll`, `timeout`, `retries`, `align`, `discover`, `fallback`, `signals` e `actions`, as chaves são **do driver** (`address`, `unit_id`, `node_id`, `eds`, `config`…): o catálogo está no anexo A da `ref/01`, e os drivers tier W e X publicam o seu esquema (`ref/06`). A `config` de um dispositivo é validada pelo `[config]` do perfil (`ref/03` §2.2.10).

### 5.1 Sinais do dispositivo

O perfil define os sinais. O manifesto só **acrescenta ou ajusta**:

```toml
[devices.meter.signals]
p_total = { max_age = "1s", record = { every = "1s" } }
f       = { limits = ["45 Hz", "55 Hz"], stuck_detect = "1m" }   # limites físicos → Bad(OutOfRange); congelado → Uncertain(Frozen)

[devices.io.signals]
di_3 = { alias = "line.photoeye" }                  # alias: o sinal passa a ter dois caminhos
do_1 = { alias = "line.ejector" }

[devices.adc.signals]
a0 = { alias = "tank.level", scale = { from = ["0.6 V", "3.0 V"], to = ["0 m", "3 m"] } }   # escala de dois pontos
```

### 5.2 Ações do dispositivo

```toml
[devices.pcs.actions.reset_fault]
permit  = "pcs.state == PcsState.fault"
role    = "operator"
timeout = "5s"
```

A ação tem de existir no perfil. A **implementação** (os passos, os registos) é do perfil, e não do manifesto (`ref/03` §2.2.12); o manifesto só a configura.

---

## 6. Saídas: envelope, árbitro, fallback

Todas as saídas (sinais com direção `output`) precisam de uma entrada aqui. **Faltar o `safe` é `J0201`.**

```toml
[outputs."pcs.p_set"]
range      = ["-125 kW", "125 kW"]     # também aceita expressões: ["-bms.p_max_discharge", "bms.p_max_charge"] (documento 16, E6)
on_range   = "clamp"                # clamp | reject | trip
rate       = "25 kW/s"
deadband   = "0.5 kW"
min_interval = "200ms"
max_actuations = "120/h"          # orçamento de atuações (relés, contactores, arranques)
safe       = "0 kW"                 # valor, ou "relinquish" (devolve o controlo à lógica do próprio equipamento, §6.1)
safer      = "equal"                # lower | higher | equal — para comparar com o fallback do dispositivo
safe_ramp  = "50 kW/s"              # opcional: ir para seguro também em rampa
interlock  = "meter.f > 49.8 Hz && meter.f < 50.2 Hz && bms.soc >= 5 % && !alarm.bms_fault.active"
on_interlock = "safe"               # safe (omissão) | hold
requires_good = ["meter.p_total", "bms.soc"]
grace      = "2s"
max_lease  = "15m"
max_manual = "8h"
verify     = { readback = "pcs.p_set_rb", tolerance = "1 kW", within = "2s" }
reset_when = "bms.state != BmsState.fault"   # sai do trip sozinha quando a condição se verifica (documento 16, E7)
group      = "fast"                 # grupo de ciclo dono; por omissão, a regra A11 (documento 16)

[outputs."line.ejector"]
safe       = false
pulse      = "50ms"                 # saída de impulso: volta a `safe` após 50 ms
schedule_depth = 32                 # 1.1 (M6): escritas agendadas; omissão 0 = desativadas
timed      = true                   # 1.1 (M6)
on_late    = "apply_now"            # 1.1 (M6): obrigatória com schedule_depth > 0
```

- `schedule_depth`, `timed`, `on_late` e `late_margin` são **1.1** (escritas agendadas, documento 02, §7.2).
- As saídas de um dispositivo `joker-link` são governadas pelo envelope do Joker Micro: declará-las aqui é `J0234` (documento 19, §6).

### 6.1 `safe = "relinquish"`

Há saídas em que o lado seguro não é um valor, mas **devolver o controlo** a quem o tinha antes: a lógica local do equipamento, o SGTC de um edifício ou o PLC existente. `safe = "relinquish"` faz isso. Nos drivers com *priority array* (BACnet), liberta a prioridade do Joker; nos equipamentos com modo "controlo remoto ativo", escreve o modo local (declarado no perfil em `relinquish = { … }`). Só é aceite em drivers que o suportam (`J0207`), e o `safer` deixa de se aplicar.

---

## 7. Grupos de ciclo

```toml
[cycles.fast]
period    = "200ms"
resources = "rt"
overrun   = "skip"                  # skip | catch_up | degrade | trip
trip_after = 10
trace     = { continuous = "6h" }   # anel de traço exato em disco (jk trace-dump); custo ≈ N_in × 16 B por tick (documento 16, §7)
order     = ["peak_shaver"]         # ordem das tarefas no tick (as não listadas correm depois, por ordem lexicográfica de <app>.<tarefa>)

[cycles.slow]
period    = "5s"
resources = "rt"
```

A prioridade `SCHED_FIFO` é atribuída automaticamente (rate-monotonic). O `jk check` aplica a admissão de tempo real (M23). A ordem lexicográfica das tarefas não listadas, e não "a ordem do manifesto", torna o resultado independente da ordem das camadas de overlays.

Nomes reservados de grupos implícitos (`ref/01` §5.11): `default` (100 ms, o grupo dono das saídas escritas só por async, pontes, CLI ou implementações async, 16 A11), `default_<período>` (tarefas de apps sem `cycles`) e `calc`.

---

## 8. Apps

```toml
[apps.peak_shaver]
source  = "oci://registry.example.com/joker/peak-shaver:1.4.2"   # ou path = "apps/peak-shaver"
digest  = "sha256:9f2c…"            # fixado no lock se omitido
writer  = { level = "optimizer", priority = 13, lease = "2s" }
resources = { cycle = "rt", async = "bg", memory = "32 MiB" }
requires_clock = "synced"           # any (omissão) | holdover | synced

[apps.peak_shaver.bind]
grid_power = "meter.p_total"
soc        = { signal = "bms.soc", as = "fraction" }
battery    = "pcs.p_set"
# forecast = "model:load_forecast"  # porta de modelo: 1.1 (a porta declara-se `optional` para correr numa 1.0)

[apps.peak_shaver.secrets]          # nome local na app = segredo do site
tariff_token = "secret:tariff_api_token"

[apps.peak_shaver.params]
limit    = "400 kW"
reserve  = "10 %"

[apps.peak_shaver.cycles]
control = "fast"                    # tarefa de ciclo `control` → grupo `fast`

[apps.peak_shaver.net]
allow = ["api.tariffs.example.com:443"]

[apps.peak_shaver.shadow]          # M16: esta versão corre em sombra contra outra
of        = "peak_shaver@1.3.9"
until     = { duration = "72h", max_divergence = "5 kW p95" }
```

- Uma porta de app não ligada é `J0110`, a não ser que seja `optional` no código.
- Um parâmetro fora da gama declarada pela app é `J0120`.
- `writer.level` + `priority` definem o nível no árbitro; dois escritores no mesmo nível da mesma saída é `J0210`. Se uma saída tem **um só escritor**, o `writer` pode ser omitido: fica `control:12` com lease de 3 períodos do grupo. Com dois ou mais escritores, é obrigatório (`J0211`).
- Sem `cycles`, as tarefas de ciclo da app correm num grupo implícito `default_<período>` com o período pedido pela app (e sem RT garantido). O grupo `default` (100 ms) é outro: é o da regra A11 do documento 16.
- `writer.kind = "proposal"` (M18, **1.1**): a app só pode `propose`, e as escritas diretas são `J0212`.
- **Segredos** (D6): `[apps.<app>.secrets]` é uma **tabela** que liga cada nome local que a app declara no código a um segredo do site (`"secret:<nome>"`). Só a instância async os pode pedir. Um segredo obrigatório sem ligação, ou uma ligação a um nome que a app não declara, é erro (`ref/02` §2.4.7). As outras chaves que aceitam `secret:` (credenciais de uplinks, pontes e dispositivos) estão marcadas na `ref/01`.
- **Memória:** sem `resources.memory`, a omissão depende do tier da app: 16 MiB no ciclo e 64 MiB no async em Rust, 64/64 MiB em TypeScript, 128/128 MiB em Python (`ref/05` §19). O SDK põe a sugestão no `joker:meta`, e o manifesto vence.
- **`requires_clock`:** vale o mais estrito entre o que a app declara no código e o que o manifesto diz (`ref/04`).

---

## 9. Modelos, streams e pipelines (**1.1**)

Tudo o que está neste bloco é da 1.1 (ADR 0019): a 1.0 valida a forma e recusa-o com `J0290`.

```toml
[models.load_forecast]
source   = "models/load-tcn-v3.jkm"
backend  = ["burn:cuda", "burn:vulkan", "burn:cpu"]   # cadeia de fallback
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
to     = "vision.detections"        # evento: list<detection, 32> + ts do frame
```

O detalhe está no documento 06.

### 9.1 Outras chaves referidas noutros documentos

| Chave | Onde | Significado | Detalhe |
|---|---|---|---|
| `align = "<dispositivo>"` | `[devices.*]` | agenda o poll logo a seguir ao do dispositivo indicado (reduz o skew) | 08, rodada 1 |
| `fallback.keepalive` | `[devices.*]` | intervalo de renovação das escritas para o watchdog do equipamento | 01 §8 |
| `requires_clock` | `[apps.*]` | `"any"` \| `"holdover"` \| `"synced"`; sem a qualidade pedida, as saídas da app vão para `safe` | 01 §18 |
| `permit_uncertain = true` | `[outputs.*]` | um permissivo `Uncertain` conta como verdadeiro | 02 §4 |
| `stuck_detect` | sinais | `Uncertain(Frozen)` após o tempo indicado sem mudança | 02 §4 |
| `record = false` | `[streams.*]` | o stream não é gravado (omissão); **1.1** | 01 §11 |
| `trace = { continuous, async }` | `[cycles.*]` | anel de traço exato; `async = true` grava também as leituras async | 09, ataque 10 |
| `[signals."<caminho>"]` | raiz | ajustes a um sinal independentes do dispositivo que o fornece (`debounce`, `deadband`, `max_age`, `limits`, `stuck_detect`); úteis no manifesto comum de um par de overlays sim/hardware | trilha, partes 4–5 |
| `[training.*]` | raiz | trabalhos de treino no dispositivo; **1.1** | 06 §9 |
| `[apps.*.fallback]` | apps | controlador de fallback e condição (gramática KPI); **1.1** | 06 §7 |
| `[apps.*.shadow]` | apps | modo sombra e critérios de promoção de uma app | 06 §10 |
| `[models.*.shadow]` | modelos | o mesmo para modelos; **1.1** | 06 §10 |
| `max_shelve` | `[alarms.*]` | TTL máximo de um `jk alarm shelve` (omissão 24 h) | `ref/01` §5.15 |

### 9.2 Instâncias de classes (M24)

```toml
[instances.p1]
class = "std/pump@1"                       # "<pacote>/<nome>@<major>"; std/ pode omitir-se
map = { run_cmd = "io.do_1", running = "io.di_1", fault = "io.di_2", pressure_out = "ai.ch1" }
capabilities = ["pressure"]                # velocidade fixa: sem a capacidade variable_speed

[instances.p2]
class = "pump@1"
implemented_by = "app:pump_ctl"       # instância virtual (documento 14, §7)
# uma instância nasce de map, implemented_by ou sim (uma só); sim = true, ou uma tabela de parâmetros do simulador da classe
```

`implemented_by` aceita `"app:<nome>"` (uma instância da app, com o `bind` de `[apps.<nome>]`) ou `{ app = "<nome>", bind = { … } }` (uma instância da app por instância virtual). As ações de instâncias compostas e virtuais configuram-se em `[instances.<i>.actions.<a>]` (`permit`, `role`, `timeout`), e vencem as sugestões da classe (documento 17, §4.2).

Os dispositivos cujo perfil declara `[implements."<classe>"]` são instâncias implícitas com o nome do dispositivo. Nas ligações das apps, uma porta de classe aceita um nome de instância, uma lista, um seletor (`"vav_3*"`) ou `"class:<classe>"` (todas as instâncias). O detalhe está no documento 14. Num `map`, um membro de classe (`member boot: std/button@1`) liga-se ao nome de uma instância, e uma coleção a uma lista de nomes (ref/01 §5.13).

---

## 10. Derivados, alarmes, grupos atómicos

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
on_rate = "independent"             # independent (omissão) | proportional
```

Os `calc` correm no grupo de ciclo mais rápido que os consome, ou num grupo implícito `calc` de 100 ms.

---

## 11. Telemetria

```toml
[telemetry.record]                  # historian local
"meter.**"   = { every = "1s", keep = { raw = "7d", "1m" = "400d" } }
"pcs.p_set"  = { on_change = true, keep = { raw = "90d" } }
"acc*.x"     = { blocks = true, keep = { raw = "48h" } }   # 1.1: sinais em bloco (M7)
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
password = "secret:mqtt_password"   # credenciais só por referência a um segredo
```

A omissão de `classes` é `{ alarms = "alarm.**" }`: nada mais sai do nó sem ser pedido (documento 12, §10). O `**` é sempre um segmento inteiro.

---

## 12. Pontes

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
# uma ponte também pode escrever diretamente numa saída: to = ["pcs.p_set"], writer = { level = …, priority = …, lease = … }
```

- Em `subscribe`, `type` é o tipo da mensagem ROS 2 e `fields` diz que campo vai para que sinal de `to`, pela ordem (`ref/01` §5.18).
- Uma escrita por uma ponte é de um escritor declarado, com nível, lease e envelope. A ponte autentica-se pelo protocolo dela (sessão OPC UA com certificado; o Modbus não tem nenhuma), e não leva o nonce da API (`ref/00` §9).

---

## 13. Segurança e papéis

```toml
[security]
trust = { apps = ["keys/org-apps.pub"], config = ["keys/site-ops.pub"] }   # só restringe o bundle de confiança do nó
api   = { listen = ["unix:/run/joker/api.sock", "tcp:0.0.0.0:7443"] }         # TCP é sempre mTLS

[security.roles]
operator = ["cert:CN=ops-*", "local:group=joker-ops", "deck:acme/group:operadores"]
admin    = ["cert:CN=admin-*"]
```

- **`[security.trust]` só restringe** (D7, ADR 0025). As âncoras e a política (que chave assina que papel) vêm do **bundle de confiança** do nó, assinado por uma raiz fora de linha (`ref/02` §2.5). No manifesto, cada lista por papel (`runtime`, `config`, `apps`, `drivers`, `profiles`, `boards`, `classes`, `firmware`, `secrets`, `deck`; `models` na 1.1) tem de ser um subconjunto das chaves do bundle com esse papel (senão `J0292`, ref/01 §5.19), e `revoked` acrescenta revogações. Uma configuração não pode declarar a sua própria raiz de confiança: uma geração assinada por uma chave que ela própria declara não provaria nada.
- **`api.listen`** e **`[security.roles]`** acrescentam ao que o `node.toml` já dá (as escutas e os papéis mínimos do nó, sempre em vigor), e nunca retiram (`ref/02` §2.2). Mudar as escutas ou os papéis exige janela de confirmação em `prod` (a razão `api`, documento 01, §15).
- **Não há `mtls`:** o TCP é sempre mTLS. A chave antiga `mtls` é `J0140`.
- **Principais:** `cert:<atributo>=<padrão>` (`CN`, `OU`, `O`, ou `SAN`), `local:user=<nome>`, `local:group=<nome>`, e os do Deck, `deck:<deck_id>/user:<id>` e `deck:<deck_id>/group:<g>` (`ref/09` §2.2, `ref/10`). Os papéis são cumulativos (`admin` ⊇ `operator` ⊇ `viewer`); o de um principal é o mais alto das regras que casam com ele.

---

## 14. O `joker.lock` (M2)

Gerado por `jk lock` e versionado. Fixa tudo o que não é declarativo. Excerto da forma canónica (a completa, com todas as tabelas e campos, está na `ref/02` §2.1):

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

[discovered.pcs]                     # resultado da descoberta SunSpec (M2)
driver = "modbus-tcp"
profile = "joker/sunspec@1"             # o lock usa sempre a forma completa
at = "2026-09-20T10:14:03Z"
serial = "A1B2C3"
firmware = "PCS-FW 4.12.0"
digest = "sha256:…"                  # hash do mapa efetivo
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

[classes."std/pump@1"]               # M24: versão exata e hash do contrato de cada classe usada
resolved = "1.2.0"
contract = "sha256:…"

[wcet]                               # medido por `jk bench` no alvo (M23)
"peak_shaver.control" = { p999 = "38us", max = "51us", samples = 100000, board = "revpi-connect-5", runtime = "1.0.3", app = "sha256:9f2c…", measured = "2026-09-20T10:20:00Z" }

[accepted]                           # exceções justificadas, com quem as aprovou e o valor aprovado
accept_missing_fallback."painel.led" = { reason = "LED indicador; sem consequências físicas", by = "user:dl", at = "2026-09-18T09:02:11Z" }
widen."pcs.p_set.range" = { reason = "PCS novo com 150 kW nominal; aprovado por DL em 2026-09-18", value = ["-150 kW", "150 kW"], by = "user:dl", at = "2026-09-18T09:02:11Z" }
```

- O lock também fixa os drivers tier W e X (`[drivers.*]`, com o digest do executável da arquitetura do alvo), os MCUs (`[mcu.*]`) e, na 1.1, os modelos. O hash do manifesto efetivo está na `ref/01` §8.3; o **hash da geração**, que cobre manifesto, lock, contratos e artefactos, é o do descritor `joker.config.v1+cbor` (`ref/10` §2.6.2).
- No `[wcet]` ficam o p99,9 e o máximo. As apps de tier 2 entram na admissão pelo **máximo** (D15).
- Em `[accepted]`, o `value` existe para que uma aprovação para 150 kW não sirva para 200 kW.

**Deriva:** no arranque, o driver compara o estado real (firmware, modelos descobertos, fallback) com o lock. Se houver diferença, gera `J0401 deriva de dispositivo` e, em `prod`, **não liga as saídas desse dispositivo** até alguém correr `jk lock --update pcs`, rever e aplicar. Uma troca de firmware que mexa no mapa de registos não pode passar despercebida. Num MCU, a deriva compara também o hash da geração e o do catálogo anunciados no `HELLO` (`ref/02` §3.1.5, `ref/07`).

---

## 15. Códigos de validação (seleção)

Os `J…` são erros. Em `mode = "dev"`, os que só importam em produção (por exemplo, o fallback do dispositivo em falta) aparecem como avisos `W…` com o mesmo número (`W0205`). O registo completo, com gravidades, está no documento 16, §9; os códigos do manifesto, na `ref/01` §10, e os do lock, na `ref/02` §2.6.

| Código | Significado |
|---|---|
| `J0101` | sinal desconhecido (com sugestão por distância de edição) |
| `J0140` | chave desconhecida (com sugestão) |
| `J0107` | unidade incompatível na ligação |
| `J0108` | tipo de grandeza incompatível (W vs var) |
| `J0110` | porta obrigatória sem ligação |
| `J0120` | parâmetro fora da gama declarada pela app |
| `J0121` | parâmetro desconhecido (a app não o declara) |
| `J0201` | saída sem `safe` |
| `J0202` | `rate` com unidade incoerente com a saída |
| `J0205` | fallback do dispositivo incoerente com `safe` (M1) |
| `J0206` | saída com escritas agendadas sem `on_late` |
| `J0207` | `safe = "relinquish"` num driver sem suporte de relinquish |
| `J0210` | dois escritores no mesmo nível do árbitro |
| `J0211` | saída com vários escritores e um deles sem `writer` declarado |
| `J0212` | escritor só-proposta com escrita direta |
| `J0220` | overlay alarga o envelope sem `widen` |
| `J0290` | chave reservada para a 1.1 numa runtime 1.0 |
| `J0301` | expressão com dimensões incompatíveis |
| `J0305` | janela de expressão acima do máximo |
| `J0350` | grupo de ciclo não escalonável (M23) |
| `J0351` | grupo abaixo de 10 ms sem `kernel = "preempt_rt"` |
| `J0352` | (aviso) grupo ≥ 10 ms sem PREEMPT_RT: jitter não garantido |
| `J0370` | barramento sobrecarregado (documento 05, §5) |
| `J0380` | memória estimada do site acima de 70% da RAM da placa |
| `J0360` | classe de recursos sem CPUs ou com CPUs sobrepostas a `rt` |
| `J0401` | deriva de dispositivo face ao lock |
| `J0501` | modelo sem backend disponível na placa alvo |
| `J0601` | membro obrigatório de uma classe sem mapeamento na instância |
| `J0602` | capacidade exigida pela app (`requires`) ou declarada na instância sem os membros mapeados |
| `J0603` | mapeamento `const:` num membro `out` (documento 14, §5.2) |
| `J0604` | colisão de nomes entre uma instância e um dispositivo no primeiro segmento (documento 16, P3) |
| `J0610` | número de instâncias fora dos limites de `Many<C, MIN, MAX>` |
| `J0620` | mais de 3 camadas de instâncias virtuais em cadeia |
| `J0630` | versão major da classe da instância diferente da pedida pela app |
| `J0640` | `jk class verify` numa instância não simulada sem `--allow-actuation` (ref/03 §2.6.3) |
| `J0641` | `jk class verify` recusado: há um nível `safety` ou `manual` ativo nas saídas da instância (ref/03 §2.6.3) |
| `J0650` | membro de classe ligado a um nome que não é uma instância (ref/01 §5.13) |
| `J0651` | instância ligada a um membro de classe com outra classe ou outra major |
| `J0652` | instância ligada a um membro de classe sem as capacidades que o membro exige (`@requires`) |
| `J0653` | número de instâncias ligadas a um membro de classe fora de `[min..max]`, ou lista num membro fixo |
| `J0654` | ciclo entre instâncias compostas: uma instância é membro de si mesma |
| `J0655` | (aviso) instância com membros `out` que chega a apps diferentes por caminhos diferentes, um deles por uma composição (documento 16, P6) |
| `J0656` | instância virtual de uma classe cujos métodos chamam métodos dos membros (documento 17, §4.1) |
| `J0502` | `.jkm` com uma versão do Burn incompatível com a runtime |
| `J0510` | pipeline com formato incompatível entre passos |
