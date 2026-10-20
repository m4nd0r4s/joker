<!-- spec-lint: allow-portuguese -->
# Glossary and translation style

This document fixes the English term for every domain word of the Joker specification (ADR 0029, decision 4). Every translation of `docs/`, `spec/` and `wit/` follows it, and so do code, error messages, commits, pull requests and issues. When a translator meets a Portuguese domain word that is not here, they add it here in the same pull request, before using it.

The glossary is not normative about semantics: what a term *means* is defined by the document named in the last column (and, for the core, by `docs/16` and `ref/00`). The glossary only fixes how it is *said*.

Conventions of the tables: **PT** is the Portuguese source term as it appears in the corpus (singular, with the usual article dropped); **EN** is the only accepted English term; *avoid* lists tempting translations that must not be used. Where a Portuguese word has several meanings, each meaning has its own row.

---

## 1. Style

### 1.1 Normative wording

| Portuguese | English | Notes |
|---|---|---|
| tem de, é obrigatório, exige | **must**, **is required** | the only form for a requirement |
| não pode, nunca, é proibido | **must not**, **never** | "never" is fine for invariants ("quality never disappears silently") |
| pode, é permitido | **may** | permission, not possibility; for possibility use "can" |
| não tem de, é opcional | **need not**, **is optional** | |
| deve, convém | **should** | only where the source is a recommendation; never turn a "tem de" into "should" |
| recusa (validation) | **rejects** | `jk check` rejects a manifest; a node refuses a connection |
| por omissão | **by default**; *o valor por omissão* → **the default** | avoid "by omission" |
| reservado | **reserved** | a surface that exists in the grammar but is not stable (ADR 0028) |

Do not use RFC 2119 capitals (MUST, SHALL). Lowercase **must / must not / may** carry the same weight in this corpus.

### 1.2 Language and voice

- **American spelling**: behavior, color, initialize, synchronize, serialize, catalog, license (noun and verb), canceled, modeling, artifact, fulfill, center, gray.
- The source addresses the reader as *tu*; English uses **you**, and tutorials use the imperative ("Run `jk sim`", not "You should run").
- Keep sentences short, as the source does. Do not add explanations the source does not have, and do not drop qualifiers ("in 1.0", "only in `prod`").
- "o Joker" → **Joker** (no article): "Joker runs", not "The Joker runs". "o Joker Micro", "o Deck" → **Joker Micro**, **the Deck** (Deck takes the article; Joker, Joker Micro, Joker Hub and Joker Studio do not).
- Product and project names are never translated: Joker, Joker Micro, Joker Link, Joker Hub, Joker Deck, Joker Studio, `jk`, `jokerd`.
- Headings keep their numbers (`## 7. Envelope`); the text after the number is translated.

### 1.3 Numbers, units and dates

- Decimal separator is a **point** (`0.5 s`, never `0,5 s`). In prose, thousands are grouped with a comma (`10,000 samples`); code, literals and tables of raw values are copied as they are.
- A **space** separates a number from its unit symbol: `400 kW`, `250 kWh`, `100 ms`, `20 °C`, `9600 bit/s`. No space before `%` (`30%`). Unit symbols are never pluralized or translated (`3 kW`, not `3 kWs`).
- Duration and quantity literals inside code spans keep the source form exactly: `"100ms"`, `"1.5 kW"` (ref/00 §3).
- Ranges use an en dash without spaces: `150–650 kW`, `ref/01–ref/10`.
- Ordinals: `1.º`, `3.ª` → `1st`, `3rd`. "a 1.1", "a v1" → **1.1**, **v1** ("in 1.1", "the v1 scope").
- Dates are ISO 8601 (`2026-10-21`); months in prose are written in English ("October 2026").
- Times and sizes: `B`, `KiB`, `MiB` as in the source; `µs` with the micro sign.

### 1.4 What is never translated

- **Identifiers and literals**: crate names, types, traits, functions, WIT names, TOML keys and values, JSON fields, enum variants, quality reasons (`clock_unsynced`), CLI commands and flags, file names inside code, environment variables, media types, URNs.
- **Codes**: `J0795`, `J-E0001`, M-numbers (`M15`), C/D/E/X item ids used in the stress documents, ADR numbers.
- **Section and document numbers**: `docs/16 §3`, `ref/02 §2.1.13`, "document 23", "ADR 0015". Remissions point at numbers, so they survive the translation unchanged.
- **Text inside code blocks** is code: keep it, except comments and human-readable strings in illustrative examples, which are translated (AGENTS.md rules 11 and 12). A string that is a normative message (a code's `message` in `spec/codes.toml`, an error text quoted from a reference) is taken from its source, never retranslated by hand.
- English words already used as terms in the Portuguese source stay as they are: lease, latch, tick, snapshot, trip, fallback, overlay, rollout, inbox, watchdog, historian, uplink, bundle, blob, digest, handle, backend, pipeline, stream, replay, commit, deploy, guest, host, harness.

### 1.5 Remissions and links

- "documento 16, §3", "o 16 §3", "docs/16 §3" → **document 16, §3** or **docs/16 §3**; "ref/02, §2.1" → **ref/02 §2.1**. Keep whichever short form the source uses.
- "trilha 4 §5", "a parte 4 da trilha" → **track 4 §5**, **part 4 of the track**; "exemplo 00" → **example 00**.
- "remissão" → **cross-reference** (in prose about the corpus itself).
- File names follow the map in Appendix A. Markdown anchors change when a heading is translated: whoever translates a heading fixes every link to its anchor in the same commit; `tools/spec-lint` reports the ones left behind.
- "secção" → **section**; "anexo" → **appendix**; "índice" → **index**; "tabela" → **table**.

---

## 2. Core model: signals, values, quality and time

| PT | EN | Notes / source |
|---|---|---|
| sinal | **signal** | 00, 02 |
| sinal escalar | **scalar signal** | 02 §1 |
| sinal em bloco | **block signal** | waveform in blocks of N samples; 02 §1 |
| stream | **stream** | bulk data; 02 §1 |
| sinal derivado | **derived signal** | |
| sinal interno | **internal signal** | |
| sinal publicado, publicados | **published signal**, **published** | `PUBLISH` registration of an app |
| amostra | **sample** | value, quality, time, seq |
| valor | **value** | |
| valor aplicado, último valor aplicado | **applied value**, **last applied value** | |
| qualidade | **quality** | `Good`, `Uncertain(reason)`, `Bad(reason)` |
| motivo (de qualidade) | **reason** | `Uncertain(motivo)` → `Uncertain(reason)`; avoid "motive", "cause" |
| qualidade dominante | **dominant quality** | 16 §3 |
| qualidade derivada | **derived quality** | |
| grandeza | **quantity** | physical quantity; avoid "magnitude" |
| unidade | **unit** | |
| dimensão | **dimension** | |
| literal de duração / de quantidade | **duration literal** / **quantity literal** | ref/00 §3 |
| caminho | **path** | `device.group.signal` |
| alias | **alias** | plural **aliases** |
| espaço de nomes | **namespace** | |
| tempo, instante | **time**, **instant** | |
| relógio | **clock** | the `Clock` trait |
| relógio de parede | **wall clock** | |
| monotónico | **monotonic** | |
| sequência, seq | **sequence**, **seq** | |
| período (de amostragem) | **(sampling) period** | |
| forma de onda | **waveform** | |
| tipo escalar | **scalar type** | |
| ação | **action** | command-type port with a result; 02 §8 |
| alarme | **alarm** | 02 §11 |
| evento | **event** | |
| expressão | **expression** | the manifest expression language |
| casos-limite | **edge cases** | |
| determinismo, determinista, determinístico | **determinism**, **deterministic** | |
| traço | **trace** | `.jktrace`; avoid "track", "log" |
| anel de traço | **trace ring** | |
| segmento de estado | **state segment** | in a trace; 16 §7 |
| gravação, gravar | **recording**, **record** | |
| reproduzir, replay | **replay** | |
| verdade de fundo | **ground truth** | 12 |
| carimbo (de tempo), carimbo da origem | **timestamp**, **source timestamp** | `ts_src`; 16 §3.3 |
| lido por poll, leitura | **read by poll**, **read** | 16 §3.4 |
| prazo | **deadline** | 16 A14, §7 |
| vírgula flutuante | **floating point** | 19 |

---

## 3. Runtime architecture

| PT | EN | Notes / source |
|---|---|---|
| núcleo | **core** | `joker-core`; "o núcleo `no_std`" → **the `no_std` core**; a CPU core is **CPU core** |
| runtime (a) | **runtime** | "a runtime coringa" → **the wildcard runtime** |
| camada | **layer** | |
| imagem de processo | **process image** | ADR 0004 |
| registo (de sinais) | **(signal) registry** | 01 §3.1 |
| armazém de últimos valores | **last-value store** | 01 §3.2 |
| aquisição, estágio de aquisição | **acquisition**, **acquisition stage** | 01 §3.3 |
| coerência de aquisição | **acquisition coherence** | 02 §6.2 |
| domínio de ciclo | **cycle domain** | real-time threads |
| domínio assíncrono | **async domain** | tokio runtime |
| ciclo | **cycle** | |
| grupo de ciclo | **cycle group** | |
| tarefa, tarefa de ciclo | **task**, **cycle task** | |
| tick, tick do grupo | **tick**, **group tick** | |
| limite de tick | **tick boundary** | where a hot swap takes effect |
| caminho do tick | **tick path** | "não aloca no caminho do tick" → "does not allocate on the tick path" |
| latch, instante do latch | **latch**, **latch instant** | |
| inbox (do grupo) | **(group) inbox** | |
| grupo dono, dispositivo dono, driver dono | **owning group**, **owning device**, **owning driver** | avoid "owner group" |
| escritor, um só escritor | **writer**, **single writer** | |
| posse, posse exclusiva | **ownership**, **exclusive ownership** | "posse dos pinos" → **pin ownership** |
| escalonador, escalonamento | **scheduler**, **scheduling** | |
| admissão de tempo real | **real-time admission** | M23 |
| classe de recursos | **resource class** | `rt`, `ml`, `bg` |
| orçamento | **budget** | |
| overrun | **overrun** | |
| alocação, alocar | **allocation**, **allocate** | |
| bloquear, bloqueante | **block**, **blocking** | |
| fila, fila cheia | **queue**, **full queue** | |
| perda (de amostras) | **drop** | "perde amostras contadas" → "drops counted samples" |
| contador | **counter** | |
| arranque, arrancar | **startup**, **start** (process); **boot** (device, MCU) | |
| reinício, reiniciar | **restart** | |
| troca a quente | **hot swap** | |
| persistência, estado persistente | **persistence**, **persistent state** | |
| historian (local) | **(local) historian** | `joker-store` |
| store-and-forward | **store-and-forward** | |
| uplink | **uplink** | `joker-link` crate |
| ponte | **bridge** | API bridges (Home Assistant, OPC UA server) |
| API local | **local API** | |
| observabilidade | **observability** | |
| rastreio causal | **causal tracing** | M3 |
| buffers partilhados | **shared buffers** | M4 |
| gravação por gatilho | **triggered recording** | M7 |
| caixa latest-wins, caixa do driver, caixa de valor | **latest-wins box**, **driver box**, **value box** | 01 §3.4, 18 P1 |
| malha (de controlo), malha de corrente | **loop**, **current loop** | 11, 12, 19 |
| executor de interrupção | **interrupt executor** | Embassy; 12, 19 |
| medição de *fuel* | ***fuel* metering** | wasmi; 19 §5 |
| saúde | **health** | |
| modo de falha, falha | **failure mode**, **failure** / **fault** | "falha" of a device or bus → **fault**; of a process or plan → **failure** |
| plataforma | **platform** | |
| números alvo, *alvo* | **target figures**, *target* | figures to validate |

---

## 4. Writes: arbiter, envelope, leases and safety

| PT | EN | Notes / source |
|---|---|---|
| escrita | **write** | |
| proposta, propor | **proposal**, **propose** | an app proposes writes |
| buffer de propostas | **proposal buffer** | |
| ciclo de vida de uma proposta | **lifecycle of a proposal** | 02 §7.1 |
| aplicada (escrita) | **applied**, **committed** | "escritas já aplicadas (*committed*)" → "writes already applied (*committed*)" |
| saída (sinal comandado) | **output** | the commanded signal with envelope and `safe`; not an exit gate |
| origem (de um escritor) | **origin** | writer = (origin, level); 16 A1 |
| aceite, limitada, rejeitada | **accepted**, **clamped**, **rejected** | the three outcomes; ref/00 §4.4 |
| resultado de escrita | **write result** | |
| escritas agendadas | **scheduled writes** | M6 |
| grupos atómicos | **atomic groups** | M20 |
| árbitro, arbitragem | **arbiter**, **arbitration** | ADR 0011 |
| nível (do árbitro) | **level** | `safety`, `manual`, … |
| prioridade | **priority** | |
| escritores só-proposta | **proposal-only writers** | M18 |
| registos com tipo | **typed records** | proposal buffer; 18 P1 |
| escrita efetiva | **effective write** | |
| envelope | **envelope** | range, rate, interlocks, safe state, lease |
| passo do envelope | **envelope step** | |
| gama, fora da gama | **range**, **out of range** | avoid "gamut" |
| taxa | **rate** (limit) | |
| interlock | **interlock** | |
| estado seguro | **safe state** | ADR 0006 |
| estado seguro em três camadas | **three-layer safe state** | 01 §8 |
| estado de reset (do hardware) | **(hardware) reset state** | |
| lease, leases | **lease**, **leases** | never translated |
| expirar, expirado | **expire**, **expired** | |
| renovar, renovação | **renew**, **renewal** | |
| trip | **trip** | output forced to safe state until reset |
| fallback do dispositivo, do equipamento | **device fallback**, **equipment fallback** | |
| cadeia de fallback, controlador de fallback, registos de fallback | **fallback chain**, **fallback controller**, **fallback registers** | |
| bilhete de escrita, bilhete de ação | **write ticket**, **action ticket** | `jk_ticket_t` |
| intenção, aprovação | **intent**, **approval** | M18, 1.1 |
| orçamento de atuações | **actuation budget** | |
| modo de manutenção | **maintenance mode** | |
| registo de decisões | **decision log** | M3; avoid "decision register" |
| registo de método | **method record** | in the proposal buffer; 17 §4.1 |
| guarda de entrada | **input guard** | M17, ML |
| sombra, modo sombra | **shadow**, **shadow mode** | ADR 0012 |
| atuar | **actuate** | |
| watchdog (de hardware) | **(hardware) watchdog** | |
| segurança | **safety** (functional, outputs, interlocks) / **security** (keys, signatures, access) | always choose by meaning; "nível `safety`" stays `safety` |

---

## 5. Apps, Wasm host and SDKs

| PT | EN | Notes / source |
|---|---|---|
| app, apps | **app**, **apps** | |
| componente (Wasm) | **(Wasm) component** | |
| mundo (WIT) | **world** | `joker:runtime` |
| porta (de uma app) | **port** | input, output, parameter or action |
| porta de entrada / de saída | **input port** / **output port** | not to be confused with **exit gate** (§11) |
| parâmetro | **parameter** | |
| ligação (*binding*) | **binding** | port ↔ signal in the manifest |
| tabela de portas | **port table** | |
| capacidade, capacidades | **capability**, **capabilities** | host capabilities; also firmware capabilities |
| instância de ciclo, instância async | **cycle instance**, **async instance** | two instances of the same component; 01 §5 |
| caixa de correio | **mailbox** | |
| contexto de serviço | **service context** | `[service]` |
| hospedeiro, host | **host** | |
| convidado, guest | **guest** | |
| chamada, chamador | **call**, **caller** | |
| exportar, importar, importação | **export**, **import** | |
| SDK de apps | **app SDK** | |
| harness de testes | **test harness** | `joker-test` |
| relógio virtual | **virtual clock** | |
| escritor de teste | **test writer** | |
| simulação, simulador, modelo de simulação | **simulation**, **simulator**, **simulation model** | |
| cenário | **scenario** | `.scenario.toml` |
| planta simulada | **simulated plant** | |
| semente | **seed** | random seed |
| lógica de negócio | **business logic** | |
| programador de negócio | **business programmer** | persona; 14, 15 |
| sinal de vida | **heartbeat signal** | 14 §2 |

---

## 6. Manifest, lock, configuration and files

| PT | EN | Notes / source |
|---|---|---|
| manifesto | **manifest** | `joker.toml`; 03, ref/01 |
| manifesto de entrada | **input manifest** | |
| manifesto efetivo | **effective manifest** | after overlays |
| overlay, overlay de entrada | **overlay**, **input overlay** | |
| lock, um lock por manifesto de entrada | **lock**, **one lock per input manifest** | `joker.lock` |
| deriva | **drift** | lock drift |
| geração | **generation** | a versioned configuration; 01 §15 |
| blob de geração, hash da geração | **generation blob**, **generation hash** | |
| troca de geração | **generation swap** | |
| aplicar, aplicação transacional | **apply**, **transactional apply** | |
| aplicar com confirmação | **apply with confirmation** | ADR 0015 |
| janela de confirmação | **confirmation window** | |
| rollback | **rollback** | |
| estado desejado | **desired state** | |
| pré-visualização | **preview** | `previewManualWrite` |
| chave (TOML) | **key** | "chave reservada" → **reserved key** |
| chaves inteiras | **integer keys** | CBOR |
| esquema | **schema** | |
| validação, validar | **validation**, **validate** | |
| ficheiro, pasta | **file**, **directory** | avoid "folder" except in UI text |
| raiz do projeto | **project root** | |
| projeto | **project** | |
| tabela de identificação | **identification table** | ref/03 §2.1.6 |
| tipo no nome do ficheiro | **type in the file name** | `<name>.<type>.toml`, ADR 0026 |
| artefacto, tipo de artefacto | **artifact**, **artifact type** | American spelling |
| descritor | **descriptor** | |
| digest do descritor, do lock, da imagem | **descriptor digest**, **lock digest**, **image digest** | |
| metadados | **metadata** | |
| linhagem | **lineage** | M10 |
| registo de códigos | **code registry** | `spec/codes.toml`; 16 §9 |
| código, código de saída | **code**, **exit code** | |
| erro, aviso | **error**, **warning** | severity names stay `error`, `warning`, `error-in-prod` |
| erro de utilização | **usage error** | |
| erro do pedido | **request error** | |
| tipo de media | **media type** | |
| chave de idempotência | **idempotency key** | |
| codificação | **encoding** | |
| enumeração | **enumeration** | |
| variante de enum | **enum variant** | |
| identificador | **identifier** | |
| marcações de versão | **version markers** | `@unstable`, `x-joker-since` |
| política de versões | **versioning policy** | ADR 0022 |
| estável, instável | **stable**, **unstable** | |
| major, minor, patch | **major**, **minor**, **patch** | |
| versionado, versionar | **version-controlled**, **version-control** | 15 |
| armazém por conteúdo | **content-addressed store** | 15 §2.5 |

---

## 7. Devices, boards, profiles, drivers and HAL

| PT | EN | Notes / source |
|---|---|---|
| placa | **board** | 15 §1 |
| perfil de placa | **board profile** | |
| dispositivo | **device** | |
| equipamento, tipo de equipamento | **equipment**, **equipment type** | uncountable: "a piece of equipment" or name the device |
| perfil | **profile** | reusable map of a device |
| mapa de registos | **register map** | |
| registo (Modbus, hardware) | **register** | |
| escala | **scale** | |
| driver, drivers da árvore, driver do catálogo | **driver**, **in-tree drivers**, **catalog driver** | |
| tier (N, W, X) | **tier** | |
| plano de leitura | **read plan** | |
| transporte | **transport** | |
| barramento, orçamento de barramento | **bus**, **bus budget** | |
| trama | **frame** | bus frame |
| mestre, escravo | **client**, **server** (Modbus); **controller**, **target** (I²C, SPI) | use "master/slave" only when quoting a standard verbatim |
| descoberta | **discovery** | |
| catálogo | **catalog** | |
| conformidade de drivers, suite de conformidade | **driver conformance**, **conformance suite** | `joker-drvtest` |
| sonda | **probe** | debug probe |
| pinos | **pins** | |
| periférico | **peripheral** | |
| rede de campo | **fieldbus** | |
| leitura, leitor | **read**, **reader** | |
| debounce, flanco | **debounce**, **edge** | |
| integrador de hardware | **hardware integrator** | |
| fabricante | **vendor** / **manufacturer** | vendor for software and NPUs, manufacturer for equipment |
| duplos de equipamento | **equipment doubles** | `joker-drvtest`; 13 |
| quadro (elétrico) | **cabinet** | 12, 19 |

---

## 8. Classes and instances

| PT | EN | Notes / source |
|---|---|---|
| classe, classe de equipamento | **class**, **equipment class** | `led@1`; 14, ADR 0017 |
| instância | **instance** | |
| instância implícita / composta / virtual | **implicit** / **composite** / **virtual instance** | |
| classe composta | **composite class** | ADR 0027 |
| implementação ativa | **active implementation** | |
| membro (de classe) | **(class) member** | |
| método, consulta | **method**, **query** | |
| adaptador | **adapter** | |
| pacote de classes | **class package** | |
| simulador de classe | **class simulator** | |
| contrato, hash de contrato | **contract**, **contract hash** | |
| IDL de classes | **class IDL** | `.jkc`, 17 |
| gramática | **grammar** | |
| registo (tipo do `.jkc`), registo de instâncias | **record**, **instance registry** | a `record` type vs the registry of instances; 17 §3.3, §6 |
| membro fixo, membro coleção | **fixed member**, **collection member** | 17 §3.2, §8 |
| agregação | **aggregation** | `any`, `count`; 17 §8 |
| profundidade (da composição) | **depth** | `JC0501`; 17 §8 |
| camadas de implementação | **implementation layers** | `J0620`; 14 §7.2 |
| cumprir (uma classe) | **fulfill** | a profile or instance fulfills a class |
| sugestão (`@*_hint`) | **hint** | 17 |
| método de topo | **top-level method** | 17 §4.1 |
| autor de abstrações | **abstraction author** | persona; 24 |

---

## 9. ML (1.1)

| PT | EN | Notes / source |
|---|---|---|
| modelo | **model** | |
| pipeline nativo | **native pipeline** | ADR 0014 |
| pré-processamento, pós-processamento | **preprocessing**, **postprocessing** | |
| treino (no dispositivo) | **(on-device) training** | |
| inferência | **inference** | |
| backend por omissão | **default backend** | Burn; ADR 0009 |
| acelerador | **accelerator** | |
| saídas de modelos | **model outputs** | |

---

## 10. Fleet, family, agent, OTA and security

| PT | EN | Notes / source |
|---|---|---|
| família | **family** | Joker, Micro, Hub, Deck, Studio; 20 |
| local-first | **local-first** | ADR 0021 |
| frota | **fleet** | |
| servidor de frota | **fleet server** | |
| plano de controlo | **control plane** | |
| nó | **node** | "nó ↔ Deck" → "node ↔ Deck" |
| agente, protocolo do agente | **agent**, **agent protocol** | `agent/1`; ref/10 |
| inventário | **inventory** | |
| coorte | **cohort** | |
| rollout, ondas | **rollout**, **waves** | "rollout em ondas" → **rollout in waves** |
| canário | **canary** | |
| paragem por KPI | **KPI stop** | |
| painel | **dashboard** | Deck UI; "painel de leitura" → **read-only dashboard** |
| OTA, recetor de OTA | **OTA**, **OTA receiver** | 19, ref/08 §2.12.6 |
| bootloader | **bootloader** | |
| imagem (de firmware) | **(firmware) image** | |
| descritor de firmware | **firmware descriptor** | |
| anti-rollback, pisos | **anti-rollback**, **floors** | `min_counter`; ref/10 §2.4.4 |
| contador de segurança | **security counter** | |
| enrolamento, enrolar | **enrollment**, **enroll** | American spelling; avoid "enrolment" |
| porta de enrolamento | **enrollment port** | |
| canal de arranque | **bootstrap channel** | the Unix socket, local or over SSH |
| segredo, segredos por destinatário | **secret**, **per-recipient secrets** | ADR 0024 |
| destinatário | **recipient** | |
| cifrar, cifrado, decifrar | **encrypt**, **encrypted**, **decrypt** | |
| chave de escrow | **escrow key** | |
| assinatura, assinado | **signature**, **signed** | |
| identidade, chave de identidade | **identity**, **identity key** | ADR 0025 |
| certificado de utilizador | **user certificate** | |
| utilizador | **user** | |
| palavra-passe | **password** | |
| chaves da organização | **organization keys** | |
| bundle de confiança, cadeia de bundles | **trust bundle**, **bundle chain** | |
| política de confiança | **trust policy** | |
| papel de assinatura | **signing role** | 15 |
| responsável de segurança | **security officer** | persona; 15 |
| integrador do site, engenheiro do site | **site integrator**, **site engineer** | personas; 15 |
| multi-inquilino | **multi-tenant** | 11 |
| CA local | **local CA** | |
| memória de nonces | **nonce memory** | |
| proteção de dados | **data protection** | 26 |
| técnico de campo | **field technician** | persona |
| operador | **operator** | |
| operador de rede | **grid operator** | in energy cases; "rede" as computer network stays **network** |

---

## 11. Joker Micro and Joker Link

| PT | EN | Notes / source |
|---|---|---|
| microcontrolador, MCU | **microcontroller**, **MCU** | |
| modo firmware, modo crate, biblioteca C | **firmware mode**, **crate mode**, **C library** | 19 |
| MCU autónomo | **standalone MCU** | |
| coprocessador | **coprocessor** | |
| firmware expansor | **expander firmware** | 24 |
| estado do link, chave do link | **link state**, **link key** | |
| negociação | **negotiation** | |
| em série, por série | **over serial** | |
| flash, geração em flash | **flash**, **generation in flash** | |

---

## 12. Development bench and learning track

| PT | EN | Notes / source |
|---|---|---|
| bancada (de desenvolvimento) | **(development) bench** | 23 |
| firmware de bancada | **bench firmware** | |
| papel, papéis | **role**, **roles** | |
| portátil | **laptop** | |
| ligações (físicas) | **wiring** | the bench wiring; a network link stays **connection** |
| trilha (de estudo) | **learning track**; short form **the track** | directory `track/`; avoid "trail", "path" |
| parte (da trilha) | **part** | "a parte 1" → **part 1** |
| módulo (da trilha) | **module** | A, B, C, D |
| Vais aprender | **You will learn** | standard block in the track |
| Antes de começar | **Before you start** | |
| Estado no fim desta parte | **State at the end of this part** | |
| Próximo | **Next** | |
| percurso guiado | **guided walkthrough** | `examples/00` |
| exemplo | **example** | directory `examples/` |
| proposta (documento) | **proposal** | directory `proposals/`, non-normative |
| sensor de toque | **touch sensor** | |
| LED, pisca, piscar | **LED**, **blinks**, **blink** | |

---

## 13. Planning, analysis and review vocabulary

| PT | EN | Notes / source |
|---|---|---|
| desenho (de software) | **design** | "documento de desenho" → **design document**; never "drawing" |
| especificação, spec | **specification**, **spec** | |
| normativo, ilustrativo | **normative**, **illustrative** | |
| referência (normativa) | **(normative) reference** | `docs/ref/` |
| precedência | **precedence** | |
| fase, Fase 0 | **phase**, **Phase 0** | |
| porta de saída (de uma fase) | **exit gate** | roadmap; not an output port |
| ponto de decisão | **decision point** | |
| fatia vertical | **vertical slice** | M0 |
| âmbito | **scope** | |
| frente (de trabalho) | **front** | the parallel work fronts of AGENTS.md §3 |
| tarefa | **task** | |
| plano de implementação | **implementation plan** | |
| prontidão | **readiness** | 18 |
| rodada | **round** | stress round, API round, readiness round |
| estresse, caso de estresse | **stress**, **stress case** | |
| ataque | **attack** | the design attacked by a stress case |
| aguenta, aguenta com refinamento | **holds**, **holds with refinement** | verdict column |
| quebrava, quebrou | **broke** | verdict column; "quebrava devagar" → **broke slowly** |
| parcial | **partial** | |
| correção, correção incorporada | **fix**, **fix incorporated** | |
| resolvido, corrigido, aplicado | **resolved**, **fixed**, **applied** | |
| achado | **finding** | |
| custo | **cost** | |
| lacuna (a fechar) | **gap (to close)** | |
| refinamento | **refinement** | |
| veredicto | **verdict** | |
| auditoria | **audit** | |
| revisão final | **final review** | 26 |
| revisão de âmbito | **scope review** | 12 |
| marco | **milestone** | 13 |
| executor (de conformidade) | **runner** | 13 |
| noturno (CI) | **nightly** | 13 |
| piloto de bancada, piloto de campo | **bench pilot**, **field pilot** | 11 |
| congelar, checklist de congelamento | **freeze**, **freeze checklist** | 18 |
| adenda | **addendum** | 18, 19 |
| marca (registada) | **trademark** | 18 |
| mudanças de desenho, primitivas | **design changes**, **primitives** | 10 |
| limites | **limits** | 12 |
| fora do nicho | **out of niche** | 12 |
| regra de ouro | **golden rule** | |
| regra do Cravo | **the Cravo rule** | proper name: conformance is written from the spec only; "sharing code is sharing bugs" |
| casos de conformidade, relatório de conformidade | **conformance cases**, **conformance report** | |
| vetores de teste | **test vectors** | |
| público, comprador, canal | **audience**, **buyer**, **channel** | 24 |
| ecossistema | **ecosystem** | 25 |
| licença | **license** | |
| modelo de negócio | **business model** | |
| DX | **DX** | developer experience |
| ADR: Estado, Data, aceite | **Status**, **Date**, **accepted** | ADR header |
| ADR: Contexto, Decisão, Consequências, Alternativas | **Context**, **Decision**, **Consequences**, **Alternatives** | ADR sections |
| alterada por, substitui, substituída por | **amended by**, **replaces**, **superseded by** | ADR header |
| coberta, coberta (1.1) | **covered**, **covered (1.1)** | key coverage tables in ref/01 |

---

## 14. Energy and industrial domain words (examples and stress cases)

| PT | EN |
|---|---|
| bateria, BESS | **battery**, **BESS** |
| inversor | **inverter** |
| FV, kWp | **PV**, **kWp** |
| medidor, contador (de energia) | **meter** |
| ponto de ligação (à rede) | **point of connection** |
| injeção | **export** (to the grid) |
| recetor de telecomando | **ripple-control receiver** |
| bomba, bomba de calor | **pump**, **heat pump** |
| caudal | **flow rate** |
| pressão, temperatura | **pressure**, **temperature** |
| falta de energia | **power outage** |
| inspeção visual | **visual inspection** |
| manutenção preditiva | **predictive maintenance** |
| comissionamento | **commissioning** |
| UTA | **AHU** |
| estação de bombagem, bomba principal | **pumping station**, **lead pump** |
| boia, fim de curso, relé térmico | **float**, **limit switch**, **thermal relay** |
| ida, retorno (água) | **supply**, **return** |
| proteção contra ciclos curtos | **protection against short cycling** |
| contador certificado | **certified meter** |
| sinópticos, ecrãs de operação | **mimic diagrams**, **operator screens** |
| troca (ativo/standby) | **switchover** |
| blocos funcionais | **function blocks** |
| SGTC | **BMS** (building management system) |
| alarme que pisca | **flapping alarm** (a LED **blinks**) |
| piso (de um edifício) | **floor** |

---

## Appendix A. File-name map

Every file under `docs/` keeps its number and gets an English slug (ADR 0029, decision 3). The directories `trilha/`, `exemplos/` and `propostas/` become `track/`, `examples/` and `proposals/`. The rename is one commit, and every remission and link in `docs/`, `spec/`, `wit/`, `README.md` and `AGENTS.md` was rewritten with it. Files whose name does not change are omitted.

<!-- spec-lint: links-off -->

| Old path | New path |
|---|---|
| `docs/00-visao-geral.md` | `docs/00-overview.md` |
| `docs/01-arquitetura.md` | `docs/01-architecture.md` |
| `docs/02-modelo-de-sinais.md` | `docs/02-signal-model.md` |
| `docs/03-manifesto.md` | `docs/03-manifest.md` |
| `docs/09-arquitetura-sob-estresse.md` | `docs/09-architecture-under-stress.md` |
| `docs/10-mudancas-de-desenho.md` | `docs/10-design-changes.md` |
| `docs/12-limites.md` | `docs/12-limits.md` |
| `docs/13-plano-de-implementacao.md` | `docs/13-implementation-plan.md` |
| `docs/14-classes-e-instancias.md` | `docs/14-classes-and-instances.md` |
| `docs/15-ficheiros.md` | `docs/15-files.md` |
| `docs/16-semantica-do-nucleo.md` | `docs/16-core-semantics.md` |
| `docs/18-prontidao-v1.md` | `docs/18-v1-readiness.md` |
| `docs/20-familia-e-deck.md` | `docs/20-family-and-deck.md` |
| `docs/21-da-v1-a-v2.md` | `docs/21-from-v1-to-v2.md` |
| `docs/22-rodada-da-api.md` | `docs/22-api-round.md` |
| `docs/23-bancada-de-desenvolvimento.md` | `docs/23-development-bench.md` |
| `docs/24-publico-e-usos.md` | `docs/24-audience-and-uses.md` |
| `docs/25-licoes-do-ecossistema-rust-embedded.md` | `docs/25-lessons-from-the-rust-embedded-ecosystem.md` |
| `docs/26-revisao-final.md` | `docs/26-final-review.md` |
| `docs/adr/0001-rust-para-a-runtime.md` | `docs/adr/0001-rust-for-the-runtime.md` |
| `docs/adr/0002-apps-como-componentes-wasm.md` | `docs/adr/0002-apps-as-wasm-components.md` |
| `docs/adr/0003-o-sinal-como-abstracao-unica.md` | `docs/adr/0003-the-signal-as-the-single-abstraction.md` |
| `docs/adr/0004-imagem-de-processo-e-dois-dominios.md` | `docs/adr/0004-process-image-and-two-domains.md` |
| `docs/adr/0005-nenhuma-escrita-contorna-o-envelope.md` | `docs/adr/0005-no-write-bypasses-the-envelope.md` |
| `docs/adr/0006-estado-seguro-obrigatorio.md` | `docs/adr/0006-mandatory-safe-state.md` |
| `docs/adr/0007-relogio-como-input.md` | `docs/adr/0007-clock-as-input.md` |
| `docs/adr/0008-drivers-em-tres-tiers.md` | `docs/adr/0008-drivers-in-three-tiers.md` |
| `docs/adr/0009-burn-como-backend-por-omissao.md` | `docs/adr/0009-burn-as-default-backend.md` |
| `docs/adr/0010-manifesto-toml-e-lock.md` | `docs/adr/0010-toml-manifest-and-lock.md` |
| `docs/adr/0011-arbitro-com-prioridades-e-leases.md` | `docs/adr/0011-arbiter-with-priorities-and-leases.md` |
| `docs/adr/0012-sombra-antes-de-atuar.md` | `docs/adr/0012-shadow-before-actuating.md` |
| `docs/adr/0013-telemetria-nunca-bloqueia.md` | `docs/adr/0013-telemetry-never-blocks.md` |
| `docs/adr/0014-pipelines-nativos.md` | `docs/adr/0014-native-pipelines.md` |
| `docs/adr/0015-aplicar-com-confirmacao.md` | `docs/adr/0015-apply-with-confirmation.md` |
| `docs/adr/0016-um-processo.md` | `docs/adr/0016-one-process.md` |
| `docs/adr/0017-classes-de-equipamento.md` | `docs/adr/0017-equipment-classes.md` |
| `docs/adr/0018-um-formato-por-tipo.md` | `docs/adr/0018-one-format-per-type.md` |
| `docs/adr/0019-ml-na-1-1.md` | `docs/adr/0019-ml-in-1-1.md` |
| `docs/adr/0020-nucleo-no-std-e-joker-micro.md` | `docs/adr/0020-no-std-core-and-joker-micro.md` |
| `docs/adr/0021-familia-local-first.md` | `docs/adr/0021-local-first-family.md` |
| `docs/adr/0022-politica-de-versoes.md` | `docs/adr/0022-versioning-policy.md` |
| `docs/adr/0023-referencias-normativas.md` | `docs/adr/0023-normative-references.md` |
| `docs/adr/0024-segredos-por-destinatario.md` | `docs/adr/0024-per-recipient-secrets.md` |
| `docs/adr/0025-identidade-e-assinaturas.md` | `docs/adr/0025-identity-and-signatures.md` |
| `docs/adr/0026-tipo-no-nome-do-ficheiro.md` | `docs/adr/0026-type-in-the-file-name.md` |
| `docs/adr/0027-classes-compostas.md` | `docs/adr/0027-composite-classes.md` |
| `docs/adr/0028-estavel-igual-implementado.md` | `docs/adr/0028-stable-equals-implemented.md` |
| `docs/exemplos/00-primeiro-projeto.md` | `docs/examples/00-first-project.md` |
| `docs/exemplos/01-bess-peak-shaving.md` | `docs/examples/01-bess-peak-shaving.md` |
| `docs/exemplos/02-inspecao-visual.md` | `docs/examples/02-visual-inspection.md` |
| `docs/exemplos/03-manutencao-preditiva.md` | `docs/examples/03-predictive-maintenance.md` |
| `docs/exemplos/README.md` | `docs/examples/README.md` |
| `docs/propostas/idl-classes.md` | `docs/proposals/class-idl.md` |
| `docs/ref/00-convencoes.md` | `docs/ref/00-conventions.md` |
| `docs/ref/01-manifesto.md` | `docs/ref/01-manifest.md` |
| `docs/ref/02-lock-no-e-cliente.md` | `docs/ref/02-lock-node-and-client.md` |
| `docs/ref/03-perfis-placas-sim.md` | `docs/ref/03-profiles-boards-sim.md` |
| `docs/ref/04-wit-e-sdk-rust.md` | `docs/ref/04-wit-and-rust-sdk.md` |
| `docs/ref/05-sdk-python-typescript.md` | `docs/ref/05-python-and-typescript-sdks.md` |
| `docs/ref/09-api-http.md` | `docs/ref/09-http-api.md` |
| `docs/ref/10-protocolo-do-agente.md` | `docs/ref/10-agent-protocol.md` |
| `docs/trilha/01-ola-bancada.md` | `docs/track/01-hello-bench.md` |
| `docs/trilha/02-o-pi-como-no.md` | `docs/track/02-the-pi-as-a-node.md` |
| `docs/trilha/03-sensor-temperatura.md` | `docs/track/03-temperature-sensor.md` |
| `docs/trilha/04-esp32-modbus.md` | `docs/track/04-esp32-modbus.md` |
| `docs/trilha/05-toque-e-falhas.md` | `docs/track/05-touch-and-faults.md` |
| `docs/trilha/06-trocar-de-placa.md` | `docs/track/06-switching-boards.md` |
| `docs/trilha/07-classes.md` | `docs/track/07-classes.md` |
| `docs/trilha/08-a-tua-classe.md` | `docs/track/08-your-own-class.md` |
| `docs/trilha/09-joker-micro.md` | `docs/track/09-joker-micro.md` |
| `docs/trilha/10-logica-no-mcu.md` | `docs/track/10-logic-on-the-mcu.md` |
| `docs/trilha/11-producao-e-frota.md` | `docs/track/11-production-and-fleet.md` |
| `docs/trilha/README.md` | `docs/track/README.md` |

<!-- spec-lint: links-on -->

Unchanged: `docs/04-sdk.md`, `docs/05-drivers.md`, `docs/06-ml.md`, `docs/07-cli.md`, `docs/08-stress-tests.md`, `docs/11-roadmap.md`, `docs/17-jkc.md`, `docs/19-joker-micro.md`, `docs/README.md`, `docs/adr/0029-english-project-language.md`, `docs/adr/README.md`, `docs/ref/06-drivers.md`, `docs/ref/07-joker-link.md`, `docs/ref/08-joker-micro-abi.md`.
