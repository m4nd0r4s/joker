# 05 — Drivers, perfis e placas

Os drivers são o fosso competitivo e também o maior custo. Este documento explica como se escrevem, como se descrevem os equipamentos (perfis), como se partilha um barramento e por onde começa o catálogo. O detalhe normativo está nas referências (ADR 0023): os três tiers na ref/06, os perfis, as placas e a simulação na ref/03.

---

## 1. Três tiers

| Tier | Onde corre | Linguagem | Para quê | Custo |
|---|---|---|---|---|
| **N** (nativo) | dentro do `jokerd`, threads `io`/`rt/drv` | Rust | protocolos de base: Modbus, OPC UA, GPIO, I²C, SPI, CAN, CANopen, EtherCAT, piControl, M-Bus, MQTT, V4L2 | um bug de memória derruba o processo (mitigado por Rust sem `unsafe` fora de `*-sys`) |
| **W** (Wasm) | dentro do `jokerd`, em sandbox | Rust, C, Go, … → Wasm | protocolos de terceiros e de fabricantes sobre transportes simples (série, TCP, UDP, I²C, SPI) | ~2–5× mais lento; sem acesso direto a ioctls |
| **X** (externo) | processo `joker-drv-<nome>` | qualquer | SDKs binários de fabricantes (câmaras GigE, NPUs, bibliotecas proprietárias), código não confiável | IPC por memória partilhada; +20–50 µs por lote |

Regra: **N** para o que é universal e crítico, **W** para o ecossistema e **X** para o que não se controla.

---

## 2. O contrato comum

Há **um só contrato** para os três tiers: as mesmas operações, os mesmos registos e os mesmos códigos; muda só o transporte da chamada (chamada Rust, chamada Wasm, JDP). A referência é a ref/06: §2 para o contrato, §3 para os traits Rust do crate `joker-hal`, que servem os três tiers (`Planner`, com `alloc`, para o plano; `Driver`, `no_std`, para o runtime).

| Operação | O que faz | Obrigatória |
|---|---|---|
| `plan` | valida a configuração e os endereços e agrupa os pontos em lotes; **pura** | sim |
| `open`, `connect`, `disconnect` | ciclo de vida da instância; `connect` devolve a identidade e, com `discover = true`, o mapa descoberto | sim |
| `read` | uma execução de um lote: valores crus e o estado de cada ponto | sim |
| `write` | escritas **aplicadas**, pela ordem dada | com a capacidade `write` |
| `read-addr`, `write-addr` | acesso por endereço, para os passos que o núcleo executa | com `raw-access` |
| `invoke` | ações `native` (um método OPC UA, um SDO) | com `native-actions` |
| `scan`, `health` | descoberta de rede (`jk discover`) e saúde | `health` sim; `scan` com `scan` |

- `plan` é **puro** (sem I/O, relógio nem aleatoriedade) e corre no `jk check`, no `jk build` e na preparação da geração: os erros de perfil (endereço inválido, sobreposição, grupo coerente que não cabe num pedido) aparecem antes do deploy. O mesmo pedido dá o mesmo plano, byte a byte.
- O driver entrega e recebe **valores crus** (registos, bytes, bools, inteiros já tipados). Escala, unidade, byte e word order, arredondamento na escrita e qualidade derivada são do estágio de aquisição e de saída do núcleo (documento 01, §3.3), a partir do perfil. Um driver novo não tem de reimplementar isso, e dois drivers não o implementam de maneiras diferentes.
- **O núcleo conduz a ligação e executa os passos dos perfis** (`[connect]`, `[relinquish]`, `[config] apply`, ações com `steps` e o fallback M1) sobre as duas primitivas `read-addr` e `write-addr`. Um driver novo ganha ações, fallback e relinquish sem escrever código para isso. As repetições e o backoff também são do núcleo: uma tentativa por chamada.
- Cada driver publica um **descritor** (nome, versão, protocolos, barramentos, capacidades e os esquemas das chaves de `config`, de endereço, de `quirks` e do mapa descoberto). O núcleo valida o manifesto e o perfil contra ele **antes** do `plan`, com as mesmas mensagens em todos os drivers, também para um tier X cujo executável não corre no portátil.

---

## 3. Planos de leitura

O plano agrupa pontos em **lotes**, e cada lote é um pedido no protocolo:

- **Modbus:** coalescência de registos contíguos dentro de `max_regs` (125 por omissão, ou o que o perfil disser) e com buracos até `max_gap` (por omissão 8 registos, porque ler lixo é mais barato do que outro pedido). As omissões de todos os `quirks` estão na ref/03 §2.2.6. Respeita `forbidden` (endereços que o equipamento rejeita ou que têm efeitos laterais na leitura, que existem mesmo).
- **OPC UA:** subscrições com `MonitoredItems` e `samplingInterval` por grupo de poll, e `Read` em lote para o resto.
- **CANopen:** PDOs mapeados (push) e SDO para o resto (lento, só para parâmetros).
- **SunSpec:** um lote por modelo descoberto, com o modelo de escala (`*_SF`) lido no mesmo pedido que os valores. O detalhe importa: se o fator de escala e o valor vierem de pedidos diferentes, pode ler-se o valor novo com a escala antiga.

Cada lote tem um **período** (o `poll` do dispositivo ou o de sinais individuais) e uma **prioridade**.

**Parâmetros a pedido.** Dicionários de objetos com milhares de entradas (CANopen OD, parâmetros de variadores) não se leem em ciclo. Os sinais com direção `param` e `poll = "on_demand"` são lidos no `connect`, depois de cada escrita e quando uma app ou a CLI os pede (`jk read --refresh`). Ficam no registo como qualquer outro sinal, com a idade real à vista.

---

## 4. Coerência de aquisição

Todos os pontos de um lote recebem o mesmo `acq_id` (documento 02, §6.2). Os perfis podem forçar pontos no mesmo lote com a tabela `[coherent]`, um grupo por chave (`ac = ["ac.p", "ac.q", "ac.pf"]`, ref/03 §2.2.9), e o `plan` falha se não couberem num pedido (`J0813`). Os pares valor/`scale_factor` são grupos implícitos. O driver tem de ter a capacidade `coherent`.

---

## 5. Escalonador de barramento

Barramentos partilhados (RS-485, I²C, SPI, um PLC com uma só ligação TCP declarada como `bus`) têm **um escalonador** com filas por prioridade:

1. escritas aplicadas (commit) e ações (incluindo os passos por endereço);
2. lotes dos sinais usados por grupos de ciclo, por ordem de período;
3. lotes de telemetria;
4. descoberta e diagnóstico.

Só uma chamada está em curso no barramento. Os dispositivos TCP sem `bus` e os barramentos CAN não se serializam entre dispositivos: cada um tem a sua ligação, ou as tramas são assíncronas (ref/06 §2.14).

**Orçamento do barramento (verificado no `jk check`, `J0370`).** Para cada barramento, o `jk check` estima a ocupação:

```
Modbus RTU, 19200 baud, 8E1 (11 bits/char):
  pedido FC03 = 8 bytes, resposta com n registos = 5 + 2n bytes
  t(n) = (8 + 5 + 2n) · 11 / 19200 s + turnaround + latência do escravo (perfil: 10 ms)
  t(20) = 53 · 11 / 19200 s ≈ 30 ms  + 5 ms + 10 ms ≈ 45 ms

  3 medidores × 2 lotes × 45 ms = 270 ms de trabalho a cada 200 ms
  → ocupação 135% → J0370: barramento sobrecarregado; com o alvo de 80% de ocupação,
    o poll mínimo viável é ~340 ms (ou juntar os dois lotes de cada medidor num só, se o
    perfil o permitir: 3 × (53+40)·11/19200 + … ≈ 3 × 68 ms = 204 ms → ainda 102%)
```

Isto apanha o erro mais comum em projetos Modbus RTU: pedir a 10 medidores num RS-485 o que só cabe em 3. O `J0370` sai acima de 100%; o alvo de 80% dá o poll mínimo da ajuda. O modelo para os outros barramentos (I²C, SPI, CAN, Ethernet partilhada) está na ADR 0049.

---

## 6. Perfis

Um perfil descreve um **modelo de equipamento**, não uma instância. Vive no registo de perfis (Git + OCI) ou em `profiles/` do projeto.

```toml
# profiles/eastron/sdm630@1.profile.toml
schema = 1

[profile]
name     = "eastron/sdm630"
version  = "1.2.0"
driver   = "modbus"                  # modbus-rtu ou modbus-tcp servem
vendor   = "Eastron"
docs     = "https://…"
quirks   = { max_regs = 40, max_gap = 0, min_interval = "20ms" }

[defaults]
fc        = 4                        # input registers
type      = "f32"
word_order = "big"                   # big | little
byte_order = "big"

[signals]
"l1.v"   = { at = 0,   unit = "V" }
"l2.v"   = { at = 2,   unit = "V" }
"l3.v"   = { at = 4,   unit = "V" }
"l1.i"   = { at = 6,   unit = "A" }
"p_total" = { at = 52, unit = "W", quantity = "active_power" }
"q_total" = { at = 60, unit = "var", quantity = "reactive_power" }
"f"      = { at = 70,  unit = "Hz", limits = ["45 Hz", "65 Hz"] }
"e_imp"  = { at = 72,  unit = "kWh", quantity = "energy", monotonic = true }

[coherent]
power = ["p_total", "q_total"]
```

Com saídas, ações e fallback:

```toml
# excerto de profiles/acme/pcs-125@3.profile.toml
[signals]
"p_set"    = { at = 40100, fc = 16, type = "i16", scale = 0.1, unit = "kW", direction = "output" }
"p_set_rb" = { at = 40100, fc = 3,  type = "i16", scale = 0.1, unit = "kW" }
"mode"     = { at = 40101, fc = 6,  type = "u16", enum = "PcsMode", direction = "output" }
"state"    = { at = 30010, fc = 4,  type = "u16", enum = "PcsState" }
"alarms"   = { at = 30011, fc = 4,  type = "bits16", bits = { 0 = "overtemp", 1 = "dc_fault", 5 = "grid_fault" } }

[enums]
PcsMode  = { off = 0, standby = 1, p_control = 2 }
PcsState = { off = 0, standby = 1, running = 2, fault = 3 }

[fallback]                           # M1
timeout_register = { at = 40200, fc = 6, type = "u16", unit = "s" }
value_register   = { at = 40201, fc = 6, type = "i16", scale = 0.1, unit = "kW" }
applies_to       = ["p_set"]

[actions.reset_fault]
steps = [{ write = { at = 40300, fc = 6, value = 1 } }, { wait = "500ms" }, { write = { at = 40300, fc = 6, value = 0 } }]
```

**Versões:** `perfil@major`. Uma mudança de endereço ou de escala é major, e acrescentar um sinal é minor (tabela completa na ref/03 §2.2.17). O lock fixa a versão exata. No manifesto, `profile = "sunspec"` é a forma curta de `profile = "joker/sunspec@1"`, o perfil de descoberta embutido na runtime (ref/03 §2.2.15).

**Classes implementadas:** um perfil pode declarar que o equipamento cumpre classes (documento 14), com o mapeamento dos membros para os seus sinais. Cada dispositivo com esse perfil passa a ser uma instância implícita dessas classes.

```toml
[implements."inverter.pv@2"]
map = { p_ac = "ac.p", p_limit_pct = "w_max_lim_pct", state = "state" }
capabilities = ["reactive"]
```

Um perfil só entra no registo público se as suas implementações passarem os testes de conformidade das classes (`jk class verify`).

**Importadores:** `jk profile import` a partir de modelos SunSpec (JSON oficial), NodeSets OPC UA (XML), EDS/DCF CANopen, ESI EtherCAT (XML) e folhas de registos em CSV. O resultado é um perfil TOML revisto por uma pessoa: o importador acelera, mas não substitui essa revisão.

---

## 7. Descoberta (M2)

- `jk discover modbus-tcp://192.168.10.0/24 --unit 1..10`: só funções de leitura, com limite de ritmo, e nunca escreve.
- `jk discover sunspec://192.168.10.30`: percorre a cadeia de modelos a partir de 40000.
- `jk discover opcua://plc:4840`: browse com filtro por tipo.
- `jk discover can0`: LSS/heartbeat de CANopen.

Com `discover = true` no manifesto, o driver faz a descoberta no `connect`, compara com o `joker.lock` e **não usa mapas não fixados em `prod`** (documento 03, §14).

---

## 8. Tier W: drivers em Wasm

O mundo é o `joker:driver`, em `wit/joker-driver.wit` (ref/06 §4). Na 1.0 é **instável** e só serve os drivers da árvore; passa a estável e aberto a terceiros na 1.2 (ref/06 §4, regra de estabilidade). Tem **a mesma versão** do `joker:runtime` que importa: `joker:driver@0.1.0` importa `joker:runtime/types@0.1.0` e `joker:runtime/log@0.1.0`, e na 1.0 ambos passam a `@1.0.0`. As interfaces são `types` (os registos do contrato), `host` (relógio, prazo da chamada, estado do link, progresso de ações), `transport` (porta série, TCP, UDP, I²C, SPI, CAN, entregues como um `link` em `open`) e `guest`, a exportada, com as operações do §2. Os erros são enums fechados (`batch-failure`, `write-status`, `action-failure`) e `error { code, message }` onde é para pessoas; só a configuração, os endereços e os `quirks`, que são de cada protocolo, viajam em CBOR, com o esquema declarado no descritor (secção custom `joker:driver-meta`).

O `plan` corre numa instância própria, só com `log` (o transporte faz trap com `J0882`), com *fuel* determinista: o mesmo componente dá o mesmo plano no portátil e no nó. Em runtime, cada instância tem o seu `Store`, cede a thread `io` a cada 1 ms de CPU contínua e tem um limite de CPU por chamada. Um trap põe os pontos em `bad(not_connected)` e o link em `stopped`, e a instância é recriada com backoff. O host da 1.0 fornece série, TCP e UDP; I²C, SPI e CAN entram na 1.2.

O driver recebe **só** o transporte que o manifesto lhe deu (`bus = "rs485"`), e não abre sockets à sua escolha. É o sítio certo para protocolos como DLMS/COSEM, IEC 62056-21, protocolos proprietários de inversores e EEBus sobre um transporte fornecido. O SDK Rust (`joker-driver-sdk`) gera o componente a partir dos mesmos traits do tier N.

---

## 9. Tier X: Joker Driver Protocol (JDP)

Um processo externo `joker-drv-<nome>` fala JDP com o `jokerd` (referência: ref/06 §5; `spec/cddl/jdp.cddl`, `spec/c/jdp.h`):

- **controlo:** socket Unix `SOCK_SEQPACKET` em `/run/joker/drv/<nome>.sock`, mensagens CBOR (`hello`/`welcome` com a versão `jdp`, plano, ciclo de vida, acesso por endereço, ações, scan, saúde, logs);
- **dados:** dois anéis SPSC em memória partilhada, cada um numa memfd com **um só escritor** (a do `jokerd` selada), com notificação por `eventfd`: pedidos de leitura e escritas num sentido, amostras e acks no outro;
- **streams** (1.1): descritores dmabuf/memfd passados por `SCM_RIGHTS`, sem cópia.

**Arranque e isolamento.** Em produção, o `jokerd` (que não é root) arranca cada driver como uma **unidade systemd transitória** com `DynamicUser`, sandbox (`ProtectSystem=strict`, `DevicePolicy=closed`, só os dispositivos e endereços do manifesto) e limites de memória e CPU; uma regra polkit limita-o às unidades `joker-drv-*`. A ligação é autenticada pelo cgroup do par (`SO_PEERCRED`) e por um token de arranque. Sem systemd, `launch = "spawn"`. O `jokerd` trata tudo o que vem do driver como hostil e valida cada entrada antes de a usar.

O `jokerd` supervisiona o processo: se ele morrer, os sinais passam a `bad(not_connected)`, o link a `stopped`, e o processo é reiniciado com backoff; as saídas ficam cobertas pelo fallback do dispositivo. Um SDK em C (`libjdp`) e em Rust (`joker-jdp`) torna um SDK de fabricante num driver com ~200 linhas.

---

## 10. Catálogo inicial

A coluna diz a fase até à 1.0 (documento 11) ou, depois dela, a versão (documento 21).

| Driver | Tier | Base | Fase ou versão |
|---|---|---|---|
| `sim` | N | — | 0 |
| `modbus-tcp`, `modbus-rtu` | N | `tokio-modbus` | 1 |
| `gpio` (chardev v2), `i2c`, `spi` | N | `gpio-cdev`/`linux-embedded-hal` | 1 |
| `sysfs` (LEDs e temperaturas de qualquer placa Linux: `/sys/class/leds`, `/sys/class/thermal`) | N | — | 1 |
| `revpi` (piControl: DIO, AIO, MIO, gateways) | N | ioctl em `/dev/piControl0` | 1 |
| `esp32` (só Joker Micro: pads de toque, RSSI do Wi-Fi, tempo de funcionamento; ref/01 anexo A) | N | `esp-hal` | 2 |
| `mqtt` (entrada) | N | `rumqttc` | 1 |
| `sunspec` (sobre modbus) | N | perfis + descoberta | 1 |
| `opcua` (cliente) | N | `async-opcua` | 2 |
| `v4l2` / `libcamera` | N / X | `v4l` / libcamera | 1.1 |
| `can`, `canopen` | N | `socketcan` | 2 |
| `m-bus` (medidores de energia, calor, água) | N | a escrever | 2 |
| `onewire`, `serial-line` (protocolos de texto) | N | — | 2 |
| `spi-adc` (ADCs com relógio próprio, blocos) | N | `spidev` + DMA | 1.1 |
| `ethercat` | N | `ethercrab` | 1.2 |
| `bacnet-ip` | X → N | `bacnet-stack` (C) em X, depois nativo | 1.2 (os dois tiers) |
| `gige-vision` | X | Aravis | 1.2 |
| `iec-104`, `iec-61850 (MMS)` | X | bibliotecas existentes | 1.2 |
| `dlms-cosem` | W | a escrever | 1.2 |
| `eebus` | W/X | — | 1.2 |

**`sysfs` nos LEDs.** Ao abrir um LED como saída, o driver põe o `trigger` a `none` (senão o kernel continua a comandá-lo, com o `mmc0` ou o `heartbeat`) e repõe o `trigger` anterior ao fechar. O pacote do `jokerd` instala uma regra udev que dá ao grupo `joker` escrita em `brightness` e `trigger` de `/sys/class/leds/*`, para que o `jokerd` não precise de correr como `root`. O detalhe está na ref/01, anexo A.

---

## 11. Perfis de placa

Um perfil de placa (`board = "…"`) descreve o hardware fixo:

```toml
# boards/revpi-connect-5.board.toml
schema = 1

[board]
name    = "revpi-connect-5"
version = "1.0.0"
arch    = "aarch64"
cpus    = 4
memory  = "4 GiB"
recommended = { rt = [3], io = [2], ml = [1, 2], bg = [0] }

[watchdog]
device  = "/dev/watchdog0"
timeout = "60s"

[io.led_a1]
driver    = "gpio"
line      = "…"
direction = "output"

[io.relay]
driver    = "gpio"
line      = "…"
direction = "output"
fallback  = false                # o que o hardware impõe quando o jokerd ou a placa param (M1)

[serial]
"rs485-0" = "/dev/ttyRS485-0"

# [accel.<nome>]: só o que a placa garante; o resto deteta-se (reservado para 1.1)
```

O IO fixo aparece como `board.<nome>` (`board.relay`). A chave `fallback` diz o estado que o **hardware** impõe, e conta como fallback do dispositivo (M1) na verificação face ao `safe`; o `safe` continua a ser do manifesto (ADR 0006). O formato completo, com as placas de microcontroladores, está na ref/03 §2.3.

Placas previstas na Fase 1: `rpi4`, `rpi5`, `cm4`, `cm5`, `revpi-connect-4`, `revpi-connect-5`, `x86-generic`. Na Fase 2: `jetson-orin-nano`, `jetson-orin-nx`.

---

## 12. Ligação, reconexão e saúde

O estado do link de cada dispositivo é o enum `link_state` da ref/00 §6. O efetivo é o pior entre o que o driver reporta e o que o núcleo calcula das chamadas (ref/06 §2.12).

| Estado do link | Qualidade dos sinais | Comportamento |
|---|---|---|
| `connecting` | `uncertain(initial)` nos pontos sem valor | backoff 250 ms → 30 s, ×2, com jitter ±20% |
| `up` | a do valor | — |
| `degraded` (≥ 20% das últimas 20 execuções de lotes com timeout ou falha de comunicação) | `uncertain(stale)` nos lotes afetados | alarme `joker.device.<d>.degraded` |
| `down` | `bad(comm_failure)` | alarme `joker.device.<d>.offline`; saídas: fallback do dispositivo |
| `drift` (M2) | valores lidos normalmente, saídas bloqueadas em `prod` | `J0401` com o `kind` (identidade, firmware, mapa, fallback); alarme `joker.device.<d>.drift` |
| `stopped` | `bad(not_connected)` | o driver não está a correr (processo X morto, trap W, crashloop, ou parado à mão com `jk device stop`, documento 07 §4); distingue "o software caiu" de "o cabo caiu" |

O driver só reporta os quatro primeiros; `drift` e `stopped` são do núcleo.

**Dispositivos `joker-link`** (um Joker Micro, documento 19 §6). O modo da sessão decidido no `HELLO` (`session_mode`, ref/00 §6) dá o estado do link (ref/07 §7.2):

| Sessão do Joker Link | Estado do link | Qualidade | Escritas do nó |
|---|---|---|---|
| sem sessão, ou handshake em curso | `connecting` | `uncertain(initial)` até à primeira amostra | não |
| `full` | `up` (ou `degraded`, com perdas ≥ 20% em 10 s) | a que vem no `SAMPLES` | sim |
| `observe` (o MCU não corresponde ao lock ou à geração) | `drift` | a que vem | não em `prod` (`J-E2319`) |
| `maintenance` (MCU sem geração válida, ou catálogo inválido) | `down` | `bad(config_error)` | não; só OTA, traço, tempo, reinício, catálogo e rotação de chaves |
| `refused` (sem major comum, identidade errada) | `down` | `bad(config_error)` | não |
| sessão perdida (`link_timeout`, `BYE`, `retransmit` esgotado) | `down` | `bad(comm_failure)`; alarme `joker.device.<d>.offline` | não |

Em todos os casos, **quem manda nas saídas do MCU é o MCU**: se a sessão cair, os leases remotos expiram lá e o `safe` é aplicado lá (ref/07 §7.4).

Métricas por dispositivo, iguais nos três tiers e medidas pelo núcleo: latência p50/p99 por lote, timeouts, reconexões, bytes, eficiência de coalescência (registos úteis / lidos) e os contadores próprios do driver.
