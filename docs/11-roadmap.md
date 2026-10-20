# 11 — Roadmap

**A v1 é a runtime coringa que fala com os dispositivos, em Linux e em microcontroladores, com uma frota gerível.** O machine learning é a **1.1**, e a v1 é construída preparada para ele (ADR 0019). Os microcontroladores entram na v1 (ADR 0020), e com eles o núcleo é `no_std` desde o primeiro dia.

Quatro fases até à v1.0, com portas de saída mensuráveis e um ponto de decisão depois da Fase 1. As durações são estimativas para uma equipa de 3–4 pessoas com experiência em Rust e automação, mais uma pessoa no Deck a partir da Fase 3. A regra de ordem continua: **primeiro o que torna o sistema seguro, depois o que o torna útil, e por fim o que o torna escalável.**

```
 Fase 0 ──▶ Fase 1 ──▶ [decisão] ──▶ Fase 2 ──▶ Fase 3 ──▶ v1.0 ──▶ v1.1 (ML) ──▶ v1.x / v2
 núcleo      MVP Linux              Joker Micro   frota e
 no_std                             e interop     confiança
```

---

## Fase 0 — Núcleo determinista e portável (≈ 8 semanas)

**Objetivo:** o `joker-core` completo, **`no_std`**, sem I/O e sem alocação no tick, testado por conformidade e propriedades. A especificação de referência é o documento 16, com a representação da ref/00.

**Antes de qualquer funcionalidade**, as três tarefas que fecham as referências no código (documento 22, §8):

- **0a — compilar as referências:** o crate `joker-spec-tests` compila os blocos Rust das referências como doctests, corre `tsc` nos blocos TypeScript, valida os CDDL contra os exemplos, gera o esquema JSON do manifesto a partir dos tipos e valida contra ele todos os blocos TOML, e valida o OpenAPI. A CI falha se uma referência e o código divergirem.
- **0b — o registo de códigos:** todos os códigos das referências em `crates/joker-manifest/codes.toml` (16 §9), sem colisões.
- **0c — os vetores de teste:** os do Joker Link (ref/07 §4.8), os da ABI C (ref/08) e os casos de conformidade do núcleo passam a ficheiros em `conformance/`, corridos em Linux e num MCU.

**Hardware:** enquanto não houver outro, as Fases 0–2 validam-se na bancada do documento 23 (Raspberry Pi 3 B+, Pi Zero W, ESP32 original), com QEMU para as arquiteturas que faltam; cada porta de saída diz onde foi validada.

Depois:

- `joker-units`, `joker-expr`, `joker-core` (registo, slots, imagem, árbitro, envelope, trips, alarmes, expansão de métodos e consultas de classes, traço), todos `no_std`, com capacidades fixas e a feature `alloc` só para o Linux.
- `joker-manifest` (TOML → modelo validado, J-codes, `extends`, lock v1, admissão RT).
- `joker-drivers/sim` e `joker-test` (harness com relógio virtual).
- `conformance/` com ≥ 150 casos, um ficheiro por suite (`conformance/<área>/<suite>.conformance.toml`, esquema da ref/03 §2.6).
- CI: o núcleo compila para `x86_64`, `aarch64`, **`thumbv8m.main-none-eabihf` e `riscv32imac-unknown-none-elf`** (com a conformidade em QEMU, a bloquear) e para Xtensa (sem bloquear); a bateria de conformidade corre no ESP32 real da bancada e, quando houver placa, num RP2350 (documento 13, §5).

**Portas de saída:**

1. Os 150+ casos passam, e o proptest do árbitro e do envelope corre 10⁶ casos sem violar invariantes.
2. **Os mesmos casos dão os mesmos bytes em x86_64, num ESP32 real (bancada) e em Cortex-M33 e RISC-V em QEMU**, até haver uma placa RP2350 (documento 23, §5.1).
3. O caminho do tick não aloca (guarda no-alloc em CI; no MCU, sem alocador ligado).
4. Envelope + árbitro ≤ 300 ns por saída em x86; o custo por saída num Cortex-M33 a 150 MHz fica medido e publicado.
5. O mesmo traço dá os mesmos bytes em 1 000 execuções.

## Fase 1 — MVP em Linux (≈ 12 semanas)

**Objetivo:** o caso da rodada 1 (BESS + FV) em bancada, de ponta a ponta.

- `jokerd`: threads, classes de recursos (M8), PREEMPT_RT, supervisor, watchdog de hardware, gerações e `apply --confirm`.
- `joker-host`: wasmtime, AOT, pooling, epochs, duas instâncias por app, buffer de propostas com registos de método.
- `joker-sdk` (Rust) com os backends Wasm e nativo (este último usado pelo Joker Micro na Fase 2), `joker:runtime@0.1`.
- Drivers: `modbus-tcp`, `modbus-rtu`, `sunspec` (descoberta, M2), `gpio`, `i2c`, `sysfs` (LEDs e temperatura das placas, documento 23), `revpi`, `mqtt` (entrada). Fallback do dispositivo (M1).
- Ações (M19), grupos atómicos (M20), versões (M21), qualidade do relógio (M14), alarmes (M9), árbitro (M15), rastreio causal (M3).
- Classes (M24), parte declarativa: `jkc` (documento 17), registo, codegen Rust, instâncias implícitas e compostas, e a `std/` inicial, que é a lista do documento 14, §10.
- `joker-store` v1, `joker-link` v1 (MQTT, Sparkplug B).
- `joker-api` (ref/09), `joker-client` (Rust, ref/05) e a `jk` (os comandos das partes 1–7 da trilha, incluindo `jk profile import`/`test` e `jk board probe`/`check`/`new`).
- Placas: `rpi4`, `rpi5`, `cm4`, `cm5`, `revpi-connect-4`, `revpi-connect-5`, `x86-generic`; `rpi3b-plus`, a placa da bancada (documento 23, §7).

**Portas de saída:**

1. Piloto de bancada BESS durante 30 dias sem intervenção humana, com a matriz de falhas (documento 09, ataque 4) injetada.
2. Jitter em CM4 e CM5 com PREEMPT_RT: p99,9 ≤ 200 µs a 1 ms.
3. RSS ≤ 90 MB num site de 1 000 sinais e 3 apps.
4. **DX:** uma pessoa nova faz as partes 1–7 da trilha (módulos A e B: sinais e classes, do simulador ao Pi 3 e ao Pi Zero W da bancada do documento 23) só com a documentação, com a primeira app em simulação em < 30 min. Duas secções dessas partes usam o que só chega depois, e ficam fora desta medição: o cliente Python da parte 3, §9 (as bibliotecas cliente Python e TypeScript são da Fase 3; a API que ele usa é desta fase) e o `jk class verify` no hardware da parte 7, §9 (os testes de conformidade das classes são da Fase 2).

### Ponto de decisão (fim da Fase 1)

| Critério | Limiar |
|---|---|
| Piloto BESS | 30 dias limpos |
| RT | ≤ 2× o alvo |
| DX | as medições acima, com 3 pessoas |
| Custo de perfis | SunSpec + 2 PCS perfilados e testados em < 2 semanas |

Se o RT falhar, a promessa em Linux desce para grupos ≥ 10 ms, e as malhas rápidas passam explicitamente para o Joker Micro. Se a DX falhar, redesenha-se o SDK antes da Fase 2. Se o custo dos perfis falhar, investe-se primeiro nos importadores.

## Fase 2 — Joker Micro e interoperabilidade (≈ 12 semanas)

**Objetivo:** o mesmo núcleo em microcontroladores, os três modos de entrega, e o nó Linux a falar com o mundo industrial à volta.

- `joker-micro` sobre Embassy, com alvos de referência **RP2350** (M1), **ESP32-C6** (M2, Wi-Fi) e **STM32G4/H5** (M1/M2).
- Modos de entrega: **firmware**, **biblioteca C** (`libjoker_node.a` + `joker_node.h`) e **crate Rust** (documento 19, §4).
- Geração embutida e **geração em flash** (firmware genérico + blob assinado).
- **Joker Link** (UART/RS-485, CAN, USB, UDP e TCP; ref/07) e o driver `joker-link` no `jokerd`, com perfis gerados a partir do `HELLO`.
- Drivers do MCU: GPIO, ADC, PWM, I²C, SPI, UART, Modbus RTU mestre e **escravo**, CAN.
- OTA de MCUs (A/B, assinado) pelo Joker Link.
- No Linux: `opcua` (cliente), `can`, `canopen`, `m-bus`; pontes (M11) servidor OPC UA e servidor Modbus.
- Classes, parte ativa: instâncias virtuais, testes de conformidade das classes, simuladores de classe (a parte 8 da trilha, e o §9 da parte 7, passam a funcionar).

**Portas de saída:**

1. **Coprocessador em bancada** (documento 19, U1): RevPi ou Pi + RP2350 durante 30 dias, com o `jokerd` morto 100 vezes e as saídas sempre no `safe` em ≤ lease.
2. Um nó autónomo ESP32-C6 (U2) a controlar uma bomba de calor simulada, com geração em flash atualizada remotamente 10 vezes.
3. Um exemplo de integração em firmware ESP-IDF existente pela ABI C (U5).
4. Orçamento: núcleo + Link + Modbus RTU + 1 app + 64 sinais ≤ 160 KB de flash e ≤ 48 KB de RAM num RP2350.
5. Jitter de ciclo no RP2350 a 1 kHz: p99,9 ≤ 10 µs.
6. **DX:** as partes 8–10 da trilha (a tua classe; o ESP32 como Joker Micro, pelo Joker Link em série e por Wi-Fi) correm na bancada só com a documentação.

## Fase 3 — Frota, confiança e v1.0 (≈ 12 semanas)

**Objetivo:** gerir centenas de nós e MCUs sem visitas ao local.

- OTA da runtime Linux (A/B); OTA de MCUs orquestrado pelo nó.
- **Protocolo do agente** `agent/1` (ref/10), publicado; crates `joker-agent` e `joker-agent-proto` (`no_std`, para os MCUs autónomos).
- **Joker Deck MVP**: inventário, estado desejado a partir de Git, rollouts com ondas e paragem por KPI, saúde da frota. **Joker Hub mínimo**: registo OCI + verificações de publicação.
- Orçamentos de uplink (M12), overlays completos (M13), modo sombra **para apps** (M16).
- Ponte Zenoh/ROS 2; uplink Zenoh e OTLP.
- **SDK TypeScript** (`@joker/sdk`, `@joker/test`, `@joker/classes-<pacote>`) e bibliotecas cliente Python e TypeScript da API (ref/05). O SDK Python fica com a API fechada na ref/05 e sai na 1.2.

**Portas de saída:**

1. Piloto de campo com 50 nós Linux e 50 MCUs em 3 tipos de hardware durante 60 dias, com 2 atualizações de runtime, 4 de apps e 2 de firmware de MCUs, sem visitas.
2. Um rollout mau parado automaticamente pelo Deck (documento 20, D2) num ensaio controlado.
3. Orçamento de 50 MB/mês cumprido em LTE, com alarmes entregues em < 60 s.
4. **DX:** a parte 11 da trilha (produção, segredos, o Deck no portátil com dois nós e um rollout parado no canário) corre na bancada só com a documentação, e o §9 da parte 3 com o cliente Python.

**No fim da Fase 3: release v1.0.**

---

## v1.1 — Machine learning

- `joker-ml`, `.jkm`, backends Burn (Flex, CubeCL CPU, Vulkan, CUDA), ORT e HailoRT (documento 06).
- Streams (M4), pipelines (M5), escritas agendadas (M6), **sinais em bloco (M7)**, com aquisição de alta taxa pelo Joker Micro.
- Linhagem (M10), sombra e promoção de modelos (M16), guarda de entrada e controlador de fallback (M17), escritores só-proposta (M18), saídas não deterministas no traço (M22).
- Treino no dispositivo.
- Experiência: Burn Flex `no_std` em MCUs M2 (modelos pequenos de anomalia).
- Portas de saída: as bancadas de visão e de vibração (documento 08, rodadas 2 e 3) e as seis validações do Burn (documento 06, §12.6).

### Preparado para ML (o que a v1 já inclui)

A v1 não implementa ML, mas não obriga a mudar nada para o receber:

| Na v1 | Para quê na 1.1 |
|---|---|
| o pacote WIT declara as interfaces `ml` e `frames` como `@unstable`, fora do mundo estável: recusadas em `prod` (`J-E0003`), stubs em `dev` (ref/04 §2.9) | a forma ainda pode mudar antes de existir o ML; na 1.1 passam a `@since(1.1.0)`, e acrescentar funções é minor (M21). As escritas agendadas (M6) e as intenções (M18) levam o mesmo tratamento |
| o mundo `joker:driver` e o SDK tier W existem como `@unstable`, só para os drivers da árvore (`J0860` fora dela em `prod`; ref/06 §4) | estáveis e abertos a terceiros na **1.2**, não na 1.1 |
| a classe de recursos `ml` existe no manifesto e nos cpusets | os modelos entram sem mexer no escalonamento |
| o formato do traço reserva a secção de saídas de modelos (vazia) | o replay com ML (M22) não muda o formato |
| o lock, o agente e o Hub reservam o tipo de artefacto `joker.model.v1+jkm` | o OTA de modelos é mais um artefacto |
| o registo de sinais reserva os tipos bloco e stream | M4 e M7 não mudam o `SignalId` nem o slot |
| o envelope e o árbitro não distinguem apps de modelos | um modelo que atua é um escritor como os outros |

## v1.x e v2

O plano depois da 1.0 está no **documento 21** (e a política de versões na ADR 0022). Em resumo:

| Versão | Tema | Cabeça de cartaz |
|---|---|---|
| 1.1 | inteligência | ML (Burn, ORT, Hailo), streams, pipelines, sinais em bloco |
| 1.2 | indústria e ecossistema | Structured Text, SDK de drivers tier W estável e aberto a terceiros (instável na 1.0, ref/06 §4), SDK Python, BACnet, EtherCAT, IEC 104/61850, DLMS, EEBus |
| 1.3 | segurança e operação | modo gateway, protocolos seguros, SBOM, Joker Studio, operação remota e multi-inquilino no Deck |
| 1.4 | Micro+ | Wasm em MCUs M2 (`wasmi`), ML em MCUs, baixo consumo, LoRaWAN se houver procura |
| 1.5 (LTS) | frota em escala | Hub público, observabilidade e auditoria da frota, MCUs autónomos em escala |
| 2.0 | o site como um sistema | ativo/standby, malha de nós, mundo WIT 2, manifesto de site, Joker OS se houver procura |
| trilha própria | — | subconjunto certificável (SIL) do Joker Micro; aprendizagem federada (v2.x) |

## O que fica deliberadamente para depois

| Tema | Porquê esperar |
|---|---|
| threads Wasm partilhadas | à espera de estabilização no Component Model (candidato ao mundo WIT 2, documento 21, §8) |
| aprendizagem federada | precisa de frota real e de casos concretos (v2.x) |
| SCADA/HMI de operação | outro produto (documento 12, §12) |
