# 24 — Público e usos

Este documento diz **para quem é o Joker**, o que cada pessoa faz com ele, e o que ainda lhe falta para cada uma. É análise e planeamento, como os documentos 11 e 21: as propostas do §5 só passam a normativas quando entrarem numa referência, com ADR.

---

## 1. A tese: um núcleo, dois públicos

O Joker tem **um público profissional**, que paga e que define o desenho, e **um público generalista**, que aprende, experimenta e faz comunidade. Servem-se os dois com o **mesmo núcleo**, por esta ordem de razões:

1. **O que protege uma bateria de 1 MWh protege um relé na garagem.** Sinais tipados com unidade e qualidade, envelope, `safe`, leases, testes com relógio virtual e replay não são "coisas industriais": são o que falta às plataformas de automação para amadores.
2. **As abstrações de alto nível são a ponte.** Quando os drivers e as classes industriais existirem, quem só quer "ligar a luz quando o botão é premido" usa `std/led@1` e `std/button@1` sem saber o que é um registo Modbus. A camada 3 (documento 14, §1) é, por construção, a camada de um público mais largo.
3. **Um público não pode enfraquecer o outro.** O que o generalista precisa entra como **omissões** em `mode = "dev"`, **atalhos** na `jk` e **classes** novas; nunca como exceções às garantias do núcleo. Uma saída sem `safe` continua a ser `J0201` para toda a gente.

O público profissional vem primeiro no roadmap porque é quem paga e porque um desenho feito para ele serve o outro; o contrário não é verdade.

## 2. As pessoas

### 2.1 O comprador principal: o integrador de energia e automação

Instala e opera BESS, PV, bombas de calor e carregadores em muitos sites, com equipamento de vários fabricantes e requisitos do operador de rede. É para ele que as decisões difíceis pendem. O que compra:

- um site é **um template com overlays**, não um projeto novo (`templates/joker.bess-v2.toml`, ADR 0026);
- um perfil escrito uma vez serve **todos os sites** com esse equipamento (Hub);
- o **envelope** protege o equipamento de um bug da app;
- o **Deck** faz rollouts com canário, paragem por KPI e rollback (documento 20).

### 2.2 Quem trabalha com o Joker todos os dias

| Pessoa | O que faz | Onde trabalha | O que mais lhe importa |
|---|---|---|---|
| **Programador de controlo** | escreve a lógica (peak shaving, SG-Ready, gestão de carga) | `apps/`, classes, `jk dev`, `jk test`, cenários, `jk replay` | testar sem hardware; a app não muda quando o equipamento muda |
| **Integrador de hardware** | põe equipamento novo a falar | `*.profile.toml`, CSV → `jk profile import csv`, `jk profile test`, `*.board.toml`, `jk board probe` | um mapa de registos vira perfil numa tarde; o Hub já ter o equipamento |
| **Técnico de campo e operador** | comissiona, diagnostica, força saídas, reconhece alarmes | `jk status`, `jk read`, `jk write --ttl`, `jk alarms`, `jk doctor` | perceber o que se passa sem ler código; nada perigoso sem querer |
| **Responsável de frota (OT/DevOps)** | gere nós, chaves, versões e segredos | Deck, `fleet.toml`, `jk ca`, `jk key`, `jk secret`, `jk publish` | rollouts que param sozinhos; um Deck que não lê segredos nem escreve sinais |

Numa equipa pequena, uma pessoa faz as quatro coisas; a trilha de estudo foi desenhada para essa pessoa.

### 2.3 O canal: quem faz o ecossistema crescer

| Pessoa | Como usa | Quando fica bem servida |
|---|---|---|
| **Fabricante de equipamento (OEM)** | põe o Joker Micro dentro do produto (biblioteca C, crate ou firmware) e publica o perfil e a classe do equipamento no Hub; o produto fala Joker Link com qualquer gateway Joker | 1.0 técnica (ABI C, Joker Link, `implements`); **bloqueado pela licença** (documento 18, §6) |
| **Engenheiro de automação clássico (PLC)** | quer IEC 61131, e não Rust | **1.2** (Structured Text); até lá é o público mais mal servido |
| **Autor de drivers e de classes** | escreve drivers tier W e pacotes de classes para o Hub | 1.2 (SDK tier W estável, Hub com verificação de conformidade) |

### 2.4 O público generalista

| Pessoa | Como usa | Quando fica bem servida |
|---|---|---|
| **Maker e programador curioso** | tem um Raspberry Pi em casa e quer automatizar coisas em Rust ou em Python, com placas baratas (ESP32, RP2040/RP2350) como extensões | parcialmente na 1.0 (Rust, trilha, simulador); bem servido com as propostas do §5 |
| **Quem aprende** (estudante, formação técnica) | faz a trilha com a bancada do documento 23 | 1.0 (trilha, bancada sem compras) |
| **Engenheiro de dados** | lê o historian para análise; mais tarde treina modelos | 1.0 (cliente Python, `jk history`, exportação); ML na 1.1 |

### 2.5 Quem não é público

Funções de segurança certificadas (SIL), controlo de movimento abaixo de 1 ms, SCADA e HMI de sala de controlo, e **domótica de consumo com centenas de integrações de marcas** (é o terreno do Home Assistant; o Joker não compete nas integrações, e liga-se a ele, §5 P8). O documento 12 tem os porquês.

## 3. Como cada pessoa usa o Joker: a primeira hora e a primeira semana

| Pessoa | Primeira hora | Primeira semana |
|---|---|---|
| Integrador (site novo) | `jk new site --template bess`, `jk check`, `jk dev --env sim` | perfis do Hub, overlay do site, `jk lock --node`, `jk apply --confirm`, Deck com o canário |
| Programador de controlo | `jk app new`, cenário, `jk test` com relógio virtual | a app com classes, replay de um incidente real (`jk replay --diff`) |
| Integrador de hardware | `jk profile import csv mapa.csv`, `jk profile check` | `jk profile test --uri …` no equipamento, `implements`, publicação no Hub com relatório de conformidade |
| Técnico de campo | `jk status`, `jk read 'pcs.**' --watch` | `jk write … --ttl --reason`, `jk alarm ack`, `jk doctor`; hoje **só pela CLI** (§4) |
| Frota | `jk ca init`, `jk node enroll … --deck` | `fleet.toml`, ondas, `halt_if`, rotação de segredos |
| OEM | `jk build --target mcu:<placa> --lib`, `joker_node.h` | o produto a exportar a classe dele pelo Joker Link; perfil e classe no Hub |
| Maker | instalar o `jokerd` no Pi, `jk init`, piscar o LED `ACT` (trilha 1–2) | a placa inteira como dispositivo, um ESP32 como extensão, um script em Python (§5 P1–P3) |

## 4. O que esta leitura revela

1. **O técnico de campo não tem interface gráfica na v1.** O Studio chega na 1.3. Um técnico de campo raramente vive numa CLI, e o comprador principal sente isto no primeiro comissionamento. → P4.
2. **O catálogo do dia 1 decide a adoção.** Sem perfis prontos para os equipamentos mais comuns, o primeiro projeto de cada cliente começa por escrever perfis. → P5.
3. **Os carregadores de veículos falam OCPP**, e o OCPP não está em nenhum plano (documentos 11 e 21). Para um integrador de energia é um protocolo de primeira linha. → P6.
4. **O OEM depende da licença**, e esse canal não arranca sem ela.
5. **O maker quer Python na 1.0**, e o SDK Python de apps é da 1.2 (documento 21, §4). O cliente Python existe na 1.0, mas pela API só se escreve como escritor `manual`, com TTL. → P3.
6. **O maker quer "a placa inteira"** sem declarar linha a linha, e quer usar um Arduino ou um ESP32 como extensão sem escrever firmware. → P1 e P2.

## 5. Propostas

| # | Proposta | Serve | Versão proposta |
|---|---|---|---|
| P1 | A placa inteira como dispositivo | maker, integrador | 1.0 |
| P2 | Firmware expansor para MCUs | maker, integrador (coprocessador U1), OEM | 1.0 (Fase 2) |
| P3 | Apps externas (qualquer linguagem, pela API, como escritores declarados) | maker, engenheiro de dados | 1.0 ou 1.1 |
| P4 | Painel web só de leitura servido pelo `jokerd` | técnico de campo, maker | 1.0 |
| P5 | Catálogo do dia 1 (lista priorizada de perfis e classes) | integrador | 1.0 |
| P6 | OCPP (1.6J e 2.0.1) | integrador de energia | 1.2 ou 1.3 |
| P7 | Classes `std/` para o público generalista | maker, quem aprende | 1.x |
| P8 | Ponte para o Home Assistant (MQTT *discovery*) | maker | 1.x |

### P1 — A placa inteira como dispositivo

Hoje o perfil de placa descreve o hardware fixo (`[io.<nome>]` → `board.<nome>`, ref/03 §2.3.2), e as linhas do conector GPIO declaram-se uma a uma num dispositivo `gpio`. A proposta: **um dispositivo que é a placa inteira**.

```toml
[target]
board = "rpi3b-plus@1"

[devices.placa]
driver = "board"                       # proposta: expõe o que o perfil de placa descreve
lines  = { gpio17 = "output", gpio27 = { direction = "input", bias = "pull-up" } }
```

- O driver `board` expõe o IO fixo (LEDs, temperatura, botões da placa) e as linhas do conector que o manifesto pede, com os nomes impressos na placa (`placa.gpio17`, `placa.act_led`, `placa.cpu_temp`), usando os drivers que já existem (`gpio`, `sysfs`) por baixo.
- O perfil de placa ganha `implements` para o IO fixo: o LED `ACT` cumpre `std/led@1`, e a app escreve `io.placa.act_led.on()`.
- `jk init --board rpi3b-plus` cria este dispositivo, e `jk board show` diz que linhas estão livres.
- Não é uma classe `rpi3@1`: uma placa é um **dispositivo** (ADR 0027, "um papel, não um chip"); os papéis (LED, botão, sensor) é que são classes.

### P2 — Firmware expansor para MCUs

É a ideia do Firmata, com as garantias do Joker. Um **firmware Joker Micro genérico** por placa, que exporta pelo Joker Link **todos os pinos** do perfil da placa, sem apps nem lógica local:

- grava-se uma vez (`jk flash --expander --board esp32-devkit`);
- o nó Linux declara no **seu** manifesto o que cada pino é (`esp.gpio2 = "output"`, `esp.gpio0 = { direction = "input", bias = "pull-up" }`, `esp.t0 = "touch"`); o `jk apply` compila e envia a geração ao MCU, como na parte 9 da trilha;
- o `safe` e o lease das saídas vivem no MCU: se o Pi morrer, os pinos vão para `safe`. É o que o Firmata não faz.

Serve ao maker ("uso um ESP32 como extensão do Pi"), ao integrador (é o coprocessador U1 do documento 19, sem firmware por projeto) e ao OEM (a placa de desenvolvimento antes do produto). **Placas:** as dos alvos do Joker Micro (ESP32, RP2040/RP2350, STM32) e as placas Arduino baseadas nelas (Nano ESP32, Nano RP2040). As placas AVR clássicas (Uno com 2 KB de RAM) **não** chegam para o núcleo.

### P3 — Apps externas

Uma app que corre **fora** do `jokerd`, em qualquer linguagem, e fala pela API com o cliente (`joker_client`, ref/05 parte B), mas **declarada no manifesto como escritor**, com nível e lease:

```toml
[apps.rega]
kind   = "api"                         # proposta (não confundir com o `kind = "extern"` do Joker Micro, ref/08)
writer = { level = "control:12", lease = "5s" }
writes = ["jardim.valvula"]
reads  = ["jardim.**", "meteo.**"]
```

- Escreve pelo árbitro e pelo envelope como qualquer escritor; se o processo morrer, o lease expira e a saída vai para `safe`.
- **Não tem** o que as apps Wasm têm: sandbox, determinismo e replay exato. O `jk check` e o `jk status` dizem-no, e em `prod` exige-se uma justificação no lock.
- Dá Python (e qualquer outra linguagem) na 1.0 sem esperar pelo SDK de tier 2, e continua útil depois dele: um script de integração não precisa de ser Wasm.

### P4 — Painel web só de leitura

O `jokerd` serve em `/ui` uma página estática (sem JavaScript de terceiros) com sinais e qualidade, alarmes, estado dos dispositivos e dos links, gerações e a saúde das instâncias, **só de leitura**, pela mesma API e os mesmos papéis (`viewer`). As escritas continuam na `jk` até ao Studio da 1.3. Cobre a maior parte do que o técnico de campo precisa no comissionamento, com pouco código.

### P5 — Catálogo do dia 1

Uma lista priorizada, a publicar no Hub com relatório de conformidade antes da 1.0: o perfil SunSpec (inversores Fronius, SMA e outros que o cumprem), medidores (Janitza UMG, Eastron SDM630/SDM120, Carlo Gavazzi EM), dois ou três BESS comuns, bombas de calor com SG-Ready e Modbus, módulos de I/O Modbus genéricos, e as placas `rpi4`, `rpi5`, `rpi3b-plus`, `revpi-connect-5`, `esp32-devkit`, `rp2350-pico2`. A lista final é uma decisão comercial; o critério é "o que aparece em 80% dos sites dos primeiros clientes".

### P6 — OCPP

Os carregadores comunicam com um sistema central por OCPP (WebSocket + JSON). O Joker pode ser o **sistema central local** de um site (o carregador liga-se ao nó), o que dá gestão de carga local sem nuvem. É um driver tier N com servidor WebSocket, e a classe `std/evse@1` (já na `std/` inicial, documento 14 §10). Proposta: 1.2, com o tema "indústria e ecossistema".

### P7 — Classes `std/` para o público generalista

`std/relay@1`, `std/pwm_out@1`, `std/servo@1`, `std/rgb_led@1`, `std/humidity_sensor@1`, `std/presence@1`. São pequenas, descrevem papéis e servem também a indústria (um relé é um relé).

### P8 — Ponte para o Home Assistant

Uma ponte (M11) que publica instâncias e sinais escolhidos por MQTT com *discovery* do Home Assistant, e aceita comandos como um escritor declarado. O Joker não compete nas integrações de consumo; entra na casa como o sítio onde a lógica séria corre.

## 6. Riscos de servir os dois públicos

- **Dispersão:** cada proposta do §5 é pequena, mas juntas são uma frente. A regra da ADR 0022 continua a valer: um tema por versão, no máximo três funcionalidades de cabeça.
- **Suporte:** um público generalista traz muitas perguntas. A trilha, o simulador e mensagens de erro com código e ajuda são a primeira linha.
- **Segurança por omissão:** um maker expõe portas sem pensar. A API continua a escutar só no socket Unix por omissão, e o TCP exige sempre mTLS.
- **Imagem:** "o Joker é para amadores" afasta o comprador industrial. A comunicação separa os dois, e o produto é o mesmo.

## 7. O que fica para decidir

1. Quais das propostas P1–P8 entram, e em que versão (e as que entram na 1.0 precisam de referência e ADR).
2. A licença (bloqueia o OEM e condiciona a comunidade).
3. A lista final do catálogo do dia 1.
