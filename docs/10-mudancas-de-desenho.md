# 10 — Mudanças de desenho

As 26 primitivas que saíram das rodadas de estresse (documento 08) e dos ataques à arquitetura (documento 09), e os refinamentos que não chegaram a ser primitivas. Todas já estão incorporadas nos documentos 01–07, e a coluna **Referência** diz onde cada uma está especificada ao detalhe em `docs/ref/` (ADR 0023). Este documento é o índice e a justificação curta de cada uma.

Regra usada para decidir entre primitiva e "fica na app": **uma primitiva entra no núcleo se, sem ela, a garantia depende de disciplina de quem escreve a app.** Se é só conveniência, fica em biblioteca ou na app. Foi por isso que a alocação de potência entre inversores (08, rodada 1d) ficou de fora.

---

## 1. Primitivas

| # | Primitiva | Origem | O que resolve | Onde está | Fase | Referência |
|---|---|---|---|---|---|---|
| **M1** | Fallback do dispositivo declarado e verificado | 08·R1a | o estado seguro do equipamento quando o Joker não está lá | 01 §8, 05 §6 | 1 | ref/01 §5.8, ref/02 §2.1.12, ref/06 §2.10 |
| **M2** | Descoberta com resultado no `joker.lock` e deteção de deriva | 08·R1b | mapas de registos que mudam com o firmware | 03 §14, 05 §7 | 1 | ref/02 §2.1.11, §3.1.3–§3.1.5; ref/06 §2.11 |
| **M3** | Rastreio causal das escritas | 08·R1c | provar que uma decisão resultou de certas entradas | 01 §4, 07 §4 | 1 | ref/09 §5.7 |
| **M4** | Streams com buffers partilhados | 08·R2a | dados volumosos (imagem, nuvens de pontos) | 01 §11 | 1.1 | ref/01 §6.2, ref/06 §5.6 |
| **M5** | Pipelines nativos | 08·R2b | levar dados volumosos ao acelerador sem passar pelo Wasm | 06 §5 | 1.1 | ref/01 §6.3 |
| **M6** | Escritas agendadas (`write_at`, `on_late`) | 08·R2c | atuação precisa a partir de uma decisão assíncrona | 02 §7.2 | 1.1 | ref/04 §2.3 (`write-at`, `@unstable`) |
| **M7** | Sinais em bloco e gravação por gatilho | 08·R3a,c | formas de onda de alta taxa | 01 §11, 03 §11 | 1.1 | ref/01 §6 |
| **M8** | Classes de recursos | 08·R3d | treino e ML a disputar CPU com o controlo | 01 §2, 03 §3 | 1 | ref/01 §5.4 |
| **M9** | Alarmes de primeira classe | 08·R3f | alarmes que piscam e acabam desligados | 02 §11 | 1 | ref/01 §5.15, ref/09 §5.8 |
| **M10** | Linhagem de modelos | 08·R3e | "que modelo, treinado com quê?" | 06 §11 | 1.1 | ref/02 §2.1.9 (reservado) |
| **M11** | Pontes declarativas (ROS 2, OPC UA/Modbus servidor) | 08·R4b | interoperabilidade sem atalhos ao envelope | 01 §14, 03 §12 | 2 | ref/01 §5.18 |
| **M12** | Orçamentos de uplink | 08·R5b | LTE com dados contados | 01 §13 | 3 | ref/01 §5.17, ref/10 §3.11 |
| **M13** | Sobreposições de manifesto (`extends`) | 08·R5c | milhares de sites sem deriva de configuração | 03 §2 | 3 | ref/01 §7 |
| **M14** | Qualidade do relógio | 08·R5d | arrancar em 1970 sem RTC | 01 §18 | 1 | ref/00 §3, ref/01 §5.5 |
| **M15** | Árbitro com 16 níveis, leases e TTL manual | 08·R6a | "o operador mexeu e o modelo desfez" | 01 §6 | 1 | ref/00 §5, ref/09 §5.6 |
| **M16** | Modo sombra e promoção | 08·R6b | pôr em produção sem fé | 06 §10, 03 §8 | 3 (apps) · 1.1 (modelos) | ref/09 §5.14 |
| **M17** | Guarda de entrada e controlador de fallback | 08·R6c | modelos fora da distribuição | 06 §7 | 1.1 | ref/01 §6.1 (reservado) |
| **M18** | Escritores só-proposta (intenções com aprovação) | 08·R6d | LLMs e sistemas sem confiança | 01 §6.1 | 1.1 | ref/09 §5.9, ref/04 §2.4 (`propose`) |
| **M19** | Ações (comandos com resultado) | 09·A1 | nem tudo é pôr um valor | 02 §8 | 1 | ref/04 §4.9, ref/07 §6.15, ref/09 §5.10 |
| **M20** | Grupos atómicos e ordem de escrita | 09·A1, 08·R4a | escritas multi-registo coerentes; rodas no mesmo frame | 02 §7.3 | 1 | ref/01 §5.16 |
| **M21** | Política de versões (WIT, manifesto, lock, `.jkm`) | 09·A8 | frotas com versões misturadas | 09, ataque 8 | 1 | ref/00 §1, ref/04 §2.8, ref/10 §3.14 |
| **M22** | Saídas não deterministas gravadas; replay de async | 09·A10 | replay exato com ML em GPU | 06 §8 | 1.1 | ref/04 §2.7 |
| **M23** | Admissão de tempo real por WCET | 09·A2,A5 | grupos impossíveis descobertos em campo | 01 §4 | 1 | ref/01 §5.11, ref/02 §2.1.13 |
| **M25** | Núcleo portável `no_std` e Joker Micro (firmware, biblioteca C, crate) | revisão de âmbito | o mesmo núcleo em Linux e em MCUs; tempo real duro; amostragem por hardware | 19 | 0 (núcleo) · 2 (MCU) | ref/08 |
| **M26** | Joker Link e perfis gerados a partir do `HELLO` | revisão de âmbito | MCUs como dispositivos de um nó Linux, com o `safe` garantido no MCU | 19 §6 | 2 | ref/07 |
| **M24** | Classes de equipamento e instâncias (implícitas, compostas, virtuais), escritas em `.jkc` | 14, 17 | lógica de negócio em termos do domínio; apps grandes organizadas por equipamento | 14, 17 | 1–2 | ref/03 §2.6–§2.7, ref/09 §5.11 |

---

## 2. Refinamentos (sem primitiva nova)

| # | Refinamento | Origem | Onde está |
|---|---|---|---|
| R1 | `on-cycle` com imagem empacotada (uma chamada por tarefa) | 09·A2 | 01 §9, 04 §2, ref/04 §3 |
| R2 | Configuração do wasmtime para 32 bits (sem reservas de 4 GiB) | 09·A3 | 01 §9 |
| R3 | `oom_score_adj` diferenciado | 09·A3 | 01 §17 |
| R4 | Caixas latest-wins para os drivers + bit `force_safe` do supervisor | 09·A4 | 01 §3.4 |
| R5 | `stuck_detect` e qualidade `Frozen` | 09·A4 | 02 §4 |
| R6 | `J0351`: < 10 ms exige PREEMPT_RT; `jk doctor --rt` com `cyclictest`/`hwlatdetect` | 09·A5 | 07 §3 |
| R7 | Guarda no-alloc no caminho do tick, em CI | 09·A5 | AGENTS §2 |
| R8 | Orçamento de atuações (`max_actuations`) | 09·A9 | 01 §7 |
| R9 | Relaxed SIMD desativado e NaN canonicalizados no ciclo | 09·A10 | 01 §9 |
| R10 | Regra de cliente único + pontes para SCADA | 09·A11 | 12 §5 |
| R11 | `safe = "relinquish"` | 08·R6 | 03 §6.1 |
| R12 | `on_late` obrigatório e sem valor por omissão | 08·R2c | 02 §7.2 |
| R13 | `correlate` em pipelines (trigger ↔ frame) | 08·R2d | 08 R2 |
| R14 | Taxa proporcional em grupos atómicos | 08·R4a | 02 §7.3 |
| R15 | `param` com `poll = "on_demand"` | 09·A1 | 05 §3 |
| R16 | `J0380`: memória estimada do site | 08·R5g | 03 §15 |

---

## 3. Decisões rejeitadas (e porquê)

| Proposta | Porque ficou de fora |
|---|---|
| Saídas agrupadas com alocação (distribuir 30% por N inversores) | é política, não mecanismo; pertence à app |
| Um valor de *relinquish default* separado do `safe` | seria uma segunda verdade sobre o que é seguro |
| Uma instância Wasm por app com mutex | inversão de prioridade no caminho RT (09·A6) |
| Plugins nativos Rust dinâmicos para drivers | Rust sem ABI estável; terceiros usam W ou X (09·A7) |
| Funções de utilizador nas expressões | a linguagem fechada é o que torna o custo e a análise previsíveis |
| `struct` como tipo de sinal de dispositivo | complica slots e historian; os prefixos + coerência de aquisição chegam |
| Servidor de frota dentro do Joker | é outro produto; o Joker é o agente (08·R5f) |
| Compilar para NPUs de fabricante | compiladores proprietários e offline (08·R2e) |
| Amostragem por software acima de ~1 kHz | o Linux não o garante; exige ADC com relógio próprio (08·R3b) |
