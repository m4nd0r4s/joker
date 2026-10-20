# 23 — A bancada de desenvolvimento

Este documento diz **com que hardware se desenvolve e testa o Joker agora**, e como cada fase do roadmap (documento 11) se valida com ele.

**A regra desta bancada: não se compra nada.** Tudo o que está aqui funciona só com as três placas que existem, os cabos que elas já usam (alimentação, micro-USB, cartões SD), o portátil e a rede Wi-Fi. O que precisaria de uma peça a mais está marcado como **compra futura (opcional)** e nunca bloqueia uma fase: ou se cobre de outra forma (simulação, QEMU), ou fica declarado como não validado na bancada.

---

## 1. Inventário e papéis

| Placa | O que é | Papel |
|---|---|---|
| **Raspberry Pi 3 Model B+** (2017) | 4× Cortex-A53 a 1,4 GHz, 1 GB, aarch64, Ethernet (por USB 2.0), Wi-Fi, 40 pinos | **Nó Linux principal** (`lab`). `jokerd` com o wasmtime completo (Cranelift em aarch64), API, historian, drivers. Onde se mede o jitter com PREEMPT_RT. Alimenta o ESP32 pelo USB. |
| **Raspberry Pi Zero W v1** | 1× ARM1176 (ARMv6) a 1 GHz, 512 MB, 32 bits, só Wi-Fi | **Plataforma fraca e segundo nó** (`zero`). Prova que o `jokerd` se degrada com avisos e não parte; cliente Modbus TCP do ESP32 pelo Wi-Fi; segundo nó para o Deck (Fase 3). |
| **ESP32** (original, placa de desenvolvimento) | 2× Xtensa LX6 a 240 MHz, 520 KB de SRAM, FPU de precisão simples, Wi-Fi, Bluetooth, ponte USB-série na placa (CP2102 ou CH340), LED no GPIO2, botão `BOOT` no GPIO0 | **Primeiro Joker Micro.** Na Fase 1, um equipamento Modbus "de verdade", por RTU e por TCP ao mesmo tempo (§4); na Fase 2, coprocessador do Pi 3 pelo Joker Link e nó por Wi-Fi. |
| portátil | — | desenvolvimento, `jk`, CI local, QEMU, e o Deck na Fase 3 |

O **Bluetooth** do ESP32 não é usado: nenhuma interface da v1 o prevê.

## 2. Topologia

```
                          Wi-Fi / rede local
      ┌───────────────┬──────────────────────┬─────────────────────────┐
      │               │                      │                         │
 ┌────┴─────┐   ┌─────┴──────────────┐   ┌───┴─────────────┐    ┌──────┴────────────┐
 │ portátil │   │ Raspberry Pi 3 B+  │   │ Pi Zero W       │    │ ESP32             │
 │ jk, CI,  │──▶│ jokerd (nó "lab")  │   │ jokerd (nó      │    │ Fase 1: servidor  │
 │ QEMU,    │   │ mosquitto (F2/F3)  │   │ "zero"): cliente│───▶│ Modbus TCP :502   │
 │ Deck(F3) │   │                    │   │ Modbus TCP      │    │ (Fase 2: link UDP)│
 └──────────┘   │   USB ─────────────┼───┼─────────────────┼───▶│ UART0 pela ponte  │
                └────────────────────┘   └─────────────────┘    │ USB (RTU / link)  │
                                                                └───────────────────┘
       O cabo micro-USB do ESP32 ligado ao Pi 3 dá alimentação e uma porta série
       (/dev/serial/by-id/usb-Silicon_Labs_CP2102_…): Modbus RTU na Fase 1, Joker Link
       na Fase 2. O Pi Zero W só chega ao ESP32 pelo Wi-Fi. Sem fios de jumper.
```

**O truque que evita comprar peças:** a ponte USB-série da placa do ESP32 (CP2102 ou CH340, conforme o fabricante) é uma porta série real para o Pi 3. Serve para o Modbus RTU e para o Joker Link sem transceptor RS-485 e sem fios. Na Fase 2, o `UART0` do ESP32 deixa de ser consola e passa a ser o link; as mensagens do ROM de arranque a 115 200 baud chegam antes do firmware e são lixo que o COBS descarta até ao primeiro `0x00` (ref/07 §3.2).

**O Pi Zero W não pode usar o USB.** Tem um só micro-USB OTG e não há adaptador na bancada: fala com o ESP32 pelo Wi-Fi, por Modbus TCP na Fase 1 (é isso que o firmware de bancada serve, §4). O ESP32 continua alimentado pelo USB do Pi 3 (ou do portátil).

## 3. Sinais reais sem componentes

A trilha de estudo (`docs/track/`) pede uma saída, uma entrada discreta, uma entrada analógica ruidosa e um equipamento remoto. Sem comprar nada, os equivalentes são:

| Placa | Sinal | Como | Tipo |
|---|---|---|---|
| Pi 3, Pi Zero | **LED `ACT`** da placa | driver `sysfs`, `/sys/class/leds/ACT/brightness` (§7) | saída `bool` |
| Pi 3, Pi Zero | **temperatura do CPU** | driver `sysfs`, `/sys/class/thermal/thermal_zone0/temp` (m°C) | entrada `f64`, `°C`. É o "processo físico" da trilha: aquece com carga (`stress-ng --cpu 4 --timeout 5m`) e arrefece sozinha |
| ESP32 | **LED da placa** (GPIO2 na maioria das placas; algumas não têm) | GPIO | saída `bool` |
| ESP32 | **botão `BOOT`** (GPIO0, em todas as placas) | GPIO com pull-up, ativo em baixo | entrada `bool` |
| ESP32 | **sensor de toque** (T0 no GPIO4; basta tocar no pino com o dedo) | periférico *touch* | cru `u16`, sinal `u64`: é o "potenciómetro" da bancada, um valor analógico real e ruidoso, que **desce** quando se toca |
| ESP32 | **RSSI do Wi-Fi** | rádio | cru `i16`, sinal `i64` em dBm (sem unidade no catálogo, porque é logarítmica: `unit = "1"`, com a descrição) |
| ESP32 | **tempo de funcionamento** | temporizador | cru `u32` em `s`, sinal `f64` em `s`, `monotonic` |

Os tipos dos sinais são os que a aquisição dá aos tipos crus do perfil (ref/03 §2.2.7: um inteiro cru com `unit` diferente de `1` passa a `f64`), e são os mesmos que o driver `esp32` do Joker Micro dá na Fase 2 (ref/01 anexo A): trocar de firmware não muda os tipos.

Chega para cobrir o que a trilha quer ensinar: uma saída com envelope e `safe`, uma entrada discreta com flancos, uma entrada analógica com ruído e `deadband`, um sensor que muda devagar e se pode aquecer, um equipamento remoto com qualidade, perda de comunicação e fallback do equipamento.

**O que não há:** tensões e correntes reais, relés, um sensor industrial verdadeiro. Os pinos de ADC do ESP32 estão soltos e só dão ruído.

## 4. O firmware de bancada do ESP32 (Fase 1)

Na Fase 1 ainda não existe o Joker Micro, mas o `jokerd` precisa de um equipamento real para testar os drivers `modbus-rtu` e `modbus-tcp`, a perda de comunicação e o fallback M1. Por isso o ESP32 começa como **um equipamento Modbus simples**, com um firmware pequeno em Rust (`esp-hal`, `esp-wifi` e um servidor Modbus `no_std`, por exemplo o `rmodbus`), em `tools/bench/esp32-modbus/` do repositório do Joker, gravado com `espflash`.

**Dois transportes, uma tabela.** O firmware serve **ao mesmo tempo**:

- **Modbus RTU no `UART0`**, pela ponte USB: 115 200 baud, 8N1, unidade 1;
- **Modbus TCP no Wi-Fi**, na porta 502, unidade 1.

Os dois leem e escrevem a **mesma** tabela de registos, e qualquer pedido válido, venha de onde vier, conta para o temporizador do fallback.

| Registo | FC | Tipo | Sinal |
|---|---|---|---|
| coil 0 | 1 (ler), 5 (escrever) | `bool` | LED da placa |
| discrete input 0 | 2 | `bool` | botão `BOOT` (já invertido: 1 = premido) |
| input register 0 | 4 | `u16` | leitura do sensor de toque |
| input register 1 | 4 | `i16` | RSSI em dBm (0 sem Wi-Fi) |
| input registers 2–3 | 4 | `u32` | segundos desde o arranque |
| holding register 0 | 3 | `u16` | versão do firmware de bancada |

**Credenciais do Wi-Fi.** Entram no build por variáveis de ambiente, e nunca no Git:

```sh
cd tools/bench/esp32-modbus
BENCH_WIFI_SSID="bancada" BENCH_WIFI_PASSWORD="…" cargo build --release
espflash flash --monitor target/xtensa-esp32-none-elf/release/esp32-modbus
```

Sem as duas variáveis, o firmware compila só com o RTU (e o RSSI fica a 0). No arranque, antes de o `UART0` passar a servir Modbus, o firmware escreve na consola série uma linha com a versão, o SSID e o **IP** obtido por DHCP (`bench-esp32 v1 · wifi bancada · ip 192.168.1.60 · modbus-tcp :502 · modbus-rtu uart0 115200 8N1`), e repete-a a cada ligação ao Wi-Fi. O `espflash --monitor` mostra-a logo depois de gravar; mais tarde, lê-se no Pi 3 com `picocom -b 115200` na porta `by-id`, **com o `jokerd` parado** (é a mesma porta). O endereço do `joker.zero.toml` (§6.4) é este IP: fixa-se com uma reserva DHCP no router.

**Fallback do próprio equipamento (M1):** se não receber nenhum pedido Modbus, por nenhum dos transportes, durante 2 s, o firmware apaga o LED. É o comportamento que o `[fallback]` do perfil declara, e é o que o `jk doctor` verifica.

**O perfil** é do projeto, e não do registo: `profiles/local/bench-esp32@1.profile.toml` (é o que a trilha escreve na parte 4, a partir do mapa acima em CSV, com o `jk profile import csv`; ref/03 §2.2.19). Depois de resolvidos os `# REVER`:

```toml
# profiles/local/bench-esp32@1.profile.toml
schema = 1

[profile]
name        = "local/bench-esp32"
version     = "1.0.0"
driver      = "modbus"
description = "Firmware de bancada do ESP32 (documento 23, §4): LED, botão, toque, RSSI, tempo de funcionamento"

[defaults]
poll = "100ms"

[signals]
"led"     = { at = 0, fc = 5, type = "bool", direction = "output", readback = "led_rb", safe_hint = false }
"led_rb"  = { at = 0, fc = 1, type = "bool" }
"button"  = { at = 0, fc = 2, type = "bool", label = "BOOT" }
"touch"   = { at = 0, fc = 4, type = "u16", deadband = 5, description = "valor cru do sensor de toque T0 (GPIO4); desce quando se toca" }
"rssi"    = { at = 1, fc = 4, type = "i16", description = "RSSI do Wi-Fi em dBm; 0 sem ligação" }
"uptime"  = { at = 2, fc = 4, type = "u32", unit = "s", monotonic = true }
"fw"      = { at = 0, fc = 3, type = "u16", direction = "param" }

[fallback]
timeout    = "2s"
value      = false
applies_to = ["led"]
```

O mesmo perfil serve os dois transportes: a família `modbus` aceita `modbus-rtu` e `modbus-tcp` (ref/03 §2.2.2). Testa-se sem manifesto com o `jk profile test` e as URIs da ref/03 §2.2.20:

```sh
jk profile test local/bench-esp32@1 --node lab \
   --uri "modbus-rtu:/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0?baud=115200&unit=1"
jk profile test local/bench-esp32@1 --node zero --uri "modbus-tcp://192.168.1.60:502?unit=1"
```

**Um equipamento, um mestre de cada vez.** O firmware aceita pedidos pelos dois transportes, mas **dois nós não devem falar ao mesmo tempo com o ESP32**. Os dois escreveriam o LED (coil 0) sem árbitro comum, porque o árbitro é de cada nó (documento 16, A1), e os pedidos de um manteriam vivo o temporizador do fallback quando o outro morre, o que esconde exatamente o que a parte 5 da trilha quer mostrar. Antes de o `zero` falar com o ESP32 (§6.4), o `lab` deixa de o fazer (`lab$ sudo systemctl stop jokerd`, ou uma geração sem o dispositivo `esp`); e quando o ESP32 passa a Joker Micro (Fase 2), o `esp` do `zero` fica `bad(comm_failure)`, porque o firmware Joker já não serve Modbus. O Joker não o consegue impedir: um equipamento Modbus não sabe quem são os seus mestres, e cada nó só vê os seus próprios pedidos. É uma regra da bancada, e é também a regra de um site real.

Na Fase 2, o **mesmo ESP32** recebe o firmware do Joker Micro, e os mesmos sinais passam a chegar pelo Joker Link, com os mesmos nomes e os mesmos tipos (§3). O manifesto do Pi 3 só muda o dispositivo (§6.5), e as apps não mudam. É o antes e depois mais claro que se pode ter da ADR 0020.

## 5. O que se valida em cada fase

### 5.1 Fase 0 — Núcleo determinista e portável

| Porta de saída (documento 11) | Na bancada | Observações |
|---|---|---|
| núcleo `no_std`, conformidade no Linux | portátil e Pi 3 (aarch64) | |
| **os mesmos bytes num MCU** | **ESP32** (Xtensa, `xtensa-esp32-none-elf`), por série | o Rust para Xtensa usa a toolchain da Espressif (`espup`): não está no LLVM oficial, e a CI tem de a instalar. Por isso o job Xtensa da CI **não bloqueia** (documento 13, §5), e o MSRV do núcleo fica ≤ à versão de Rust da toolchain Xtensa mais recente (13 §5.1) |
| a mesma conformidade em ARM Cortex-M e RISC-V | **QEMU no portátil**: `mps2-an505` (Cortex-M33, a arquitetura do RP2350) e `virt` riscv32 (a do ESP32-C6) | cobre as arquiteturas dos alvos de referência sem as placas; prova os bytes, não o tempo. São estes os jobs que **bloqueiam** a CI (13 §5) |
| WCET no chip | ESP32, com o contador de ciclos (`CCOUNT`), relatado por série | sem sonda: o ESP32 original só tem JTAG com um adaptador externo (compra futura) |

### 5.2 Fase 1 — MVP em Linux

| Porta de saída | Na bancada | Observações |
|---|---|---|
| `jokerd` num nó real, API, `jk dev` remoto, replay | **Pi 3** | |
| drivers `modbus-rtu`, `modbus-tcp`, `sysfs` | ESP32 (§4): RTU pela ponte USB do Pi 3, TCP pelo Wi-Fi a partir do Pi Zero W; `sysfs` nos dois Pi | o RS-485 **half-duplex** (vez de fala, `turnaround`) não se testa: a ponte USB é full-duplex. Declarado |
| fallback do equipamento (M1) e perda de comunicação | desligar o cabo do ESP32: LED apaga em 2 s pelo próprio firmware, sinais `bad(comm_failure)` no Pi 3 | |
| jitter medido com PREEMPT_RT | Pi 3, kernel com `PREEMPT_RT` (no kernel oficial desde a 6.12), `isolcpus=3` | **registado, sem alvo**: os alvos do documento 01 §21 (≤ 100 µs p99,9) são para CM4/CM5. A Ethernet do Pi 3 passa pelo USB e gera interrupções: medir com e sem rede. A medição externa num pino exige 2 fios e o ESP32 como analisador (compra futura: fios) |
| degradação sem RT | Pi Zero: 1 núcleo, kernel `standard` | espera-se `J0352` em todos os grupos, e o `jokerd` tem de funcionar com ele |
| trilha, partes 01–07 (a porta DX do documento 11) | Pi 3, Pi Zero W e ESP32, com os sinais do §3 | §6.6; o `jk class verify` no hardware (parte 7, §9) só na Fase 2 |

**Pi Zero W e o wasmtime.** O documento 01 §22 prevê o Pulley em armv7. O Zero é ARMv6, um degrau abaixo. **A confirmar na Fase 1:** se o wasmtime com Pulley compilar para `arm-unknown-linux-gnueabihf`, o Zero corre apps Wasm (devagar); se não, corre só apps nativas (documento 04, §5) e continua útil como segundo nó e como cliente Modbus TCP do ESP32. Nos dois casos fica no nível `dev` (§7).

**Nunca se compila no nó** (documento 07, §1, princípio 8). O `jokerd`, as apps e o AOT compilam-se no portátil (ou na CI), por build cruzado: `aarch64-unknown-linux-gnu` para o Pi 3 e `arm-unknown-linux-gnueabihf` para o Pi Zero W. O `jk runtime build` e o `jk dev --node` compilam no portátil e enviam o resultado; as Raspberry não têm `cargo` nem toolchain. Com 1 GiB (Pi 3) e 512 MiB (Pi Zero W) de RAM e um cartão SD (§7), compilar o `jokerd` na placa levaria horas e gastaria o cartão.

### 5.3 Fase 2 — Joker Micro e interoperabilidade

| Porta de saída | Na bancada | Observações |
|---|---|---|
| Joker Link em série | ESP32 ↔ Pi 3 pela ponte USB (até ~921 600 baud, conforme a ponte) | o caso U1 do documento 19 a outra escala |
| Joker Link em IP | ESP32 por Wi-Fi, UDP para o Pi 3 | com cifra obrigatória (`J0179`) |
| matar o `jokerd` → `safe` pelo MCU | `systemctl kill jokerd` no Pi 3; o LED do ESP32 segue o `safe` dentro do lease | a porta de saída mais importante da ADR 0020, validável já |
| OTA do blob de geração e do firmware | pelo link, com retoma depois de desligar o cabo a meio | A/B com o bootloader da Espressif (§7) |
| nó autónomo (U2) | ESP32 com MQTT por Wi-Fi para um broker no Pi 3 | |
| alvos RP2350, ESP32-C6, STM32 | **não há placas**: conformidade em QEMU (§5.1); o firmware completo só no ESP32 original | a porta "três alvos de referência" passa a "um alvo real e duas arquiteturas em QEMU" até haver hardware |
| CAN, RS-485 | **não se testam** | declarado |

### 5.4 Fase 3 — Frota, confiança e v1.0

| Porta de saída | Na bancada | Observações |
|---|---|---|
| Deck MVP com nós reais | Deck no portátil; nós Pi 3 e Pi Zero; ESP32 como MCU atrás do Pi 3 **ou** como MCU autónomo | dois nós Linux chegam para rollouts, coortes e rollback |
| nó offline durante muito tempo | desligar o Wi-Fi do Pi Zero dias seguidos | |
| escala (centenas de nós) | **agentes simulados** no portátil (o protocolo do agente é o mesmo, ref/10) | a escala não precisa de hardware |
| TPM | **não há** (o Pi 3 não tem): identidade em ficheiro, e o `jk doctor` diz-o (`J0933`, aviso) | declarado |

## 6. Manifestos da bancada

São os ficheiros do projeto da trilha, `joker-bancada/` (`[project] name = "bancada"`), no estado em que ficam depois da parte 6: um manifesto comum e um overlay por nó (ref/01 §7). As partes seguintes acrescentam instâncias, classes e apps ao `joker.toml`, e o `joker.lab-link.toml` (parte 9); os ficheiros de nó não mudam. Cada manifesto de entrada tem o seu lock (`joker.sim.lock`, `joker.lab.lock`, `joker.zero.lock`, `joker.lab-link.lock`; documento 07, §1); o `joker.rpi.toml` só entra por `extends` e não tem lock. Os overlays chamam-se `joker.<variante>.toml` e escolhem-se com `--env <variante>` (`jk apply --env zero` é o `joker.zero.toml`; ADR 0026).

```
joker.toml ◀── joker.sim.toml                                          (portátil, tudo simulado)
     ▲
     └──── joker.rpi.toml ◀── joker.lab.toml ◀── joker.lab-link.toml    (Pi 3; ESP32 por RTU, depois pelo link)
                  ▲
                  └──── joker.zero.toml                                 (Pi Zero W; ESP32 por Modbus TCP)
```

### 6.1 `joker.toml`: o que é igual em todo o lado

```toml
# joker-bancada/joker.toml — comum: saídas e envelopes, ciclos, calc, alarmes, apps
[project]
name  = "bancada"
joker = "1.0"
mode  = "dev"

[outputs."pi.act_led"]
safe = false

[outputs."esp.led"]
safe = false

[cycles.main]
period = "20ms"

[calc]
temp_media  = "avg(pi.cpu_temp, 10s)"
toque_nivel = "clamp((60 - esp.touch) / 45, 0, 1)"

[alarms.cpu_quente]
when     = "calc.temp_media > 70 °C"
delay_on = "10s"
severity = "medium"
message  = "CPU a {calc.temp_media}"

[alarms.esp_offline]
when     = "!good(esp.uptime)"
delay_on = "5s"
severity = "high"
message  = "ESP32 sem comunicação"

[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led", temp = "calc.temp_media", botao = "esp.button", led_estado = "esp.led", ajuste = "calc.toque_nivel" }
params = { half_period = "500 ms", t_frio = "45 °C", t_quente = "70 °C" }
cycles = { pisca = "main" }
```

Não tem `[target]` nem `[devices]`: os sinais `pi.*` e `esp.*` vêm dos overlays, e é por isso que o mesmo `joker.toml` corre no simulador (`joker.sim.toml`, com `driver = "sim"` nos dois dispositivos) e nas duas placas.

### 6.2 `joker.rpi.toml`: o que as duas Raspberry têm em comum

```toml
# joker-bancada/joker.rpi.toml — o dispositivo "pi" de qualquer Raspberry Pi (sysfs)
extends = ["joker.toml"]

[devices.pi]
driver = "sysfs"
poll   = "1s"

[devices.pi.signals]
act_led  = { led = "ACT", direction = "output" }
cpu_temp = { thermal = "thermal_zone0" }
```

Até à parte 5, este `[devices.pi]` vive no `joker.lab.toml`; a parte 6 tira-o para aqui quando aparece a segunda placa (uma refatoração de overlays: o manifesto efetivo do `lab` não muda, e o `jk diff` prova-o).

### 6.3 `joker.lab.toml`: o Pi 3, com o ESP32 por Modbus RTU

```toml
# joker-bancada/joker.lab.toml — nó "lab" (Raspberry Pi 3 B+)
extends = ["joker.rpi.toml"]

[target]
board = "rpi3b-plus@1"

[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
bg = { cpus = [0, 1] }

[buses.esp32]
kind = "serial"
port = "/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0"
baud = 115200

[devices.esp]
driver  = "modbus-rtu"
bus     = "esp32"
unit_id = 1
profile = "local/bench-esp32@1"
```

O nome da porta série depende da ponte da placa do ESP32 (CP2102 ou CH340): usa-se sempre o caminho estável de `/dev/serial/by-id/`, que o `jk doctor` e o `GET /v1/node/hardware` mostram (ref/09 §5.2).

### 6.4 `joker.zero.toml`: o Pi Zero W, com o ESP32 por Modbus TCP

```toml
# joker-bancada/joker.zero.toml — nó "zero" (Raspberry Pi Zero W)
extends = ["joker.rpi.toml"]

[target]
board = "rpi-zero-w"

[devices.esp]
driver  = "modbus-tcp"
address = "192.168.1.60:502"
unit_id = 1
profile = "local/bench-esp32@1"
```

- **A placa é do projeto:** `boards/rpi-zero-w.board.toml`, escrita na parte 6 com o `jk board probe --node zero` e validada com o `jk board check --node zero` (ref/03 §2.3.7; o resultado está no §7).
- **Sem `[resources]`:** com um só núcleo não há isolamento, e o grupo `main` fica sem classe `rt`. Com `kernel = "standard"`, o `jk check` dá `J0352` (aviso) no grupo `main`, e o `jokerd` tem de funcionar com ele (§5.2).
- **O mesmo perfil, outro transporte:** a família `modbus` aceita `modbus-tcp` (ref/03 §2.2.2), e o `address` é o IP que o firmware de bancada mostra na consola (§4). Os caminhos (`esp.led`, `esp.touch`) e a app não mudam.
- Só um nó fala com o ESP32 de cada vez (§4).

### 6.5 `joker.lab-link.toml`: o mesmo ESP32 pelo Joker Link (Fase 2)

```toml
# joker-bancada/joker.lab-link.toml — o ESP32 passa a ser um Joker Micro
extends = ["joker.lab.toml"]

[buses.esp32]
baud = 921600

[devices.esp]
driver  = "joker-link"
unit_id = { unset = true }
profile = { unset = true }
auth    = "required"
report  = "20ms"
refresh = "1s"

[outputs]
"esp.led" = { unset = true }      # o envelope desta saída passa a viver no MCU
```

O dispositivo mantém o nome `esp`, e o firmware do Joker Micro exporta os sinais com os mesmos nomes e tipos (`led`, `button`, `touch`, `rssi`, `uptime`; o LED e o botão pelo driver `gpio` do MCU, `line = "gpio2"` e `line = "gpio0"`; o toque, o RSSI e o tempo de funcionamento pelo driver `esp32`, ref/01 anexo A): os caminhos (`esp.led`) não mudam, e as apps também não. O perfil é gerado do catálogo do `HELLO` (ref/03 §2.2.16). O envelope e o `safe` do LED passam a viver **no MCU**: o manifesto do Joker Micro (`micro/esp/joker.toml`, compilado com `jk build --target mcu:esp32-devkit`, ref/08) declara-os, e o nó Linux não arbitra essa saída (ref/07 §8.1). A chave do link vem do enrolamento do MCU (`jk node enroll <mcu> --link esp`). Para o link por Wi-Fi (parte 10), o manifesto do MCU declara `[net.wifi]` com a palavra-passe como `secret:` (ref/01 §5.22), e o enrolamento leva `--ca` para o MCU poder receber segredos (ref/08 §2.17).

### 6.6 A trilha de estudo nesta bancada

A trilha (`docs/track/`, partes 01–11) é o plano de testes desta bancada: cada parte corre num estado conhecido e valida uma parte do que as Fases 1–3 prometem. O detalhe, com comandos e saídas, está em cada parte; aqui fica o que cada uma prova do hardware.

| Parte | Onde | O que valida da bancada |
|---|---|---|
| 01 | portátil | nada de hardware: `jk dev`, o painel e os cenários contra o simulador (Fase 0/1) |
| 02 | Pi 3 | instalação e enrolamento de um nó real; a placa `rpi3b-plus`; o driver `sysfs` no LED `ACT`. Sem fallback de hardware, o LED fica como estava quando o `jokerd` para: é o `J0205` em `prod`, a lição da parte |
| 03 | sim → Pi 3 | a temperatura do CPU pelo `sysfs`, aquecida com `stress-ng`; `calc`, alarmes, historian e a API HTTP num nó real |
| 04 | sim → Pi 3 + ESP32 | o driver `modbus-rtu` pela ponte USB; o perfil `local/bench-esp32@1` a partir do CSV (`jk profile import csv`, `jk profile test`); flancos do botão e `readback` do LED |
| 05 | sim → Pi 3 + ESP32 | perda de comunicação real (desligar o cabo): `bad(comm_failure)` no Pi 3 e o LED do ESP32 apagado em 2 s pelo próprio firmware (fallback M1); qualidade, `get_or` e replay |
| 06 | Pi Zero W + ESP32 por Wi-Fi | o driver `modbus-tcp`; uma placa ARMv6 de nível `dev` escrita com `jk board probe` e `jk board check`; a degradação sem RT (`J0352`) |
| 07 | sim → Pi 3 (e Zero) | as classes `std/*` sobre sinais reais, com o mesmo cenário da parte 5 |
| 08 | sim → Pi 3 | uma instância virtual (`status_light@1`) implementada por uma app, no hardware |
| 09 | Pi 3 + ESP32 (firmware Joker) | o Joker Link em série pela ponte USB; `jk flash` por `espflash`; o `safe` do LED no MCU quando se mata o `jokerd` (a porta de saída mais importante da ADR 0020) |
| 10 | ESP32 (+ Pi 3) | uma app nativa no MCU; o driver `esp32`; `[net.wifi]`; o link por UDP com cifra obrigatória (`J0179`) |
| 11 | portátil (Deck) + Pi 3 + Pi Zero W | assinaturas e segredos em `prod`; o Deck MVP com dois nós reais, coortes e um rollout que para no canário (overruns no Pi Zero W) |

## 7. Perfis de placa

Os três ficheiros seguem a ref/03 §2.3. O `rpi3b-plus` e o `esp32-devkit` estão no registo; o `rpi-zero-w` não, porque é de nível `dev`: é o que a trilha escreve na parte 6, com o `jk board probe` e os `# REVER` resolvidos (ref/03 §2.3.7), e fica em `boards/` do projeto. O Pi 3 fica no nível `2` (aarch64, sem CI com hardware); o Pi Zero W e o ESP32 original ficam no nível `dev`: servem para desenvolver e nunca são alvos de produção (01 §22). As duas Raspberry declaram `storage = "sd"`: arrancam de um cartão SD, e o `jk check` e o `jk doctor` avisam (`J0381`) quando a carga de escrita projetada passa do que um cartão aguenta (01 §12 e §19).

```toml
# boards/rpi3b-plus.board.toml
schema = 1

[board]
name         = "rpi3b-plus"
version      = "1.0.0"
arch         = "aarch64"
tier         = "2"
vendor       = "Raspberry Pi"
model        = "Raspberry Pi 3 Model B+"
cpus         = 4
memory       = "1 GiB"
clock        = "1.4 GHz"
storage      = "sd"
kernel       = "preempt_rt"
recommended  = { rt = [3], io = [2], bg = [0, 1] }
boot_cmdline = ["isolcpus=3", "nohz_full=3", "rcu_nocbs=3"]

[watchdog]
device  = "/dev/watchdog0"
timeout = "15s"

[gpio]
chip = "pinctrl-bcm2835"

[io.act_led]
driver    = "sysfs"
led       = "ACT"
direction = "output"

[io.cpu_temp]
driver  = "sysfs"
thermal = "thermal_zone0"

[i2c]
"i2c1" = "/dev/i2c-1"

[spi]
"spi0" = "/dev/spidev0.0"

[serial]
"uart0" = "/dev/serial0"

[net]
"eth"  = "eth0"
"wifi" = "wlan0"
```

```toml
# boards/rpi-zero-w.board.toml
schema = 1

[board]
name        = "rpi-zero-w"
version     = "1.0.0"
arch        = "armv6"
tier        = "dev"
vendor      = "Raspberry Pi"
model       = "Raspberry Pi Zero W (v1.1)"
cpus        = 1
memory      = "512 MiB"
clock       = "1 GHz"
storage     = "sd"
kernel      = "standard"
recommended = { rt = [0], io = [0], bg = [0] }

[watchdog]
device  = "/dev/watchdog0"
timeout = "15s"

[gpio]
chip = "pinctrl-bcm2835"

[io.act_led]
driver    = "sysfs"
led       = "ACT"
direction = "output"

[io.cpu_temp]
driver  = "sysfs"
thermal = "thermal_zone0"

[net]
"wifi" = "wlan0"
```

```toml
# boards/esp32-devkit.board.toml
schema = 1

[board]
name       = "esp32-devkit"
version    = "1.0.0"
os         = "none"
arch       = "xtensa-esp32-none-elf"
mcu_class  = "m2"
chip       = "esp32"
tier       = "dev"
vendor     = "Espressif"
model      = "ESP32 (Xtensa LX6), placa de desenvolvimento genérica"
memory     = "520 KiB"
flash      = "4 MiB"
fpu        = "sp"
clock      = "240 MHz"
reset_safe = false
stack      = "16 KiB"

[capacities]
signals     = 256
outputs     = 64
groups      = 4
instances   = 32
apps        = 4
alarms      = 32
programs    = 64
trace_bytes = "16 KiB"
kv_bytes    = "16 KiB"

[irq]
priorities = 3

[pins.gpio0]
functions = ["gpio"]
note      = "botão BOOT (pino de arranque: não usar como saída)"
[pins.gpio2]
functions = ["gpio", "pwm"]
note      = "LED da placa, na maioria das placas (pino de arranque, puxado a baixo)"
[pins.gpio4]
functions     = ["gpio", "adc", "touch"]
touch_channel = 0
note          = "sensor de toque T0"
[pins.gpio1]
functions = ["gpio", "uart"]
[pins.gpio3]
functions = ["gpio", "uart"]

[uart.uart0]
tx       = "gpio1"
rx       = "gpio3"
max_baud = 921600

[adc]
bits = 12
vref = "3.3 V"

[watchdog]
kind    = "esp32-rwdt"
timeout = "1s"

[link]
transports = ["uart", "udp", "tcp"]
default    = { transport = "uart", port = "uart0", baud = 115200 }

[boot]
bootloader = "esp-idf"
slots      = 2

[areas]
bootloader = { offset = 0x001000, size = 0x007000 }
boot_state = { offset = 0x009000, size = 0x002000 }
firmware   = { offset = 0x010000, size = 0x180000 }
fw_staging = { offset = 0x190000, size = 0x180000 }
gen_a      = { offset = 0x310000, size = 0x010000 }
gen_b      = { offset = 0x320000, size = 0x010000 }
persist    = { offset = 0x330000, size = 0x020000 }
identity   = { offset = 0x350000, size = 0x002000 }

[probe]
tool     = "espflash"
protocol = "serial"

[net]
wifi     = true
ethernet = false
```

Notas:

- **`reset_safe = false`** no ESP32: no reset, os GPIO ficam como entradas até o firmware os configurar; só os pinos de arranque têm pull interno (o GPIO2 tem pull-down, e por isso o LED fica apagado). Na bancada não há carga ligada, e o `mode = "dev"` aceita-o; em `prod` é `J0786`, que a parte 11 da trilha aceita com `accept_unsafe_reset` e a justificação (ref/01 §5.22).
- As **partições** seguem a tabela do bootloader da Espressif (a tabela de partições em `0x8000` fica fora das áreas do Joker); a troca A/B do firmware usa as partições OTA dele.
- **Gravação por série** (`espflash`, pela ponte USB), sem sonda: `jk flash` com `tool = "espflash"`.
- O ESP32 original não está na tabela de alvos da v1 (19 §3.1), mas é da mesma família do ESP32-S3 (Xtensa, FPU simples) e tem memória da classe M2: as capacidades são as do S3 reduzidas.

## 8. O que esta bancada não prova

| Lacuna | Consequência | Como se fecha (quando houver) |
|---|---|---|
| nenhum MCU ARM Cortex-M real | o alvo M1 de referência (RP2350) só em QEMU; `probe-rs` e `jk bench --target mcu` por sonda não se testam | compra futura (opcional): Raspberry Pi Pico 2 e Debug Probe |
| nenhum ESP32-C6 (RISC-V, sem FPU) | o caso `J0780` (vírgula flutuante por software) não se mede | compra futura: um ESP32-C6 |
| RS-485 half-duplex e CAN | a vez de fala do Joker Link e o Modbus RTU em barramento partilhado, e o link em CAN, ficam sem teste real | compra futura: adaptadores USB-RS485 e CAN |
| hardware da classe dos alvos RT (CM4/CM5) | os números do documento 01 §21 não se confirmam; os do Pi 3 ficam registados como referência pessimista | hardware de CI (pendente, documento 18) |
| medição externa do jitter | só a medição interna (traço, métricas, `cyclictest`) | 2 fios de jumper, com o ESP32 como analisador lógico (captura de flancos com o temporizador) |
| TPM | identidade só em ficheiro | placa com TPM ou elemento seguro |
| equipamentos industriais reais | sem PLCs, inversores, contadores | pilotos |

Nenhuma destas lacunas bloqueia uma fase: cada porta de saída afetada fica marcada **"validada na bancada"** ou **"validada em QEMU/simulação"**, e o resto espera por hardware.

## 9. O que isto mudou noutros documentos

- **ref/01, anexo A, e documento 05 §10:** driver novo **`sysfs`** (tier N, 1.0), para o LED e a temperatura de qualquer placa Linux (`/sys/class/leds`, `/sys/class/thermal`).
- **ref/03 §2.3:** `arch` aceita `armv6` (Linux, só nível `dev`) e o triplo `xtensa-esp32-none-elf`; `[boot] bootloader` aceita `esp-idf`; `[probe]` ganha `tool` (`probe-rs` \| `espflash`) e o protocolo `serial`.
- **ref/08 §2.15.1:** linha do ESP32 original como alvo de desenvolvimento.
- **01 §22:** o Raspberry Pi 3 no nível 2; o Pi Zero W (ARMv6) e o ESP32 original no nível `dev`.
- **07:** `jk flash` grava por sonda (`probe-rs`) ou por série (`espflash`).
- **11:** as portas de saída das Fases 0–2 dizem onde se validam enquanto houver só esta bancada.
- **ref/03 §2.2.19–§2.2.20 e §2.3.7, documento 07 §7:** `jk profile import csv`, a gramática das URIs de equipamento do `jk profile test`, e `jk board probe`, `jk board check`, `jk board new` e `jk board publish`, para escrever o perfil do firmware de bancada e o da placa `rpi-zero-w`; `[pins] touch_channel` nas placas MCU.
- **ref/09 §5.2 e OpenAPI:** `GET /v1/node/hardware` (`NodeHardware`), o inventário só de leitura que o `jk board probe` usa.
- **ref/01 anexo A e §5.22:** o driver `esp32` do Joker Micro (toque, RSSI, tempo de funcionamento) e a secção `[net.wifi]` (`J0240`–`J0242`); **ref/08 §2.13.3 e §2.17:** a secção `NET` do blob e o caminho da palavra-passe do Wi-Fi em `SECRETS`; **ref/02 §2.4.8:** a exceção dos segredos de um MCU coprocessador com Wi-Fi.
