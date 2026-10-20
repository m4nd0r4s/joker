# 13 — Plano de implementação

Como sair do papel: o workspace, as dependências entre crates, as frentes de trabalho e as primeiras tarefas da Fase 0 por ordem.

---

## 1. Workspace

```
Cargo.toml                 workspace; edition 2024; MSRV fixado em rust-toolchain.toml (um por família de alvo, §5.1)
crates/
  joker-units/             unidades, dimensões, grandezas, parsing de "125 kW"      (sem deps)
  joker-core/              registo, slots, imagem, árbitro, envelope, alarmes, traço (← units, expr)
  joker-expr/              linguagem de expressões → bytecode                       (← units)
  joker-manifest/          joker.toml, joker.lock, overlays, J-codes, codes.toml, admissão RT  (← core, units, expr)
  joker-hal/               traits Planner, Driver e RtPart, planos, transportes, Cx (ref/06 §3)  (← core)
  joker-drivers/sim/       planta simulada                                          (← hal)
  joker-drivers/modbus/    tcp + rtu + sunspec                                      (← hal)
  joker-drivers/gpio/      chardev v2                                               (← hal)
  joker-drivers/revpi/     piControl                                                (← hal)
  joker-drivers/mqtt/      entrada MQTT                                             (← hal)
  joker-driver-sdk/        SDK de drivers tier W em Rust (export_driver!)           (← hal)
  joker-jdp/               JDP: protocolo, anéis, cliente e SDK de drivers tier X   (← hal)
  joker-drvtest/           suite de conformidade de drivers e duplos de equipamento (← hal)
  joker-host/              wasmtime, mundo WIT, capacidades, AOT                    (← core)
  joker-sdk/               SDK Rust (guest), macros em joker-sdk-macros             (← WIT)
  joker-test/              harness com relógio virtual                              (← core, host, sim)
  joker-store/             historian e cursores                                     (← core)
  joker-link/              MQTT/Sparkplug, depois Zenoh e OTLP                      (← core, store)
  joker-api/               HTTP + WebSocket (ref/09)                                (← core)
  joker-client/            biblioteca cliente Rust da API; a jk usa-a (ref/05)      (← spec/openapi)
  joker-agent/             agente nó ↔ Deck, agent/1 (ref/10)                       (← agent-proto, core)  (Fase 3)
  joker-agent-proto/       mensagens do agente em CBOR, no_std, partilhadas com o Micro e o Deck  (Fase 3)
  joker-micro/             runtime para MCUs (Embassy): hospedeiro no_std do núcleo, modos firmware/lib C/crate; módulo ffi com a ABI C  (Fase 2)
  joker-link-proto/        protocolo Joker Link, no_std, partilhado por jokerd e joker-micro  (Fase 2)
  joker-jkc/               compilador de classes .jkc (parser, verificação, forma canónica, codegen)  (← expr, units)
  joker-ml/ joker-ml-burn/ joker-ml-ort/ joker-ml-hailo/                            (1.1)
  joker-spec-tests/        tarefa 0a: compila e valida as referências contra o código (documento 22, §8)
  jokerd/                  binário do daemon
  joker-cli/               crate da CLI; o binário chama-se `jk` (o nome `jk` está ocupado no crates.io)
wit/                       joker-runtime.wit, joker-driver.wit, joker-sim.wit
spec/                      cddl/, openapi/, c/: os ficheiros máquina-legíveis das referências (ADR 0023)
conformance/               casos em dados (TOML), um ficheiro por suite: conformance/<área>/<suite>.conformance.toml
xtask/                     rt-bench, conformance runner, release
```

**Regra `no_std`** (ADR 0020; **esta é a lista normativa**, e a ADR 0020, o documento 19 e o AGENTS.md remetem para ela): `joker-units`, `joker-expr`, `joker-core`, `joker-link-proto` e `joker-agent-proto` são `#![no_std]`, sem alocação no caminho do tick; o CI compila-os para `thumbv8m.main-none-eabihf` e `riscv32imac-unknown-none-elf` em cada commit (e para Xtensa, sem bloquear; §5). `joker-hal`, `joker-driver-sdk` e `joker-micro` também são `no_std`, com `alloc` e `std` como features (ref/06 §3.1, ref/08 §2.11). A sincronização do núcleo é só por `critical-section` e `portable-atomic` (ref/08 §2.4, regra 8).

**Referências e código mudam juntos** (ADR 0023): cada frente é dona das referências da sua superfície (§2), e a CI da tarefa 0a falha se um bloco de uma referência ou um ficheiro de `wit/` ou `spec/` divergir do código.

**Regra de dependências** verificada em CI (`cargo deny` + um teste que lê o grafo): `joker-core` só depende de `joker-units` e `joker-expr`; nenhum crate de domínio depende de `jokerd`.

---

## 2. Frentes

| Frente | Crates | Referências de que é dona | Primeiro marco |
|---|---|---|---|
| Núcleo | `joker-units`, `joker-expr`, `joker-core` | 16 e ref/00 (com todas as frentes a rever) | envelope + árbitro com conformidade |
| Manifesto | `joker-manifest` | ref/01; ref/02 §2.1 e §3.1 (lock) | `jk check` sobre os exemplos deste repositório |
| Host | `joker-host`, `joker-sdk`, `wit/` | ref/04; ref/05 parte A (SDKs de tier 2) | app de exemplo a correr no harness |
| Drivers | `joker-hal`, `joker-drivers/*`, `joker-driver-sdk`, `joker-jdp`, `joker-drvtest` | ref/06; ref/03 §2.2–§2.4 (perfis, placas, simulação) | Modbus TCP contra um simulador |
| Serviços | `joker-store`, `joker-link` | — (documento 01, §12–§13) | historian com WAL e retenção |
| Daemon, API e CLI | `jokerd`, `joker-api`, `joker-client`, `joker-cli` | ref/09; ref/02 §2.2–§2.5 (nó, cliente, segredos, confiança); ref/05 parte B (clientes) | `jk dev` com planta simulada |
| Joker Micro e Link (Fase 2) | `joker-micro`, `joker-link-proto` | ref/07, ref/08 | núcleo + link num RP2350 |
| Agente (Fase 3) | `joker-agent`, `joker-agent-proto` | ref/10 | enrolamento e estado desejado contra um Deck de teste |
| Classes | `joker-jkc`, `classes/std/` | 17; ref/03 §2.7 (pacotes) | a `std/` inicial (documento 14, §10) a passar a sua conformidade |
| Conformidade | `conformance/`, `joker-spec-tests` | ref/03 §2.5–§2.6 (cenários e esquema de conformidade); tarefas 0a e 0c | 150 casos, escritos por quem não escreve o núcleo |

A frente de conformidade escreve os casos **a partir da spec**, sem ler o código do núcleo. É a regra do Cravo: partilhar código é partilhar bugs.

---

## 3. Formato dos casos de conformidade

O esquema é um só para o núcleo e para as classes (`[suite]`, `[[case]]`, `do`, `expect`, `at`, `equals`, com os literais canónicos em minúsculas), definido na ref/03, §2.6, a partir das regras do documento 16, §8. Há **um ficheiro por suite**, `conformance/<área>/<suite>.conformance.toml`, com `schema = 1` como primeiro par. O executor (`cargo xtask conformance`) corre cada caso contra o `joker-core` com relógio virtual e compara resultado a resultado. Os valores esperados são escritos à mão a partir da especificação. Os mesmos ficheiros correm num MCU pelo firmware de conformidade (ref/08 §3.2, tarefa 22).

---

## 4. As primeiras tarefas da Fase 0

| # | Tarefa | Frente | Depende de |
|---|---|---|---|
| 0a | `joker-spec-tests`: doctests dos blocos Rust das referências, `tsc` nos blocos TypeScript, CDDL validado contra os exemplos, esquema JSON do manifesto gerado dos tipos e validado contra todos os blocos TOML, OpenAPI validado, contagens da prosa verificadas contra as fontes (§5.1); falha a CI se referência e código divergirem | Conformidade | — |
| 0b | `codes.toml`: todos os códigos das referências (a semente `spec/codes.toml`, que é a fonte das contagens) num registo único, e o teste de CI que o verifica | Manifesto | — |
| 0c | vetores de teste do Joker Link (ref/07 §4.8), da ABI C (ref/08) e casos de conformidade do núcleo como ficheiros em `conformance/`, corridos em Linux e num MCU | Conformidade | — |
| 1 | `joker-units`: o catálogo do documento 16, §5 (dimensões, prefixos, `°C`/`K` afim, grandezas), parse de literais | Núcleo | — |
| 2 | Tabela de conversões e testes com casos de UCUM | Núcleo | 1 |
| 3 | `joker-expr`: léxico e parser com spans (para erros ao estilo rustc) | Núcleo | 1 |
| 4 | `joker-expr`: verificação de tipos e unidades (`J0301`…) | Núcleo | 3 |
| 5 | `joker-expr`: bytecode e VM de pilha sem alocação; funções de janela | Núcleo | 4 |
| 6 | `joker-core`: `Slot`, seqlock e testes com loom | Núcleo | — |
| 7 | `joker-core`: registo imutável e `SignalId` densos | Núcleo | 6 |
| 8 | `joker-core`: imagem por grupo (latch, staging) | Núcleo | 7 |
| 9 | `joker-core`: árbitro de 16 níveis com leases e TTL manual | Núcleo | 8 |
| 10 | `joker-core`: envelope (passos 0–8) e trips | Núcleo | 5, 9 |
| 11 | `joker-core`: grupos atómicos com taxa proporcional | Núcleo | 10 |
| 12 | `joker-core`: alarmes (máquina de estados, delay, shelving) | Núcleo | 5 |
| 13 | `joker-core`: traço no formato do documento 16, §7, e rastreio causal (bitset de leituras) | Núcleo | 8 |
| 14 | `conformance/`: executor do esquema da ref/03 §2.6, e os primeiros 50 casos (árbitro A1–A14, envelope E1–E10, qualidade §3) | Conformidade | spec |
| 15 | `joker-manifest`: parse, modelo, J-codes com spans | Manifesto | 1, 4 |
| 16 | `joker-manifest`: overlays (`extends`) e envelope que só aperta | Manifesto | 15 |
| 17 | `joker-manifest`: admissão RT (rate-monotonic) e orçamento de barramento | Manifesto | 15 |
| 18 | `joker-drivers/sim`: geradores, dinâmicas, falhas | Drivers | 7 |
| 19 | `joker-test`: harness com relógio virtual (ainda sem Wasm: tarefas nativas de teste) | Host | 10, 18 |
| 20 | `xtask rt-bench` e guarda no-alloc no caminho do tick | Núcleo | 10 |
| 21 | `codes.toml`: carregado pelo `joker-manifest`, com os J-codes do documento 16, §9, e os da tarefa 0b | Manifesto | 0b, 15 |
| 22 | CI: build `no_std` para `thumbv8m.main-none-eabihf` e `riscv32imac-unknown-none-elf` com conformidade em QEMU (bloqueia), para `xtensa-esp32-none-elf` com conformidade no ESP32 da bancada (não bloqueia), e num RP2350 por sonda quando houver placa (§5) | Núcleo | 6, 14 |

---

## 5. CI

| Job | Onde | Quando | Bloqueia |
|---|---|---|---|
| fmt, clippy, testes | x86_64 | cada commit | sim |
| referências (`joker-spec-tests`: doctests, `tsc`, CDDL, esquema do manifesto, OpenAPI, contagens do §5.1) | x86_64 | cada commit | sim |
| conformidade | x86_64 | cada commit | sim |
| proptest longo (10⁶) | x86_64 | noturno | — |
| cross-build aarch64, armv7, `arm-unknown-linux-gnueabihf` (ARMv6), riscv64 | x86_64 (cross) | cada commit | sim |
| cross-build `no_std` **`thumbv8m.main-none-eabihf`** e **`riscv32imac-unknown-none-elf`**, e conformidade em QEMU (`mps2-an505`, `virt` riscv32) | x86_64 (cross) | cada commit | **sim** |
| tamanho do `jokerd` (`minimal` e completo, `cargo bloat`, 01 §21) | x86_64 (cross aarch64) | cada commit | sim |
| cross-build **Xtensa** (`xtensa-esp32-none-elf`, toolchain `esp`) e conformidade no ESP32 da bancada | runner com a toolchain `esp` | cada commit no núcleo | **não**: o resultado aparece no PR e abre um alerta, mas não impede o merge |
| conformidade num RP2350 (HIL) | runner com sonda | cada commit no núcleo, quando houver placa | sim |
| testes em hardware (CM4, CM5, RevPi Connect 5, x86 IPC) | runners próprios | noturno, e em PR com a etiqueta `hw` | — |
| `rt-bench` com PREEMPT_RT | runners próprios | noturno; regressão > 5% no p99,9 falha | sim |
| bancada HIL (Fase 1+) | bancada BESS | noturno | — |

**Porque o Xtensa não bloqueia.** A toolchain `esp` (`espup`) está fora do LLVM oficial e segue o Rust estável com atraso; um atraso dela não pode parar o núcleo. A conformidade de referência corre em Cortex-M33 e RISC-V com toolchains oficiais, e o Xtensa é um alvo a mais, nunca o único (documento 25, §5). Uma falha no Xtensa corrige-se antes do fim da fase.

### 5.1 Toolchains e MSRV

| Regra | Porquê |
|---|---|
| **Um `rust-toolchain.toml` por família de alvo:** o da raiz (toolchain oficial: Linux, `thumbv8m`, `riscv32`) e um em `crates/joker-micro/esp/` (toolchain `esp`, instalada pelo `espup` com versão fixada numa imagem de CI) | as duas famílias têm calendários diferentes; com um só ficheiro, a mais lenta travava a outra |
| **O MSRV do núcleo** (`joker-units`, `joker-expr`, `joker-core`, `joker-link-proto`, `joker-agent-proto`) é **≤ à versão de Rust da toolchain Xtensa mais recente**; os outros crates podem exigir mais | o núcleo tem de compilar para o ESP32 da bancada com a mesma fonte |
| **As dependências 0.x** (Embassy, `embedded-io`, `esp-hal` `unstable`, `esp-radio`, `probe-rs`) têm versão exata no `Cargo.lock` do workspace, e só aparecem nos hospedeiros e nas ferramentas, nunca no núcleo | uma quebra a montante não chega ao núcleo nem à ABI C |
| **Atualizar toolchains e dependências 0.x é uma tarefa explícita, uma vez por fase**, com a conformidade (Linux, QEMU e placas) como porta de entrada | atualizações a conta-gotas partem um alvo sem ninguém ver |
| **O Xtensa nunca é o único alvo** de uma verificação de conformidade | é a toolchain de terceiros; ver o parágrafo anterior |

As contagens que a prosa ainda repete (códigos, operações da API, mensagens do Joker Link) são verificadas pelo `joker-spec-tests` (tarefa 0a) contra as fontes: `crates/joker-manifest/codes.toml`, o OpenAPI e o CDDL. Onde é fácil, a prosa remete para a fonte em vez de repetir o número.
