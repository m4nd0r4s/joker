# 18 — Rodada de prontidão: estamos prontos para fechar a v1 e implementar?

A última rodada de estresse antes de congelar a documentação. Três frentes:

1. uma **auditoria completa** dos 56 ficheiros à procura de contradições, lacunas de especificação e erros (45 achados);
2. **nove ataques de ponta a ponta**, que seguem uma funcionalidade por todas as camadas, já com a `.jkc`;
3. uma **verificação de nomes** (crates, CLI, vocabulário).

Conclusão no §6: **sim, a v1 pode ser fechada**, com quatro decisões que não são técnicas e que são do dono do projeto.

---

## 1. Ataques de ponta a ponta

Cada ataque segue um pedido desde o código de quem escreve até ao equipamento e de volta, e pergunta em cada fronteira: *quem é o dono deste passo, e está especificado?*

### P1 — Um método, do `.jkc` ao registo Modbus e de volta

`classes/std/pump.jkc` → `jkc` gera o handle Rust → a app chama `p1.run_at(60 %)` → registo no buffer de propostas → expansão no núcleo → árbitro → envelope → caixa do driver → FC16 → readback → traço → replay.

| Fronteira | Achado | Correção |
|---|---|---|
| app → núcleo | o buffer de propostas só tinha escritas de 12 B, com porta `u16`; não havia forma de transportar uma chamada de método, e `u16` limitava uma app a 65 535 portas | registos com tipo: escrita (14 B, porta `u32`) e método (10 B + argumentos) — documento 04, §2 |
| núcleo | a expansão de métodos não tinha passo no tick, e `own()` não existia na linguagem de expressões | passo 5a (documento 01, §4); `own()` no documento 02, §10 e no 16, §4 |
| traço | as chamadas de método não eram gravadas, e o replay não as podia reproduzir | registos de método no formato do traço (documento 16, §7) |
| `jk check` | os metadados das apps não tinham equipamentos nem classes | `joker:meta` com equipamentos, classes e hashes de contrato (documento 04, §2) |

### P2 — Uma ação numa instância virtual

`jk action p2.reset --key k1` → permissivo → papel → deduplicação → … e depois? A implementação é uma app Wasm, e o mundo WIT não tinha como lhe entregar a ação. **Correção:** export `on-action` (documento 04, §2). E onde se configura o permissivo de uma ação de uma instância que não é um dispositivo? **Correção:** `[instances.<i>.actions.<a>]`, com precedência sobre as sugestões da classe (documento 17, §4.2).

### P3 — A bomba de arranque por impulsos (adaptador `edge`)

Três interações perigosas que ninguém tinha visto:

- o **keepalive** (documento 01, §8) reenviaria o impulso de arranque a cada 5 s;
- o bit **`force_safe`** do supervisor escreve numa caixa de valor, mas uma saída com `edge` precisa de uma **ação** de paragem;
- no **arranque** de uma geração, o equipamento pode estar a marchar sem que o Joker o saiba.

**Correção** (documento 17, §5.2): sem keepalive em membros `edge`, e fallback de comunicação obrigatório no próprio equipamento; `force_safe` executado pelo driver como a ação `fall`; e sincronização no arranque, executando a ação do nível `safe`.

### P4 — Três bombas, uma implementação

O documento 14 ligava `p1`, `p2` e `p3` à mesma `pump_ctl`, que tinha **um** `bind`. Três bombas com um contactor. **Correção:** `implemented_by = { app, bind }` cria uma instância da app por instância virtual (documento 17, §5.3; documento 03, §9.2).

### P5 — Camadas com latência zero

O documento 14 prometia latência nula entre a app de negócio e a implementação no mesmo grupo, mas no tick do documento 01 o árbitro só corria depois de todas as tarefas, e a chave `order` não existia. **Correção:** passo 4b (comandos virtuais arbitrados logo a seguir à tarefa que os produz) e `order` nos grupos de ciclo (documento 03, §7).

### P6 — Uma geração nova a meio de uma rampa

Uma configuração é aplicada enquanto o PCS sobe de 20 para 80 kW a 25 kW/s. Qual é o "último valor aplicado" da geração nova? E a rampa continua? A especificação não dizia. **Correção:** documento 16, A7 (o valor efetivo passa pelo envelope em todos os ticks) e A8 (uma geração que mantém a saída herda o último valor aplicado).

### P7 — A trilha como teste de aceitação da Fase 1

A porta de saída 4 da Fase 1 é "fazer a trilha só com a documentação". Ao percorrê-la contra o roadmap (a trilha antiga, de oito partes; a de hoje, de onze partes sobre a bancada do documento 23, mantém a porta nas partes 1–7, documento 11):

- a parte 8 usa instâncias virtuais, que são da Fase 2 → a porta passa a ser **partes 1–7**;
- a biblioteca `std/` da Fase 1 não tinha `humidity_sensor` nem `pump` → acrescentadas;
- a dinâmica da planta simulada **falhava a sua própria verificação de unidades** (`%` subtraído a `m3/h`) → corrigida;
- o I²C usava um caminho de ficheiro onde o manifesto espera um barramento → `[buses.i2c1]`;
- a parte 4 prometia uma qualidade `Substituted` nas saídas que a runtime não consegue produzir → retirada, com a regra no documento 16, §3.1.

### P8 — Os nomes

- **`--device` contra `[devices]`.** Na CLI, `--device` escolhia a placa com `jokerd`; no manifesto, `[devices]` são os equipamentos com que ela fala. Numa instalação, `jk read sala.temp --device sala` seria ambíguo para qualquer pessoa. **Correção:** o conceito passa a chamar-se **nó** (`--node`, `jk node enroll`, `JK_NODE`), e entra no vocabulário do documento 15.
- **crates.io** (verificado em 2026-09-24): o nome `joker` já existe (um tokenizer de JavaScript de 2015) e `jk` também. Estão livres `joker-core`, `joker-sdk`, `joker-host`, `joker-runtime`, `jokerd` e `jkc`. **Decisão:** todos os crates publicados usam o prefixo `joker-`; o SDK é o crate `joker-sdk`, importado nas apps como `joker` (`joker = { package = "joker-sdk", … }` no `Cargo.toml` gerado pelo `jk app new`), e a CLI é o crate `joker-cli` com o binário `jk`.
- **Marca.** "Joker" é um nome muito usado. Uma verificação de marca é trabalho jurídico e não foi feita. Fica como decisão do dono (§6).

### P9 — A Fase 0 pode começar só com a documentação?

A auditoria encontrou nove zonas que bloqueavam tarefas concretas da Fase 0: formato do traço, esquema dos casos de conformidade, ~12 regras do árbitro e do envelope, tabela de qualidade, casos-limite das expressões, catálogo de unidades, esquema do manifesto, registo de códigos e semântica dos aliases. **Correção:** o documento 16 (semântica do núcleo) fecha-as e passa a ter precedência sobre os outros documentos nessas matérias. O esquema JSON do manifesto passa a ser **gerado** a partir dos tipos do `joker-manifest`, com o documento 03 como fonte.

---

## 2. Correções da auditoria

Os 45 achados da auditoria, agrupados. Todos estão corrigidos nesta versão, ou reclassificados de forma explícita no documento 16, §10.

| Grupo | Achados | Onde ficou |
|---|---|---|
| execução de métodos, WIT, metadados | método no buffer de propostas; `own()`; `joker:meta` com classes; `on-action`; porta `u32` | 01 §4, 02 §10, 04 §2, 16 §4, 17 §4 |
| instâncias virtuais e camadas | uma instância de app por instância virtual; passo 4b; `order` | 03 §7 e §9.2, 14 §7 e §9, 17 §5.3 |
| omissões e códigos | `schedule_depth` = 0; `J0352` separado de `J0351`; registo único de códigos | 02 §7.2, 03 §15, 16 §9, trilha antiga 01 |
| lock | `[classes]`, `[wcet]`, `accept_missing_fallback` | 03 §14 |
| instâncias | `sim = true \| tabela`; `const:` proibido em `out`; um envelope por sinal; colisões de caminhos | 03 §9.2, 14 §5.2, 16 §6, 17 §5.2 |
| qualidade | tabela normativa de `get`/`get_uncertain`/`get_or`; sem contaminação automática das saídas | 16 §3, 02 §4, trilha antiga 04 |
| CLI | `jk call`, `jk action` em instâncias, `jk runtime build`, `jk migrate`, `jk read --refresh`, `--from/--to`, `jk class fmt/check/diff/doc/gen`, `--node` | 07 |
| versões | exemplos alinhados com `joker:runtime@0.1.0`; o `joker` do manifesto como versão mínima da runtime e do esquema | 03, 04, 09, trilha |
| trilha e roadmap | porta DX = partes 1–7; `std/` com `humidity_sensor` e `pump` | 11, trilha README |
| caminhos e nomes | aliases de instâncias; `to` de pipelines; apps em `snake_case`; `[buses.i2c1]`; canal `a0` | 02 §9, 04, 16 §6, trilha antiga 04, exemplo 00 |
| KPI de modelos | gramática própria, separada das expressões de controlo | 06 §10.1 |
| `.jkc` | versão completa na declaração; `map` com adaptadores; `std/` com barra e implícito; resposta dos métodos em async; tipos de retorno e listas; precedência das ações; `edge` vs keepalive e `force_safe`; regra de saúde; bomba com `variable_speed` | 17 |
| números e factos | RSS alvo 60 MB (e porta da Fase 1 a 90 MB); `pump_ctl` a passar a sua conformidade; unidades da planta simulada; `Stale` antes de `CommFailure`; `g` → `gn`; ADS1115 com shunt | 01 §21, 11, 14 §7, 04 §6, trilha antiga 04, 16 §5, exemplo 00 |
| estado das ADRs | todas passam a "aceite (v1)"; ADR 0018 nova | adr/ |
| precedência | ordem explícita entre documentos; exemplos e trilha são ilustrativos | docs/README, AGENTS |

---

## 3. O que continua aberto (e porque não bloqueia)

| Tema | Quando se decide | Porque não bloqueia a Fase 0 |
|---|---|---|
| fórmula exata da admissão RT, `dt()` depois de `skip`, transições de alarmes a partir de `Unknown`, orçamento de barramentos além do RTU, base dos caminhos de `extends` | ADRs durante a Fase 0 (documento 16, §10) | cada uma pertence a uma tarefa concreta e tem um valor por omissão razoável |
| replay exato de tarefas async (`trace.async`) | Fase 2 | a Fase 0 só grava o domínio de ciclo |
| `jk class extract` a partir de crates | depois da v1 | as classes locais em crates já funcionam dentro das apps |
| métodos como funções Wasm puras | só com um caso real | extension traits e implementações ativas cobrem os casos conhecidos |
| números dos SDKs TS e Python | Fase 3 (TS) e 1.2 (Python) | tier 2 |
| as seis validações do Burn | porta de saída da Fase 2 | o ML começa na Fase 2 |
| `jk lsp` com expressões embebidas nos TOML | Fase 1 | é DX, não semântica |
| servidor de frota | o Joker Deck (documento 20, ADR 0021; documento 12, §7) | — |

---

## 4. Os riscos à entrada da implementação

| # | Risco | Sinal de alarme | Mitigação já no plano |
|---|---|---|---|
| 1 | **Âmbito.** 24 primitivas, uma IDL e 18 documentos são muito para uma equipa de 3–4 pessoas | a Fase 0 passa das 8 semanas; tarefas da Fase 2 a entrarem na 1 | fases com portas de saída; ponto de decisão depois da Fase 1; crates opcionais por feature (documento 09, ataque 12) |
| 2 | **Tempo real por provar.** Os alvos de jitter são estimativas | a Fase 1 não chega a 2× o alvo | medir por placa desde a Fase 0 (`xtask rt-bench`); a promessa desce para grupos ≥ 10 ms se falhar |
| 3 | **Custo dos perfis e drivers.** O fosso competitivo é também o maior custo | perfilar um PCS demora mais de uma semana | importadores (SunSpec, CSV) e `jk profile test` na Fase 1; métrica explícita no ponto de decisão |
| 4 | **Wasm em movimento.** WASI 0.3 é recente, e o Component Model continua a evoluir | uma versão do wasmtime parte o mundo WIT | versão do wasmtime fixada por minor da runtime; política M21; adaptadores |
| 5 | **Burn pré-1.0** | breaking changes entre minors | versão fixada; ORT como segundo backend desde a Fase 2; ADR 0009 revista pelas validações |
| 6 | **Toolchains e dependências embedded 0.x** (Embassy, `embedded-io`, `esp-hal` `unstable`, toolchain Xtensa fora do LLVM oficial; documento 25, §5) | um `cargo update` parte um alvo e não outro; a toolchain `esp` atrasa-se face ao Rust estável | um `rust-toolchain.toml` por família (oficial e `esp`); versões exatas das dependências 0.x; atualização uma vez por fase, com a conformidade como porta; o Xtensa nunca é o único alvo e não bloqueia a CI (documento 13, §5 e §5.1) |

---

## 5. Checklist de congelamento

- [x] Cada decisão de arquitetura tem uma ADR aceite (0001–0018).
- [x] A semântica do núcleo está especificada ao nível de casos de conformidade (documento 16).
- [x] A IDL de classes está especificada, com gramática, execução e versões (documento 17).
- [x] Os exemplos TOML dos documentos são sintaticamente válidos.
- [x] Não há contradições conhecidas entre documentos normativos, e a ordem de precedência está escrita.
- [x] O plano da Fase 0 tem tarefas com dependências e referências à especificação (documento 13).
- [x] Os limites estão declarados (documento 12).
- [x] Os nomes de crates foram verificados.
- [ ] Licença escolhida.
- [ ] Nome e marca confirmados.
- [ ] Equipa e donos das frentes definidos (AGENTS.md, §3).
- [ ] Hardware de CI (CM4, CM5, RevPi Connect 5, PC x86) disponível ou encomendado.

Os quatro itens por marcar **não são técnicos** e são do dono do projeto.

---

## 6. Veredicto

**Sim: a documentação pode ser congelada como v1, e a Fase 0 pode começar.**

- A Fase 0 é o núcleo sem I/O, e está agora totalmente especificada pelos documentos 16, 01, 02 e 03, com um esquema de conformidade que uma equipa independente consegue escrever sem ler código.
- O que ainda não está decidido está listado, tem dono e tem momento (§3), e nada disso é necessário para as primeiras 21 tarefas.
- As decisões que faltam são quatro, e nenhuma é técnica:

| Decisão | Recomendação | Porquê |
|---|---|---|
| **Licença** | Apache-2.0 OR MIT (dupla), como o Rust, o wasmtime e o Burn | compatível com todo o ecossistema de que o Joker depende; se a intenção for open core, decidir **já** quais crates ficam fechados, antes do primeiro commit público |
| **Nome** | manter "Joker" como nome do projeto e `joker-*` nos crates | os crates estão livres; a verificação de marca deve ser pedida a um jurista antes de qualquer lançamento público |
| **Equipa** | uma pessoa por frente da Fase 0: núcleo, manifesto, conformidade (separada do núcleo, pela regra do Cravo) | a conformidade só vale se quem a escreve não escreve o núcleo |
| **Hardware de CI** | 1× CM4, 1× CM5, 1× RevPi Connect 5 e 1× PC x86 com PREEMPT_RT, desde a Fase 0 | a porta de saída de RT tem de ser medida em hardware real desde o início |

### Primeiras duas semanas

1. Criar o repositório (jj sobre git) com o `AGENTS.md`, o `README.md`, os `docs/` desta v1 com a tag `docs-v1`, e o `rust-toolchain.toml`.
2. Esqueleto do workspace do documento 13, §1, com a regra de dependências em CI.
3. `joker-units` (tarefa 1) e `codes.toml` (tarefa 21), que desbloqueiam tudo o resto.
4. Em paralelo, a frente de conformidade começa os primeiros 20 casos do documento 16, §8, sem ver código.
5. `xtask rt-bench` a correr num CM5 com PREEMPT_RT com uma tarefa vazia: o primeiro número real do projeto.

A partir daqui, **mudar o desenho é escrever uma ADR** (AGENTS.md, §1).

---

## 7. Adenda: revisão de âmbito

Depois do congelamento, o âmbito da v1 mudou em três pontos (ADRs 0019, 0020 e 0021):

| Mudança | Efeito na prontidão |
|---|---|
| **ML passa para a 1.1** | reduz o risco 5 (Burn pré-1.0) para a v1; nada da Fase 0 muda |
| **Joker Micro na v1** (Fase 2) | a Fase 0 ganha uma exigência que **tem** de ser cumprida desde a primeira linha: o núcleo `no_std`, sem alocação, com capacidades fixas, com CI em `thumbv8m.main-none-eabihf` e `riscv32` (conformidade em QEMU) e no ESP32 da bancada, e num RP2350 quando houver placa (documento 13, tarefa 22 e §5) |
| **Família e Deck** | o protocolo do agente e o Deck MVP entram na Fase 3; nada da Fase 0 muda |

**O veredicto mantém-se:** a v1 pode ser fechada e a Fase 0 pode começar, agora com a regra `no_std` no `AGENTS.md`. Às quatro decisões do §6 junta-se uma quinta, ligada à licença: **o modelo de negócio da família** (documento 20, §6). O hardware de CI passa a incluir um RP2350 com sonda de depuração desde a Fase 0.

## 8. A rodada seguinte

Depois desta prontidão, todas as interfaces foram fechadas em referências normativas (`docs/ref/`, ADR 0023), e a Fase 0 ganhou três tarefas iniciais que as compilam e validam no código (0a–0c). O que se decidiu, os conflitos entre referências e o seu estresse estão no **documento 22**.
