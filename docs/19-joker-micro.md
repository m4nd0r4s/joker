# 19 — Joker Micro: o Joker em microcontroladores

Documento normativo (ADR 0020), com o detalhe das interfaces nas referências (ADR 0023): o Joker Link na ref/07 e a ABI C, o crate e o blob de geração na ref/08. Até aqui, "microcontroladores" era a Fase 5 e um produto à parte. Passa a fazer parte da **v1**, e isso muda uma decisão que tem de ser tomada **antes** da primeira linha de código: **o núcleo do Joker é `no_std` desde o primeiro dia.**

A ideia é a mesma de sempre, levada até ao fim: a carta que vale por qualquer outra. A mesma app, os mesmos sinais, o mesmo árbitro, o mesmo envelope e as mesmas classes, num Raspberry Pi **ou** num microcontrolador de 5 €.

---

## 1. Porque importa, e o que muda no desenho

Um microcontrolador faz bem exatamente o que o Linux faz mal:

| | Linux (`jokerd`) | Microcontrolador (Joker Micro) |
|---|---|---|
| tempo real | *soft*, ~100 µs de jitter a 1 kHz com PREEMPT_RT (documento 12, §2) | *hard*, jitter de µs, ciclos até ~10 kHz |
| amostragem | não passa de ~1 kHz por software (documento 12, §3) | temporizadores de hardware + DMA, dezenas a centenas de kS/s |
| arranque | segundos | milissegundos |
| sobreviver a um crash | watchdog + systemd, com uma janela sem controlo | watchdog de hardware; as saídas ficam no `safe` do próprio MCU |
| custo, consumo | 30–150 €, 3–10 W | 1–15 €, mW |
| capacidade | ML, historian, pontes, muitas apps | poucas apps, sem historian, sem ML (na v1) |

Por isso o Joker Micro não é um "Joker pequeno": é a **outra metade**. Três padrões de uso, todos na v1:

1. **Nó autónomo.** Um MCU com Wi-Fi ou Ethernet (ESP32-C6, STM32H5) é o nó inteiro: um controlador de bomba de calor, um contador inteligente, um sensor com lógica.
2. **Coprocessador de I/O de um nó Linux.** O Raspberry Pi pensa; o MCU (um RP2350 numa placa de I/O) segura as saídas com o seu envelope e o seu `safe`. Se o Linux morrer, o MCU mantém a segurança. É a resposta ao problema da parte 2 da trilha (o LED `ACT` do Pi não tem fallback: com o `jokerd` morto, fica como estava), e a trilha resolve-o na parte 9, com o `safe` do LED no ESP32.
3. **Biblioteca dentro de firmware existente.** Um fabricante que já tem firmware em C (Zephyr, ESP-IDF, FreeRTOS, bare-metal) integra o Joker como uma biblioteca: o firmware continua a ser dono do hardware, e o Joker dá-lhe sinais, envelope, árbitro, classes e ligação ao resto da frota.

---

## 2. Arquitetura: um núcleo, dois hospedeiros

```
                        ┌──────────────────────── partilhado (no_std, sem alocação no tick) ───────────┐
                        │ joker-units · joker-expr · joker-core (registo, slots, imagem, árbitro,       │
                        │ envelope, alarmes, consultas e métodos de classes, traço)  · joker-link-proto │
                        └──────────────────────────────────────────────────────────────────────────────┘
               ┌────────────────────────────┐                        ┌──────────────────────────────────┐
  Linux        │ jokerd                      │        MCU             │ joker-micro                      │
               │ tokio · wasmtime · drivers  │                        │ Embassy · embedded-hal · drivers │
               │ historian · pontes · API    │ ◀── Joker Link ──────▶ │ apps nativas · flash · watchdog  │
               └────────────────────────────┘                        └──────────────────────────────────┘
```

**A decisão estrutural (ADR 0020):** o núcleo (`joker-units`, `joker-expr`, `joker-core` e os restantes crates da lista do documento 13, §1) é `#![no_std]`. Nenhuma estrutura do caminho do tick aloca, e todas têm capacidade fixa conhecida no build. O `alloc` é uma feature opcional, usada só no Linux para construir o registo a partir do manifesto em runtime. Isto custa pouco se for feito já, e é muito caro de fazer depois (documento 18, adenda).

---

## 3. O que corre num MCU

| Peça | No Joker Micro (v1) | Nota |
|---|---|---|
| sinais, qualidade, tempo, unidades | sim | o mesmo núcleo |
| imagem de processo, grupos de ciclo | sim | cada grupo é um executor de interrupção do Embassy, com prioridade de hardware |
| árbitro, envelope, trips, alarmes | sim | idênticos, bit a bit, aos do Linux (a mesma bateria de conformidade) |
| classes e instâncias | sim | tipos gerados pelo `jkc` em `no_std`; métodos e consultas executados pelo mesmo núcleo |
| apps | **nativas**, em Rust | a mesma API do SDK; sem Wasm na v1 (§5) |
| tarefas async | parcial | `every`, `on`, `on_event`; sem `history`, sem `infer`, sem rede arbitrária |
| `kv` | sim, em flash com wear-leveling | pequeno (≤ 16 KiB) |
| alocação | **não** no núcleo, nos drivers nem nas apps | exceção declarada: o heap de tamanho fixo do rádio do ESP32 (`esp-alloc` e `esp-rtos`), só no firmware com Wi-Fi, fora do núcleo e contado no `J0710` (ref/08 §2.15.2) |
| drivers | GPIO, ADC, PWM, I²C, SPI, UART, Modbus RTU **mestre e escravo**, CAN, 1-Wire | sobre `embedded-hal` 1.0 onde há trait; o ADC e o CAN usam dois traits próprios do `joker-hal` (`AdcRead`, `CanFrames`), e o 1-Wire um pino *open-drain*. A tabela é a da ref/06 §3.7 |
| historian | não | só um anel de amostras em RAM, enviado pelo Joker Link |
| uplink | Joker Link; MQTT direto em MCUs com rede | sem Sparkplug completo na v1 |
| traço | anel em RAM (tamanho configurável), despejado pelo link ou por sonda | o mesmo formato (documento 16, §7) |
| OTA | sim, A/B com bootloader (embassy-boot ou MCUboot), imagens assinadas; o recetor é **obrigatório** no modo firmware e no modo crate (ref/08 §2.12.6, `J0795`): um MCU no campo recebe firmware novo sem visita | pelo Joker Link, ou pelo Deck em MCUs com rede |
| ML | **não na v1** | Burn Flex `no_std` avaliado na 1.1 |

### 3.1 Alvos da v1

| Classe | Exemplos | RAM | Uso típico |
|---|---|---|---|
| **M1** (referência) | RP2350 (2× Cortex-M33, 520 KB), STM32G4 | 128–520 KB | coprocessador de I/O, controlador pequeno |
| **M2** (com rede) | ESP32-C6 / ESP32-S3, STM32H5/H7 | 320 KB–1 MB | nó autónomo com Wi-Fi ou Ethernet |
| fora de âmbito | < 128 KB de RAM, Cortex-M0 sem FPU a < 64 MHz | — | o núcleo cabe, mas não sobra espaço útil para apps |

A FPU conta: o ESP32-C6 **não tem FPU**, e toda a vírgula flutuante é por software (aviso `J0780` para `f32`, `J0721` para `f64`); o RP2350 tem precisão simples com um coprocessador de dupla. A placa declara-a (`fpu`, ref/03 §2.3), e a tabela completa dos alvos está na ref/08.

Orçamento alvo (a medir na fase do MCU): núcleo + Joker Link + Modbus RTU + 1 app + 64 sinais ≤ **160 KB de flash** e ≤ **48 KB de RAM**.

---

## 4. Do projeto ao microcontrolador: `jk build --target mcu:…`

O mesmo projeto, o mesmo `jk check`. O que muda é o que o build produz. Um MCU não interpreta TOML: o manifesto é **compilado**.

```
joker.toml + joker.lock + classes (.jkc) + apps (Rust)
   │  jk check            (as mesmas regras, mais os limites do MCU: J07xx)
   ▼
geração compilada
   ├─ tabelas: registo de sinais, envelopes, árbitros, bytecode das expressões e consultas, instâncias
   ├─ apps ligadas estaticamente (SDK com o backend nativo)
   └─ drivers escolhidos por features
   │
   ├─▶ modo firmware:   firmware completo (ELF/UF2/bin)          → jk flash (probe-rs) ou OTA
   ├─▶ modo biblioteca: libjoker_node.a + joker_project.h (ABI C) → para firmware em C existente
   └─▶ modo crate:      crate Rust para projetos Embassy          → para firmware em Rust existente
```

### 4.1 Firmware genérico + geração em flash

Compilar uma imagem por site não escala (3 000 sites seriam 3 000 firmwares; documento 08, rodada 5). Por isso há duas formas de levar uma geração:

- **geração embutida:** tabelas `const` dentro do firmware. É a mais simples e o melhor para protótipos e produtos fechados;
- **geração em flash:** um firmware **genérico** por placa e conjunto de apps, e a geração do site num **blob binário assinado** (`.jkgen`, assinado pelo papel `config`, com CRC-32C) numa partição própria, carregado no arranque. Mudar a configuração de um site é enviar um blob de alguns KB, e não um firmware novo. O formato está na ref/08.

Os dois usam as mesmas estruturas de capacidade fixa. A capacidade máxima (sinais, saídas, grupos, instâncias) é fixada no build do firmware genérico, e o `jk check` recusa uma geração que não caiba (`J0710`).

### 4.2 Modo biblioteca: a ABI C

Para o terceiro padrão do §1. O firmware existente continua a mandar no hardware e no escalonamento; o Joker é chamado:

```c
#include "joker_project.h"      /* gerado pelo jk build; inclui joker_node.h, o header fixo */

static jk_node_t node;           /* estado estático, sem malloc */

uint64_t jk_now_us(void) { return board_timer_us(); }    /* o tempo vem de fora (ADR 0007) */

JK_NORETURN void jk_fatal(jk_status_t code, uint32_t detail) {
    (void)code; (void)detail;
    for (;;) { esp_restart(); }  /* sem sandbox: reset para o estado de reset do hardware */
}

static const jk_platform_t platform = {                  /* flash: persistência e gerações */
    .area_info = flash_info, .area_read = flash_read,
    .area_write = flash_write, .area_erase = flash_erase,
};

void app_main(void) {
    jk_status_t st = jk_init(&node, &JK_GENERATION, &platform, jk_now_us());
    if (st != JK_OK) { jk_fatal(st, 0); }                  /* ABI, interface ou geração errada */
    TickType_t last = xTaskGetTickCount();
    for (;;) {
        uint64_t now = jk_now_us();
        jk_set_input(&node, JK_SIG_TANK_LEVEL, read_level_m(), JK_GOOD, now);
        jk_tick(&node, JK_GROUP_MAIN, now);                /* latch → apps → árbitro → envelope → commit */
        bool pump; jk_quality_t q;
        if (jk_get_output(&node, JK_SIG_PUMP_RUN, &pump, &q) == JK_OK) {
            gpio_set_level(PUMP_PIN, pump);                /* o valor é sempre admissível */
        }
        jk_link_poll(&node, uart_rx, uart_tx);             /* link + um passo de serviço (persistência, async) */
        vTaskDelayUntil(&last, pdMS_TO_TICKS(10));
    }
}
```

- **Dois headers:** `joker_node.h` é fixo e versionado (`JK_ABI_VERSION`, política da ADR 0022); `joker_project.h` é gerado pelo `jk build --target mcu:<placa> --lib`, com os identificadores (`JK_SIG_…`, `JK_GROUP_…`, `JK_WRITER_…`). Os identificadores são índices da **interface externa** do build, ligados **por nome** à geração carregada no `jk_init`: continuam válidos quando chega uma geração nova por flash. O `jk_init` é uma macro que passa o tamanho do nó, a versão da ABI e o hash da interface, e um header regenerado com a biblioteca antiga é recusado (`JK_E_ABI`) antes de ler qualquer entrada.
- Sem alocação, sem threads próprias, sem estado global escondido; reentrante por `jk_node_t`. O integrador define dois símbolos: `jk_now_us` e `jk_fatal`.
- **Um *panic* de app é fatal.** No MCU não há sandbox: uma app ligada estaticamente que entra em *panic* chama `jk_fatal`, que não volta (reset). As saídas ficam no estado de reset do hardware, que a placa tem de declarar seguro (`reset_safe`, `J0786`). O mesmo vale para uma invariante interna partida.
- **Código C que escreve em saídas** é uma app `kind = "extern"` no manifesto, com `writes` e `reads`: propõe com o seu escritor (`jk_propose_bool(&node, JK_WRITER_PANEL, …)`), com nível e lease, e passa pelo árbitro e pelo envelope como as outras.
- **Persistência:** um trip, um reconhecimento ou um parâmetro alterado são gravados em flash no próximo `jk_service` (ou `jk_link_poll`), e o integrador chama-o pelo menos a cada `service_deadline` (100 ms por omissão). É a diferença declarada face ao `fsync` do Linux: uma falta de energia dentro dessa janela perde o trip, e a saída arranca em `safe`.
- **Watchdog:** `jk_supervise` numa ISR de temporizador força o `safe` de um grupo parado a 3 períodos e diz quando se pode alimentar o watchdog de hardware.

A ABI completa está na ref/08 e em `spec/c/joker_node.h`; um exemplo que compila está em `spec/c/example/`.

---

## 5. Apps: a mesma app, dois alvos

```rust
use joker::prelude::*;

#[joker::app(name = "level", version = "0.2.0")]
pub struct Level {
    #[input(unit = "m")]   level: Input<f32>,
    #[output]              pump:  Output<bool>,
    #[param(unit = "m", default = 1.5)] target: Param<f32>,
}

#[joker::cycle(period = "10ms")]
fn control(io: &mut CycleIo<Level>, _st: &mut ()) -> Result<()> {
    io.pump.set(io.level.get()? > io.target.get());
    Ok(())
}
```

- No Linux, compila para Wasm e corre no wasmtime. No MCU, o **mesmo código** compila para `thumbv8m.main-none-eabihf` com o backend nativo do SDK (contrato `NativeApp`, ref/08 §2.11.5) e é ligado ao firmware. Sem sandbox, um *panic* é `jk_fatal` (§4.2).
- O SDK marca em compilação o que não existe no MCU (`history`, `infer`, `frames`, rede arbitrária), e o `jk build --target mcu` falha com `J0720` e uma sugestão.
- **Números:** muitos MCUs só têm FPU de precisão simples, e alguns nenhuma. As portas aceitam `f32`, e o `jk bench --target mcu` (por sonda) mede o WCET no próprio chip. O `jk build` procura no objeto de cada app as rotinas de vírgula flutuante por software: `f64` num MCU sem FPU de dupla precisão dá `J0721`; `f32` num MCU sem FPU (ESP32-C6) dá `J0780`.
- **Wasm no MCU** (com o interpretador `wasmi`, que é `no_std`, determinista e com medição de *fuel*) fica para a 1.4, em MCUs M2: dá troca de apps sem reflash, com um custo de desempenho grande. Não é preciso na v1.

---

## 6. Joker Link

O protocolo entre um Joker Micro e um `jokerd` (ou o Deck, documento 20). A referência ao nível do fio é a **ref/07**, com o CDDL em `spec/cddl/joker-link.cddl` e vetores de teste.

| | |
|---|---|
| transportes | UART, RS-485 (full e half-duplex), USB CDC e TCP, com COBS + CRC-32C; CAN/CAN-FD (ISO-TP) e UDP, um datagrama por trama. O **CRC-32C** está em todos, também em TCP e UDP |
| codificação | CBOR com chaves inteiras, e identificadores numéricos resolvidos pelo catálogo anunciado no `HELLO` |
| segurança | chave por nó (do `jk node enroll`), ChaCha20-Poly1305. Configura-se **nos dois lados, sem negociação** (não há *downgrade*): obrigatória em TCP e UDP, opcional em barramentos fechados dentro de um quadro |
| entrega | três classes: efémera (`PROPOSE`, `TIME`), de estado (`SAMPLES`: uma retransmissão leva o valor **atual**, nunca o antigo) e fiável (ações, métodos, eventos, alarmes, OTA) |

As mensagens principais (a lista completa, com os campos e os papéis, está na ref/07 §6.1):

| Mensagem | Sentido | Conteúdo |
|---|---|---|
| `HELLO` | ↔ | versão do protocolo, identidade, hash da geração e do catálogo, descritor do firmware, contratos das classes; a resposta decide o modo da sessão |
| `SAMPLES` | MCU → Linux | amostras alteradas (seq, qualidade, `dt`), em lote |
| `PROPOSE` | Linux → MCU | propostas nas saídas do MCU, com nível do árbitro e lease |
| `OUTCOMES` | MCU → Linux | resultados das escritas (o envelope que decide é o do MCU) |
| `CALL` / `ACTION` / `RESULT` | Linux → MCU / MCU → Linux | métodos e ações de classes, com a chave de idempotência |
| `EVENTS`, `ALARMS` | MCU → Linux | eventos e transições de alarme |
| `TIME` | ↔ | sincronização em três fases (sonda, eco, acerto), com a qualidade do relógio; sem ela, o MCU fica em `Holdover` |
| `CATALOG`, `TRACE`, `OTA` | ↔ | catálogo por intervalos, anel de traço, firmware ou blob de geração assinados (com teste, confirmação e reversão) |

**Modos da sessão** (`session_mode`, ref/00 §6), decididos pelo nó Linux no `HELLO`: `full` (tudo); `observe` (o MCU não corresponde ao lock ou à geração: o nó lê, mas não escreve em `prod`, e dispara o alarme de deriva); `maintenance` (MCU sem geração válida: só OTA, traço, tempo, reinício, catálogo e rotação de chaves); `refused` (sem versão comum, ou identidade errada). O estado do link que cada modo dá ao dispositivo está no documento 05, §12.

**Como o `jokerd` vê um MCU:** como um **dispositivo** com o driver `joker-link`. O perfil é **gerado a partir do catálogo** que o MCU anuncia (o hash vai no `HELLO`, os bytes pelo `CATALOG`, e o lock fixa o hash): os sinais, as ações e as classes que o MCU exporta aparecem como sinais e instâncias implícitas do nó Linux. Nada se escreve à mão.

**Quem manda nas saídas do MCU:** o **MCU**. O `jokerd` é um escritor como os outros, com nível e lease. Se o link cair, o lease expira **no MCU** e o envelope do MCU aplica o `safe`. O fallback do dispositivo (M1) deixa de ser uma coisa a verificar e passa a ser o próprio desenho.

---

## 7. Estresse: dez casos

**U1 — Coprocessador de I/O de um Raspberry Pi.** Um RP2350 com 16 DI e 16 DO, ligado por UART a 1 Mbaud. Uma trama cifrada com 32 sinais alterados no mesmo tick tem ~250 B, ~2,5 ms no fio; com as alterações espalhadas pelo período, ~3,5 ms (ref/07 §11.2). Para ciclos de 10 ms no Linux, chega, e o UART é full-duplex. **Não cabe em CAN clássico** a 500 kbit/s (150% do barramento, `J0732`); em CAN-FD cabe com folga. Mata-se o `jokerd`: em ≤ lease + um período (40 ms) as saídas vão para `safe` pelo MCU, e o LED sem fallback da parte 2 da trilha deixa de ser um problema (a trilha mostra-o na parte 9, com o ESP32). **Aguenta.**

**U2 — Controlador autónomo de bomba de calor (ESP32-C6).** Modbus RTU para a bomba, dois relés SG-Ready, Modbus **escravo** para o SGTC do edifício e MQTT por Wi-Fi para o Deck. Sem Linux. **Achado:** 3 000 sites exigiam 3 000 firmwares. **Correção:** firmware genérico com a geração em flash (§4.1).

**U3 — Vibração a 25,6 kS/s num STM32H7.** Um ADC por DMA gera blocos de 2 560 amostras, e o MCU calcula RMS e bandas e envia blocos e features por Ethernet para o `jokerd`. Resolve o limite da amostragem por software (documento 12, §3). **Aguenta**, com os sinais em bloco (M7) movidos para a 1.1 junto com o ML (documento 11).

**U4 — Malha de corrente de um motor a 20 kHz.** É FOC, e pertence ao firmware do drive, e não ao Joker. O Joker Micro faz ciclos até ~10 kHz com lógica simples num Cortex-M33 a 150 MHz (a medir). **Fronteira declarada** (documento 12).

**U5 — Integração em firmware ESP-IDF existente.** O fabricante chama `jk_tick` na sua própria tarefa FreeRTOS. **Achados:** a ABI tinha de ser sem alocação, reentrante por nó e com IDs gerados, o que está garantido no §4.2; os IDs são da interface externa, e não da geração, para sobreviverem a uma geração nova em flash. O tempo tem de vir de fora (`jk_now_us()`), o que é o ADR 0007 aplicado ao pé da letra. **Aguenta.**

**U6 — A mesma app no Linux e no MCU.** A app usa `history()` para uma média de 24 h. No MCU dá `J0720`, com a sugestão: "uma janela do núcleo (`avg(x, 15m)`) ou mover o cálculo para o nó Linux". A app faz contas em `f64` num RP2350: `J0721`, e o `jk bench --target mcu` mostra o custo real. **Aguenta, com erros claros.**

**U7 — Classes através do link.** O MCU exporta uma instância `temp_sensor@1`, e o `jokerd` recebe-a como instância implícita. Se o contrato no MCU for `temp_sensor@1.1.0` e o do nó Linux `1.0.0`, o hash difere mas a major é a mesma: aceite. Se a major diferir: `J0630` no nó Linux e alarme de deriva. Uma instância **composta** (a bomba de calor do U2 como `heat_pump@1`, com compressor, sondas e SG-Ready) chega com a árvore de membros e os mesmos caminhos aninhados; o nó vê só a instância de topo, e a renomeação de `instances` aplica-se a esse nome (ref/07 §6.3 e §8.3; o exemplo completo está no documento 14, §9.3). **Aguenta.**

**U8 — Memória esgotada.** 300 sinais num RP2040 (264 KB). O `jk check` calcula RAM e flash a partir da geração e das capacidades do firmware, antes de gravar: `J0710` com o detalhe por componente. **Aguenta.**

**U9 — Depurar uma falha em campo.** O anel de traço do MCU (16 KB ≈ alguns segundos) é congelado num trip e despejado pelo link, ou lido por sonda. O `jk replay` no portátil corre o **mesmo núcleo** e reproduz o que o MCU decidiu. **Aguenta**, e é um dos maiores ganhos de ter um só núcleo.

**U10 — Segurança num RS-485 dentro de um quadro contra Wi-Fi.** Num barramento fechado, a autenticação pode ser dispensada (declarado nos dois lados, no manifesto e no blob do MCU, e visível no `jk doctor`); como não há negociação, um atacante não consegue baixar a segurança de um link que a exige. Em IP é obrigatória, e o enrolamento dá a cada MCU a sua chave. **Aguenta.**

---

## 8. O que isto pede ao resto da documentação

| Documento | Alteração |
|---|---|
| AGENTS.md | invariante: os crates da lista `no_std` do documento 13, §1, são `no_std`, sem alocação no tick, com capacidades fixas |
| 01 §22 | microcontroladores passam a tier 1 (M1) e tier 2 (M2) |
| 11 | uma fase própria para o Joker Micro na v1; o ML passa para a 1.1 |
| 12 | os limites §2, §3 e §8 passam a ter resposta |
| 13 | a Fase 0 compila o núcleo para `thumbv8m.main-none-eabihf` em CI e corre a conformidade também em hardware (RP2350) |
| 16 | a conformidade é a mesma nos dois hospedeiros: o mesmo caso tem de dar os mesmos bytes |
| códigos | `J0710` (geração não cabe no MCU), `J0720` (API indisponível no MCU), `J0721` (aviso: `f64` num MCU sem FPU de dupla precisão), `J0780` (aviso: `f32` num MCU sem FPU); os restantes das gamas `J073x`–`J079x` e `J-E2000`–`J-E2999` estão na ref/07 e na ref/08 |
| ref/07, ref/08 | a forma fechada do §4.2 e do §6: ABI C, crate, blob `.jkgen`, descritor de firmware `.jkfw`, e o Joker Link ao nível do fio |
