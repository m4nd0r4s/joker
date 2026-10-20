# 09 — Arquitetura sob estresse

O documento 08 atacou o desenho com casos de uso. Este ataca a **arquitetura em si**: se a abstração aguenta os protocolos reais, se os orçamentos de tempo e memória fecham, o que acontece quando cada peça falha, se o tempo real em Linux é credível, se o sistema se estende sem tocar no núcleo, como sobrevive a versões e a atacantes, e se o "coringa" não se torna um canivete suíço impossível de manter.

Cada ataque termina com uma de três conclusões: **aguenta**, **aguenta com refinamento** (alteração já incorporada nos documentos 01–07) ou **primitiva nova** (M-n, documento 10).

---

## Ataque 1 — Onde a abstração de sinais deixa passar detalhe

Pegou-se em cada protocolo do catálogo e perguntou-se: o que é que o protocolo sabe exprimir e o sinal não?

| Semântica do protocolo | Exemplo | Cabe? | Como |
|---|---|---|---|
| valor com timestamp e qualidade | OPC UA DataValue | sim | é a origem do modelo |
| push por mudança | OPC UA subscription, BACnet COV, CAN PDO, GPIO edge | sim | `subscribe` no driver |
| **comandos** com argumentos e resultado | método OPC UA, reset Modbus por impulso, SDO CANopen, calibração | **não** | **M19 ações** |
| **escritas multi-registo coerentes e ordenadas** | u32 em 2 registos, modo+setpoint, "escreve parâmetros e depois commit" | **não** | **M20 grupos atómicos e ordem** |
| leitura com efeitos laterais | registos que limpam alarmes ao serem lidos | sim, com cuidado | `forbidden` no perfil; lidos só como ação |
| tipos estruturados | ExtensionObject OPC UA, estruturas S7 | parcial | achatados em caminhos com prefixo; os campos lidos juntos partilham `acq_id` |
| arrays grandes | espetro de um analisador de rede, perfil de temperatura | parcial | como sinal em bloco (M7) com `period` = passo espacial, ou como evento |
| dicionário de objetos com milhares de parâmetros | CANopen OD, parâmetros de variador | sim, com refinamento | direção `param` com `poll = "on_demand"`: lido no `connect` e quando pedido, não em ciclo |
| alarmes e eventos do equipamento | OPC UA A&C, eventos BACnet | sim | forma **evento** (documento 02, §1) |
| histórico no equipamento | OPC UA Historical Access, logs de inversores | não | **fronteira**: o Joker não importa histórico de terceiros no núcleo; uma ação pode ir buscá-lo |
| transferência de ficheiros, firmware | FTP de inversores, SDO por blocos | sim | ação longa com progresso (M19) |
| sessões com estado e autenticação por utilizador | OPC UA com utilizador por operador | parcial | o Joker liga-se com uma identidade de serviço; a identidade do operador fica no registo de decisões, e não chega ao equipamento |
| prioridades no próprio equipamento | BACnet priority array | sim | M15 + `safe = "relinquish"` |

**Conclusão: primitivas novas M19 e M20.** O resto cabe, alguns casos com refinamento (`param` com poll a pedido, histórico como ação). A abstração perde detalhe em dois pontos (sessões por utilizador e histórico no equipamento), e ambos ficam declarados no documento 12.

---

## Ataque 2 — O orçamento do caminho quente

Um site pesado de referência: **5 000 sinais**, 40 dispositivos, 3 grupos de ciclo (1 ms com 1 app, 100 ms com 4 apps, 1 s com 3 apps), 300 saídas, historian a 1 Hz em todos os sinais e a 10 Hz em 500.

### 2.1 Primeira versão, e porque não fechava

A primeira versão do contrato WIT tinha só `read(port)` e `write(port, v)` como chamadas ao host. Uma app do grupo de 100 ms com 200 entradas e 50 saídas faz **250 chamadas ao host por tick**. Uma chamada ao host no wasmtime com o ABI canónico custa dezenas a centenas de ns, conforme o tipo (a medir). Com `variant value` e `record sample` a atravessar a fronteira, a estimativa ficava em ~150 ns por chamada, ou seja ~40 µs por tick só em fronteira. Não é mortal a 100 ms, mas é o termo dominante, e a 1 ms com 100 entradas são ~15 µs (1,5% do período), mais do que tudo o resto da infraestrutura junta.

### 2.2 Refinamento: imagem empacotada

`on-cycle` passa a receber a **imagem de entradas empacotada** da app e a devolver as **propostas empacotadas**:

```wit
on-cycle: func(task: u32, image: list<u8>) -> cycle-result;
```

- O layout é fixado no `init` (a ordem das portas vem de `joker:meta`): um cabeçalho de 64 B, **24 B por amostra** (valor 8, código e motivo de qualidade 1 + 1, flags 2, `seq` 4 truncado, `ts_acq` 8) e 24 B de resultado por saída, que dão `last_outcome()` sem chamadas ao host.
- O host escreve a imagem numa arena pré-alocada e o ABI canónico copia-a numa única operação para um buffer do guest reservado no `init`. **No `on-cycle` a app não aloca:** uma alocação é trap `J-E0903` (a primeira versão usava um alocador *bump* reposto a cada tick, que deixava ponteiros pendentes no estado; ref/04 §4.13).
- As propostas voltam num buffer de **registos com tipo**: `WRITE` de 14 B (`kind u8 · flags u8 · port u32 · value u64`), `METHOD` de 10 B + argumentos, `PUBLISH`, `COUNTER_ADD`, `EMIT` e `NOTE`. A porta é `u32` (uma app não fica limitada a 65 535 portas) e os métodos de classe viajam no mesmo buffer (documento 18, P1).
- Os SDKs de tier 2 usam a mesma imagem. `cycle-io.read/write` ficam para o acesso dinâmico e para os valores que não cabem em `u64` (strings, bytes, registos).

O layout exato, a validação e os erros estão na ref/04, §3.

### 2.3 Orçamento resultante (estimativas a validar no CM4)

| Etapa | Grupo 1 ms (1 app, 60 in, 12 out) | Grupo 100 ms (4 apps, 800 in, 150 out) |
|---|---|---|
| latch (10 ns/sinal) | 0,6 µs | 8 µs |
| expressões (permissivos, `calc`) | 1 µs | 15 µs |
| empacotar e copiar a imagem | 0,5 µs | 6 µs |
| chamada Wasm (entrada/saída) | 1,5 µs | 4 × 1,5 = 6 µs |
| lógica da app | 20 µs (declarado) | 4 × 200 µs = 800 µs |
| árbitro + envelope (300 ns/saída) | 3,6 µs | 45 µs |
| commit + filas + traço | 1 µs | 10 µs |
| **total** | **~28 µs (2,8% do período)** | **~890 µs (0,9%)** |

A infraestrutura fica abaixo de 10 µs no grupo de 1 ms, e o custo dominante passa a ser a lógica das apps, que é o que deve ser. **Aguenta com refinamento** (incorporado nos documentos 01, §9, e 04, §2; forma fechada na ref/04, §3).

### 2.4 Throughput

| Fluxo | Taxa | Custo estimado | Núcleo |
|---|---|---|---|
| ingestão (40 dispositivos, média 50 ms) | ~100 k amostras/s | ~200 ns cada → ~2% de um núcleo `io` | `io` |
| historian (5 000 × 1 Hz + 500 × 10 Hz) | 10 k/s | desprezável | `bg` |
| uplink (Sparkplug, 1 min, com alias) | ~83/s | desprezável | `bg` |
| expressões `calc` a 100 ms | 200 × 10/s | desprezável | `rt` |

O estrangulamento real de um site destes são os **barramentos** (Modbus RTU), e não a CPU. Por isso o orçamento de barramento no `jk check` (documento 05, §5) vale mais do que qualquer otimização do núcleo.

---

## Ataque 3 — Memória e footprint

### 3.1 Orçamento num CM4 (2 GB) para o site de referência

| Componente | Estimativa |
|---|---|
| `.text` do `jokerd` (páginas partilhadas, só leitura) | 20–30 MB |
| slots (5 000 × 32 B) + imagens + arenas | < 1 MB |
| filas SPSC e anéis de traço | 4 MB |
| instâncias Wasm (8 apps × 2 instâncias × 4–16 MiB) | 64–256 MB **virtuais**, ~20 MB residentes |
| historian: caches de blocos + WAL | 32 MB |
| uplink e API | 8 MB |
| modelo TCN pequeno (Burn CPU) | 5–20 MB |
| **RSS total** | **~100–130 MB** |

### 3.2 O problema que só aparece em 32 bits

Por omissão, o wasmtime reserva **4 GiB de espaço virtual por memória linear**, com páginas de guarda, para eliminar as verificações de limites. Num processo de 64 bits isso não custa nada. **Num armv7 de 32 bits é impossível**: o espaço de endereçamento inteiro do processo é ~3 GiB. A configuração do motor passa a depender da arquitetura: em 32 bits, memórias com verificação explícita de limites, sem guard pages e com `memory_reservation` igual ao `memory.max` declarado. Custa ~10–20% de desempenho a mais, que se soma ao Pulley. **Aguenta com refinamento** (documento 01, §9).

### 3.3 Streams

O pool de uma câmara 1080p NV12 com 6 buffers ocupa ~19 MB, e uma câmara 4K ocupa ~75 MB. O `jk check` soma os pools e compara-os com a RAM da placa (`J0380`, documento 08, rodada 5).

### 3.4 OOM

- O `jokerd` corre com `oom_score_adj = -900`, e os processos `joker-ml-*` e `joker-drv-*` com `+500`. Numa situação de falta de memória, o kernel mata primeiro o modelo e não o controlo.
- As instâncias Wasm têm limites rígidos (pooling), e uma app que tenta crescer além do `memory.max` recebe uma falha de `memory.grow` (e normalmente faz trap), sem afetar mais ninguém.

---

## Ataque 4 — Matriz de injeção de falhas

Para cada componente e cada modo de falha: quem deteta, o que acontece, e se o desenho original tinha resposta.

| Componente | crash | hang | lento | valor errado | exaustão |
|---|---|---|---|---|---|
| driver (leitura) | reinício do driver; `Bad(NotConnected)` | timeout por lote | `Degraded` | **sensor congelado com `Good`** → ver (b) | fila de ingestão cheia → perda contada |
| driver (escrita) | idem | **escrita que nunca volta** → ver (a) | escritas acumuladas → ver (a) | readback (`verify`) | — |
| app (ciclo) | trap → tick sem a app | epoch | overrun da app | envelope | `memory.max` |
| app (async) | trap → reinício | sem efeito no ciclo; lease expira | idem | envelope | `memory.max` |
| modelo | processo isolado reinicia | deadline | fallback de backend | guarda OOD (parcial) | OOM → processo morto primeiro |
| grupo de ciclo | o grupo é uma thread do `jokerd` | **supervisor** → ver (c) | overrun | — | — |
| historian | serviço reinicia | não bloqueia o núcleo (filas) | perda contada | — | disco cheio → retenção |
| uplink | idem | idem | backlog no historian | — | orçamento |
| `jokerd` | systemd + fallback do dispositivo | watchdog de hardware | — | — | oom_score_adj |
| placa | watchdog de hardware | idem | throttling térmico → M8, alarme | — | — |

Três respostas que faltavam:

**(a) Escrita que encrava.** Um equipamento aceita a ligação TCP e nunca responde ao FC16. Com uma fila FIFO de escritas para o driver, as escritas seguintes acumulam-se e a primeira a sair, quando o equipamento acordar, é a mais velha. **Refinamento:** a fila de saídas para cada driver é uma **caixa por saída com "o último ganha"** (latest-wins), e não uma FIFO. Só as escritas agendadas, os impulsos e os grupos atómicos usam uma fila ordenada, e essa é limitada. Cada escrita tem timeout, e ao fim de N timeouts o dispositivo passa a `Down`, com o fallback do dispositivo a cobrir.

**(b) Sensor congelado com qualidade `Good`.** É um transmissor 4–20 mA avariado, ou um PLC intermédio que deixou de atualizar mas continua a responder. O valor não muda durante horas e a qualidade diz `Good`. **Refinamento:** `stuck_detect = "30m"` por sinal (e, nos perfis, para grandezas que variam sempre, como a frequência da rede): um valor bit-a-bit igual durante esse tempo passa a `Uncertain(Frozen)`. Novo motivo de qualidade no documento 02, §4.

**(c) Supervisor e grupo encravado.** O supervisor põe as saídas de um grupo encravado em `safe`, mas quem escreve nas filas dos drivers é o grupo, e o princípio de escritor único impede o supervisor de escrever lá. **Refinamento:** cada caixa de saída tem um bit atómico **`force_safe`**, que só o supervisor pode ativar e que o driver verifica antes de cada envio. O caminho de emergência não passa pelo grupo e não viola o escritor único dos valores.

**Aguenta com refinamento** (documentos 01, §3 e §20, e 02, §4).

---

## Ataque 5 — Tempo real em Linux: o que pode correr mal

Um p99,9 de 100 µs num grupo de 1 ms só se consegue se nenhuma das seguintes coisas acontecer no núcleo `rt`:

| Fonte de latência | Mitigação no desenho | Verificação |
|---|---|---|
| page faults | `mlockall(MCL_CURRENT\|MCL_FUTURE)`, stacks pré-tocadas, instâncias de ciclo pré-tocadas | `jk doctor --rt` conta os *major faults* durante 60 s |
| alocação no host durante o tick | invariante 5 do `AGENTS.md` | guarda *no-alloc* em debug e em CI (`assert_no_alloc` no caminho do tick) |
| locks e inversão de prioridade | nenhum lock no caminho do tick; seqlocks e SPSC | revisão + teste de estresse com carga em `io`/`bg` |
| escalonamento de frequência | governor `performance` no núcleo `rt` | `jk doctor` |
| interrupções | afinidade de IRQs fora do núcleo `rt` (exceto a NIC do EtherCAT, que vai *para* o núcleo `rt/drv`) | `jk doctor` lê `/proc/interrupts` |
| SMIs (x86) | não se mitigam em software | `hwlatdetect` no `jk doctor --rt`; placas com SMIs acima de 50 µs ficam marcadas `rt: unsuitable` |
| throttling térmico | M8: treino e ML fora do `rt`, alarme de temperatura | sinal `joker.sys.temp` |
| wasmtime: inicialização preguiçosa | primeira chamada feita no `init`, com a instância aquecida | medição do primeiro tick |
| wasmtime: traps usam sinais | handler de sinais instalado antes de subir a prioridade | teste de trap sob carga |
| logs a partir do ciclo | anel lock-free, drenado pela thread `svc` | — |
| escrita em disco a partir do ciclo | proibida; o traço vai para um anel, drenado em `bg` | — |

**Conclusão honesta:** num kernel sem PREEMPT_RT, os alvos de 1 ms não se garantem, e o `jk check` recusa grupos abaixo de 10 ms sem `kernel = "preempt_rt"` (`J0351`). Com PREEMPT_RT, que está no kernel principal desde a 6.12, e CPU isolada, o alvo é plausível em CM4/CM5 e x86, mas **tem de ser medido por placa** antes de ser prometido. Por isso o perfil de placa leva o resultado do `jk doctor --rt` de referência. **Aguenta, com a condição de medir.**

---

## Ataque 6 — O modelo de instâncias Wasm

Um `Store` do wasmtime não é `Sync`. Uma app com tarefas de ciclo (thread RT) e tarefas async (tokio) não pode usar a mesma instância em paralelo. As alternativas avaliadas:

| Opção | Problema | Decisão |
|---|---|---|
| uma instância com mutex | uma tarefa async a segurar o `Store` durante uma inferência bloqueia o tick → **inversão de prioridade** no caminho RT | rejeitada |
| uma instância, com as tarefas async a correr *entre* ticks na thread RT | põe trabalho async no núcleo RT e rebenta o orçamento | rejeitada |
| threads Wasm com memória partilhada | não estão estabilizadas no Component Model e trariam de volta locks dentro do guest | rejeitada por agora; revisitar quando *shared-everything threads* estabilizarem |
| **duas instâncias do mesmo componente**, que partilham só sinais internos e caixas de correio | o estado não é partilhado implicitamente, e isso tem custo de DX | **aceite** |

O custo de DX é real, e o SDK tem de o tornar óbvio:

- `CycleIo<T>` e `TaskIo<T>` são tipos diferentes, e o estado de uma tarefa de ciclo (`st: &mut Ctl`) não é visível para as tarefas async;
- `Internal<T>` e `Mailbox<T, N>` são os únicos canais, e a documentação do SDK abre com um diagrama que os mostra;
- o erro de compilação para `static mut` partilhado explica porquê.

A vantagem inesperada: **o isolamento dá robustez**. Um trap numa tarefa async não mata a instância de ciclo, e uma fuga de memória numa tarefa async não mexe no ciclo. **Aguenta**, com o custo de DX assumido (ADR 0004).

---

## Ataque 7 — Extensibilidade: acrescentar sem tocar no núcleo

| Quero acrescentar… | Ponto de extensão | Toca no `joker-core`? | ABI |
|---|---|---|---|
| um protocolo novo | trait `Driver` (N), mundo `joker:driver` (W), JDP (X) | não | N: interno (feature de build); W: WIT versionado; X: JDP versionado |
| um equipamento novo | perfil TOML | não | esquema de perfil versionado |
| uma placa nova | perfil de placa | não | idem |
| um acelerador novo | trait `InferenceBackend` | não | interno, ou plugin com C ABI (`joker-ml-abi`) |
| uma linguagem nova para apps | qualquer toolchain que produza um componente para `joker:runtime` | não | WIT |
| uma operação de pipeline | trait `PipelineOp` com negociação de formato | não | interno |
| um tipo de uplink | trait `Uplink` | não | interno |
| uma ponte | trait `Bridge` (tem de declarar escritor, M11) | não | interno |
| uma função nas expressões | **sim** | sim | a linguagem de expressões é fechada de propósito |
| um passo do envelope | **sim** | sim | exige ADR |

Sobre o ABI nativo: o Rust não tem ABI estável, e plugins nativos em Rust carregados dinamicamente são uma fonte conhecida de falhas subtis. Por isso **os drivers e backends tier N são compilados para dentro** (um build personalizado do `jokerd` com as features escolhidas, `jk runtime build --features ethercat,bacnet`), e a extensão por terceiros faz-se por **Wasm** (W) ou por **processo** (X, JDP). O único plugin nativo dinâmico são os modelos gerados (documento 06, §3.2), com uma ABI C mínima e verificada. **Aguenta.**

---

## Ataque 8 — Versões e compatibilidade (M21)

Há sete coisas com versão que se cruzam num dispositivo: runtime, mundo WIT das apps, esquema do manifesto, formato do lock, perfis, formato `.jkm` (e a versão do Burn dentro dele) e mundo WIT dos drivers W.

Cenário que partiu a primeira versão: uma frota com metade dos dispositivos em runtime 0.3 e metade em 0.4, e uma app nova compilada contra `joker:runtime@0.4.0` que usa uma função nova. O que acontece quando o agente de frota a envia para um 0.3?

**Política (M21):**

| Artefacto | Regra |
|---|---|
| mundo WIT `joker:runtime` | semver. Até à 1.0, a runtime suporta o mundo atual e **o minor anterior**, através de um adaptador gerado (composição de componentes). **Depois da 1.0, serve todos os `joker:runtime@1.y` com y ≤ o seu** (regra de ouro, ADR 0022). Um mundo *mais novo* do que a runtime é recusado **antes do deploy** pela `jk` e pelo agente (`J-E0001`, a partir dos metadados). |
| esquema do manifesto | versionado com a runtime: `joker = "0.4"` no `[project]` é a versão mínima da runtime e, portanto, do esquema. Até à 1.0, `jk migrate` reescreve manifestos entre minors, e a runtime aceita o minor atual e o anterior; na série 1, uma runtime 1.x aceita qualquer manifesto 1.y com y ≤ x (ref/01 §9). |
| `joker.lock` | `version = 1` em toda a série 1.x; numa minor só se acrescentam secções e campos opcionais, e a `jk` escreve só o subconjunto da versão `joker` do projeto. A v2 lê `version = 1` e escreve `version = 2` depois de `jk migrate` (ref/02 §3.1.6). |
| perfis | `nome@major`; o lock fixa a versão exata. |
| `.jkm` | `format = N` no `model.toml`, e `burn = "0.22.x"` no payload Burn. A runtime rejeita um Burn de outra minor (`J0502`) e sugere `jk model migrate`. |
| mundo `joker:driver` | como o `joker:runtime`, com a mesma versão do `joker:runtime` que importa (ref/06 §4.8). |
| JDP | negociação de versão no `hello`; o `jokerd` suporta a versão corrente e a anterior (ref/06 §5.4). |

**Consequência para o roadmap:** até à 1.0, cada minor da runtime pode partir o mundo WIT, mas nunca sem adaptador para o minor anterior. Depois da 1.0, o mundo major é estável durante toda a major, e a janela de adaptadores cobre todas as 1.y. As interfaces reservadas para a 1.1 (`ml`, `frames`, escritas agendadas, intenções) ficam `@unstable` fora do mundo estável, para que a sua forma não fique presa antes de existirem (ref/04 §2.8–§2.9). **Primitiva nova: M21.**

---

## Ataque 9 — Segurança: modelo de ameaças

**Ativos:** o processo físico (o mais importante), as credenciais do dispositivo, os dados de processo e os modelos (propriedade intelectual).

| Adversário / falha | Ataque | Mitigação | Resíduo |
|---|---|---|---|
| app com bug ou maliciosa | escapar ao sandbox | Wasm sem ambient authority; só handles ligados; sem FS/rede por omissão | bugs no wasmtime (superfície pequena e auditada) |
| idem | DoS ao ciclo | epoch, `memory.max`, admissão por WCET | — |
| idem | **fazer mal dentro do envelope** (p. ex. ciclar a bateria ou um contactor para o desgastar) | taxa, `min_interval` e **orçamento de atuações** (`max_actuations = "20/h"`) | uma app autorizada pode sempre fazer más escolhas dentro dos limites: é o custo de a autorizar |
| cloud / uplink comprometido | enviar comandos | escritas remotas são escritores declarados com nível e lease; mTLS; nonce + timestamp com janela de ±30 s contra replay | comandos válidos dentro do envelope passam |
| atacante na rede local | **falar diretamente com o equipamento** (Modbus TCP não tem autenticação) | nenhuma no Joker | **fronteira**: segmentação de rede (VLAN de campo, o Joker como único host com duas NICs); documento 12, §5 |
| idem | atacar a API local | socket Unix por omissão; TCP só com mTLS; papéis | — |
| acesso físico | ler a chave, trocar o cartão SD | TPM/elemento seguro quando existe; artefactos assinados | secure boot é do SO (documento 12) |
| cadeia de fornecimento | artefacto adulterado | assinaturas por tipo de artefacto; SBOM; builds reprodutíveis como objetivo | chaves de assinatura comprometidas |
| modelo envenenado | treino no dispositivo com dados manipulados | linhagem (M10), validação obrigatória, sombra (M16) | ataques subtis de envenenamento passam na validação |

Dois refinamentos saíram daqui: o **orçamento de atuações** no envelope (passo 6, documento 01, §7) e a **regra de cliente único** do ataque 11. **Aguenta com refinamento.**

---

## Ataque 10 — Determinismo e replay: onde não chega

| Fonte de não determinismo | Tratamento |
|---|---|
| relógio | input (ADR 0007) ✓ |
| entradas do ciclo | snapshot no traço ✓ |
| parâmetros e config | geração no traço ✓ |
| saídas de modelos em GPU/NPU | gravadas (M22) ✓ |
| **aritmética de vírgula flutuante em Wasm** | o Wasm é determinista, exceto nos bits de NaN e no **relaxed SIMD**. **Refinamento:** o relaxed SIMD fica desativado nas instâncias de ciclo e os NaN são canonicalizados (`cranelift_nan_canonicalization`). Custa pouco e garante que o mesmo `.wasm` dá os mesmos bits em aarch64 e x86_64 |
| aleatoriedade nas apps | `random` com semente vinda do host e gravada ✓ |
| **tarefas async** | leem slots ao vivo, com ordem e timing que dependem do escalonador. O replay por omissão injeta as **propostas** async gravadas no tick em que chegaram (efeitos na fronteira). Com `trace.async = true`, gravam-se também as leituras das tarefas async e o replay passa a ser exato, a um custo que fica visível |
| apps nativas | reprodutíveis só na mesma arquitetura |

**Primitiva nova: M22** (saídas não deterministas gravadas; política de replay para async). Com refinamento nas flags do motor Wasm.

---

## Ataque 11 — Escala: onde o desenho encontra limites

| Dimensão | Limite do desenho | O que parte primeiro |
|---|---|---|
| sinais por dispositivo Joker | ~100 k (3,2 MB de slots) | a ingestão e os barramentos, muito antes |
| grupos de ciclo | ≤ 3 por núcleo `rt` (rate-monotonic) | a admissão (M23) |
| apps | ~50 (pooling, 2 instâncias cada) | a memória virtual em 32 bits |
| subscrições na API | ~1 000 clientes WebSocket | a CPU `bg` |
| **ligações a um equipamento Modbus TCP** | o equipamento aceita 1–4 ligações | **o SCADA e o Joker a disputar ligações**, com timeouts aleatórios de ambos os lados |

A última linha é um problema de campo clássico. **Refinamento — regra de cliente único:** num site com Joker, o Joker deve ser o **único cliente** de cada equipamento de campo, e os outros sistemas leem através das pontes (servidor Modbus e servidor OPC UA, M11). O `jk doctor` deteta ligações concorrentes quando o equipamento o permite (contadores de sessões em alguns perfis), e o documento 12 recomenda a regra explicitamente. **Aguenta com refinamento.**

---

## Ataque 12 — Coringa ou canivete suíço? O custo da complexidade

O documento 10 lista 23 primitivas (24, com as classes de equipamento do documento 14, que são opcionais e compilam para portas). O risco é o Joker tornar-se o que critica: um sistema enorme, difícil de aprender e de manter.

### 12.1 O custo de cada primitiva

| Primitiva | Crate | Opcional no build? | Toca no caminho quente? |
|---|---|---|---|
| M1 fallback do dispositivo, M2 lock | manifest, hal | não | não |
| M3 rastreio causal | core | não | sim: bitset de leituras por tick (~100 ns) |
| M4 streams, M5 pipelines | ml, drivers | **sim** (`vision`) | não (domínio ml) |
| M6 escritas agendadas | core, hal | não | sim, só nas saídas com `schedule_depth > 0` |
| M7 blocos | core, store | **sim** (`waveforms`) | não |
| M8 classes de recursos | jokerd | não | não |
| M9 alarmes | core | não | sim: avaliação por tick (bytecode) |
| M10 linhagem, M16 sombra, M17 guarda | ml | **sim** (`ml`) | não |
| M11 pontes | link | **sim** por ponte | não |
| M12 orçamentos, M13 overlays, M14 relógio | link, manifest, core | não | M14: flag por amostra |
| M15 árbitro | core | não | sim (~50 ns por saída) |
| M18 propostas | api | não | não |
| M19 ações, M20 atómicos | core, hal | não | M20: sim, só nas saídas agrupadas |
| M21 versões, M22 determinismo, M23 admissão | host, manifest | não | não |
| M24 classes e instâncias | manifest, sdk, core (consultas) | não | só as consultas, como sinais derivados |

**O `joker-core` tem de caber em ~15 k linhas** (alvo, a vigiar na revisão). Tudo o que é domínio (visão, formas de onda, ML, pontes) vive em crates opcionais, e um build `minimal` (a lista está na ref/06 §3.8, e o alvo de tamanho no 01 §21) exclui-os.

### 12.2 Revelação progressiva na DX

O manifesto mínimo tem 16 linhas, e quem começa só precisa de 4 conceitos (sinal, porta, ligação, envelope). O `mode = "dev"` é o que o deixa correr sem o fallback do equipamento verificado; em `prod`, a mesma saída GPIO sem fallback é `J0205` (ref/01):

```toml
[project]
name = "bomba"
mode = "dev"

[devices.io]
driver = "gpio"

[devices.io.signals]
line_4 = { line = 4, alias = "tank.low" }
line_5 = { line = 5, alias = "pump.run", direction = "output" }

[outputs."pump.run"]
safe = false

[apps.level]
path = "apps/level"
bind = { low = "tank.low", pump = "pump.run" }
```

Árbitro, sombra, pipelines, overlays e orçamentos aparecem **quando são precisos**, com valores por omissão seguros e um `jk check` que explica cada um. É a mesma disciplina do Cargo: o `Cargo.toml` mínimo tem 3 linhas, e o máximo tem centenas.

### 12.3 Conformidade como contenção

O envelope, o árbitro, a imagem de processo e a semântica de qualidade têm uma **bateria de conformidade em dados** (`conformance/<área>/<suite>.conformance.toml`, com o esquema único da ref/03 §2.6: casos com o esperado escrito à mão a partir da spec) e testes por propriedades. É o que permite acrescentar primitivas sem partir as garantias. **Aguenta, com vigilância permanente.**

---

## Resumo

| # | Ataque | Conclusão | Resultado |
|---|---|---|---|
| 1 | fugas da abstração | primitiva nova | **M19** ações, **M20** grupos atómicos e ordem; `param` com poll a pedido |
| 2 | orçamento do caminho quente | refinamento | imagem empacotada em `on-cycle`; registos de proposta com tipo (ref/04 §3) |
| 3 | memória | refinamento | configuração do wasmtime para 32 bits; `oom_score_adj` |
| 4 | injeção de falhas | refinamento | caixas latest-wins para drivers; `stuck_detect`/`Frozen`; bit `force_safe` do supervisor |
| 5 | tempo real | aguenta, com medição | `J0351`; `jk doctor --rt`; guarda no-alloc em CI |
| 6 | instâncias Wasm | aguenta | duas instâncias por app (ADR 0004) |
| 7 | extensibilidade | aguenta | tier N compilado para dentro; terceiros por W/X |
| 8 | versões | primitiva nova | **M21** |
| 9 | segurança | refinamento | orçamento de atuações; fronteira da rede de campo |
| 10 | determinismo | primitiva nova | **M22**; relaxed SIMD desligado e NaN canonicalizados |
| 11 | escala | refinamento | regra de cliente único via pontes |
| 12 | complexidade | aguenta, com vigilância | builds por features; revelação progressiva; conformidade |

A **M23** (admissão de tempo real por WCET) vem dos ataques 2 e 5 juntos e está descrita no documento 01, §4.
