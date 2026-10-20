# Trilha de estudo: da bancada à frota

Uma trilha em quatro módulos e onze partes. Começa numa pasta vazia no portátil e acaba com dois nós Linux geridos por um Deck, um ESP32 a correr o Joker Micro, uma classe tua e uma versão má de uma app travada no canário. Tudo com o hardware da bancada (documento 23): um Raspberry Pi 3 B+, um Raspberry Pi Zero W, um ESP32 e o portátil.

A trilha é três coisas ao mesmo tempo:

1. **o curso**: quem a termina entende o projeto inteiro, do tick do núcleo ao rollout de uma frota;
2. **a demonstração da API da v1.0**: cada comando, ficheiro, chave e saída é o que as referências de `../ref/` especificam, e as referências prevalecem quando divergem;
3. **o teste de aceitação da DX e o plano de testes na bancada real** (documento 11, porta de saída 4; documento 23, §5).

> O Joker está em fase de desenho. As saídas mostradas são as especificadas; quando a Fase 1 existir, cada parte é corrida na bancada e as saídas reais substituem estas.

---

## O ritmo

A trilha avança sempre do simulado para o real, e sobe de camada em camada:

```
             sinais (A)                      classes (B)      Joker Micro (C)     frota (D)
 portátil    1 ─ 3a ─ 4a ─ 5a                7a ─ 8a
                  │     │     │               │     │
 hardware    2 ─ 3b ─ 4b ─ 5b ─ 6            7b ─ 8b ──────▶  9 ─ 10  ─────────▶  11
             Pi 3   Pi 3+ESP32   Pi Zero      Pi 3             Pi 3 + ESP32        Deck + Pi 3 + Pi Zero
```

- **Sim → real.** Cada comportamento novo é primeiro escrito e testado no simulador (com relógio virtual, em milissegundos), e só depois vai para a placa. A passagem para o hardware mexe só em manifestos.
- **Sinais → classes → MCU → frota.** Primeiro aprende-se o que está por baixo (sinais, qualidade, envelope, árbitro); depois sobe-se para as classes, que tiram trabalho à app; depois o controlo desce para o microcontrolador; e por fim gere-se tudo como uma frota.

---

## O percurso

### Módulo A — Sinais e o nó Linux

| Parte | O que constróis | Hardware | Conceitos do Joker |
|---|---|---|---|
| [01](01-hello-bench.md) | um projeto do zero; a app `luz` 0.1 pisca o LED `ACT` **simulado**, com testes e um cenário | portátil | projeto, manifesto e overlays desde o início, driver `sim`, sinal, porta e ligação, saída com `safe`, grupo de ciclo, hot-reload, `jk read`/`jk trace`, `joker-test` com relógio virtual, cenários |
| [02](02-the-pi-as-a-node.md) | o mesmo LED, agora o `ACT` **real** do Pi 3, com a app sem mudanças | Pi 3 B+ | instalação do `jokerd`, CA e enrolamento, `jk doctor --rt`, perfil de placa, `[resources]`, driver `sysfs`, lock por manifesto, `jk apply --confirm`, gerações e rollback, escrita manual e árbitro, estado seguro em três camadas |
| [03](03-temperature-sensor.md) | a temperatura do CPU como processo físico: o LED pisca mais depressa quanto mais quente está o Pi; alarme, historian, API (app 0.2) | Pi 3 B+ | entradas e qualidade, `calc` com janelas, alarmes, historian, API HTTP e o cliente Python, `stress-ng` como carga |
| [04](04-esp32-modbus.md) | o ESP32 como equipamento Modbus RTU: o perfil escrito a partir do mapa de registos, o botão `BOOT` liga e desliga o sistema (app 0.3) | Pi 3 + ESP32 por USB | barramentos (`dtr`/`rts`), `jk profile import csv`, `jk profile test`, `identity` do perfil, flancos e `debounce`, `readback`, fallback do equipamento |
| [05](05-touch-and-faults.md) | o sensor de toque como botão rotativo; o sistema tolera a perda do ESP32 (app 0.4); um replay prova a correção (app 0.4.1) | Pi 3 + ESP32 | sinais analógicos ruidosos, `deadband`, `get_or` e qualidade substituída, falhas simuladas, `jk device stop`, fallback M1 real, `jk record`/`jk replay` a partir do estado gravado |
| [06](06-switching-boards.md) | o mesmo projeto no Pi Zero W, com o ESP32 por Wi-Fi; o perfil de uma placa que ainda não existe (app 0.4.1, sem mudar) | Pi Zero W + ESP32 por Wi-Fi | overlays por refatoração (`joker.rpi.toml`), `modbus-tcp`, `jk board probe`/`check`/`new`, nível `dev`, degradação sem RT |

### Módulo B — Classes

| Parte | O que constróis | Hardware | Conceitos do Joker |
|---|---|---|---|
| [07](07-classes.md) | a mesma app sobre `Led`, `Button`, `TempSensor`, `AnalogIn`; o cenário da parte 5 passa sem alterações (app 0.5) | sim → Pi 3 (e Zero) | classes e instâncias (implícita e compostas), `implements` no perfil, `jk class show`, `jk explain <instância>`, `jk status --instances`, `jk class verify` no hardware |
| [08](08-your-own-class.md) | uma classe tua, `status_light@1`, e a sua implementação; a app de negócio fica com ~20 linhas (app 0.6) | sim → Pi 3 | `jk class new`, `.jkc`, instâncias virtuais, `#[joker::implements]`, conformidade, as quatro pessoas do documento 14; extra opcional: uma classe composta, `local/bench_io@1` |

### Módulo C — Joker Micro

| Parte | O que constróis | Hardware | Conceitos do Joker |
|---|---|---|---|
| [09](09-joker-micro.md) | o ESP32 passa a correr o Joker Micro; os mesmos sinais chegam pelo Joker Link, e o `safe` do LED vive no MCU (app 0.6, sem mudar) | Pi 3 + ESP32 (firmware Joker) | `jk build --target mcu:esp32-devkit`, `jk flash`, `jk node enroll --link`, `joker.lab-link.toml`, envelope no MCU, matar o `jokerd` com o LED seguro |
| [10](10-logic-on-the-mcu.md) | uma app nativa no MCU (`reflexo`) e o link por Wi-Fi | ESP32 (+ Pi 3) | apps nativas no MCU, driver `esp32`, `[net.wifi]`, link UDP com cifra, nó autónomo |

### Módulo D — Frota

| Parte | O que constróis | Hardware | Conceitos do Joker |
|---|---|---|---|
| [11](11-production-and-fleet.md) | produção (assinaturas, segredos), o Deck no portátil com dois nós, um rollout em ondas que para no canário (apps 0.7 e 0.7.1) | portátil (Deck) + Pi 3 + Pi Zero W | `mode = "prod"`, chaves e confiança, `secrets.toml.enc`, `joker-deck`, `fleet.toml`, coortes, ondas e `halt_if` |

---

## Material

**Só o da bancada, e nada se compra.** Nenhuma parte pede uma breadboard, um LED, uma resistência, um fio de jumper, um ADC ou um adaptador RS-485.

| Peça | O que é | Onde entra |
|---|---|---|
| portátil | macOS, Linux ou Windows com WSL2; Rust estável; a `jk`; SSH | todas |
| **Raspberry Pi 3 Model B+** | 4× Cortex-A53, 1 GB, aarch64, Wi-Fi; o nó `lab` | 2–11 |
| **Raspberry Pi Zero W v1** | 1× ARM1176 (ARMv6), 512 MB, só Wi-Fi; o nó `zero` | 6, 7, 11 |
| **ESP32 original** (placa de desenvolvimento) | Xtensa LX6, ponte USB-série CP2102 ou CH340, LED no GPIO2, botão `BOOT` no GPIO0, sensor de toque T0 no GPIO4, Wi-Fi | 4–11 |
| cabos e cartões | as fontes das placas, um cabo micro-USB (ESP32 → Pi 3: alimentação e porta série), dois cartões microSD | — |
| rede | uma rede Wi-Fi local onde estão o portátil e as três placas | 2–11 |

Os sinais reais saem todos das próprias placas (documento 23, §3): o LED `ACT` e a temperatura do CPU dos dois Pi, e o LED, o botão, o toque, o RSSI e o tempo de funcionamento do ESP32. **A temperatura do CPU do Pi é o "processo físico" da trilha**: aquece com carga (`stress-ng --cpu 4 --timeout 5m`) e arrefece sozinha, com uma constante de tempo de um minuto. É isso que dá dinâmica real sem componentes.

O ESP32 liga-se ao Pi 3 pelo cabo micro-USB. O Pi Zero W não tem como o ligar por USB (só tem um micro-USB OTG, e não há adaptador), e por isso fala com ele por Wi-Fi, em Modbus TCP (parte 6).

---

## A pasta, no fim da trilha

```
joker-bancada/
  joker.toml                                  manifesto comum: saídas e envelopes, ciclos, calc, alarmes, apps, instâncias
  joker.sim.toml                              overlay: extends joker.toml → tudo simulado (portátil)
  joker.rpi.toml                              overlay: extends joker.toml → o dispositivo `pi` (sysfs), comum às duas Raspberry (parte 6)
  joker.lab.toml                              overlay: extends joker.rpi.toml → nó lab (Pi 3): placa, recursos RT, ESP32 por Modbus RTU
  joker.zero.toml                             overlay: extends joker.rpi.toml → nó zero (Pi Zero W): placa, ESP32 por Modbus TCP
  joker.lab-link.toml                         overlay: extends joker.lab.toml → o ESP32 como Joker Micro pelo Joker Link (parte 9)
  joker.sim.lock  joker.lab.lock  joker.zero.lock  joker.lab-link.lock
                                              um lock por manifesto de entrada (o joker.rpi.toml só entra por extends)
  mapa-esp32.csv                              parte 4 (o mapa de registos de onde saiu o perfil)
  boards/rpi-zero-w.board.toml                perfil de placa (parte 6)
  profiles/local/bench-esp32@1.profile.toml   perfil de equipamento (parte 4; implements na parte 7)
  classes/local/status_light.jkc              classe (parte 8)
  classes/local/status_light.conformance.toml suite de conformidade da classe (parte 8)
  micro/esp/joker.toml  micro/esp/joker.lock  manifesto do Joker Micro (partes 9–10) e o seu lock ([wcet] do chip, parte 10)
  micro/esp/apps/reflexo/                     app nativa do MCU (parte 10)
  sim/pi.sim.toml  sim/esp.sim.toml           modelos de simulação
  sim/scenarios/*.scenario.toml               cenários: pisca, aquece, botao, falha_esp
  apps/luz/                                   a app de negócio, 0.1 → 0.7.1
  apps/status_light_ctl/                      parte 8
  secrets.toml.enc                            partes 9–11 (destinatário node:esp, wifi_bancada, mqtt_bancada)
  keys/*.pub                                  parte 11
  fleet.toml                                  parte 11 (o que o Deck lê do repositório)
```

O extra opcional da parte 8 (`classes/local/bench_io.jkc` e o seu `.conformance.toml`, a instância `esp_io` e a app `apps/bench_diag/`) faz-se num ramo à parte, `extra-bench-io`, e não fica nesta pasta.

Cada ficheiro diz o tipo no nome (ADR 0026): `joker.<variante>.toml` é um overlay, e `jk … --env <variante>` escolhe-o; `.board.toml`, `.profile.toml`, `.sim.toml`, `.scenario.toml` e `.conformance.toml` são os outros tipos.

Até à parte 5, `joker.lab.toml` estende diretamente `joker.toml` e tem o `[devices.pi]`. Na parte 6, quando aparece a segunda placa Raspberry, o `[devices.pi]` sai para `joker.rpi.toml`, e `joker.lab.toml` e `joker.zero.toml` passam a estendê-lo. É uma lição de overlays: refatora-se quando aparece o segundo caso, e não antes.

## As duas regras

1. **A app só muda quando muda o comportamento ou a camada.** O comportamento muda nas partes 3, 4, 5 e 11; a camada muda nas partes 7 (classes) e 8 (a tua classe). Passar do simulador para o hardware (parte 2), de uma placa para outra (parte 6) ou do Modbus para o Joker Link (parte 9) mexe só em manifestos.
2. **Mudar de camada ou de placa não muda o comportamento.** O cenário `sim/scenarios/falha_esp.scenario.toml`, escrito na parte 5, passa sem alterações nas versões 0.4 (e na correção 0.4.1), 0.5 e 0.6 da app, e o mesmo manifesto comum corre no Pi 3 e no Pi Zero W.

| Versão da `luz` | Parte | O que mudou |
|---|---|---|
| 0.1 | 01 | pisca o `led` com `half_period` |
| 0.2 | 03 | pisca mais depressa quanto mais quente está o CPU; `Err` sem temperatura `good` → `safe` |
| 0.3 | 04 | o botão do ESP32 liga e desliga o sistema, que arranca ligado; o LED do ESP32 mostra o estado |
| 0.4 | 05 | o toque ajusta a sensibilidade; tolerante à perda do ESP32 |
| 0.4.1 | 05 | correção vista no replay: sem ESP32, não propõe o LED dele (fica até à parte 6) |
| 0.5 | 07 | igual, sobre handles de classe |
| 0.6 | 08 | igual, sobre a instância `sinal` (`status_light@1`) |
| 0.7 | 11 | modo `alert` com o alarme `cpu_quente` |
| 0.7.1 | 11 | "melhoria" má de propósito: overruns no Pi Zero W, travada no canário |

## Convenções

- **Prompts.** `$` é um comando no portátil; `lab$` no Pi 3 (por SSH, `ssh pi@lab.local`); `zero$` no Pi Zero W (`ssh pi@zero.local`). Quase tudo corre no portátil: a `jk` fala com os nós pela API.
- **Nós.** `lab` (Pi 3 B+, `lab.local`) e `zero` (Pi Zero W, `zero.local`). A CA local fica em `~/org-ca` (parte 2).
- **Dispositivos**, com os mesmos nomes no simulador e no hardware, para que os caminhos nunca mudem:
  - `pi`: o Raspberry Pi. `pi.act_led` (`bool`, saída) e, a partir da parte 3, `pi.cpu_temp` (`f64`, `°C`). Driver `sim` no portátil, `sysfs` nas placas.
  - `esp`: o ESP32. `esp.led` (saída), `esp.led_rb`, `esp.button`, `esp.touch`, `esp.rssi`, `esp.uptime`, `esp.fw`. Driver `sim` no portátil, `modbus-rtu` no Pi 3 (partes 4–5), `modbus-tcp` no Zero (parte 6), `joker-link` a partir da parte 9.
- **Grupo de ciclo:** `main`, 20 ms.
- **Saídas:** blocos sem linguagem, curtos, com os códigos de `spec/codes.toml`. Em `mode = "dev"`, os códigos de gravidade `error-in-prod` aparecem como avisos `W` com o mesmo número (`W0205`); os avisos puros mantêm o `J` (`J0352`).
- **Versões.** Cada parte termina com um commit (`git commit`; com jj, `jj describe && jj new`) e diz o estado que deixa: a versão da app, os ficheiros novos e a geração de cada nó.
- **Nomes de exemplo:** o utilizador `dl`, a organização `bancada` (`jk ca init ~/org-ca --org bancada`), domínios `example.com`.

## Tempo estimado

| Parte | 01 | 02 | 03 | 04 | 05 | 06 | 07 | 08 | 09 | 10 | 11 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Tempo | 45 min | 1 h 30 | 1 h 30 | 2 h | 1 h 30 | 1 h 30 | 1 h | 1 h 30 | 2 h | 1 h 30 | 2 h 30 |

Cerca de 17 horas no total. A parte 2 inclui preparar o cartão SD; a 9 inclui instalar a toolchain Xtensa (`espup`), que demora.

## O que fica de fora, e chega na 1.x

A trilha usa só funcionalidades da v1.0 (documento 21, §2). Ficam de fora, de propósito:

| Assunto | Versão |
|---|---|
| ML (Burn, ORT, aceleradores), streams e pipelines, sinais em bloco, escritas agendadas, intenções (escritores só-proposta) | 1.1 |
| Structured Text, SDK de drivers tier W, BACnet e EtherCAT; apps em Python | 1.2 |
| Joker Studio, modo gateway, sessões remotas pelo Deck | 1.3 |
| Wasm e ML em microcontroladores, nós de baixo consumo | 1.4 |
| Hub público com evidência certificada, auditoria da frota em escala | 1.5 |
| redundância ativo/standby, malha de nós | 2.0 |

A bancada também não prova tudo (documento 23, §8): RS-485 half-duplex, CAN, um MCU ARM real e TPM ficam declarados como não validados.

---

## No fim da trilha sabes…

Cada parte liga-se aos documentos e referências que a explicam. Quem termina a trilha e lê esta coluna da direita entende o projeto.

| Parte | Sabes… | Para ir mais fundo |
|---|---|---|
| 01 | como um projeto se organiza (manifesto comum e overlays), o que é um sinal, uma porta e uma ligação, porque toda a saída tem `safe`, o que acontece num tick, e porque um teste com relógio virtual é exato | documentos 00, 01 §3–§4, 02, 03, 04, 15; ref/01 §4–§5, §7; ref/03 §2.4–§2.5; ref/04 §4, §6; ADR 0003, 0006, 0007 |
| 02 | como um nó ganha identidade sem abrir portas à rede, o que um perfil de placa promete, como se reservam CPUs para o ciclo, o que o lock fixa, porque se aplica com confirmação, como o árbitro decide entre um humano e uma app (e como se pergunta quem manda numa saída), e o que cada camada do estado seguro cobre e não cobre | documentos 01 §2, §6, §8, §15, §17, 16 §1, 23; ref/02 §2.1–§2.3, §3.2–§3.3; ref/03 §2.3; ref/09 §2.1–§2.3; ADR 0011, 0015, 0025 |
| 03 | como a qualidade viaja de um driver até à app e às expressões, como se escrevem janelas e alarmes, o que o historian guarda, e como um programa externo lê e escreve pela API | documentos 02 §10–§11, 16 §3; ref/01 §5.14–§5.17; ref/05 (parte B); ref/09; ADR 0013 |
| 04 | como um equipamento real se descreve num perfil a partir do mapa de registos, como se testa esse perfil antes de o usar, como se conta o orçamento de um barramento, e o que é o fallback M1 | documentos 05, 23 §4; ref/03 §2.2; ref/06; ref/01 anexo A; ADR 0008 |
| 05 | como uma app tolera falhas sem mentir sobre elas, como se provocam falhas no simulador e na bancada, e como um traço gravado, com o estado das apps, reproduz um incidente e prova uma correção | documentos 01 §20, 04 §7, 16 §3; ref/03 §2.4.8, §2.5; ref/04 §6; ADR 0005 |
| 06 | como o mesmo projeto muda de placa só com overlays, como se escreve o perfil de uma placa que não existe, e como o `jokerd` se degrada numa placa fraca | documentos 01 §22, 23 §5.2, §7; ref/01 §7; ref/03 §2.3 |
| 07 | o que uma classe dá a uma app, como um perfil declara que implementa uma classe, porque o cenário de falha passa igual, e como se verifica uma classe no equipamento real | documentos 14, 17; ref/04 §4.8; ADR 0017, 0018 |
| 08 | como se cria uma abstração com contrato, conformidade e implementação, e quem é dono de cada camada | documentos 14 §6–§7, 17; ref/03 §2.6; ref/04 §4.8.4 |
| 09 | como o núcleo corre num MCU com os mesmos bytes, o que o Joker Link garante, e porque o `safe` de uma saída do MCU sobrevive à morte do `jokerd` | documentos 19, 16 A12; ref/07; ref/08; ADR 0020 |
| 10 | como se escreve lógica que corre no próprio MCU, e o que muda num link IP | documentos 19 §4–§6; ref/08 §2.11; ref/07 §3 |
| 11 | o que muda em `prod`, como se assina e se distribuem segredos sem os expor, e como uma frota se atualiza em ondas com uma paragem automática | documentos 20; ref/02 §2.4–§2.5; ref/10; ADR 0021, 0022, 0024, 0025 |
